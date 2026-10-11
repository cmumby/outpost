//! The store's catalogue: downloading `index.json` and finding apps in it.

use std::collections::HashMap;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{ETAG, IF_NONE_MATCH};
use serde::Deserialize;

use crate::download;
use crate::paths::Paths;

pub const INDEX_URL: &str = "https://cmumby.github.io/outpost-index/index.json";

/// The newest index format this version of Outpost understands.
const SUPPORTED_SCHEMA: u32 = 1;

pub struct Index {
    pub apps: Vec<App>,
    /// False when this is the saved copy, because nothing changed or we're offline.
    pub fresh: bool,
}

/// The file as published. Apps are read one by one in `parse`, so a single broken
/// entry can't stop the whole store from loading.
#[derive(Deserialize)]
struct RawIndex {
    schema_version: u32,
    apps: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub icon_url: Option<String>,
    /// Added after the first release of the index, so older files may lack it.
    #[serde(default)]
    pub screenshot_urls: Vec<String>,
    pub categories: Vec<String>,
    pub source_repo: String,
    pub verified: bool,
    /// Picked by hand to appear in Featured.
    #[serde(default)]
    pub featured: bool,
    /// `curated` or `appimagehub`.
    pub origin: String,
    pub latest_version: String,
    /// When the latest version was released, as an ISO 8601 timestamp.
    #[serde(default)]
    pub released_at: Option<String>,
    /// The source repo's GitHub stars.
    #[serde(default)]
    pub stars: u64,
    /// Keyed by architecture, e.g. `x86_64`.
    pub assets: HashMap<String, Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub url: String,
    pub sha256: Option<String>,
    pub size: u64,
}

impl App {
    /// The download for this computer's architecture, if the app has one.
    pub fn asset(&self) -> Option<&Asset> {
        self.assets.get(std::env::consts::ARCH)
    }

    /// Curated or followed: apps we vouch for, rather than AppImageHub's open listing.
    pub fn is_trusted(&self) -> bool {
        self.origin != "appimagehub"
    }
}

/// Downloads the index, keeping a copy so the store still opens offline. Sends the
/// last ETag, so when nothing has changed the server replies with a tiny "not modified".
pub fn fetch(paths: &Paths) -> Result<Index, String> {
    let cached = paths.cache.join("index.json");
    let etag_file = paths.cache.join("index.etag");

    let mut request = download::client().get(INDEX_URL).timeout(Duration::from_secs(60));
    if cached.exists()
        && let Ok(etag) = fs::read_to_string(&etag_file)
    {
        request = request.header(IF_NONE_MATCH, etag.trim());
    }

    let fresh = match request.send() {
        Ok(response) if response.status() == StatusCode::NOT_MODIFIED => None,
        Ok(response) if response.status().is_success() => {
            let etag = response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
            match response.bytes() {
                Ok(body) => Some((body, etag)),
                Err(e) => return load_cached(paths, &format!("download interrupted: {}", download::describe(&e))),
            }
        }
        Ok(response) => return load_cached(paths, &format!("the store's server replied {}", response.status())),
        Err(e) => return load_cached(paths, &format!("couldn't reach the store: {}", download::describe(&e))),
    };

    let Some((body, etag)) = fresh else {
        return load_cached(paths, "the saved copy disappeared");
    };
    let mut index = parse(&body)?;
    index.fresh = true;
    let _ = fs::create_dir_all(&paths.cache);
    let _ = fs::write(&cached, &body);
    match etag {
        Some(etag) => {
            let _ = fs::write(&etag_file, etag);
        }
        None => {
            let _ = fs::remove_file(&etag_file);
        }
    }
    Ok(index)
}

/// Falls back to the last downloaded index. `why` explains why we couldn't get a new one.
fn load_cached(paths: &Paths, why: &str) -> Result<Index, String> {
    match fs::read(paths.cache.join("index.json")) {
        Ok(body) => parse(&body),
        Err(_) => Err(format!("Couldn't load the app catalogue: {why}")),
    }
}

fn parse(body: &[u8]) -> Result<Index, String> {
    let raw: RawIndex =
        serde_json::from_slice(body).map_err(|e| format!("The app catalogue is damaged: {e}"))?;
    if raw.schema_version > SUPPORTED_SCHEMA {
        return Err("The app catalogue needs a newer version of Outpost.".into());
    }
    let apps = raw
        .apps
        .into_iter()
        .filter_map(|app| serde_json::from_value(app).ok())
        .collect();
    Ok(Index { apps, fresh: false })
}

/// The categories shown on the home page, using the freedesktop.org names apps use.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("AudioVideo", "Audio & Video"),
    ("Development", "Developer Tools"),
    ("Education", "Education"),
    ("Game", "Games"),
    ("Graphics", "Graphics & Photography"),
    ("Network", "Internet"),
    ("Office", "Office"),
    ("Science", "Science"),
    ("System", "System"),
    ("Utility", "Utilities"),
];

pub fn in_category(app: &App, key: &str) -> bool {
    app.categories.iter().any(|category| {
        category == key || (key == "AudioVideo" && (category == "Audio" || category == "Video"))
    })
}

