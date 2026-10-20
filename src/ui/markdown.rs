//! Just enough Markdown for release notes, turned into the markup GTK labels show:
//! headings, bullet lists, **bold**, *italic*, `code` and [links](https://…).
//! Anything else comes through as plain text.

use gtk::glib::markup_escape_text;

pub fn to_pango(markdown: &str) -> String {
    let mut out = Vec::new();
    let mut in_code_block = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            out.push(format!("<tt>{}</tt>", markup_escape_text(line)));
            continue;
        }
        // Raw HTML (comments, <details>, <img>) and images don't read well as text.
        if trimmed.starts_with('<') || trimmed.starts_with("![") {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if let Some(heading) = heading_text(trimmed) {
            out.push(format!("<b>{}</b>", inline(heading)));
        } else if let Some(item) = trimmed.strip_prefix("- ").or(trimmed.strip_prefix("* ")).or(trimmed.strip_prefix("+ ")) {
            out.push(format!("{}• {}", "    ".repeat(indent / 2), inline(item)));
        } else {
            out.push(inline(trimmed));
        }
    }

    // Collapse runs of blank lines so the notes don't sprawl.
    let mut text = out.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

fn heading_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let level = line.len() - rest.len();
    (1..=6).contains(&level).then(|| rest.trim()).filter(|_| rest.starts_with(' '))
}

/// Inline formatting within a line. Text is escaped as it's copied, so nothing in the
/// notes can inject markup of its own.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some((label, url, after)) = link(rest) {
            if url.starts_with("https://") || url.starts_with("http://") {
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    markup_escape_text(url),
                    markup_escape_text(label)
                ));
            } else {
                out.push_str(&markup_escape_text(label));
            }
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "**") {
            out.push_str(&format!("<b>{}</b>", markup_escape_text(inner)));
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "`") {
            out.push_str(&format!("<tt>{}</tt>", markup_escape_text(inner)));
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "*").filter(|(inner, _)| !inner.starts_with(' ')) {
            out.push_str(&format!("<i>{}</i>", markup_escape_text(inner)));
            rest = after;
        } else {
            let next = rest.chars().next().unwrap();
            out.push_str(&markup_escape_text(&next.to_string()));
            rest = &rest[next.len_utf8()..];
        }
    }
    out
}

/// `[label](url)` at the start of `text`.
fn link(text: &str) -> Option<(&str, &str, &str)> {
    let rest = text.strip_prefix('[')?;
    let (label, rest) = rest.split_once("](")?;
    let (url, after) = rest.split_once(')')?;
    (!label.contains(']') && !url.contains(' ')).then_some((label, url, after))
}

/// Text between `marker`s at the start of `text`, like `**bold**`.
fn wrapped<'a>(text: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let rest = text.strip_prefix(marker)?;
    let end = rest.find(marker)?;
    (end > 0).then(|| (&rest[..end], &rest[end + marker.len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_the_usual_release_notes() {
        let notes = "## What's Changed\n* Faster **canvas** by @someone in [#12](https://github.com/o/r/pull/12)\n\n\n\nUse `--fast` now";
        assert_eq!(
            to_pango(notes),
            "<b>What&apos;s Changed</b>\n• Faster <b>canvas</b> by @someone in <a href=\"https://github.com/o/r/pull/12\">#12</a>\n\nUse <tt>--fast</tt> now"
        );
    }

    #[test]
    fn escapes_markup_and_drops_html() {
        assert_eq!(to_pango("<details>\nx < y & z\n![shot](a.png)"), "x &lt; y &amp; z");
    }

    /// Every release note in a locally built catalogue (outpost-index/site/notes)
    /// must come out as markup GTK accepts. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_release_notes_make_valid_markup() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../outpost-index/site/notes");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("build the index locally first") {
            let path = entry.unwrap().path();
            let mut markup = to_pango(&std::fs::read_to_string(&path).unwrap());
            // Links are handled by GTK labels themselves, before the rest of the
            // markup reaches Pango, so set them aside the same way.
            while let Some(start) = markup.find("<a href=\"") {
                let end = start + markup[start..].find("\">").unwrap() + 2;
                markup.replace_range(start..end, "");
            }
            let markup = markup.replace("</a>", "");
            if let Err(e) = gtk::pango::parse_markup(&markup, '\0') {
                panic!("{}: {e}", path.display());
            }
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn leaves_non_web_links_as_text() {
        assert_eq!(to_pango("[run](javascript:alert)"), "run");
    }
}
