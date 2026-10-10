use std::path::PathBuf;

use directories::BaseDirs;

/// Where Outpost puts things. Everything lives under the user's home directory.
pub struct Paths {
    /// `~/Applications` — the AppImages themselves.
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
        Ok(Self {
            applications: base.home_dir().join("Applications"),
            desktop_entries: base.data_dir().join("applications"),
            icons: base.data_dir().join("icons/hicolor"),
            data: base.data_dir().join("outpost"),
            cache: base.cache_dir().join("outpost"),
        })
    }

    pub fn installed_json(&self) -> PathBuf {
        self.data.join("installed.json")
    }
}
