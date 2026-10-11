mod appimage;
mod cli;
mod desktop;
mod download;
mod index;
mod install;
mod paths;
mod self_update;
mod state;
mod ui;

use adw::prelude::*;

const APP_ID: &str = "io.github.cmumby.Outpost";

fn main() -> gtk::glib::ExitCode {
    // With arguments, act as a command-line tool; without, open the window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return match cli::run(&args) {
            Ok(()) => gtk::glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e}");
                gtk::glib::ExitCode::FAILURE
            }
        };
    }

    // Electron apps such as VS Code set GDK_BACKEND=x11 for everything they launch,
    // and under XWayland GTK 4 windows can stop redrawing for good. When Wayland is
    // available, use it.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("GDK_BACKEND").as_deref() == Ok("x11") {
        // SAFETY: nothing else is running yet, so no other thread can be reading
        // the environment.
        unsafe { std::env::remove_var("GDK_BACKEND") };
    }

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_window);
    app.run()
}
