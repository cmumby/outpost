use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::appimage::{self, AppImageType};
use crate::{desktop, download};
use crate::paths::Paths;
use crate::state::{InstalledApp, InstalledState};

/// Icon sizes the hicolor theme knows about. A PNG of any other size can't be found
/// by name, so we fall back to pointing the desktop entry at the file directly.
const HICOLOR_SIZES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

/// What we need from inside the AppImage.
struct Metadata {
    desktop: String,
    /// The desktop file's name without `.desktop`, e.g. `org.example.Photocraft`.
    desktop_stem: String,
    icon: Option<Vec<u8>>,
}

pub fn install_from_file(paths: &Paths, source: &Path) -> Result<InstalledApp, String> {
    let source = source
        .canonicalize()
        .map_err(|e| format!("could not find {}: {e}", source.display()))?;
    let source_label = source.display().to_string();
    install_with(paths, &source_label, |incoming| {
        fs::copy(&source, incoming)
            .map(|_| ())
            .map_err(|e| format!("could not copy the AppImage: {e}"))
    })
}

/// `progress(downloaded, total)` is called as the download comes in.
pub fn install_from_url(
    paths: &Paths,
    url: &str,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<InstalledApp, String> {
    install_with(paths, url, |incoming| download::download(url, incoming, progress))
}

/// Shared install steps. `fetch` puts the AppImage at the path it's given.
fn install_with(
    paths: &Paths,
    source: &str,
    fetch: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<InstalledApp, String> {
    for dir in [&paths.applications, &paths.desktop_entries, &paths.data, &paths.cache] {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }

    // Bring the file in under a temporary name first, so a failed install never
    // leaves a half-finished app behind.
    let incoming = paths
        .applications
        .join(format!(".outpost-incoming-{}.AppImage", process::id()));
    let result = fetch(&incoming)
        .and_then(|()| check_appimage(&incoming, source))
        .and_then(|()| make_executable(&incoming))
        .and_then(|()| install_incoming(paths, &incoming, source));
    if result.is_err() {
        let _ = fs::remove_file(&incoming);
    }
    result
}

fn check_appimage(path: &Path, source: &str) -> Result<(), String> {
    match appimage::detect(path) {
        Ok(AppImageType::Two) => Ok(()),
        Ok(AppImageType::One) => Err(format!(
            "{source} is an old-style (type 1) AppImage, which Outpost doesn't support"
        )),
        Err(e) => Err(format!("{source} {e}")),
    }
}

fn install_incoming(paths: &Paths, incoming: &Path, source: &str) -> Result<InstalledApp, String> {
    let meta = read_metadata(paths, incoming)?;
    let id = sanitize_id(&meta.desktop_stem);
    let name = desktop::get(&meta.desktop, "Name").unwrap_or_else(|| id.clone());
    let version = desktop::get(&meta.desktop, "X-AppImage-Version");
    let appimage_path = paths.applications.join(format!("{id}.AppImage"));

    // Installing an app that's already installed replaces it.
    let mut state = InstalledState::load(&paths.installed_json())?;
    if let Some(old) = state.find(&id) {
        remove_files(old)?;
        state.apps.retain(|app| app.id != id);
    } else if appimage_path.exists() {
        return Err(format!(
            "{} already exists and wasn't installed by Outpost; move it out of the way first",
            appimage_path.display()
        ));
    }

    fs::rename(incoming, &appimage_path)
        .map_err(|e| format!("could not move the AppImage into place: {e}"))?;

    let (icon_path, icon_value) = match meta.icon.as_deref().map(|bytes| place_icon(paths, &id, bytes)) {
        Some(Ok(Some((path, value)))) => (Some(path), value),
        Some(Err(e)) => return Err(e),
        // No icon, or one we don't recognise: use a generic one.
        _ => (None, "application-x-executable".to_string()),
    };

    let desktop_path = paths.desktop_entries.join(format!("outpost-{id}.desktop"));
    let entry = desktop::rewrite(&meta.desktop, &appimage_path, &icon_value, &id, &meta.desktop_stem);
    fs::write(&desktop_path, entry)
        .map_err(|e| format!("could not write {}: {e}", desktop_path.display()))?;
    refresh_launcher(paths);

    let app = InstalledApp {
        id,
        name,
        version,
        appimage_path,
        desktop_path,
        icon_path,
        installed_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        source: source.to_string(),
    };
    state.apps.push(app.clone());
    state.save(&paths.installed_json())?;
    Ok(app)
}

pub fn uninstall(paths: &Paths, id: &str) -> Result<InstalledApp, String> {
    let mut state = InstalledState::load(&paths.installed_json())?;
    let index = state
        .apps
        .iter()
        .position(|app| app.id == id)
        .ok_or_else(|| format!("no installed app with id '{id}' (run `outpost list` to see them)"))?;

    remove_files(&state.apps[index])?;
    let app = state.apps.remove(index);
    refresh_launcher(paths);
    state.save(&paths.installed_json())?;
    Ok(app)
}

pub fn list(paths: &Paths) -> Result<Vec<InstalledApp>, String> {
    Ok(InstalledState::load(&paths.installed_json())?.apps)
}

/// Pulls just the desktop file and icon out of the AppImage into a scratch folder.
fn read_metadata(paths: &Paths, appimage: &Path) -> Result<Metadata, String> {
    let work = paths.cache.join(format!("extract-{}", process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| format!("could not create {}: {e}", work.display()))?;
    let _cleanup = RemoveDirOnDrop(work.clone());
    let root = work.join("squashfs-root");

    appimage::extract(appimage, &work, "*.desktop")?;
    let desktop_link = fs::read_dir(&root)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .find(|path| path.extension().is_some_and(|ext| ext == "desktop"))
        })
        .ok_or("the AppImage has no .desktop file, so it can't be added to the launcher")?;
    let desktop_stem = desktop_link
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let desktop_file = resolve_in_image(appimage, &work, &root, &desktop_link)
        .ok_or("the AppImage's .desktop file is a broken link")?;
    let desktop = fs::read_to_string(&desktop_file)
        .map_err(|e| format!("could not read the AppImage's .desktop file: {e}"))?;

    // Prefer the icon the desktop file names, then `.DirIcon`, the AppImage's own icon.
    // Icon extraction is best-effort: an app without an icon still installs.
    let mut candidates = Vec::new();
    if let Some(name) = desktop::get(&desktop, "Icon").filter(|name| !name.contains('/')) {
        let _ = appimage::extract(appimage, &work, &format!("{name}.*"));
        candidates.push(root.join(format!("{name}.svg")));
        candidates.push(root.join(format!("{name}.png")));
    }
    let _ = appimage::extract(appimage, &work, ".DirIcon");
    candidates.push(root.join(".DirIcon"));

    let icon = candidates
        .iter()
        .find_map(|path| resolve_in_image(appimage, &work, &root, path))
        .and_then(|path| fs::read(path).ok());

    Ok(Metadata { desktop, desktop_stem, icon })
}

/// Files at the top of an AppImage are often symlinks into `usr/share/...`. Since we
/// only extracted the files we asked for, follow each link and extract its target.
fn resolve_in_image(appimage: &Path, work: &Path, root: &Path, path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..8 {
        if current.exists() {
            return Some(current);
        }
        let target = fs::read_link(&current).ok()?;
        let resolved = normalize(&current.parent()?.join(target));
        let inside = resolved.strip_prefix(root).ok()?;
        appimage::extract(appimage, work, inside.to_str()?).ok()?;
        current = resolved;
    }
    None
}

/// Resolves `..` and `.` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Saves the icon where the launcher can find it. Returns the file path and the value
/// for `Icon=`, or `None` if the data isn't a PNG or SVG.
fn place_icon(paths: &Paths, id: &str, bytes: &[u8]) -> Result<Option<(PathBuf, String)>, String> {
    let name = format!("outpost-{id}");
    let (path, value) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if width == height && HICOLOR_SIZES.contains(&width) {
            let path = paths.icons.join(format!("{width}x{width}/apps/{name}.png"));
            (path, name)
        } else {
            let path = paths.data.join(format!("icons/{id}.png"));
            let value = path.display().to_string();
            (path, value)
        }
    } else if String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).contains("<svg") {
        (paths.icons.join(format!("scalable/apps/{name}.svg")), name)
    } else {
        return Ok(None);
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(Some((path, value)))
}

