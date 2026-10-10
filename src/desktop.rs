//! Reading and rewriting `.desktop` files (the launcher entries GNOME reads).

use std::path::Path;

/// Reads a key from the `[Desktop Entry]` group.
pub fn get(contents: &str, key: &str) -> Option<String> {
    let mut in_main_group = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
        } else if in_main_group
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Turns the desktop file shipped inside an AppImage into one for the installed copy:
/// every `Exec=` runs the installed AppImage, `Icon=` points at the installed icon,
/// and Outpost tags the entry with the app's id.
///
/// `wm_class` is the app's original desktop file name. It's added as `StartupWMClass`
/// when missing, so GNOME can still match the app's windows to this entry even though
/// we install it under a different file name.
pub fn rewrite(contents: &str, appimage: &Path, icon: &str, id: &str, wm_class: &str) -> String {
    let has_wm_class = get(contents, "StartupWMClass").is_some();
    let mut out = String::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            out.push_str(line);
            out.push('\n');
            if trimmed == "[Desktop Entry]" {
                out.push_str(&format!("TryExec={}\n", appimage.display()));
                out.push_str(&format!("X-Outpost-Id={id}\n"));
                if !has_wm_class {
                    out.push_str(&format!("StartupWMClass={wm_class}\n"));
                }
            }
            continue;
        }

        match trimmed.split_once('=').map(|(k, v)| (k.trim(), v)) {
            Some(("Exec", value)) => {
                out.push_str(&format!("Exec={}\n", replace_program(value, appimage)));
            }
            Some(("Icon", _)) => out.push_str(&format!("Icon={icon}\n")),
            // We write our own versions of these above.
            Some(("TryExec" | "X-Outpost-Id", _)) => {}
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Swaps the program at the start of an `Exec=` value for `program`, keeping the
/// arguments (like `%U`) that follow it.
fn replace_program(value: &str, program: &Path) -> String {
    let value = value.trim();
    let args = if let Some(after_quote) = value.strip_prefix('"') {
        // Quoted program: skip past the closing quote, honouring backslash escapes.
        let mut escaped = false;
        let mut end = after_quote.len();
        for (i, c) in after_quote.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => {
                    end = i + 1;
                    break;
                }
                _ => {}
            }
        }
        &after_quote[end..]
    } else {
        value.find(char::is_whitespace).map_or("", |i| &value[i..])
    };
    format!("{}{args}", quote(program))
}

/// Quotes a path for `Exec=`. The spec layers two escapes: inside the quotes,
/// `"`, `` ` ``, `$` and `\` need a backslash, and then every backslash is itself
/// doubled because desktop file values are escaped strings.
fn quote(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '\\' => quoted.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str(r"\\");
                quoted.push(c);
            }
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[Desktop Entry]
Name=Photocraft
Exec=photocraft %F
TryExec=photocraft
Icon=photocraft
Type=Application

[Desktop Action new-window]
Name=New Window
Exec=\"photo craft\" --new-window
";

    #[test]
    fn reads_keys_from_main_group_only() {
        assert_eq!(get(SAMPLE, "Name").as_deref(), Some("Photocraft"));
        assert_eq!(get(SAMPLE, "Missing"), None);
    }

    #[test]
    fn rewrites_exec_icon_and_tags_entry() {
        let out = rewrite(
            SAMPLE,
            Path::new("/home/me/Applications/photocraft.AppImage"),
            "outpost-photocraft",
            "photocraft",
            "photocraft",
        );
        assert!(out.contains("Exec=\"/home/me/Applications/photocraft.AppImage\" %F\n"));
        assert!(out.contains("Exec=\"/home/me/Applications/photocraft.AppImage\" --new-window\n"));
        assert!(out.contains("Icon=outpost-photocraft\n"));
        assert!(out.contains("TryExec=/home/me/Applications/photocraft.AppImage\n"));
        assert!(out.contains("X-Outpost-Id=photocraft\n"));
        assert!(out.contains("StartupWMClass=photocraft\n"));
        assert!(!out.contains("TryExec=photocraft"));
    }

    #[test]
    fn keeps_existing_wm_class() {
        let input = "[Desktop Entry]\nExec=app\nStartupWMClass=RealClass\n";
        let out = rewrite(input, Path::new("/a"), "i", "app", "app");
        assert!(out.contains("StartupWMClass=RealClass"));
        assert!(!out.contains("StartupWMClass=app"));
    }

    #[test]
    fn quotes_special_characters() {
        assert_eq!(quote(Path::new("/a b/$x")), r#""/a b/\\$x""#);
    }
}
