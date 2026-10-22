//! The Settings window, the About window, and choosing where apps are installed.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Change, Store};
use crate::{install, self_update};
use crate::paths::Paths;
use crate::settings::{self, Settings};

pub fn show(store: &Rc<Store>) {
    let installing = adw::PreferencesGroup::builder()
        .title("Installing Apps")
        .build();
    installing.add(&apps_folder_row(store, "Apps folder"));

    let settings = current_settings();
    let slideshow = adw::PreferencesGroup::builder()
        .title("Featured Slideshow")
        .build();
    let seconds = adw::SpinRow::with_range(
        f64::from(*settings::SLIDE_SECONDS.start()),
        f64::from(*settings::SLIDE_SECONDS.end()),
        1.0,
    );
    seconds.set_title("Seconds per slide");
    seconds.set_subtitle("How long each app shows before the next");
    seconds.set_value(settings.slide_seconds().into());
    {
        let store = store.clone();
        seconds.connect_value_notify(move |row| change_settings(&store, |s| s.slide_seconds = row.value() as u32));
    }
    let count = adw::SpinRow::with_range(
        *settings::RECOMMENDATIONS.start() as f64,
        *settings::RECOMMENDATIONS.end() as f64,
        1.0,
    );
    count.set_title("Number of recommendations");
    count.set_subtitle("Featured apps come first, then popular and newly updated ones");
    count.set_value(settings.recommendations() as f64);
    {
        let store = store.clone();
        count.connect_value_notify(move |row| change_settings(&store, |s| s.recommendations = row.value() as usize));
    }
    slideshow.add(&seconds);
    slideshow.add(&count);

    let about = adw::PreferencesGroup::new();
    let version = adw::ActionRow::builder()
        .title("Outpost version")
        .subtitle(env!("CARGO_PKG_VERSION"))
        .subtitle_selectable(true)
        .build();
    about.add(&version);

    let page = adw::PreferencesPage::new();
    page.add(&installing);
    page.add(&slideshow);
    page.add(&about);
    let dialog = adw::PreferencesDialog::builder().title("Settings").build();
    dialog.add(&page);
    dialog.present(Some(&store.window));
}

pub fn show_about(store: &Store) {
    adw::AboutDialog::builder()
        .application_name("Outpost")
        .application_icon(crate::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Find, install and update AppImages.")
        .website("https://github.com/cmumby/outpost")
        .issue_url("https://github.com/cmumby/outpost/issues")
        .build()
        .present(Some(&store.window));
}

/// A row showing where apps are installed, with a button to change it. Used in
/// Settings and on the welcome screen.
pub fn apps_folder_row(store: &Rc<Store>, title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(friendly_path(&store.paths().applications))
        .use_markup(false)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("outpost-folder-symbolic"));
    let change = gtk::Button::builder()
        .label("Change…")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&change);

    let store = store.clone();
    let shown = row.downgrade();
    change.connect_clicked(move |_| {
        let shown = shown.clone();
        choose_folder(&store, move |folder| {
            if let Some(row) = shown.upgrade() {
                row.set_subtitle(&friendly_path(folder));
            }
        });
    });
    row
}

/// Asks for a folder, then switches to it. `changed` runs once it's in use.
fn choose_folder(store: &Rc<Store>, changed: impl Fn(&Path) + 'static) {
    let dialog = gtk::FileDialog::builder()
        .title("Choose Where to Install Apps")
        .accept_label("Use This Folder")
        .modal(true)
        .build();
    let current = store.paths().applications;
    // Start from the current folder, or the nearest one above it that exists.
    if let Some(existing) = current.ancestors().find(|dir| dir.is_dir()) {
        dialog.set_initial_folder(Some(&gio::File::for_path(existing)));
    }
    let store = store.clone();
    dialog.select_folder(Some(&store.window.clone()), gio::Cancellable::NONE, move |result| {
        // Cancelling the dialog is also an error here; there's nothing to do.
        if let Ok(folder) = result
            && let Some(path) = folder.path()
        {
            change_apps_folder(&store, path, changed);
        }
    });
}

