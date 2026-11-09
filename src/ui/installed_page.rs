//! The Installed tab: apps installed through Outpost.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use super::{Change, Store, icons, settings_page};
use crate::{install, self_update};
use crate::index::App;
use crate::state::InstalledApp;

pub fn build(store: &Rc<Store>) -> gtk::Stack {
    let stack = gtk::Stack::new();
    stack.add_named(
        &adw::StatusPage::builder()
            .icon_name("outpost-system-software-install-symbolic")
            .title("Nothing Installed Yet")
            .description("Apps you install from Explore will show up here")
            .build(),
        Some("empty"),
    );

    let updates_group = adw::PreferencesGroup::builder().title("Updates").build();
    let update_all = gtk::Button::builder()
        .label("Update All")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    updates_group.set_header_suffix(Some(&update_all));
    {
        let store = Rc::downgrade(store);
        update_all.connect_clicked(move |button| {
            let Some(store) = store.upgrade() else { return };
            button.set_sensitive(false);
            for (_, app) in store.updates() {
                store.install(&app);
            }
        });
    }

    let group = adw::PreferencesGroup::builder().title("Installed").build();

    let found_group = adw::PreferencesGroup::builder()
        .title("Found in Your Apps Folder")
        .description("AppImages Outpost didn't install. Add them to open, update and uninstall them here, and to put them in your launcher.")
        .build();
    let add_all = gtk::Button::builder()
        .label("Add All")
        .valign(gtk::Align::Center)
        .build();
    found_group.set_header_suffix(Some(&add_all));
    {
        let store = Rc::downgrade(store);
        add_all.connect_clicked(move |button| {
            let Some(store) = store.upgrade() else { return };
            button.set_sensitive(false);
            add_files(&store, found_files(&store), false);
        });
    }
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&updates_group);
    content.append(&group);
    content.append(&found_group);
    stack.add_named(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&adw::Clamp::builder().maximum_size(800).child(&content).build())
            .build(),
        Some("list"),
    );

    let rows: Rc<std::cell::RefCell<Vec<(adw::PreferencesGroup, adw::ActionRow)>>> = Rc::default();
    // Redraws each update row's progress.
    let progress_shown: Rc<std::cell::RefCell<Vec<Rc<dyn Fn()>>>> = Rc::default();
    let refresh = {
        let store = Rc::downgrade(store);
        let stack = stack.clone();
        move |change: Change| {
            let Some(store) = store.upgrade() else { return false };
            if change == Change::Progress {
                for show in progress_shown.borrow().iter() {
                    show();
                }
            }
            if change != Change::Installed {
                return true;
            }
            progress_shown.borrow_mut().clear();
            for (group, row) in rows.take() {
                group.remove(&row);
            }

            let updates = store.updates();
            for (installed, app) in &updates {
                let (row, show_progress) = update_row(&store, installed, app);
                progress_shown.borrow_mut().push(show_progress);
                updates_group.add(&row);
                rows.borrow_mut().push((updates_group.clone(), row));
            }
            updates_group.set_visible(!updates.is_empty());
            update_all.set_sensitive(true);

            let mut apps = store.installed();
            apps.sort_by_key(|app| app.name.to_lowercase());
            for app in &apps {
                let row = app_row(&store, app);
                group.add(&row);
                rows.borrow_mut().push((group.clone(), row));
            }

            let found = found_files(&store);
            if settings_page::current_settings().add_found_apps {
                // Each file gets one try per session, so one that can't be added
                // isn't tried again on every refresh.
                let new: Vec<_> = AUTO_TRIED.with_borrow_mut(|tried| {
                    found.iter().filter(|file| tried.insert((*file).clone())).cloned().collect()
                });
                if !new.is_empty() {
                    add_files(&store, new, true);
                }
            }
            for file in &found {
                let row = found_row(&store, file);
                found_group.add(&row);
                rows.borrow_mut().push((found_group.clone(), row));
            }
            found_group.set_visible(!found.is_empty());
            add_all.set_sensitive(true);

            group.set_visible(!apps.is_empty());
            stack.set_visible_child_name(if apps.is_empty() && found.is_empty() { "empty" } else { "list" });
            true
        }
    };
    refresh(Change::Installed);
    store.watch(refresh);
    {
        // Look again each time the tab opens, for AppImages copied in since.
        let store = Rc::downgrade(store);
        stack.connect_map(move |_| {
            if let Some(store) = store.upgrade() {
                store.reload_installed();
            }
        });
    }
    stack
}

