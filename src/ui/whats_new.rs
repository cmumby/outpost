//! The "What's New" window for an available update: the release notes for the new
//! version, with Update and View on GitHub.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Store, markdown};
use crate::index::{self, App};
use crate::state::InstalledApp;

pub fn show(store: &Rc<Store>, installed: &InstalledApp, app: &Rc<App>) {
    let notes = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .yalign(0.0)
        .selectable(true)
        .build();
    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .halign(gtk::Align::Center)
        .margin_top(24)
        .build();
    let body = gtk::Stack::builder().vhomogeneous(false).build();
    body.add_named(&spinner, Some("loading"));
    body.add_named(&notes, Some("notes"));

    let version = gtk::Label::builder()
        .label(format!(
            "{} → {}",
            installed.version.as_deref().unwrap_or("installed version"),
            app.latest_version
        ))
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();

    let update = gtk::Button::builder()
        .label("Update")
        .css_classes(["pill", "suggested-action"])
        .build();
    let github = gtk::Button::builder()
        .label("View on GitHub")
        .css_classes(["pill"])
        .build();
    let buttons = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::Center)
        .build();
    buttons.append(&github);
    buttons.append(&update);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(6)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    content.append(&version);
    content.append(&body);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(480)
        .vexpand(true)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scrolled));
    let footer = gtk::Box::builder()
        .margin_top(12)
        .margin_bottom(18)
        .halign(gtk::Align::Center)
        .build();
    footer.append(&buttons);
    toolbar.add_bottom_bar(&footer);

    let dialog = adw::Dialog::builder()
        .title(format!("What's New in {} {}", app.name, app.latest_version))
        .content_width(560)
        .child(&toolbar)
        .build();

    {
        let (store, app, dialog) = (store.clone(), app.clone(), dialog.clone());
        update.connect_clicked(move |_| {
            store.install(&app);
            dialog.close();
        });
    }
    {
        let window = store.window.clone();
        let url = app.release_url.clone().unwrap_or_else(|| format!("{}/releases", app.source_repo));
        github.connect_clicked(move |_| {
            gtk::UriLauncher::new(&url).launch(Some(&window), gio::Cancellable::NONE, |_| {});
        });
    }

    // Fetch the notes in the background; the window opens straight away.
    let url = index::release_notes_url(app);
    glib::spawn_future_local(async move {
        let text = match url {
            Some(url) => gio::spawn_blocking(move || index::fetch_text(&url)).await.ok().flatten(),
            None => None,
        };
        let markup = text.map(|text| markdown::to_pango(&text)).filter(|markup| !markup.is_empty());
        match markup {
            Some(markup) => notes.set_markup(&markup),
            None => {
                notes.add_css_class("dim-label");
                notes.set_text("The developer didn't publish notes for this version. There may be more on its GitHub page.");
            }
        }
        body.set_visible_child_name("notes");
    });

    dialog.present(Some(&store.window));
}
