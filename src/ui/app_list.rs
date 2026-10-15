//! A scrolling list of apps, used for search results and categories. It only builds
//! rows for the apps on screen, so it stays quick with thousands of apps.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Store, icons};
use crate::index::App;

pub fn build(store: &Rc<Store>) -> (gtk::ScrolledWindow, gio::ListStore) {
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_child(Some(&row()));
    });
    {
        let store = store.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let object = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            let app = object.borrow::<Rc<App>>().clone();
            let row = item.child().and_downcast::<gtk::Box>().unwrap();
            let (icon, name, summary) = row_parts(&row);
            name.set_label(&app.name);
            summary.set_label(&app.summary);

            let item = item.downgrade();
            icons::show(&icon, app.icon_url.as_deref(), &store.paths().cache, move || {
                item.upgrade().and_then(|item| item.item()).as_ref() == Some(object.upcast_ref())
            });
        });
    }

    let view = gtk::ListView::builder()
        .model(&gtk::NoSelection::new(Some(model.clone())))
        .factory(&factory)
        .single_click_activate(true)
        .css_classes(["navigation-sidebar"])
        .build();
    {
        let store = store.clone();
        let model = model.clone();
        view.connect_activate(move |_, position| {
            if let Some(object) = model.item(position).and_downcast::<glib::BoxedAnyObject>() {
                let app = object.borrow::<Rc<App>>().clone();
                store.show_app(&app);
            }
        });
    }

    let clamp = adw::ClampScrollable::builder()
        .maximum_size(800)
        .child(&view)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    (scrolled, model)
}

pub fn set_apps(model: &gio::ListStore, apps: &[Rc<App>]) {
    let objects: Vec<_> = apps.iter().map(|app| glib::BoxedAnyObject::new(app.clone())).collect();
    model.splice(0, model.n_items(), &objects);
}

fn row() -> gtk::Box {
    let icon = gtk::Image::builder().pixel_size(48).build();
    let name = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["heading"])
        .build();
    let summary = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label"])
        .build();
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .spacing(2)
        .build();
    text.append(&name);
    text.append(&summary);

    let row = gtk::Box::builder()
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    row.append(&icon);
    row.append(&text);
    row
}

fn row_parts(row: &gtk::Box) -> (gtk::Image, gtk::Label, gtk::Label) {
    let icon = row.first_child().and_downcast::<gtk::Image>().unwrap();
    let text = icon.next_sibling().unwrap();
    let name = text.first_child().and_downcast::<gtk::Label>().unwrap();
    let summary = name.next_sibling().and_downcast::<gtk::Label>().unwrap();
    (icon, name, summary)
}
