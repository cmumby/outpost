//! The Installed tab: apps installed through Outpost.

use std::rc::Rc;

use adw::prelude::*;

use super::{Change, Store, icons};
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
            stack.set_visible_child_name(if apps.is_empty() { "empty" } else { "list" });
            true
        }
    };
    refresh(Change::Installed);
    store.watch(refresh);
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
