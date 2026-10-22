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
    /// GitHub downloads of the latest version's AppImages, by anyone. `None` in
    /// catalogues from before this was collected.
    #[serde(default)]
    pub downloads: Option<u64>,
    /// The latest release's GitHub page.
    #[serde(default)]
    pub release_url: Option<String>,
    /// Whether notes/<id>.md has the latest release's notes. (Outpost tries the file
    /// either way; see `release_notes_url`.)
    #[serde(default)]
    #[allow(dead_code)]
    pub has_release_notes: bool,
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

/// Where an app's release notes are published, next to index.json. Always worth
/// trying: a catalogue saved before the notes were published doesn't know about
/// them, and a missing file just means there are no notes.
pub fn release_notes_url(app: &App) -> Option<String> {
    let base = INDEX_URL.rsplit_once('/')?.0;
    Some(format!("{base}/notes/{}.md", app.id))
}

/// A small text file from the web, or `None` if it couldn't be fetched.
pub fn fetch_text(url: &str) -> Option<String> {
    download::client()
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.text())
        .ok()
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

/// A count in a few characters: 950, 12.4k, 1.2M.
pub fn short_count(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..10_000 => format!("{:.1}k", count as f64 / 1_000.0).replace(".0k", "k"),
        10_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0).replace(".0M", "M"),
    }
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

/// Why an app is in the Featured slideshow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick {
    Featured,
    Popular,
    New,
}

/// The Featured slideshow: every hand-picked app first, then the slots left split
/// between Popular and New & Updated (Popular gets the odd one), taking turns. If
/// one list runs out, the other fills in. Slides are built around the app's icon,
/// so those picks need one.
pub fn recommendations(apps: &[Rc<App>], count: usize) -> Vec<(Rc<App>, Pick)> {
    let featured: Vec<_> = apps.iter().filter(|app| app.featured).take(count).cloned().collect();
    let open = count - featured.len();

    let mut taken = featured.clone();
    let mut fill = |list: &[Rc<App>], wanted: usize| -> Vec<Rc<App>> {
        let picked: Vec<_> = list
            .iter()
            .filter(|app| app.icon_url.is_some() && !taken.iter().any(|t| Rc::ptr_eq(t, app)))
            .take(wanted)
            .cloned()
            .collect();
        taken.extend(picked.iter().cloned());
        picked
    };
    let (popular_list, new_list) = (popular(apps), newest(apps));
    let mut popular = fill(&popular_list, open.div_ceil(2));
    let mut new = fill(&new_list, open / 2);
    new.extend(fill(&new_list, open - popular.len() - new.len()));
    popular.extend(fill(&popular_list, open - popular.len() - new.len()));

    let mut picks: Vec<_> = featured.into_iter().map(|app| (app, Pick::Featured)).collect();
    let (mut popular, mut new) = (popular.into_iter(), new.into_iter());
    loop {
        let (p, n) = (popular.next(), new.next());
        if p.is_none() && n.is_none() {
            break;
        }
        picks.extend(p.map(|app| (app, Pick::Popular)));
        picks.extend(n.map(|app| (app, Pick::New)));
    }
    picks
}

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
            downloads: None,
            release_url: None,
            has_release_notes: false,
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

    /// An app with an icon, `stars` stars, released on day `day` of October.
    fn listed(name: &str, stars: u64, day: u32, featured: bool) -> Rc<App> {
        let mut app = app(name, "", "curated", &[]);
        let edit = Rc::get_mut(&mut app).unwrap();
        edit.icon_url = Some("https://example.com/icon.png".into());
        edit.stars = stars;
        edit.released_at = Some(format!("2026-10-{day:02}T00:00:00Z"));
        edit.featured = featured;
        app
    }

    fn picks(apps: &[Rc<App>], count: usize) -> Vec<(String, Pick)> {
        recommendations(apps, count).into_iter().map(|(app, pick)| (app.name.clone(), pick)).collect()
    }

    #[test]
    fn slideshow_fills_up_after_featured_apps() {
        let apps = [
            listed("Photocraft", 10, 1, true),
            listed("Big", 900, 2, false),
            listed("Bigger", 1000, 3, false),
            listed("Fresh", 1, 20, false),
            listed("Fresher", 2, 21, false),
            listed("Middling", 50, 4, false),
        ];
        use Pick::*;
        // One featured, then two popular and two new, taking turns.
        assert_eq!(
            picks(&apps, 5),
            [
                ("Photocraft".into(), Featured),
                ("Bigger".into(), Popular),
                ("Fresher".into(), New),
                ("Big".into(), Popular),
                ("Fresh".into(), New),
            ]
        );
        // Popular gets the odd slot.
        assert_eq!(picks(&apps, 4).iter().filter(|(_, pick)| *pick == Popular).count(), 2);
        // Featured apps always come first, even if there's only room for them.
        assert_eq!(picks(&apps, 1), [("Photocraft".into(), Featured)]);
    }

    #[test]
    fn slideshow_never_repeats_an_app_and_skips_apps_without_icons() {
        let mut no_icon = listed("NoIcon", 5000, 30, false);
        Rc::get_mut(&mut no_icon).unwrap().icon_url = None;
        // "Both" is the most popular and the newest, so it can only appear once, and
        // New & Updated runs out: Popular fills the rest.
        let apps = [no_icon, listed("Both", 1000, 25, false), listed("Old", 500, 1, false), listed("Older", 400, 1, false)];
        let names: Vec<_> = picks(&apps, 10).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["Both", "Old", "Older"]);
    }

    #[test]
    fn shortens_counts() {
        assert_eq!(short_count(950), "950");
        assert_eq!(short_count(1_000), "1k");
        assert_eq!(short_count(1_250), "1.2k");
        assert_eq!(short_count(12_400), "12k");
        assert_eq!(short_count(93_998), "93k");
        assert_eq!(short_count(1_250_000), "1.2M");
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
