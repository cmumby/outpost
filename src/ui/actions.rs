//! Installing, uninstalling and opening apps from the UI.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Change, Store};
use crate::index::App;
use crate::install::{self, StoreEntry};
use crate::state::InstalledApp;

/// How far along a download is. Written by the install thread, read by the UI.
#[derive(Default)]
pub struct InstallProgress {
    done: AtomicU64,
    /// 0 when the server didn't say.
    total: AtomicU64,
}

impl InstallProgress {
    /// Between 0 and 1, or `None` if the size is unknown.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| self.done.load(Ordering::Relaxed) as f64 / total as f64)
    }

    pub fn finished_downloading(&self) -> bool {
        self.fraction().is_some_and(|fraction| fraction >= 1.0)
    }
}

impl Store {
    pub fn progress(&self, app: &App) -> Option<Arc<InstallProgress>> {
        self.installing.borrow().get(&app.id).cloned()
    }

    /// Downloads and installs (or updates) a store app on a background thread.
    pub fn install(self: &Rc<Self>, app: &Rc<App>) {
        let Some(asset) = app.asset().cloned() else { return };
        if self.installing.borrow().contains_key(&app.id) {
            return;
        }
        let progress = Arc::new(InstallProgress::default());
        self.installing.borrow_mut().insert(app.id.clone(), progress.clone());
        self.notify(Change::Progress);

        // Redraw progress bars while anything is installing.
        let ticker = {
            let store = Rc::downgrade(self);
            glib::timeout_add_local(Duration::from_millis(150), move || match store.upgrade() {
                Some(store) => {
                    store.notify(Change::Progress);
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            })
        };

        let store = self.clone();
        let app = app.clone();
        glib::spawn_future_local(async move {
            let paths = store.paths.clone();
            let (id, version) = (app.id.clone(), app.latest_version.clone());
            let result = gio::spawn_blocking(move || {
                let entry = StoreEntry { id: &id, version: &version };
                install::install_from_url(&paths, &asset.url, Some(entry), |done, total| {
                    progress.done.store(done, Ordering::Relaxed);
                    progress.total.store(total.unwrap_or(0), Ordering::Relaxed);
                })
            })
            .await
            .unwrap_or_else(|_| Err("the installer crashed".into()));

            ticker.remove();
            store.installing.borrow_mut().remove(&app.id);
            match result {
                Ok(_) => store.toast(&format!("{} is installed", app.name)),
                Err(e) => store.alert(&format!("Couldn't Install {}", app.name), &capitalize(&e)),
            }
            store.reload_installed();
        });
    }

    /// Asks first, then removes the app.
    pub fn uninstall(self: &Rc<Self>, installed: &InstalledApp) {
        let dialog = adw::AlertDialog::new(
            Some(&format!("Uninstall {}?", installed.name)),
            Some("The app will be removed from this computer. Your files and settings in it are kept."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("uninstall", "Uninstall")]);
        dialog.set_response_appearance("uninstall", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let store = self.clone();
        let installed = installed.clone();
        dialog.connect_response(Some("uninstall"), move |_, _| {
            match install::uninstall(&store.paths, &installed.id) {
                Ok(app) => store.toast(&format!("{} was uninstalled", app.name)),
                Err(e) => store.alert(&format!("Couldn't Uninstall {}", installed.name), &capitalize(&e)),
            }
            store.reload_installed();
        });
        dialog.present(Some(&self.window));
    }

    /// Launches the app through its desktop entry, like the system launcher does.
    pub fn open(&self, installed: &InstalledApp) {
        let launched = gio_unix::DesktopAppInfo::from_filename(&installed.desktop_path)
            .ok_or_else(|| "its launcher entry is missing".to_string())
            .and_then(|info| {
                let context = WidgetExt::display(&self.window).app_launch_context();
                info.launch(&[], Some(&context)).map_err(|e| e.to_string())
            });
        if let Err(e) = launched {
            self.alert(&format!("Couldn't Open {}", installed.name), &capitalize(&e));
        }
    }

    pub fn alert(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_response("close", "Close");
        dialog.present(Some(&self.window));
    }
}

/// Our error messages start lower-case so they read well after "Error: " on the
/// command line; dialogs want a capital letter.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