fn app_row(store: &Rc<Store>, app: &InstalledApp) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(app.name.as_str())
        .subtitle(app.version.as_deref().map_or_else(String::new, |v| format!("Version {v}")))
        .use_markup(false)
        .build();
    let icon = gtk::Image::builder().pixel_size(48).build();
    icons::show_file(&icon, app.icon_path.as_deref());
    row.add_prefix(&icon);

    let open = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();
    let uninstall = gtk::Button::builder()
        .icon_name("outpost-user-trash-symbolic")
        .tooltip_text("Uninstall")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&open);
    row.add_suffix(&uninstall);

    let (store_, app_) = (store.clone(), app.clone());
    open.connect_clicked(move |_| store_.open(&app_));
    let (store_, app_) = (store.clone(), app.clone());
    uninstall.connect_clicked(move |_| store_.uninstall(&app_));

    // Apps from the store link to their store page. Others keep an invisible arrow,
    // so every row's buttons line up.
    let arrow = gtk::Image::from_icon_name("outpost-go-next-symbolic");
    row.add_suffix(&arrow);
    match store.listing_for(app) {
        Some(listing) => {
            row.set_activatable(true);
            let store = store.clone();
            row.connect_activated(move |_| store.show_app(&listing));
        }
        None => arrow.set_opacity(0.0),
    }
    row
}

/// A row under Updates, and a function that shows its download progress.
fn update_row(store: &Rc<Store>, installed: &InstalledApp, app: &Rc<App>) -> (adw::ActionRow, Rc<dyn Fn()>) {
    let versions = format!("{} → {}", installed.version.as_deref().unwrap_or("?"), app.latest_version);
    let row = adw::ActionRow::builder()
        .title(installed.name.as_str())
        .subtitle(versions.as_str())
        .use_markup(false)
        .activatable(true)
        .tooltip_text("See what's new")
        .build();
    let icon = gtk::Image::builder().pixel_size(48).build();
    icons::show_file(&icon, installed.icon_path.as_deref());
    row.add_prefix(&icon);
    {
        let (store, installed, app) = (store.clone(), installed.clone(), app.clone());
        row.connect_activated(move |_| super::whats_new::show(&store, &installed, &app));
    }

    // While the app updates, a progress bar takes the Update button's place. Both
    // sit in a slot of fixed width, so the rows stay lined up.
    let update = gtk::Button::builder()
        .label("Update")
        .valign(gtk::Align::Center)
        .halign(gtk::Align::End)
        .hexpand(true)
        .build();
    let bar = gtk::ProgressBar::builder()
        .valign(gtk::Align::Center)
        .hexpand(true)
        .visible(false)
        .build();
    let slot = gtk::Box::builder().width_request(140).build();
    slot.append(&bar);
    slot.append(&update);
    row.add_suffix(&slot);
    {
        let (store, app) = (store.clone(), app.clone());
        update.connect_clicked(move |_| store.install(&app));
    }

    let show_progress = {
        let (store, app, row) = (Rc::downgrade(store), app.clone(), row.clone());
        move || {
            let Some(store) = store.upgrade() else { return };
            let Some(progress) = store.progress(&app) else {
                bar.set_visible(false);
                update.set_visible(true);
                row.set_subtitle(&versions);
                return;
            };
            let status = match progress.fraction() {
                _ if progress.finished_downloading() => {
                    bar.set_fraction(1.0);
                    "Installing…".to_string()
                }
                Some(fraction) => {
                    bar.set_fraction(fraction);
                    format!("Downloading… {:.0}%", fraction * 100.0)
                }
                None => {
                    bar.pulse();
                    "Downloading…".to_string()
                }
            };
            update.set_visible(false);
            bar.set_visible(true);
            row.set_subtitle(&format!("{versions} · {status}"));
        }
    };
    show_progress();
    (row, Rc::new(show_progress))
}

/// AppImages in the apps folder Outpost didn't install, apart from Outpost itself
/// and ones the user hid.
fn found_files(store: &Store) -> Vec<PathBuf> {
    let paths = store.paths();
    let mut skip = settings_page::current_settings().hidden_files;
    skip.push(self_update::home(&paths));
    skip.extend(self_update::running_appimage());
    install::untracked(&paths, &skip)
}

/// A found file's name and icon, once read: `None` if it couldn't be.
type Preview = Option<(String, Option<gdk::Texture>)>;

thread_local! {
    /// Files already added automatically, or tried and failed, this session.
    static AUTO_TRIED: RefCell<std::collections::HashSet<PathBuf>> = RefCell::default();
    /// Why automatic adding failed, by file, shown on its row instead of an alert.
    static AUTO_FAILED: RefCell<HashMap<PathBuf, String>> = RefCell::default();
    /// Previews already read, by file and when it last changed, so reopening the
    /// tab doesn't run every file again.
    static PREVIEWS: RefCell<HashMap<(PathBuf, Option<std::time::SystemTime>), Preview>> = RefCell::default();
}

