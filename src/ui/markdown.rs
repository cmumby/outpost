//! Just enough Markdown for release notes. Notes are split into blocks (headings,
//! list items, paragraphs, code) so they can be laid out with proper spacing, and
//! each block's text becomes the markup GTK labels show: **bold**, *italic*,
//! `code` and links. Long GitHub links are shortened (".../pull/7155" becomes
//! "#7155") and the "by @someone in ..." credit on each line is dimmed.

use gtk::glib::markup_escape_text;

#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    /// A list item: nesting depth, then its markup.
    Item(usize, String),
    Paragraph(String),
    Code(String),
}

pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let mut paragraph: Vec<&str> = Vec::new();

    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            match code.take() {
                Some(lines) => blocks.push(Block::Code(markup_escape_text(&lines.join("\n")).to_string())),
                None => code = Some(Vec::new()),
            }
            continue;
        }
        if let Some(lines) = &mut code {
            lines.push(line);
            continue;
        }
        // Raw HTML (comments, <details>, <img>), images and rules don't read well as text.
        if trimmed.is_empty() || trimmed.starts_with('<') || trimmed.starts_with("![") || trimmed == "---" {
            flush(&mut paragraph, &mut blocks);
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if let Some(heading) = heading_text(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(inline(heading)));
        } else if let Some(item) = list_item(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item(indent / 2, inline(item)));
        } else {
            paragraph.push(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    if let Some(lines) = code {
        blocks.push(Block::Code(markup_escape_text(&lines.join("\n")).to_string()));
    }
    blocks
}

fn heading_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let level = line.len() - rest.len();
    ((1..=6).contains(&level) && rest.starts_with(' ')).then(|| rest.trim())
}

fn list_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or(line.strip_prefix("* "))
        .or(line.strip_prefix("+ "))
        .or_else(|| {
            // Numbered lists: "1. ", "12. "
            let digits = line.find(|c: char| !c.is_ascii_digit())?;
            (digits > 0).then(|| line[digits..].strip_prefix(". ")).flatten()
        })
}

/// Inline formatting within a block. Text is escaped as it's copied, so nothing in
/// the notes can inject markup of its own.
fn inline(text: &str) -> String {
    // GitHub's generated notes end most lines with "by @someone in <link>"; keep
    // the change itself prominent and the credit quiet.
    if let Some(at) = text.rfind(" by @") {
        let (change, credit) = text.split_at(at);
        if !credit.contains("**") {
            return format!("{}<span fgalpha=\"60%\">{}</span>", spans(change), spans(credit));
        }
    }
    spans(text)
}

fn spans(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some((label, url, after)) = link(rest) {
            out.push_str(&anchor(url, label));
            rest = after;
        } else if let Some((url, after)) = bare_url(rest) {
            out.push_str(&anchor(url, &short_link_text(url)));
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

/// A link if it goes to the web; anything else (javascript:, relative paths) is
/// just its text.
fn anchor(url: &str, label: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") {
        format!("<a href=\"{}\">{}</a>", markup_escape_text(url), markup_escape_text(label))
    } else {
        markup_escape_text(label).to_string()
    }
}

/// GitHub links shortened the way GitHub itself shows them: pull requests and
/// issues as "#123", compare links as "v1.0...v1.1". Other links show in full.
fn short_link_text(url: &str) -> String {
    let path = url.trim_end_matches('/');
    for kind in ["/pull/", "/issues/"] {
        if let Some((_, number)) = path.rsplit_once(kind)
            && number.chars().all(|c| c.is_ascii_digit())
        {
            return format!("#{number}");
        }
    }
    if let Some((_, range)) = path.rsplit_once("/compare/") {
        return range.to_string();
    }
    url.to_string()
}

/// `[label](url)` at the start of `text`.
fn link(text: &str) -> Option<(&str, &str, &str)> {
    let rest = text.strip_prefix('[')?;
    let (label, rest) = rest.split_once("](")?;
    let (url, after) = rest.split_once(')')?;
    (!label.contains(']') && !url.contains(' ')).then_some((label, url, after))
}

/// A plain https:// link at the start of `text`, up to the next space.
fn bare_url(text: &str) -> Option<(&str, &str)> {
    if !(text.starts_with("https://") || text.starts_with("http://")) {
        return None;
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    // A full stop or comma right after a link belongs to the sentence.
    let url = text[..end].trim_end_matches(['.', ',', ')', ';']);
    Some((url, &text[url.len()..]))
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
    fn splits_github_generated_notes_into_blocks() {
        let notes = "## What's Changed\n* Faster **canvas** by @someone in https://github.com/o/r/pull/12\n\n\n\n**Full Changelog**: https://github.com/o/r/compare/v1.0...v1.1";
        assert_eq!(
            blocks(notes),
            [
                Block::Heading("What&apos;s Changed".into()),
                Block::Item(
                    0,
                    "Faster <b>canvas</b><span fgalpha=\"60%\"> by @someone in <a href=\"https://github.com/o/r/pull/12\">#12</a></span>".into()
                ),
                Block::Paragraph(
                    "<b>Full Changelog</b>: <a href=\"https://github.com/o/r/compare/v1.0...v1.1\">v1.0...v1.1</a>".into()
                ),
            ]
        );
    }

    #[test]
    fn joins_paragraph_lines_and_keeps_code() {
        assert_eq!(
            blocks("First line\nsecond line\n\n```\nx < y\n```\n1. One"),
            [
                Block::Paragraph("First line second line".into()),
                Block::Code("x &lt; y".into()),
                Block::Item(0, "One".into()),
            ]
        );
    }

    #[test]
    fn escapes_markup_and_drops_html() {
        assert_eq!(
            blocks("<details>\nx < y & z\n![shot](a.png)"),
            [Block::Paragraph("x &lt; y &amp; z".into())]
        );
    }

    #[test]
    fn leaves_non_web_links_as_text() {
        assert_eq!(blocks("[run](javascript:alert)"), [Block::Paragraph("run".into())]);
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
            for block in blocks(&std::fs::read_to_string(&path).unwrap()) {
                let (Block::Heading(markup) | Block::Item(_, markup) | Block::Paragraph(markup) | Block::Code(markup)) =
                    block;
                // Links are handled by GTK labels themselves, before the rest of the
                // markup reaches Pango, so set them aside the same way.
                let mut markup = markup;
                while let Some(start) = markup.find("<a href=\"") {
                    let end = start + markup[start..].find("\">").unwrap() + 2;
                    markup.replace_range(start..end, "");
                }
                let markup = markup.replace("</a>", "");
                if let Err(e) = gtk::pango::parse_markup(&markup, '\0') {
                    panic!("{}: {e}\n{markup}", path.display());
                }
            }
            checked += 1;
        }
        assert!(checked > 0);
    }
}
