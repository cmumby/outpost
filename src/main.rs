mod appimage;
mod cli;
mod desktop;
mod download;
mod index;
mod install;
mod paths;
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

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_window);
    app.run()
}
