use std::path::PathBuf;

use directories::BaseDirs;

use crate::settings::Settings;

/// Where Outpost puts things. Everything lives under the user's home directory,
/// apart from the apps folder, which the user can move anywhere they can write to.
#[derive(Clone)]
pub struct Paths {
    /// Where apps are installed: `~/Applications` unless changed in Settings.
    pub applications: PathBuf,
    /// `~/.local/share/applications` — desktop entries the launcher reads.
    pub desktop_entries: PathBuf,
    /// `~/.local/share/icons/hicolor` — icons the launcher can find by name.
    pub icons: PathBuf,
    /// `~/.local/share/outpost` — Outpost's own data.
    pub data: PathBuf,
    /// `~/.cache/outpost` — scratch space, safe to delete.
    pub cache: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self, String> {
        let base = BaseDirs::new().ok_or("could not find your home directory")?;
        let settings = Settings::load(&settings_file(&base));
        Ok(Self {
            applications: settings.apps_folder.unwrap_or_else(|| default_apps_folder(&base)),
            desktop_entries: base.data_dir().join("applications"),
            icons: base.data_dir().join("icons/hicolor"),
            data: base.data_dir().join("outpost"),
            cache: base.cache_dir().join("outpost"),
        })
    }

    pub fn installed_json(&self) -> PathBuf {
        self.data.join("installed.json")
    }

    /// `~/.config/outpost/settings.json`.
    pub fn settings_file() -> Result<PathBuf, String> {
        Ok(settings_file(&BaseDirs::new().ok_or("could not find your home directory")?))
    }

    /// `~/Applications`, where apps go unless the user picks another folder.
    pub fn default_apps_folder() -> Result<PathBuf, String> {
        Ok(default_apps_folder(&BaseDirs::new().ok_or("could not find your home directory")?))
    }
}

fn settings_file(base: &BaseDirs) -> PathBuf {
    base.config_dir().join("outpost/settings.json")
}

fn default_apps_folder(base: &BaseDirs) -> PathBuf {
    base.home_dir().join("Applications")
}