/// Apps with the newest releases first. AppImageHub's open listing is only included
/// once an app has some following (`MIN_STARS`), so New & Updated isn't swamped by
/// every small project that happened to release today.
pub fn newest(apps: &[Rc<App>]) -> Vec<Rc<App>> {
    let mut newest: Vec<_> = apps
        .iter()
        .filter(|app| app.released_at.is_some() && (app.is_trusted() || app.stars >= MIN_STARS))
        .cloned()
        .collect();
    newest.sort_by(|a, b| b.released_at.cmp(&a.released_at));
    newest
}

/// Apps with the most GitHub stars first.
pub fn popular(apps: &[Rc<App>]) -> Vec<Rc<App>> {
    let mut popular = apps.to_vec();
    popular.sort_by(|a, b| b.stars.cmp(&a.stars));
    popular
}

const MIN_STARS: u64 = 100;

/// Apps matching `query`, best matches first: names before summaries, and curated
/// apps before others.
pub fn search(apps: &[Rc<App>], query: &str) -> Vec<Rc<App>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<(u8, &Rc<App>)> = apps
        .iter()
        .filter_map(|app| {
            let name = app.name.to_lowercase();
            let score = if name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else if name.contains(&query) {
                2
            } else if app.summary.to_lowercase().contains(&query) {
                3
            } else {
                return None;
            };
            Some((score, app))
        })
        .collect();
    matches.sort_by_key(|(score, app)| (*score, !app.is_trusted(), app.name.to_lowercase()));
    matches.into_iter().map(|(_, app)| app.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, summary: &str, origin: &str, categories: &[&str]) -> Rc<App> {
        Rc::new(App {
            id: name.to_lowercase(),
            name: name.into(),
            summary: summary.into(),
            description: String::new(),
            icon_url: None,
            screenshot_urls: Vec::new(),
            categories: categories.iter().map(|c| c.to_string()).collect(),
            source_repo: String::new(),
            verified: false,
            featured: false,
            origin: origin.into(),
            latest_version: "1.0".into(),
            released_at: None,
            stars: 0,
            assets: HashMap::new(),
        })
    }

    #[test]
    fn search_ranks_name_matches_and_curated_apps_first() {
        let apps = [
            app("Photo Viewer", "", "appimagehub", &[]),
            app("Editor", "Edit photos", "appimagehub", &[]),
            app("Photocraft", "", "curated", &[]),
            app("Photo", "", "appimagehub", &[]),
            app("Unrelated", "", "curated", &[]),
        ];
        let results = search(&apps, " photo");
        let names: Vec<_> = results.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Photo", "Photocraft", "Photo Viewer", "Editor"]);
        assert!(search(&apps, "  ").is_empty());
    }

    #[test]
    fn sections_sort_by_release_date_and_stars() {
        let mut quiet_hub = app("Quiet", "", "appimagehub", &[]);
        let mut known_hub = app("Known", "", "appimagehub", &[]);
        let mut followed = app("Followed", "", "followed", &[]);
        Rc::get_mut(&mut quiet_hub).unwrap().released_at = Some("2026-10-12T00:00:00Z".into());
        Rc::get_mut(&mut known_hub).unwrap().released_at = Some("2026-10-10T00:00:00Z".into());
        Rc::get_mut(&mut known_hub).unwrap().stars = 500;
        Rc::get_mut(&mut followed).unwrap().released_at = Some("2026-10-11T00:00:00Z".into());
        Rc::get_mut(&mut followed).unwrap().stars = 50;
        let apps = [quiet_hub, known_hub, followed];

        let names = |list: Vec<Rc<App>>| list.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        // The newest release is from an AppImageHub app with few stars, so it's left out.
        assert_eq!(names(newest(&apps)), ["Followed", "Known"]);
        assert_eq!(names(popular(&apps)), ["Known", "Followed", "Quiet"]);
    }

    #[test]
    fn audio_and_video_count_as_audio_video() {
        assert!(in_category(&app("A", "", "", &["Audio"]), "AudioVideo"));
        assert!(!in_category(&app("A", "", "", &["Audio"]), "Graphics"));
    }

    /// Talks to the real server, so it only runs when asked:
    /// `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn fetches_the_live_index_and_reuses_it() {
        let dir = std::env::temp_dir().join(format!("outpost-test-{}", std::process::id()));
        let paths = Paths {
            applications: dir.join("apps"),
            desktop_entries: dir.join("entries"),
            icons: dir.join("icons"),
            data: dir.join("data"),
            cache: dir.join("cache"),
        };
        let first = fetch(&paths).unwrap();
        assert!(first.fresh);
        assert!(first.apps.len() > 100);
        assert!(paths.cache.join("index.etag").exists());
        // The second fetch should get "not modified" and load the saved copy.
        let second = fetch(&paths).unwrap();
        assert!(!second.fresh);
        assert_eq!(second.apps.len(), first.apps.len());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_the_published_format() {
        let json = br#"{"schema_version": 1, "generated_at": "x", "apps": [{
            "id": "a", "name": "A", "summary": "s", "description": "d", "icon_url": null,
            "categories": [], "source_repo": "r", "verified": true, "origin": "curated",
            "latest_version": "1", "assets": {"x86_64": {"url": "u", "sha256": null, "size": 1}},
            "some_future_field": true}]}"#;
        assert_eq!(parse(json).unwrap().apps[0].assets["x86_64"].url, "u");
        assert!(parse(br#"{"schema_version": 99, "apps": []}"#).is_err());
        // A broken entry is skipped, not fatal.
        assert!(parse(br#"{"schema_version": 1, "apps": [{"id": null}]}"#).unwrap().apps.is_empty());
    }
}
