//! Just enough Markdown for release notes and app descriptions. Text is split into
//! blocks (headings, list items, paragraphs, code) so it can be laid out with
//! proper spacing, and each block's text becomes the markup GTK labels show:
//! **bold**, *italic*, `code` and links. Long GitHub links are shortened
//! (".../pull/7155" becomes "#7155") and the "by @someone in ..." credit on each
//! line is dimmed.

use gtk::glib::markup_escape_text;
use gtk::prelude::*;

#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    /// A list item: nesting depth, then its markup.
    Item(usize, String),
    Paragraph(String),
    Code(String),
}

pub fn blocks(markdown: &str) -> Vec<Block> {
    let lines = plain_lines(markdown);
    let mut blocks = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let mut paragraph: Vec<String> = Vec::new();

    let flush = |paragraph: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };

    for raw in &lines {
        let raw = raw.as_str();
        if raw.trim().starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            match code.take() {
                Some(lines) => blocks.push(Block::Code(markup_escape_text(&lines.join("\n")).to_string())),
                None => code = Some(Vec::new()),
            }
            continue;
        }
        if let Some(lines) = &mut code {
            lines.push(raw);
            continue;
        }

        let line = raw;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        // "Title" underlined with === or --- on the next line is a heading.
        if !paragraph.is_empty() && is_underline(trimmed) {
            blocks.push(Block::Heading(inline(&paragraph.join(" "))));
            paragraph.clear();
            continue;
        }
        if is_rule(trimmed) {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        // Tables: a line per row, cells separated by dots. The |---|---| line under
        // the header is dropped.
        if trimmed.len() > 1 && trimmed.starts_with('|') && trimmed.ends_with('|') {
            flush(&mut paragraph, &mut blocks);
            if trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                continue;
            }
            let cells: Vec<_> = trimmed[1..trimmed.len() - 1]
                .split('|')
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(inline)
                .collect();
            if !cells.is_empty() {
                blocks.push(Block::Paragraph(cells.join("  ·  ")));
            }
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if let Some(heading) = heading_text(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(inline(heading)));
        } else if let Some(item) = list_item(trimmed) {
            flush(&mut paragraph, &mut blocks);
            // Checklists: "- [x] Done" and "- [ ] Not yet".
            let item = match item.get(..4) {
                Some("[x] " | "[X] ") => format!("☑ {}", &item[4..]),
                Some("[ ] ") => format!("☐ {}", &item[4..]),
                _ => item.to_string(),
            };
            blocks.push(Block::Item(indent / 2, inline(&item)));
        } else {
            paragraph.push(trimmed.to_string());
        }
    }
    flush(&mut paragraph, &mut blocks);
    if let Some(lines) = code {
        blocks.push(Block::Code(markup_escape_text(&lines.join("\n")).to_string()));
    }
    blocks
}

/// The text's lines with comments, quote marks and HTML dealt with, so only
/// Markdown is left. Code blocks are left alone.
fn plain_lines(markdown: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let (mut in_code, mut in_comment) = (false, false);
    for raw in markdown.lines() {
        if raw.trim().starts_with("```") {
            in_code = !in_code;
        }
        if in_code || raw.trim().starts_with("```") {
            lines.push(raw.to_string());
            continue;
        }
        let line = html_to_markdown(&unquote(&without_comments(raw, &mut in_comment)));
        // HTML paragraphs, breaks and list items start lines of their own.
        lines.extend(line.split('\n').map(str::to_string));
    }
    lines
}

/// `line` without HTML comments, which can run over several lines.
fn without_comments(line: &str, in_comment: &mut bool) -> String {
    let mut line = line.to_string();
    if *in_comment {
        match line.find("-->") {
            Some(end) => {
                line.replace_range(..end + 3, "");
                *in_comment = false;
            }
            None => return String::new(),
        }
    }
    while let Some(start) = line.find("<!--") {
        match line[start..].find("-->") {
            Some(end) => line.replace_range(start..start + end + 3, ""),
            None => {
                line.truncate(start);
                *in_comment = true;
            }
        }
    }
    line
}

