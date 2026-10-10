//! The store window.

mod actions;
mod app_list;
mod app_page;
mod home;
mod icons;
mod installed_page;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::appimage;
use crate::index::{self, App};
use crate::install;
use crate::paths::Paths;
use crate::state::InstalledApp;
use actions::InstallProgress;

/// What changed, for pages that watch the store.
#[derive(Clone, Copy, PartialEq)]
pub enum Change {
    /// An app was installed or removed.
    Installed,
    /// A download moved along.
    Progress,
}

/// Everything the store's pages share.
pub struct Store {
    pub paths: Paths,
    pub window: adw::ApplicationWindow,
    pub nav: adw::NavigationView,
    toasts: adw::ToastOverlay,
    apps: RefCell<Rc<Vec<Rc<App>>>>,
    installed: RefCell<Vec<InstalledApp>>,
    /// Installs in progress, by store id.
    installing: RefCell<HashMap<String, Arc<InstallProgress>>>,
    /// Pages that want to know when things change. A watcher returns `false` once
    /// its page has closed, to be dropped.
    watchers: RefCell<Vec<Box<dyn Fn(Change) -> bool>>>,
}

impl Store {
    pub fn apps(&self) -> Rc<Vec<Rc<App>>> {
        self.apps.borrow().clone()
    }

    pub fn installed(&self) -> Vec<InstalledApp> {
        self.installed.borrow().clone()
    }

    /// The installed copy of a store app, matched by store id or download URL.
    pub fn installed_copy(&self, app: &App) -> Option<InstalledApp> {
        self.installed
            .borrow()
            .iter()
            .find(|installed| {
                installed.index_id.as_deref() == Some(app.id.as_str())
                    || app.assets.values().any(|asset| asset.url == installed.source)
            })
            .cloned()
    }

    /// The store listing for an installed app, if it has one.
    pub fn listing_for(&self, installed: &InstalledApp) -> Option<Rc<App>> {
        self.apps
            .borrow()
            .iter()
            .find(|app| self.installed_copy(app).is_some_and(|copy| copy.id == installed.id))
            .cloned()
    }

    pub fn watch(&self, watcher: impl Fn(Change) -> bool + 'static) {
        self.watchers.borrow_mut().push(Box::new(watcher));
    }

    /// Re-reads installed.json and tells every page.
    pub fn reload_installed(&self) {
        *self.installed.borrow_mut() = install::list(&self.paths).unwrap_or_default();
        self.notify(Change::Installed);
    }

    fn notify(&self, change: Change) {
        // Take the watchers out while calling them, in case one adds another.
        let watchers = self.watchers.take();
        let mut kept: Vec<_> = watchers.into_iter().filter(|watcher| watcher(change)).collect();
        kept.append(&mut self.watchers.borrow_mut());
        *self.watchers.borrow_mut() = kept;
    }

    pub fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    pub fn show_app(self: &Rc<Self>, app: &Rc<App>) {
        self.nav.push(&app_page::build(self, app));
    }
}

pub fn build_window(application: &adw::Application) {
    let paths = match Paths::new() {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Outpost")
        .default_width(900)
        .default_height(700)
        .width_request(360)
        .height_request(300)
        .build();
    let nav = adw::NavigationView::new();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&nav));
    window.set_content(Some(&toasts));

    let store = Rc::new(Store {
        paths,
        window: window.clone(),
        nav: nav.clone(),
        toasts,
        apps: RefCell::default(),
        installed: RefCell::default(),
        installing: RefCell::default(),
        watchers: RefCell::default(),
    });

    nav.add(&main_page(&store));
    store.reload_installed();
    window.present();
}