fn remove_files(app: &InstalledApp) -> Result<(), String> {
    let files = [Some(&app.appimage_path), Some(&app.desktop_path), app.icon_path.as_ref()];
    for path in files.into_iter().flatten() {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove {}: {e}", path.display())),
        }
    }
    Ok(())
}

/// `chmod u+x` and friends: owner can read/write/run, everyone else can read/run.
fn make_executable(path: &Path) -> Result<(), String> {
    let mut permissions = fs::metadata(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)
        .map_err(|e| format!("could not make {} executable: {e}", path.display()))
}

/// Turns a desktop file name into an id that's safe to use in file names.
fn sanitize_id(stem: &str) -> String {
    let id: String = stem
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .collect();
    let id = id.trim_matches(['.', '-']).to_string();
    if id.is_empty() { "app".to_string() } else { id }
}

/// Nudges the launcher to pick up added or removed apps. Failures don't matter.
fn refresh_launcher(paths: &Paths) {
    // GNOME notices new desktop files on its own; this refreshes the MIME-type cache.
    let _ = Command::new("update-desktop-database")
        .arg(&paths.desktop_entries)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    // GTK only re-scans an icon theme when the theme folder's own timestamp changes,
    // and icons go in subfolders that don't bump it, so bump it ourselves.
    let _ = fs::File::open(&paths.icons).and_then(|dir| dir.set_modified(SystemTime::now()));
}

struct RemoveDirOnDrop(PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_ids() {
        assert_eq!(sanitize_id("org.example.Photocraft"), "org.example.photocraft");
        assert_eq!(sanitize_id("My App"), "my-app");
        assert_eq!(sanitize_id("..."), "app");
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize(Path::new("/r/usr/../a/./b.png")), PathBuf::from("/r/a/b.png"));
    }
}