/// Checks the folder, saves the setting, and offers to move apps already installed.
fn change_apps_folder(store: &Rc<Store>, folder: PathBuf, changed: impl Fn(&Path) + 'static) {
    let old = store.paths().applications;
    if folder == old {
        return;
    }
    if let Err(e) = settings::prepare_apps_folder(&folder) {
        store.alert("Can't Use This Folder", &e);
        return;
    }
    if let Err(e) = save_apps_folder(&folder) {
        store.alert("Couldn't Save Settings", &e);
        return;
    }
    store.set_apps_folder(folder.clone());
    changed(&folder);

    gather_apps(store, Some(old));
}

/// Moves every installed app that isn't in the apps folder into it, and Outpost's own
/// AppImage too if it was in `old_folder` (the folder being left). Runs at startup
/// as well, so editing the setting in settings.json by hand works the same way.
pub fn gather_apps(store: &Rc<Store>, old_folder: Option<PathBuf>) {
    let folder = store.paths().applications;
    let outside = |path: &Path| path.parent() != Some(folder.as_path()) && path.exists();
    let apps_outside = store.installed().iter().any(|app| outside(&app.appimage_path));
    let outpost_outside = old_folder.is_some()
        && self_update::running_appimage().is_some_and(|appimage| outside(&appimage));
    if !apps_outside && !outpost_outside {
        if old_folder.is_some() {
            store.toast(&format!("Apps will be installed in {}", friendly_path(&folder)));
        }
        return;
    }

    let store = store.clone();
    glib::spawn_future_local(async move {
        let paths = store.paths();
        let result = gio::spawn_blocking(move || {
            let moved = install::move_apps(&paths, &paths.applications)?;
            let outpost = match old_folder {
                Some(old) => self_update::follow_apps_folder(&paths, &old)?,
                None => None,
            };
            Ok::<_, String>((moved, outpost))
        })
        .await
        .unwrap_or_else(|_| Err("moving apps crashed".into()));
        match result {
            Ok((moved, outpost)) => {
                if outpost.is_some() {
                    // Point Outpost's own launcher entry at its new home.
                    super::register_desktop_entry(&store.paths());
                }
                store.toast(&format!(
                    "Moved {moved} {} to {}",
                    if moved == 1 { "app" } else { "apps" },
                    friendly_path(&folder)
                ));
            }
            Err(e) => store.alert("Couldn't Move All Your Apps", &capitalize(&e)),
        }
        store.reload_installed();
    });
}

/// Remembers the choice; the default folder is saved as "no choice", so it follows
/// the home folder if that ever moves.
/// The saved settings, or the defaults if there are none.
pub fn current_settings() -> Settings {
    Paths::settings_file().map(|file| Settings::load(&file)).unwrap_or_default()
}

/// Changes and saves a setting, then tells the pages that show it.
pub fn change_settings(store: &Store, change: impl FnOnce(&mut Settings)) {
    let saved = Paths::settings_file().and_then(|file| {
        let mut settings = Settings::load(&file);
        change(&mut settings);
        settings.save(&file)
    });
    match saved {
        Ok(()) => store.notify(Change::Settings),
        Err(e) => store.toast(&format!("Couldn't save settings: {e}")),
    }
}

fn save_apps_folder(folder: &Path) -> Result<(), String> {
    let file = Paths::settings_file()?;
    let mut settings = Settings::load(&file);
    settings.apps_folder = (Paths::default_apps_folder().ok().as_deref() != Some(folder)).then(|| folder.to_path_buf());
    settings.save(&file)
}

/// Shows paths in the home folder as ~/…, which is shorter and easier to read.
pub fn friendly_path(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display()).trim_end_matches('/').to_string()
        }
        _ => path.display().to_string(),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
