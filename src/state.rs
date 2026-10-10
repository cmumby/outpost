use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

/// One app Outpost has installed, as recorded in `installed.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub appimage_path: PathBuf,
    pub desktop_path: PathBuf,
    pub icon_path: Option<PathBuf>,
    /// Seconds since the Unix epoch.
    pub installed_at: u64,
    /// Where it was installed from (a local file path for now; later a URL or index entry).
    pub source: String,
    /// The app's id in the store's index, when it was installed from the store.
    #[serde(default)]
    pub index_id: Option<String>,
}

/// The contents of `~/.local/share/outpost/installed.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct InstalledState {
    pub schema_version: u32,
    pub apps: Vec<InstalledApp>,
}

impl InstalledState {
    /// Loads the state file, or starts empty if it doesn't exist yet.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("{} is not valid: {e}", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Self {
                schema_version: SCHEMA_VERSION,
                apps: Vec::new(),
            }),
            Err(e) => Err(format!("could not read {}: {e}", path.display())),
        }
    }

    /// Writes to a temporary file and renames it over the old one, so a crash
    /// mid-write never leaves a half-written state file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).expect("state always serializes");
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path).map_err(|e| format!("could not write {}: {e}", path.display()))
    }

    pub fn find(&self, id: &str) -> Option<&InstalledApp> {
        self.apps.iter().find(|app| app.id == id)
    }
}