fn found_row(store: &Rc<Store>, file: &Path) -> adw::ActionRow {
    let file_name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let status = AUTO_FAILED
        .with_borrow(|failed| failed.get(file).map(|e| format!("Couldn't add it: {e}")))
        .unwrap_or_else(|| "Not added to Outpost".to_string());
    let row = adw::ActionRow::builder()
        .title(file_name.as_str())
        .subtitle(status.as_str())
        .use_markup(false)
        .build();
    let icon = gtk::Image::builder()
        .icon_name("outpost-application-x-executable-symbolic")
        .pixel_size(48)
        .css_classes(["dim-label"])
        .build();
    row.add_prefix(&icon);

    // Show the app's own name and icon once they've been read from inside it.
    let show = {
        let (row, icon) = (row.downgrade(), icon.downgrade());
        move |preview: &Preview| {
            let (Some(row), Some(icon), Some((name, texture))) = (row.upgrade(), icon.upgrade(), preview) else {
                return;
            };
            row.set_title(name);
            row.set_subtitle(&format!("{file_name} · {status}"));
            if let Some(texture) = texture {
                icon.set_paintable(Some(texture));
                icon.remove_css_class("dim-label");
            }
        }
    };
    let key = (file.to_path_buf(), std::fs::metadata(file).and_then(|meta| meta.modified()).ok());
    match PREVIEWS.with_borrow(|previews| previews.get(&key).cloned()) {
        Some(preview) => show(&preview),
        None => {
            let (paths, file) = (store.paths(), file.to_path_buf());
            glib::spawn_future_local(async move {
                let found = gio::spawn_blocking(move || install::preview(&paths, &file)).await.ok().flatten();
                let preview = found.map(|(name, icon)| {
                    let texture = icon.and_then(|bytes| gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok());
                    (name, texture)
                });
                show(&preview);
                PREVIEWS.with_borrow_mut(|previews| previews.insert(key, preview));
            });
        }
    }

    let add = gtk::Button::builder()
        .label("Add")
        .valign(gtk::Align::Center)
        .build();
    let hide = gtk::Button::builder()
        .label("Hide")
        .tooltip_text("Leave this file alone and stop listing it")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&hide);
    row.add_suffix(&add);
    // Matches the arrow on installed rows, so the buttons line up.
    let spacer = gtk::Image::from_icon_name("outpost-go-next-symbolic");
    spacer.set_opacity(0.0);
    row.add_suffix(&spacer);

    {
        let (store, file) = (store.clone(), file.to_path_buf());
        add.connect_clicked(move |button| {
            button.set_sensitive(false);
            button.set_label("Adding…");
            add_files(&store, vec![file.clone()], false);
        });
    }
    {
        let (store, file) = (store.clone(), file.to_path_buf());
        hide.connect_clicked(move |_| {
            settings_page::change_settings(&store, |settings| settings.hidden_files.push(file.clone()));
            store.reload_installed();
        });
    }
    row
}

/// Adds `files` to Outpost one after another in the background, then says how it
/// went. `automatic` adds (from the setting) put problems on the file's row
/// instead of in an alert.
fn add_files(store: &Rc<Store>, files: Vec<PathBuf>, automatic: bool) {
    // Store ids by app name, so an added app that's also in the store gets its
    // page and updates. Hand-picked and followed apps win when names clash.
    let mut listings: HashMap<String, (String, bool)> = HashMap::new();
    for app in store.apps().iter() {
        let key = app.name.to_lowercase();
        if listings.get(&key).is_none_or(|(_, trusted)| !trusted && app.is_trusted()) {
            listings.insert(key, (app.id.clone(), app.is_trusted()));
        }
    }
    let store = store.clone();
    glib::spawn_future_local(async move {
        let paths = store.paths();
        let results = gio::spawn_blocking(move || {
            files
                .into_iter()
                .map(|file| {
                    let result = install::add_existing(&paths, &file, |name| {
                        listings.get(&name.to_lowercase()).map(|(id, _)| id.clone())
                    });
                    (file, result)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();

        let mut added = Vec::new();
        let mut failed = Vec::new();
        for (file, result) in results {
            match result {
                Ok(app) => {
                    AUTO_FAILED.with_borrow_mut(|problems| problems.remove(&file));
                    added.push(app.name);
                }
                Err(e) if automatic => {
                    AUTO_FAILED.with_borrow_mut(|problems| problems.insert(file, e));
                }
                Err(e) => failed.push(e),
            }
        }
        match added.as_slice() {
            [] => {}
            [one] => store.toast(&format!("{one} was added")),
            many => store.toast(&format!("{} apps were added", many.len())),
        }
        if !failed.is_empty() {
            let heading = if failed.len() == 1 { "Couldn't Add an App" } else { "Couldn't Add Some Apps" };
            store.alert(heading, &failed.iter().map(|e| super::actions::capitalize(e)).collect::<Vec<_>>().join("\n\n"));
        }
        store.reload_installed();
    });
}