/// The first page: Explore and Installed tabs, with search.
fn main_page(store: &Rc<Store>) -> adw::NavigationPage {
    let stack = adw::ViewStack::new();
    let explore = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_titled_with_icon(&explore, Some("explore"), "Explore", "system-search-symbolic");
    stack.add_titled_with_icon(
        &installed_page::build(store),
        Some("installed"),
        "Installed",
        "emblem-ok-symbolic",
    );

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search apps")
        .hexpand(true)
        .build();
    let search_clamp = adw::Clamp::builder().maximum_size(500).child(&search_entry).build();
    let search_bar = gtk::SearchBar::builder().child(&search_clamp).build();
    search_bar.connect_entry(&search_entry);
    search_bar.set_key_capture_widget(Some(&store.window));
    let search_button = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Search (Ctrl+F)")
        .build();
    search_button
        .bind_property("active", &search_bar, "search-mode-enabled")
        .bidirectional()
        .build();

    let header = adw::HeaderBar::new();
    header.pack_start(&search_button);
    header.set_title_widget(Some(
        &adw::ViewSwitcher::builder()
            .stack(&stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build(),
    ));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search_bar);
    if !appimage::libfuse2_installed() {
        toolbar.add_top_bar(&libfuse2_banner(store));
    }
    toolbar.set_content(Some(&stack));

    // Explore shows a spinner while the catalogue loads, then home or search results.
    explore.add_named(
        &adw::StatusPage::builder()
            .title("Loading apps…")
            .paintable(&adw::SpinnerPaintable::new(Some(&explore)))
            .build(),
        Some("loading"),
    );
    let error_page = adw::StatusPage::builder()
        .icon_name("network-offline-symbolic")
        .title("Couldn't Load Apps")
        .build();
    let retry = gtk::Button::builder()
        .label("Try Again")
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    error_page.set_child(Some(&retry));
    explore.add_named(&error_page, Some("error"));
    let (results, results_model) = app_list::build(store);
    explore.add_named(&results, Some("results"));
    explore.add_named(
        &adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("No Results")
            .description("Try a different search")
            .build(),
        Some("no-results"),
    );

    {
        let store = store.clone();
        let explore = explore.clone();
        let stack = stack.clone();
        search_entry.connect_search_changed(move |entry| {
            if !matches!(explore.visible_child_name().as_deref(), Some("home" | "results" | "no-results")) {
                return; // Still loading.
            }
            stack.set_visible_child_name("explore");
            let results = index::search(&store.apps(), &entry.text());
            app_list::set_apps(&results_model, &results);
            let page = match (entry.text().trim().is_empty(), results.is_empty()) {
                (true, _) => "home",
                (false, true) => "no-results",
                (false, false) => "results",
            };
            explore.set_visible_child_name(page);
        });
    }

    let load: Rc<dyn Fn()> = {
        let store = store.clone();
        let explore = explore.clone();
        let error_page = error_page.clone();
        Rc::new(move || {
            explore.set_visible_child_name("loading");
            let store = store.clone();
            let explore = explore.clone();
            let error_page = error_page.clone();
            glib::spawn_future_local(async move {
                let paths = store.paths.clone();
                let result = gio::spawn_blocking(move || index::fetch(&paths))
                    .await
                    .unwrap_or_else(|_| Err("Loading the catalogue crashed.".into()));
                match result {
                    Ok(index) => {
                        *store.apps.borrow_mut() = Rc::new(index.apps.into_iter().map(Rc::new).collect());
                        if let Some(old) = explore.child_by_name("home") {
                            explore.remove(&old);
                        }
                        explore.add_named(&home::build(&store), Some("home"));
                        explore.set_visible_child_name("home");
                        // Installed rows link to store pages, which need the catalogue.
                        store.reload_installed();
                    }
                    Err(e) => {
                        error_page.set_description(Some(&e));
                        explore.set_visible_child_name("error");
                    }
                }
            });
        })
    };
    {
        let load = load.clone();
        retry.connect_clicked(move |_| load());
    }
    load();

    adw::NavigationPage::builder()
        .title("Outpost")
        .tag("main")
        .child(&toolbar)
        .build()
}

/// Shown when libfuse2 is missing, since most AppImages won't open without it.
fn libfuse2_banner(store: &Rc<Store>) -> adw::Banner {
    let banner = adw::Banner::builder()
        .title("Most apps won't open until libfuse2 is installed")
        .button_label("How to Fix")
        .revealed(true)
        .build();
    let window = store.window.clone();
    banner.connect_button_clicked(move |_| {
        let dialog = adw::AlertDialog::new(
            Some("Install libfuse2"),
            Some(&format!(
                "Most AppImages need libfuse2 to run, and this computer doesn't have it. \
                 Open a terminal and run:\n\n{}",
                appimage::libfuse2_fix()
            )),
        );
        dialog.add_response("close", "Close");
        dialog.present(Some(&window));
    });
    banner
}
