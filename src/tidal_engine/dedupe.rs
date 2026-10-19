//! One recording once. A song listed again under another track id or
//! album is the same recording: by id, by ISRC, or (for versions without
//! an ISRC) by title and first artist with a duration within two seconds,
//! where an explicit and a clean version count as one. Each check is a hash
//! lookup on keys made once per track.

use std::collections::{HashMap, HashSet};

use super::LocalTrack;

/// Durations this close are the same recording.
const SAME_LENGTH_MS: u32 = 2_000;

#[derive(Clone, Debug, Default)]
pub struct Dedupe {
    uris: HashSet<String>,
    isrcs: HashSet<String>,
    versions: HashMap<(String, String), Vec<u32>>,
}

impl Dedupe {
    /// `true` and remembered when no copy of `track` came before.
    pub fn admit(&mut self, track: &LocalTrack) -> bool {
        if self.uris.contains(&track.uri) {
            return false;
        }
        if track
            .isrc
            .as_ref()
            .is_some_and(|isrc| self.isrcs.contains(isrc))
        {
            return false;
        }
        let key = (track.duration_ms > 0)
            .then(|| version_key(track))
            .flatten();
        if let Some(key) = &key
            && track.isrc.is_none()
            && self.versions.get(key).is_some_and(|lengths| {
                lengths
                    .iter()
                    .any(|length| length.abs_diff(track.duration_ms) <= SAME_LENGTH_MS)
            })
        {
            return false;
        }
        self.uris.insert(track.uri.clone());
        if let Some(isrc) = &track.isrc {
            self.isrcs.insert(isrc.clone());
        }
        if let Some(key) = key {
            self.versions
                .entry(key)
                .or_default()
                .push(track.duration_ms);
        }
        true
    }
}

fn version_key(track: &LocalTrack) -> Option<(String, String)> {
    let title = normalised(&track.title);
    let artist = normalised(&track.artists.first()?.name);
    (!title.is_empty() && !artist.is_empty()).then_some((title, artist))
}

/// Lower case words only, without a bracketed "explicit" or "clean" tag.
fn normalised(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '(' || c == '[' {
            let close = if c == '(' { ')' } else { ']' };
            let mut inner = String::new();
            let mut closed = false;
            for d in chars.by_ref() {
                if d == close {
                    closed = true;
                    break;
                }
                inner.push(d);
            }
            let tag = inner.trim().to_lowercase();
            let is_tag = matches!(
                tag.as_str(),
                "explicit"
                    | "clean"
                    | "edited"
                    | "explicit version"
                    | "clean version"
                    | "edited version"
            );
            if !(closed && is_tag) {
                kept.push(' ');
                kept.push_str(&inner);
                kept.push(' ');
            }
        } else {
            kept.push(c);
        }
    }
    let mut out = String::with_capacity(kept.len());
    for word in kept
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::ArtistRef;

    fn song(uri: &str, title: &str, artist: &str, ms: u32, isrc: Option<&str>) -> LocalTrack {
        LocalTrack {
            uri: uri.into(),
            title: title.into(),
            artists: vec![ArtistRef {
                name: artist.into(),
                ..ArtistRef::default()
            }],
            duration_ms: ms,
            isrc: isrc.map(str::to_string),
            ..LocalTrack::default()
        }
    }

    #[test]
    fn the_same_recording_comes_once() {
        let mut seen = Dedupe::default();
        assert!(seen.admit(&song("a:1", "Song", "Band", 200_000, Some("X1"))));
        assert!(!seen.admit(&song("a:1", "Other", "Other", 1, None)), "id");
        assert!(!seen.admit(&song("a:2", "Song (Live)", "Band", 90_000, Some("X1"))));
        assert!(seen.admit(&song("a:3", "Tune [Explicit]", "Band", 180_000, None)));
        assert!(!seen.admit(&song("a:4", "tune (Clean)", "BAND", 181_500, None)));
        assert!(
            seen.admit(&song("a:5", "Tune", "Band", 190_000, None)),
            "length"
        );
        assert!(seen.admit(&song("a:6", "Tune (Live)", "Band", 180_000, None)));
    }
}
