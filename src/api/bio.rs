//! TIDAL's artist biography markup: plain text with `<br/>` breaks and
//! `[wimpLink artistId="123"]Name[/wimpLink]` / `albumId="456"` links.

use serde::{Deserialize, Serialize};

/// A biography as TIDAL sends it, with where it came from.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ArtistBio {
    /// The text with its markup, as the API returns it.
    pub text: String,
    /// "TiVo", "Wikipedia", ... when TIDAL names one.
    pub source: Option<String>,
}

/// What a biography link opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BioTarget {
    Artist(String),
    Album(String),
}

/// One run of a paragraph: plain text or a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BioPiece {
    Text(String),
    Link { text: String, target: BioTarget },
}

/// The biography as paragraphs of pieces. Blank lines and runs of `<br/>`
/// separate paragraphs; other tags go.
pub fn parse(markup: &str) -> Vec<Vec<BioPiece>> {
    let mut paragraphs: Vec<Vec<BioPiece>> = vec![Vec::new()];
    let mut text = String::new();
    let mut breaks = 0usize;
    let mut rest = markup;
    fn flush(paragraphs: &mut [Vec<BioPiece>], text: &mut String) {
        if !text.is_empty() {
            paragraphs
                .last_mut()
                .expect("one paragraph")
                .push(BioPiece::Text(std::mem::take(text)));
        }
    }
    while let Some(start) = rest.find(['[', '<']) {
        let (before, tag_and_rest) = rest.split_at(start);
        if !before.is_empty() {
            text.push_str(before);
            breaks = 0;
        }
        let close = if tag_and_rest.starts_with('[') {
            ']'
        } else {
            '>'
        };
        let Some(end) = tag_and_rest.find(close) else {
            text.push_str(tag_and_rest);
            rest = "";
            break;
        };
        let tag = &tag_and_rest[1..end];
        let after = &tag_and_rest[end + 1..];
        let lower = tag.trim().to_ascii_lowercase();
        if lower == "br" || lower == "br/" || lower == "br /" {
            flush(&mut paragraphs, &mut text);
            breaks += 1;
            if breaks == 2 {
                paragraphs.push(Vec::new());
            }
            rest = after;
        } else if lower.starts_with("wimplink") {
            let target = attribute(tag, "artistId")
                .map(BioTarget::Artist)
                .or_else(|| attribute(tag, "albumId").map(BioTarget::Album));
            let (label, tail) = match after.find("[/wimpLink]") {
                Some(close) => (&after[..close], &after[close + "[/wimpLink]".len()..]),
                None => (after, ""),
            };
            breaks = 0;
            match target {
                Some(target) if !label.is_empty() => {
                    flush(&mut paragraphs, &mut text);
                    paragraphs
                        .last_mut()
                        .expect("one paragraph")
                        .push(BioPiece::Link {
                            text: label.to_string(),
                            target,
                        });
                }
                _ => text.push_str(label),
            }
            rest = tail;
        } else if lower.starts_with("/wimplink") {
            rest = after;
        } else if tag_and_rest.starts_with('<') {
            // Some other tag: dropped.
            rest = after;
        } else {
            // A bracket that is not markup stays as written.
            text.push('[');
            rest = &tag_and_rest[1..];
        }
    }
    text.push_str(rest);
    flush(&mut paragraphs, &mut text);
    for paragraph in &mut paragraphs {
        if let Some(BioPiece::Text(first)) = paragraph.first_mut() {
            *first = first.trim_start().to_string();
        }
        if let Some(BioPiece::Text(last)) = paragraph.last_mut() {
            *last = last.trim_end().to_string();
        }
        paragraph.retain(|piece| !matches!(piece, BioPiece::Text(text) if text.is_empty()));
    }
    paragraphs.retain(|paragraph| !paragraph.is_empty());
    paragraphs
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(name)?;
    let value = tag[at + name.len()..].trim_start().strip_prefix('=')?;
    let value = value.trim_start().strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_string()).filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_paragraphs_come_apart() {
        let paragraphs = parse(
            "Born in Detroit, [wimpLink artistId=\"123\"]Dr. Dre[/wimpLink] and the album \
             [wimpLink albumId=\"45\"]The Slim Shady LP[/wimpLink].<br/><br/>Later work.",
        );
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(
            paragraphs[0],
            vec![
                BioPiece::Text("Born in Detroit, ".into()),
                BioPiece::Link {
                    text: "Dr. Dre".into(),
                    target: BioTarget::Artist("123".into())
                },
                BioPiece::Text(" and the album ".into()),
                BioPiece::Link {
                    text: "The Slim Shady LP".into(),
                    target: BioTarget::Album("45".into())
                },
                BioPiece::Text(".".into()),
            ]
        );
        assert_eq!(paragraphs[1], vec![BioPiece::Text("Later work.".into())]);
    }

    #[test]
    fn broken_markup_stays_readable() {
        let paragraphs = parse("A [wimpLink]Nobody[/wimpLink] [x] <b>bold</b> end");
        assert_eq!(
            paragraphs,
            vec![vec![BioPiece::Text("A Nobody [x] bold end".into())]]
        );
        assert!(parse("").is_empty());
    }
}
