//! One app's page: icon, description, details, and the Install / Open / Uninstall buttons.

use std::rc::Rc;

use adw::prelude::*;

use super::{Store, icons};
use crate::index::App;

pub fn build(store: &Rc<Store>, app: &Rc<App>) -> adw::NavigationPage {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&header(store, app));

    let buttons = Buttons::new(store, app);
    content.append(&buttons.stack);
    buttons.update();

    if app.asset().is_some_and(|asset| asset.sha256.is_none()) {
        content.append(
            &gtk::Label::builder()
                .label("Unverified download: this app's file hasn't been checksummed yet")
                .wrap(true)
                .css_classes(["caption", "dim-label"])
                .build(),
        );
    }

    if !app.description.is_empty() && app.description != app.summary {
        content.append(
            &gtk::Label::builder()
                .label(app.description.as_str())
                .wrap(true)
                .xalign(0.0)
                .selectable(true)
                .build(),
        );
    }
    content.append(&details(store, app));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&adw::Clamp::builder().maximum_size(700).child(&content).build())
            .build(),
    ));
    let page = adw::NavigationPage::new(&toolbar, &app.name);

    // Keep the buttons in step with installs, until the page is closed.
    let weak_page = page.downgrade();
    store.watch(move |_| {
        if weak_page.upgrade().is_none_or(|page| page.parent().is_none()) {
            return false;
        }
        buttons.update();
        true
    });
    page
}

fn header(store: &Rc<Store>, app: &Rc<App>) -> gtk::Box {
    let icon = gtk::Image::builder().pixel_size(128).build();
    icons::show(&icon, app.icon_url.as_deref(), &store.paths.cache, || true);

    let name = gtk::Label::builder()
        .label(app.name.as_str())
        .wrap(true)
        .xalign(0.0)
        .css_classes(["title-1"])
        .build();
    let summary = gtk::Label::builder()
        .label(app.summary.as_str())
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    let badge = if app.verified {
        "Verified source"
    } else if app.is_curated() {
        "Featured"
    } else {
        "From AppImageHub"
    };
    let meta = gtk::Label::builder()
        .label(format!("Version {} · {badge}", app.latest_version))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["caption"])
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .spacing(6)
        .build();
    text.append(&name);
    text.append(&summary);
    text.append(&meta);

    let header = gtk::Box::builder().spacing(24).build();
    header.append(&icon);
    header.append(&text);
    header
}

fn details(store: &Rc<Store>, app: &Rc<App>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Details").build();

    let source = adw::ActionRow::builder()
        .title("Source code")
        .subtitle(app.source_repo.as_str())
        .use_markup(false)
        .activatable(true)
        .build();
    source.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
    {
        let window = store.window.clone();
        let url = app.source_repo.clone();
        source.connect_activated(move |_| {
            gtk::UriLauncher::new(&url).launch(Some(&window), gtk::gio::Cancellable::NONE, |_| {});
        });
    }
    group.add(&source);

    if let Some(asset) = app.asset() {
        group.add(
            &adw::ActionRow::builder()
                .title("Download size")
                .subtitle(format!("{:.1} MB", asset.size as f64 / 1_000_000.0))
                .build(),
        );
    }
    if !app.categories.is_empty() {
        group.add(
            &adw::ActionRow::builder()
                .title("Categories")
                .subtitle(app.categories.join(", "))
                .use_markup(false)
                .build(),
        );
    }
    group
}

/// The row of buttons under the header, which changes as the app is installed.
struct Buttons {
    store: Rc<Store>,
    app: Rc<App>,
    stack: gtk::Stack,
    progress: gtk::ProgressBar,
    update: gtk::Button,
}

impl Buttons {
    fn new(store: &Rc<Store>, app: &Rc<App>) -> Rc<Self> {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .build();

        let install = pill("Install", "suggested-action");
        stack.add_named(&row(&[&install]), Some("install"));

        let progress = gtk::ProgressBar::builder()
            .show_text(true)
            .valign(gtk::Align::Center)
            .width_request(240)
            .build();
        stack.add_named(&row(&[&progress]), Some("progress"));

        let open = pill("Open", "suggested-action");
        let update = pill("Update", "suggested-action");
        let uninstall = pill("Uninstall", "destructive-action");
        stack.add_named(&row(&[&open, &update, &uninstall]), Some("installed"));

        stack.add_named(
            &gtk::Label::builder()
                .label(format!("Not available for this computer ({})", std::env::consts::ARCH))
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build(),
            Some("unavailable"),
        );

        let buttons = Rc::new(Self { store: store.clone(), app: app.clone(), stack, progress, update: update.clone() });

        let (store_, app_) = (store.clone(), app.clone());
        install.connect_clicked(move |_| store_.install(&app_));
        let (store_, app_) = (store.clone(), app.clone());
        update.connect_clicked(move |_| store_.install(&app_));
        let (store_, app_) = (store.clone(), app.clone());
        open.connect_clicked(move |_| {
            if let Some(installed) = store_.installed_copy(&app_) {
                store_.open(&installed);
            }
        });
        let (store_, app_) = (store.clone(), app.clone());
        uninstall.connect_clicked(move |_| {
            if let Some(installed) = store_.installed_copy(&app_) {
                store_.uninstall(&installed);
            }
        });
        buttons
    }

    fn update(&self) {
        if let Some(progress) = self.store.progress(&self.app) {
            match progress.fraction() {
                _ if progress.finished_downloading() => {
                    self.progress.set_fraction(1.0);
                    self.progress.set_text(Some("Installing…"));
                }
                Some(fraction) => {
                    self.progress.set_fraction(fraction);
                    self.progress.set_text(Some(&format!("Downloading… {:.0}%", fraction * 100.0)));
                }
                None => {
                    self.progress.pulse();
                    self.progress.set_text(Some("Downloading…"));
                }
            }
            self.stack.set_visible_child_name("progress");
        } else if let Some(installed) = self.store.installed_copy(&self.app) {
            let outdated = installed.version.as_deref() != Some(self.app.latest_version.as_str());
            self.update.set_visible(outdated && self.app.asset().is_some());
            self.stack.set_visible_child_name("installed");
        } else if self.app.asset().is_some() {
            self.stack.set_visible_child_name("install");
        } else {
            self.stack.set_visible_child_name("unavailable");
        }
    }
}

fn pill(label: &str, style: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .css_classes(["pill", style])
        .build()
}

fn row(children: &[&dyn AsRef<gtk::Widget>]) -> gtk::Box {
    let row = gtk::Box::builder().spacing(12).build();
    for child in children {
        row.append(child.as_ref());
    }
    row
}
