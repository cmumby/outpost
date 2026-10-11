//! The welcome screen shown the first time Outpost opens.

use std::fs;
use std::rc::Rc;

use adw::prelude::*;

use super::Store;
use crate::appimage;
use crate::self_update;

pub fn show_first_time(store: &Rc<Store>) {
    let seen = store.paths.data.join("welcomed");
    if seen.exists() {
        return;
    }

    let things = adw::PreferencesGroup::new();
    things.add(&row(
        "folder-symbolic",
        "Apps go in your Applications folder",
        "~/Applications, with launcher entries and icons added for you",
    ));
    things.add(&row(
        "user-home-symbolic",
        "No admin password needed",
        "Everything stays in your home folder",
    ));
    if appimage::libfuse2_installed() {
        things.add(&row("emblem-ok-symbolic", "Ready to run AppImages", "libfuse2 is installed"));
    } else {
        let fuse = row(
            "dialog-warning-symbolic",
            "Install libfuse2 first",
            &format!("Most AppImages need it. In a terminal, run: {}", appimage::libfuse2_fix()),
        );
        fuse.set_subtitle_selectable(true);
        things.add(&fuse);
    }

    // Offer to move Outpost into ~/Applications if it's running from somewhere else,
    // such as Downloads.
    if let Some(appimage) = self_update::running_appimage()
        && appimage != self_update::home(&store.paths)
    {
        let keep = row(
            "folder-download-symbolic",
            "Keep Outpost in Applications",
            "Moves it to ~/Applications/Outpost.AppImage, so it's easy to find and can update itself",
        );
        let move_button = gtk::Button::builder()
            .label("Move")
            .valign(gtk::Align::Center)
            .build();
        keep.add_suffix(&move_button);
        let store = store.clone();
        move_button.connect_clicked(move |_| match self_update::move_home(&store.paths, &appimage) {
            Ok(new_home) => {
                let _ = fs::create_dir_all(&store.paths.data);
                let _ = fs::write(store.paths.data.join("welcomed"), "");
                super::restart_from(&store, &new_home);
            }
            Err(e) => store.alert("Couldn't Move Outpost", &e),
        });
        things.add(&keep);
    }

    let start = gtk::Button::builder()
        .label("Start Exploring")
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(
        &adw::StatusPage::builder()
            .icon_name(crate::APP_ID)
            .title("Welcome to Outpost")
            .description("Find, install and update AppImages, without the hunting around")
            .css_classes(["compact"])
            .build(),
    );
    content.append(&things);
    content.append(&start);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    toolbar.set_content(Some(&content));

    let dialog = adw::Dialog::builder()
        .child(&toolbar)
        .content_width(520)
        .build();
    {
        let dialog = dialog.clone();
        start.connect_clicked(move |_| {
            dialog.close();
        });
    }
    let data = store.paths.data.clone();
    dialog.connect_closed(move |_| {
        let _ = fs::create_dir_all(&data);
        let _ = fs::write(data.join("welcomed"), "");
    });
    dialog.present(Some(&store.window));
}

fn row(icon: &str, title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row
}