/// Quotes read as ordinary text. GitHub's "> [!NOTE]" labels become a bold word.
fn unquote(line: &str) -> String {
    let mut rest = line.trim_start();
    if !rest.starts_with('>') {
        return line.to_string();
    }
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.trim_start();
    }
    for (marker, label) in [
        ("[!NOTE]", "**Note**"),
        ("[!TIP]", "**Tip**"),
        ("[!IMPORTANT]", "**Important**"),
        ("[!WARNING]", "**Warning**"),
        ("[!CAUTION]", "**Caution**"),
    ] {
        if rest.get(..marker.len()).is_some_and(|start| start.eq_ignore_ascii_case(marker)) {
            return format!("{label}{}", &rest[marker.len()..]);
        }
    }
    rest.to_string()
}

/// HTML tags in a line, as the Markdown this reader understands: paragraphs,
/// bullets, headings, links, bold, italic, code and strikethrough are kept (so the
/// result can run over several lines); images and everything else go, but their
/// text stays.
fn html_to_markdown(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        // <https://example.com> is a link written out in full.
        if (after.starts_with("http://") || after.starts_with("https://"))
            && let Some(end) = after.find('>').filter(|&end| !after[..end].contains(' '))
        {
            out.push_str(&after[..end]);
            rest = &after[end + 1..];
            continue;
        }
        let Some((tag, closing, attrs, after_tag)) = html_tag(after) else {
            // Just a "<", like "x < y".
            out.push('<');
            rest = after;
            continue;
        };
        rest = after_tag;
        match (tag.as_str(), closing) {
            ("a", false) => {
                let href = attribute(attrs, "href").filter(|href| href.starts_with("https://") || href.starts_with("http://"));
                // Only a link that closes on this line can be turned into one.
                if let (Some(href), Some(end)) = (href, rest.find("</a>")) {
                    out.push_str(&format!("[{}]({href})", html_to_markdown(&rest[..end])));
                    rest = &rest[end + 4..];
                }
            }
            ("b" | "strong" | "summary", _) => out.push_str("**"),
            ("i" | "em", _) => out.push('*'),
            ("code" | "kbd" | "tt", _) => out.push('`'),
            ("s" | "del" | "strike", _) => out.push_str("~~"),
            ("li", false) => out.push_str("\n- "),
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => out.push_str("\n\n## "),
            ("br" | "p" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6", _) => out.push_str("\n\n"),
            ("td" | "th", _) => out.push(' '),
            _ => {}
        }
    }
    out.push_str(rest);
    out
}

/// A tag at the start of `text` (just after its "<"): its lowercase name, whether
/// it's a closing tag, its attributes, and the text after its ">".
fn html_tag(text: &str) -> Option<(String, bool, &str, &str)> {
    let (closing, text) = match text.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if !text.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let end = text.find('>')?;
    let inside = &text[..end];
    if inside.contains('<') {
        return None;
    }
    let name_end = inside.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(inside.len());
    Some((inside[..name_end].to_ascii_lowercase(), closing, &inside[name_end..], &text[end + 1..]))
}

/// The value of `name="value"` (or 'value') in a tag's attributes.
fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let at = attrs.find(&format!("{name}="))? + name.len() + 1;
    let value = &attrs[at..];
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    value[1..].split(quote).next()
}

/// "===" or "---" under a line, making it a heading.
fn is_underline(line: &str) -> bool {
    line.len() >= 2 && (line.chars().all(|c| c == '=') || line.chars().all(|c| c == '-'))
}

/// A horizontal rule: "---", "***", "___" or spaced out like "* * *".
fn is_rule(line: &str) -> bool {
    let marks: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3 && ["-", "*", "_"].iter().any(|mark| marks.chars().all(|c| c.to_string() == *mark))
}

