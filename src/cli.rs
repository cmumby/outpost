//! The command-line interface, used to test install logic before there's a store UI.

use std::path::Path;

use crate::install;
use crate::paths::Paths;

const USAGE: &str = "\
Usage:
  outpost                     Open the store
  outpost install <file>      Install an AppImage from a local file
  outpost list                List installed apps
  outpost uninstall <id>      Remove an installed app";

pub fn run(args: &[String]) -> Result<(), String> {
    let paths = Paths::new()?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        ["install", file] => {
            let app = install::install_from_file(&paths, Path::new(file))?;
            println!(
                "Installed {} {} as '{}'\n  {}",
                app.name,
                app.version.as_deref().unwrap_or("(unknown version)"),
                app.id,
                app.appimage_path.display()
            );
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
