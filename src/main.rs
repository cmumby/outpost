mod appimage;
mod cli;
mod desktop;
mod download;
mod install;
mod paths;
mod state;

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
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let status = adw::StatusPage::builder()
        .icon_name("system-software-install-symbolic")
        .title("Outpost")
        .description("Your supply station for AppImages.")
        .vexpand(true)
        .build();

    let content = adw::ToolbarView::new();
    content.add_top_bar(&adw::HeaderBar::new());
    content.set_content(Some(&status));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Outpost")
        .default_width(800)
        .default_height(600)
        .content(&content)
        .build();
    window.present();
}
