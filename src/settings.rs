//! User settings, in ~/.config/outpost/settings.json, and checks on the apps folder.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Where apps are installed. `None` means the default, ~/Applications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apps_folder: Option<PathBuf>,
}

impl Settings {
    /// Missing or unreadable settings mean the defaults.
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> Result<(), String> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(self).expect("settings always serialize");
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, json)
            .and_then(|()| fs::rename(&tmp, file))
            .map_err(|e| format!("couldn't save settings to {}: {e}", file.display()))
    }
}

/// Makes sure `dir` can hold apps: creates it if needed, then checks Outpost can save
/// files there and that programs can run from it. Errors are full sentences meant
/// for the user.
pub fn prepare_apps_folder(dir: &Path) -> Result<(), String> {
    let shown = dir.display();
    if !dir.is_absolute() {
        return Err(format!("{shown} isn't a full folder path."));
    }
    fs::create_dir_all(dir).map_err(|e| match e.kind() {
        ErrorKind::PermissionDenied => format!(
            "You don't have permission to create {shown}. Choose a folder inside your home folder."
        ),
        ErrorKind::ReadOnlyFilesystem => format!("{shown} is on a drive that can't be written to."),
        _ => format!("Couldn't create {shown}: {e}."),
    })?;

    // Save a small file, mark it as a program, and check the mark stuck. Each check
    // gets its own file, since several installs can check the folder at once.
    static CHECKS: AtomicU64 = AtomicU64::new(0);
    let check = CHECKS.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!(".outpost-check-{}-{check}", process::id()));
    let checked = fs::write(&probe, "#!/bin/sh\n")
        .map_err(|e| match e.kind() {
            ErrorKind::PermissionDenied => format!(
                "You don't have permission to save files in {shown}. Choose a folder you own, such as one inside your home folder."
            ),
            ErrorKind::ReadOnlyFilesystem => format!("{shown} is on a drive that can't be written to."),
            _ => format!("Couldn't save files in {shown}: {e}."),
        })
        .and_then(|()| {
            fs::set_permissions(&probe, fs::Permissions::from_mode(0o755))
                .and_then(|()| fs::metadata(&probe))
                .map_err(|e| format!("Couldn't check {shown}: {e}."))
        })
        .and_then(|metadata| {
            if metadata.permissions().mode() & 0o100 == 0 {
                Err(format!(
                    "{shown} is on a drive that can't mark files as programs (for example, a USB stick formatted for Windows), so apps installed there couldn't run."
                ))
            } else {
                Ok(())
            }
        });
    let _ = fs::remove_file(&probe);
    checked?;

    if mounted_noexec(dir) {
        return Err(format!(
            "{shown} is on a drive that doesn't allow running programs (it's mounted with \"noexec\"), so apps installed there couldn't run. Choose a different folder."
        ));
    }
    Ok(())
}

/// Whether the drive holding `dir` is mounted with programs switched off.
fn mounted_noexec(dir: &Path) -> bool {
    let (Ok(dir), Ok(mounts)) = (dir.canonicalize(), fs::read_to_string("/proc/self/mounts")) else {
        return false;
    };
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let mount_point = PathBuf::from(unescape_mount_path(fields.nth(1)?));
            let options = fields.nth(1)?;
            Some((mount_point, options))
        })
        .filter(|(mount_point, _)| dir.starts_with(mount_point))
        // The deepest mount point containing the folder is the one it's on.
        .max_by_key(|(mount_point, _)| mount_point.as_os_str().len())
        .is_some_and(|(_, options)| options.split(',').any(|option| option == "noexec"))
}

/// /proc/self/mounts writes spaces and a few other characters as octal escapes.
fn unescape_mount_path(path: &str) -> String {
    path.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("outpost-test-settings-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn creates_a_missing_apps_folder() {
        let dir = scratch("create");
        let apps = dir.join("deep/Applications");
        prepare_apps_folder(&apps).unwrap();
        assert!(apps.is_dir());
        // The check leaves nothing behind.
        assert_eq!(fs::read_dir(&apps).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn explains_a_folder_without_permission() {
        let dir = scratch("locked");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let error = prepare_apps_folder(&dir.join("Applications")).unwrap_err();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(dir);
        // (Running as root would ignore the permissions; this is for normal users.)
        assert!(error.contains("permission"), "{error}");
    }

    #[test]
    fn checks_running_at_once_dont_trip_each_other() {
        // "Update All" checks the folder once per app, all at the same time.
        let dir = scratch("together");
        let errors: Vec<String> = (0..8)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || (0..50).filter_map(|_| prepare_apps_folder(&dir).err()).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect();
        let _ = fs::remove_dir_all(dir);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(prepare_apps_folder(Path::new("Applications")).is_err());
    }

    #[test]
    fn saves_and_loads_settings() {
        let dir = scratch("save");
        let file = dir.join("settings.json");
        assert!(Settings::load(&file).apps_folder.is_none());
        let settings = Settings { apps_folder: Some(PathBuf::from("/mnt/apps")) };
        settings.save(&file).unwrap();
        assert_eq!(Settings::load(&file).apps_folder, Some(PathBuf::from("/mnt/apps")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unescapes_mount_paths() {
        assert_eq!(unescape_mount_path("/media/me/My\\040Drive"), "/media/me/My Drive");
    }
}
