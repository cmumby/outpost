//! App icons and screenshots from the catalogue, downloaded on demand and cached on disk.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::download;

const PLACEHOLDER: &str = "application-x-executable";

thread_local! {
    /// Icons already loaded this session, so scrolling back doesn't reload them.
    static LOADED: RefCell<HashMap<String, gdk::Texture>> = RefCell::default();
}

/// Shows the icon at `url` in `image`, with a generic icon until it arrives.
/// `still_wanted` is checked when it does: list rows get reused while scrolling, so
/// by then the row may be showing a different app.
pub fn show(image: &gtk::Image, url: Option<&str>, cache: &Path, still_wanted: impl Fn() -> bool + 'static) {
    image.set_icon_name(Some(PLACEHOLDER));
    let Some(url) = url else { return };
    if let Some(texture) = LOADED.with_borrow(|loaded| loaded.get(url).cloned()) {
        image.set_paintable(Some(&texture));
        return;
    }
    let image = image.downgrade();
    let remember = url.to_string();
    load(url, &cache.join("icons"), move |texture| {
        LOADED.with_borrow_mut(|loaded| loaded.insert(remember, texture.clone()));
        if let Some(image) = image.upgrade()
            && still_wanted()
        {
            image.set_paintable(Some(&texture));
        }
    });
}

/// Shows a screenshot in `picture` once it has downloaded. Screenshots are cached on
/// disk only; they're too big to keep in memory like icons.
pub fn show_screenshot(picture: &gtk::Picture, url: &str, cache: &Path) {
    let picture = picture.downgrade();
    load(url, &cache.join("screenshots"), move |texture| {
        if let Some(picture) = picture.upgrade() {
            picture.set_paintable(Some(&texture));
        }
    });
}

/// Fetches an image on a background thread (or from the disk cache in `dir`) and
/// hands it to `loaded` on the main thread. Failures are silent: a missing image
/// just stays as its placeholder.
fn load(url: &str, dir: &Path, loaded: impl FnOnce(gdk::Texture) + 'static) {
    let url = url.to_string();
    let file = dir.join(file_name(&url));
    glib::spawn_future_local(async move {
        let Ok(Some(bytes)) = gio::spawn_blocking(move || fetch(&url, &file)).await else {
            return;
        };
        if let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
            loaded(texture);
        }
    });
}

/// Shows an icon from a local file, such as an installed app's.
pub fn show_file(image: &gtk::Image, path: Option<&Path>) {
    match path.and_then(|path| gdk::Texture::from_filename(path).ok()) {
        Some(texture) => image.set_paintable(Some(&texture)),
        None => image.set_icon_name(Some(PLACEHOLDER)),
    }
}

fn fetch(url: &str, file: &PathBuf) -> Option<Vec<u8>> {
    if let Ok(bytes) = fs::read(file) {
        return Some(bytes);
    }
    let bytes = download::client()
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.bytes())
        .ok()?
        .to_vec();
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(file, &bytes);
    Some(bytes)
}

fn file_name(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
