//! The release notes shipped inside the app, for the What's new page and the
//! update popup. `build.rs` embeds `packaging/release-notes/v*.md`.

use crate::updates::Semver;

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));

/// A run of text inside a line of release notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub code: bool,
    /// The address this run opens when clicked.
    pub link: Option<String>,
}

/// One block of release notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A section line such as **New** or **Fixed**, or a `#` heading.
    Heading(String),
    Paragraph(Vec<Span>),
    Bullet(Vec<Span>),
}

/// One release and its notes.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub version: String,
    /// `YYYY-MM-DD`, from the Flatpak metainfo.
    pub date: Option<String>,
    pub blocks: Vec<Block>,
}

/// Notes parsed from Markdown, without the affiliation line (the release
/// notes on GitHub keep it; the app does not show it).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Notes {
    pub blocks: Vec<Block>,
    /// The line saying the project is not affiliated with TIDAL, if present.
    pub footer: Option<String>,
}

/// Every embedded release, newest first.
pub fn entries() -> Vec<Entry> {
    let mut found: Vec<(Semver, Entry)> = RELEASE_NOTES
        .iter()
        .filter_map(|(version, notes)| {
            Some((
                Semver::parse(version)?,
                Entry {
                    version: (*version).to_string(),
                    date: RELEASE_DATES
                        .iter()
                        .find(|(found, _)| found == version)
                        .map(|(_, date)| (*date).to_string()),
                    blocks: parse(notes).blocks,
                },
            ))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, entry)| entry).collect()
}

/// The releases a person missed: those after `since` up to the running
/// version, newest first. Without `since`, every release.
pub fn entries_since(since: Option<&str>) -> Vec<Entry> {
    let current = Semver::parse(env!("CARGO_PKG_VERSION"));
    let since = since.and_then(Semver::parse);
    entries()
        .into_iter()
        .filter(|entry| {
            let version = Semver::parse(&entry.version);
            since
                .as_ref()
                .is_none_or(|since| version.as_ref() > Some(since))
                && (since.is_none() || current.is_none() || version <= current)
        })
        .collect()
}

/// `14 Oct 2026` for `2026-10-14`.
pub fn display_date(date: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = date.split('-');
    let year = parts.next()?.parse::<u32>().ok()?;
    let month = parts.next()?.parse::<usize>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    Some(format!(
        "{day} {} {year}",
        MONTHS.get(month.checked_sub(1)?)?
    ))
}

/// The small Markdown subset the notes use: paragraphs, `- ` bullets,
/// `#` headings, a line that is all bold as a section heading, `**bold**`,
/// `` `code` ``, `[label](address)` links and bare `https://` addresses. An
/// indented line continues the item above it.
pub fn parse(notes: &str) -> Notes {
    let mut out = Notes::default();
    let mut last_open = false;
    for raw in notes.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("<!--") {
            last_open = false;
            continue;
        }
        if line.contains("not affiliated with TIDAL") {
            out.footer = Some(spans_text(&spans(line)));
            last_open = false;
            continue;
        }
        if raw.starts_with(char::is_whitespace)
            && last_open
            && let Some(Block::Bullet(items) | Block::Paragraph(items)) = out.blocks.last_mut()
        {
            items.push(Span {
                text: " ".into(),
                bold: false,
                code: false,
                link: None,
            });
            items.extend(spans(line));
            continue;
        }
        last_open = true;
        if line.starts_with('#') {
            out.blocks.push(Block::Heading(spans_text(&spans(
                line.trim_start_matches('#').trim_start(),
            ))));
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            out.blocks.push(Block::Bullet(spans(item)));
        } else {
            let parsed = spans(line);
            if parsed.len() == 1 && parsed[0].bold {
                last_open = false;
                out.blocks
                    .push(Block::Heading(parsed.into_iter().next().unwrap().text));
            } else {
                out.blocks.push(Block::Paragraph(parsed));
            }
        }
    }
    out
}

fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// Splits a line at `**` and `` ` `` marks.
fn spans(line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut code) = (false, false);
    let mut text = String::new();
    let flush = |out: &mut Vec<Span>, text: &mut String, bold: bool, code: bool| {
        if text.is_empty() {
            return;
        }
        let text = std::mem::take(text);
        if code {
            out.push(Span {
                text,
                bold,
                code,
                link: None,
            });
        } else {
            out.extend(links(&text, bold));
        }
    };
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            flush(&mut out, &mut text, bold, code);
            code = !code;
        } else if c == '*' && !code && chars.peek() == Some(&'*') {
            chars.next();
            flush(&mut out, &mut text, bold, code);
            bold = !bold;
        } else {
            text.push(c);
        }
    }
    flush(&mut out, &mut text, bold, code);
    out
}

/// Splits `text` into plain runs and links: `[label](address)` and bare
/// `http(s)://` addresses. Punctuation that ends a sentence after a bare
/// address (`.`, `,`, `)` and the like) is not part of it.
fn links(text: &str, bold: bool) -> Vec<Span> {
    let span = |text: &str, link: Option<String>| Span {
        text: text.to_string(),
        bold,
        code: false,
        link,
    };
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let markdown = rest.find('[').and_then(|open| {
            let close = open + rest[open..].find("](")?;
            let end = close + 2 + rest[close + 2..].find(')')?;
            let address = &rest[close + 2..end];
            is_address(address).then_some((open, end + 1, &rest[open + 1..close], address))
        });
        let bare = ["https://", "http://"]
            .iter()
            .filter_map(|scheme| rest.find(scheme))
            .min()
            .map(|start| {
                let len = rest[start..]
                    .find(char::is_whitespace)
                    .unwrap_or(rest.len() - start);
                let address = rest[start..start + len]
                    .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'', '"']);
                (start, start + address.len(), address, address)
            });
        let next = match (markdown, bare) {
            (Some(m), Some(b)) => Some(if m.0 <= b.0 { m } else { b }),
            (m, b) => m.or(b),
        };
        let Some((start, end, label, address)) = next else {
            break;
        };
        if start > 0 {
            out.push(span(&rest[..start], None));
        }
        out.push(span(label, Some(address.to_string())));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        out.push(span(rest, None));
    }
    out
}

