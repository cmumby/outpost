//! The Installed tab: apps installed through Outpost.

use std::rc::Rc;

use adw::prelude::*;

use super::{Change, Store, icons};
use crate::state::InstalledApp;

pub fn build(store: &Rc<Store>) -> gtk::Stack {
    let stack = gtk::Stack::new();
    stack.add_named(
        &adw::StatusPage::builder()
            .icon_name("system-software-install-symbolic")
            .title("Nothing Installed Yet")
            .description("Apps you install from Explore will show up here")
            .build(),
        Some("empty"),
    );

    let group = adw::PreferencesGroup::new();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&group);
    stack.add_named(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&adw::Clamp::builder().maximum_size(800).child(&content).build())
            .build(),
        Some("list"),
    );

    let rows: Rc<std::cell::RefCell<Vec<adw::ActionRow>>> = Rc::default();
    let refresh = {
        let store = Rc::downgrade(store);
        let stack = stack.clone();
        move |change: Change| {
            let Some(store) = store.upgrade() else { return false };
            if change != Change::Installed {
                return true;
            }
            for row in rows.take() {
                group.remove(&row);
            }
            let mut apps = store.installed();
            apps.sort_by_key(|app| app.name.to_lowercase());
            for app in &apps {
                let row = app_row(&store, app);
                group.add(&row);
                rows.borrow_mut().push(row);
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
        .icon_name("user-trash-symbolic")
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

    // Apps from the store link to their store page.
    if let Some(listing) = store.listing_for(app) {
        row.set_activatable(true);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let store = store.clone();
        row.connect_activated(move |_| store.show_app(&listing));
    }
    row
}
