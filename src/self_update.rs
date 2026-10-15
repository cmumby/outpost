//! Outpost updating and moving its own AppImage.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::Mutex;

use crate::{download, install, settings};
use crate::index::Asset;
use crate::paths::Paths;

/// Outpost's own id in the index (from `curated/outpost.toml`).
pub const INDEX_ID: &str = "outpost";

/// Where Outpost lives when it's installed properly.
pub fn home(paths: &Paths) -> PathBuf {
    paths.applications.join("Outpost.AppImage")
}

/// Where Outpost's own AppImage went, if it was moved while running.
static MOVED_TO: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The AppImage file Outpost is running from, or `None` for a development build.
/// (The running program itself is inside a temporary mount; the AppImage runtime
/// puts the real file's path in $APPIMAGE.)
pub fn running_appimage() -> Option<PathBuf> {
    if let Some(moved) = MOVED_TO.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(moved);
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

/// After the apps folder changes: moves Outpost's own AppImage along with the apps,
/// if it was in the old apps folder. Returns where it went.
pub fn follow_apps_folder(paths: &Paths, old_folder: &Path) -> Result<Option<PathBuf>, String> {
    let Some(appimage) = running_appimage() else { return Ok(None) };
    let dest = home(paths);
    if appimage.parent() != Some(old_folder) || appimage == dest || !appimage.exists() {
        return Ok(None);
    }
    install::move_file(&appimage, &dest).map_err(|e| format!("couldn't move Outpost to {}: {e}", dest.display()))?;
    *MOVED_TO.lock().unwrap_or_else(|e| e.into_inner()) = Some(dest.clone());
    Ok(Some(dest))
}

/// Whether version `candidate` is newer than `current`, comparing the numbers in
/// them in order, so 1.10 is newer than 1.9.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn numbers(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect()
    }
    numbers(candidate) > numbers(current)
}

/// Downloads the new version next to the running AppImage, checks it, then swaps it
/// in. Linux keeps the running copy open until Outpost exits, so this is safe; the
/// new version starts next time.
pub fn update(appimage: &Path, asset: &Asset, progress: impl FnMut(u64, Option<u64>)) -> Result<(), String> {
    let incoming = appimage.with_file_name(format!(".Outpost-update-{}.AppImage", process::id()));
    let result = download::download(&asset.url, &incoming, progress).and_then(|actual| {
        if let Some(expected) = &asset.sha256
            && expected.to_lowercase() != actual
        {
            return Err("the download doesn't match the checksum in the catalogue, so Outpost wasn't updated".into());
        }
        fs::set_permissions(&incoming, fs::Permissions::from_mode(0o755))
            .and_then(|()| fs::rename(&incoming, appimage))
            .map_err(|e| format!("couldn't replace {}: {e}", appimage.display()))
    });
    if result.is_err() {
        let _ = fs::remove_file(&incoming);
    }
    result
}

/// Moves the running AppImage into ~/Applications as Outpost.AppImage.
pub fn move_home(paths: &Paths, appimage: &Path) -> Result<PathBuf, String> {
    let dest = home(paths);
    if appimage == dest {
        return Ok(dest);
    }
    settings::prepare_apps_folder(&paths.applications)?;
    install::move_file(appimage, &dest).map_err(|e| format!("couldn't move Outpost to {}: {e}", dest.display()))?;
    Ok(dest)
}

/// Starts `appimage` once this process has exited. Only one copy of Outpost runs at
/// a time, so starting it straight away would just hand over to the closing copy.
pub fn relaunch_after_exit(appimage: &Path) -> Result<(), String> {
    Command::new("sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#)
        .arg("sh")
        .arg(process::id().to_string())
        .arg(appimage)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't restart Outpost: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_the_appimage_into_applications() {
        let dir = std::env::temp_dir().join(format!("outpost-test-move-{}", process::id()));
        let paths = Paths {
            applications: dir.join("Applications"),
            desktop_entries: dir.join("entries"),
            icons: dir.join("icons"),
            data: dir.join("data"),
            cache: dir.join("cache"),
        };
        let downloaded = dir.join("Downloads/Outpost-x86_64.AppImage");
        fs::create_dir_all(downloaded.parent().unwrap()).unwrap();
        fs::write(&downloaded, "appimage").unwrap();

        let moved = move_home(&paths, &downloaded).unwrap();
        assert_eq!(moved, home(&paths));
        assert!(!downloaded.exists());
        assert_eq!(fs::read_to_string(&moved).unwrap(), "appimage");
        // Moving again from home is a no-op.
        assert_eq!(move_home(&paths, &moved).unwrap(), moved);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.10", "1.9"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }
}