fn is_address(text: &str) -> bool {
    (text.starts_with("https://") || text.starts_with("http://"))
        && !text.contains(char::is_whitespace)
}

/// Whether `current` is a newer version than `last_run`.
pub fn is_newer(current: &str, last_run: &str) -> bool {
    match (Semver::parse(current), Semver::parse(last_run)) {
        (Some(current), Some(last)) => current > last,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_embedded_newest_first() {
        let all = entries();
        assert!(!all.is_empty());
        for pair in all.windows(2) {
            assert!(
                Semver::parse(&pair[0].version) > Semver::parse(&pair[1].version),
                "{} before {}",
                pair[0].version,
                pair[1].version
            );
        }
    }

    #[test]
    fn a_bold_lead_in_stays_bold_and_the_rest_does_not() {
        let notes = parse("- **Music videos.** Play `videos` here.");
        let [Block::Bullet(items)] = notes.blocks.as_slice() else {
            panic!("{:?}", notes.blocks);
        };
        assert!(items[0].bold && items[0].text == "Music videos.");
        assert!(!items[1].bold && items[1].text == " Play ");
        assert!(items[2].code && items[2].text == "videos");
        assert_eq!(spans_text(items), "Music videos. Play videos here.");
    }

    #[test]
    fn sections_paragraphs_and_bullets_are_told_apart() {
        let notes = parse("Intro line.\n\n**New**\n- One\n  more\n* Two\n\n# Fixed\n");
        assert_eq!(notes.blocks.len(), 5);
        assert!(matches!(&notes.blocks[0], Block::Paragraph(_)));
        assert_eq!(notes.blocks[1], Block::Heading("New".into()));
        let Block::Bullet(first) = &notes.blocks[2] else {
            panic!()
        };
        assert_eq!(spans_text(first), "One more");
        assert!(matches!(&notes.blocks[3], Block::Bullet(_)));
        assert_eq!(notes.blocks[4], Block::Heading("Fixed".into()));
    }

    #[test]
    fn the_affiliation_line_is_left_out_of_the_body() {
        let line = "Tidalfast is an independent project and is not affiliated with TIDAL.";
        let notes = parse(&format!("Hi.\n\n{line}\n"));
        assert_eq!(notes.blocks.len(), 1);
        assert_eq!(notes.footer.as_deref(), Some(line));
        for entry in entries() {
            assert!(!entry.blocks.iter().any(|block| matches!(block,
                Block::Paragraph(s) | Block::Bullet(s) if spans_text(s).contains("not affiliated"))));
        }
    }

    /// A bare address and a `[label](address)` link are links; the rest of
    /// the line stays text.
    #[test]
    fn addresses_and_markdown_links_become_links() {
        let notes = parse(
            "**Full changelog:** https://github.com/o/r/compare/v1.0.0...v1.0.1\n\n\
             Which file is under [Install](https://github.com/o/r#install) in the README.",
        );
        let [Block::Paragraph(first), Block::Paragraph(second)] = notes.blocks.as_slice() else {
            panic!("{:?}", notes.blocks);
        };
        let link = first
            .iter()
            .find(|span| span.link.is_some())
            .expect("a link");
        assert_eq!(link.text, "https://github.com/o/r/compare/v1.0.0...v1.0.1");
        assert_eq!(link.link.as_deref(), Some(link.text.as_str()));
        assert!(first[0].bold && first[0].link.is_none());
        let install = second
            .iter()
            .find(|span| span.link.is_some())
            .expect("a link");
        assert_eq!(install.text, "Install");
        assert_eq!(
            install.link.as_deref(),
            Some("https://github.com/o/r#install")
        );
        assert_eq!(
            spans_text(second),
            "Which file is under Install in the README."
        );
        // A full stop after an address ends the sentence, not the address.
        let Block::Paragraph(dot) = &parse("See https://example.com/a.").blocks[0] else {
            panic!()
        };
        assert_eq!(dot[1].link.as_deref(), Some("https://example.com/a"));
        assert_eq!(dot[2].text, ".");
    }

    #[test]
    fn only_the_missed_releases_are_listed() {
        let all = entries();
        assert_eq!(entries_since(None).len(), all.len());
        assert!(entries_since(Some("99.0.0")).is_empty());
        let newest = &all[0].version;
        let oldest = &all[all.len() - 1].version;
        let missed = entries_since(Some(oldest));
        assert_eq!(missed.len(), all.len() - 1);
        assert!(missed.iter().all(|entry| entry.version != *oldest));
        assert!(entries_since(Some(newest)).is_empty());
    }

    #[test]
    fn dates_come_from_the_metainfo() {
        assert_eq!(display_date("2026-10-14").as_deref(), Some("14 Oct 2026"));
        assert_eq!(display_date("nonsense"), None);
        assert!(entries().iter().all(|entry| entry.date.is_some()));
    }

    #[test]
    fn only_a_higher_version_counts_as_newer() {
        assert!(is_newer("0.13.0", "0.12.0"));
        assert!(is_newer("0.12.0", "0.12.0-beta.1"));
        assert!(!is_newer("0.12.0", "0.12.0"));
        assert!(!is_newer("0.11.0", "0.12.0"));
        assert!(!is_newer("0.12.0", "garbage"));
    }
}