fn heading_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let level = line.len() - rest.len();
    // Some write "## Title ##".
    ((1..=6).contains(&level) && rest.starts_with(' ')).then(|| rest.trim().trim_end_matches('#').trim_end())
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
    let text = &decode_entities(without_images(text).trim());
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
        let after_word = out.chars().last().is_some_and(char::is_alphanumeric);
        if let Some((label, url, after)) = link(rest) {
            // A link around nothing, like a badge whose image was removed, goes too.
            let label = label.replace("**", "").replace('`', "");
            if !label.trim().is_empty() {
                out.push_str(&anchor(url, label.trim()));
                rest = after;
            } else if out.is_empty() || out.ends_with(' ') {
                rest = after.trim_start();
            } else {
                rest = after;
            }
        } else if let Some(escaped) = rest.strip_prefix('\\').and_then(|after| after.chars().next()).filter(char::is_ascii_punctuation) {
            // \# and \_ mean a plain # and _.
            out.push_str(&markup_escape_text(&escaped.to_string()));
            rest = &rest[1 + escaped.len_utf8()..];
        } else if let Some((emoji, after)) = emoji(rest) {
            out.push_str(emoji);
            rest = after;
        } else if let Some((url, after)) = bare_url(rest) {
            out.push_str(&anchor(url, &short_link_text(url)));
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "**") {
            out.push_str(&format!("<b>{}</b>", markup_escape_text(inner)));
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "__").filter(|_| !after_word) {
            // Not inside_words_like__this__, which are names, not bold.
            out.push_str(&format!("<b>{}</b>", markup_escape_text(inner)));
            rest = after;
        } else if let Some((inner, after)) = wrapped(rest, "~~") {
            out.push_str(&format!("<s>{}</s>", markup_escape_text(inner)));
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

/// Images (`![alt](url)`) can't show inline, and their alt text is usually a badge
/// label like "build passing", so they're left out.
fn without_images(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        match link(&rest[start + 1..]) {
            Some((_, _, after)) => rest = after,
            None => {
                out.push_str("![");
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// HTML character codes like `&nbsp;` and `&#39;`, as the characters they stand for.
fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let decoded = after.find(';').filter(|&end| end <= 8).and_then(|end| {
            let name = &after[..end];
            let c = match name {
                "nbsp" => ' ',
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "mdash" => '—',
                "ndash" => '–',
                "hellip" => '…',
                "copy" => '©',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, &after[end + 1..]))
        });
        match decoded {
            Some((c, after)) => {
                out.push(c);
                rest = after;
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// GitHub's emoji codes like `:rocket:`, for the ones release notes use most.
fn emoji(text: &str) -> Option<(&'static str, &str)> {
    let rest = text.strip_prefix(':')?;
    let end = rest.find(':')?;
    let emoji = match &rest[..end] {
        "rocket" => "🚀",
        "tada" => "🎉",
        "sparkles" => "✨",
        "bug" => "🐛",
        "fire" => "🔥",
        "zap" => "⚡",
        "memo" => "📝",
        "books" => "📚",
        "package" => "📦",
        "inbox_tray" => "📥",
        "wrench" => "🔧",
        "hammer" => "🔨",
        "hammer_and_wrench" => "🛠️",
        "lock" => "🔒",
        "art" => "🎨",
        "lipstick" => "💄",
        "recycle" => "♻️",
        "boom" => "💥",
        "warning" => "⚠️",
        "construction" => "🚧",
        "white_check_mark" => "✅",
        "heavy_check_mark" => "✔️",
        "x" => "❌",
        "arrow_up" => "⬆️",
        "arrow_down" => "⬇️",
        "new" => "🆕",
        "star" => "⭐",
        "heart" => "❤️",
        "green_heart" => "💚",
        "globe_with_meridians" => "🌐",
        "computer" => "💻",
        "penguin" => "🐧",
        "apple" => "🍎",
        "gear" => "⚙️",
        "mag" => "🔍",
        "pencil2" => "✏️",
        "speech_balloon" => "💬",
        "point_right" => "👉",
        "eyes" => "👀",
        "pray" => "🙏",
        "+1" | "thumbsup" => "👍",
        _ => return None,
    };
    Some((emoji, &rest[end + 1..]))
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

/// A block laid out with room around it, for release notes and app descriptions.
pub fn widget(block: Block, first: bool) -> gtk::Widget {
    match block {
        Block::Heading(markup) => {
            let label = text_label(&markup);
            label.add_css_class("heading");
            // Space above each heading, apart from one that starts the notes.
            label.set_margin_top(if first { 0 } else { 14 });
            label.upcast()
        }
        Block::Item(depth, markup) => {
            // A hanging indent: wrapped lines line up under the text, not the bullet.
            let row = gtk::Box::builder()
                .spacing(10)
                .margin_start(6 + 22 * depth as i32)
                .build();
            row.append(
                &gtk::Label::builder()
                    .label(if depth == 0 { "•" } else { "◦" })
                    .yalign(0.0)
                    .css_classes(["dim-label"])
                    .build(),
            );
            let text = text_label(&markup);
            text.set_hexpand(true);
            row.append(&text);
            row.upcast()
        }
        Block::Paragraph(markup) => {
            let label = text_label(&markup);
            label.set_margin_top(if first { 0 } else { 4 });
            label.upcast()
        }
        Block::Code(markup) => {
            let label = text_label(&markup);
            label.add_css_class("monospace");
            let frame = gtk::Frame::builder().child(&label).css_classes(["view"]).build();
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            label.set_margin_start(10);
            label.set_margin_end(10);
            frame.upcast()
        }
    }
}

pub fn text_label(markup: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .selectable(true)
        .build();
    label.set_markup(markup);
    label
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
    fn tables_become_a_line_per_row() {
        assert_eq!(
            blocks("| Platform | File |\n|---|:---:|\n| Linux | `app.AppImage` |"),
            [Block::Paragraph("Platform  ·  File".into()), Block::Paragraph("Linux  ·  <tt>app.AppImage</tt>".into())]
        );
    }

    #[test]
    fn underlined_titles_are_headings_and_rules_vanish() {
        assert_eq!(
            blocks("Changes\n=======\nText\n\n* * *\nMore\n---"),
            [Block::Heading("Changes".into()), Block::Paragraph("Text".into()), Block::Heading("More".into())]
        );
        assert_eq!(blocks("## Speedy ##"), [Block::Heading("Speedy".into())]);
    }

    #[test]
    fn html_keeps_its_text_and_links() {
        assert_eq!(
            blocks("<details><summary>Changelog</summary>\n<p>Fixed <b>crash</b> <img src=\"x.png\"> on <a href=\"https://x.org/a\">start</a><br></p>\n</details>"),
            [
                Block::Paragraph("<b>Changelog</b>".into()),
                Block::Paragraph("Fixed <b>crash</b>  on <a href=\"https://x.org/a\">start</a>".into())
            ]
        );
        // Catalogues from before descriptions were cleaned up.
        assert_eq!(
            blocks("<p>Features:</p>\n<ul>\n  <li>Simple Qt UI.</li>\n  <li>No size limits.</li>\n</ul>"),
            [
                Block::Paragraph("Features:".into()),
                Block::Item(0, "Simple Qt UI.".into()),
                Block::Item(0, "No size limits.".into()),
            ]
        );
        assert_eq!(
            blocks("<!-- hidden\nstill hidden -->\nShown <https://x.org>"),
            [Block::Paragraph("Shown <a href=\"https://x.org\">https://x.org</a>".into())]
        );
    }

    #[test]
    fn quotes_alerts_and_checklists() {
        assert_eq!(
            blocks("> [!WARNING]\n> Back up first.\n\n- [x] Done\n- [ ] Not yet"),
            [
                Block::Paragraph("<b>Warning</b> Back up first.".into()),
                Block::Item(0, "☑ Done".into()),
                Block::Item(0, "☐ Not yet".into()),
            ]
        );
        // Short quotes in other scripts once crashed the alert check.
        assert_eq!(blocks("> خبر"), [Block::Paragraph("خبر".into())]);
    }

    #[test]
    fn badges_entities_escapes_and_emoji() {
        assert_eq!(
            blocks("[![build](https://x.org/b.svg)](https://x.org/ci) :rocket: Fast&nbsp;&amp;\\#1 ~~old~~ __new__ my__name__"),
            [Block::Paragraph("🚀 Fast &amp;#1 <s>old</s> <b>new</b> my__name__".into())]
        );
    }

    #[test]
    fn leaves_non_web_links_as_text() {
        assert_eq!(blocks("[run](javascript:alert)"), [Block::Paragraph("run".into())]);
    }

    /// Every release note in a locally built catalogue (outpost-index/site/notes,
    /// or the folder in OUTPOST_NOTES_DIR) must come out as markup GTK accepts.
    /// Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_release_notes_make_valid_markup() {
        let dir = std::env::var_os("OUTPOST_NOTES_DIR").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../outpost-index/site/notes"),
            std::path::PathBuf::from,
        );
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
