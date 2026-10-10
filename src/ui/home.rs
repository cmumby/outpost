//! The Explore tab's front page: featured apps, then categories.

use std::rc::Rc;

use adw::prelude::*;

use super::{Store, app_list, icons};
use crate::index::{self, App};

pub fn build(store: &Rc<Store>) -> gtk::ScrolledWindow {
    let apps = store.apps();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();

    let featured = adw::PreferencesGroup::builder()
        .title("Featured")
        .description("Hand-picked apps we've checked come from their official source")
        .build();
    for app in apps.iter().filter(|app| app.is_curated()) {
        featured.add(&app_row(store, app));
    }
    content.append(&featured);

    let categories = adw::PreferencesGroup::builder().title("Categories").build();
    for &(key, title) in index::CATEGORIES {
        let count = apps.iter().filter(|app| index::in_category(app, key)).count();
        if count == 0 {
            continue;
        }
        let row = adw::ActionRow::builder()
            .title(title)
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Label::builder().label(count.to_string()).css_classes(["dim-label"]).build());
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let store = store.clone();
        row.connect_activated(move |_| show_category(&store, key, title));
        categories.add(&row);
    }
    content.append(&categories);

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&adw::Clamp::builder().maximum_size(800).child(&content).build())
        .build()
}

fn app_row(store: &Rc<Store>, app: &Rc<App>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(app.name.as_str())
        .subtitle(app.summary.as_str())
        // Names like "Tom & Jerry" would otherwise be read as markup.
        .use_markup(false)
        .activatable(true)
        .build();
    let icon = gtk::Image::builder().pixel_size(48).build();
    icons::show(&icon, app.icon_url.as_deref(), &store.paths.cache, || true);
    row.add_prefix(&icon);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let store = store.clone();
    let app = app.clone();
    row.connect_activated(move |_| store.show_app(&app));
    row
}

fn show_category(store: &Rc<Store>, key: &str, title: &str) {
    let apps: Vec<_> = store.apps().iter().filter(|app| index::in_category(app, key)).cloned().collect();
    let (list, model) = app_list::build(store);
    app_list::set_apps(&model, &apps);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&list));
    store.nav.push(&adw::NavigationPage::new(&toolbar, title));
}
