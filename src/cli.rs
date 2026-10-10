//! The command-line interface, used to test install logic before there's a store UI.

use std::io::Write;
use std::path::Path;

use crate::appimage;
use crate::install;
use crate::paths::Paths;

const USAGE: &str = "\
Usage:
  outpost                     Open the store
  outpost install <file|url>  Install an AppImage from a local file or a web link
  outpost list                List installed apps
  outpost uninstall <id>      Remove an installed app";

pub fn run(args: &[String]) -> Result<(), String> {
    let paths = Paths::new()?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        ["install", source] => {
            let app = if source.starts_with("https://") || source.starts_with("http://") {
                let mut progress = Progress::default();
                let result = install::install_from_url(&paths, source, None, |done, total| progress.show(done, total));
                progress.finish();
                result?
            } else {
                install::install_from_file(&paths, Path::new(source))?
            };
            println!(
                "Installed {} {} as '{}'\n  {}",
                app.name,
                app.version.as_deref().unwrap_or("(unknown version)"),
                app.id,
                app.appimage_path.display()
            );
            if !appimage::libfuse2_installed() {
                println!(
                    "\nHeads up: {} probably won't open yet, because this computer is missing \
                     libfuse2, which most AppImages need. To fix it, run:\n  {}",
                    app.name,
                    appimage::libfuse2_fix()
                );
            }
        }
        ["list"] => {
            let apps = install::list(&paths)?;
            if apps.is_empty() {
                println!("No apps installed yet.");
            }
            for app in apps {
                println!(
                    "{:<28} {:<24} {:<12} {}",
                    app.id,
                    app.name,
                    app.version.as_deref().unwrap_or("-"),
                    app.appimage_path.display()
                );
            }
        }
        ["uninstall", id] => {
            let app = install::uninstall(&paths, id)?;
            println!("Uninstalled {}", app.name);
        }
        ["help" | "--help" | "-h"] => println!("{USAGE}"),
        _ => return Err(format!("didn't understand that command\n\n{USAGE}")),
    }
    Ok(())
}

/// A one-line download progress readout that redraws in place.
#[derive(Default)]
struct Progress {
    last_shown: Option<u64>,
}

impl Progress {
    fn show(&mut self, done: u64, total: Option<u64>) {
        const MB: u64 = 1024 * 1024;
        // Redraw at most once per percent (or per megabyte when the size is unknown).
        let step = total.map_or(done / MB, |total| done * 100 / total.max(1));
        if self.last_shown == Some(step) {
            return;
        }
        self.last_shown = Some(step);
        match total {
            Some(total) => eprint!("\rDownloading… {step}% ({} of {} MB)", done / MB, total / MB),
            None => eprint!("\rDownloading… {} MB", done / MB),
        }
        let _ = std::io::stderr().flush();
    }

    fn finish(&self) {
        if self.last_shown.is_some() {
            eprintln!();
        }
    }
}
