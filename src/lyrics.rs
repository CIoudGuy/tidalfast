//! Lyrics for the playing track, from TIDAL or LRCLIB.
//!
//! [LRCLIB](https://lrclib.net) provides plain and LRC-synced lyrics without an
//! account or key. Tidalfast tries TIDAL's transcription first when the
//! playback session is signed in, then LRCLIB.
//!
//! Matching starts with an exact lookup, then ranks search results. Track
//! length is the strongest signal for distinguishing versions with the same
//! title.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

const API: &str = "https://lrclib.net/api";
/// Lyrics do not change; a cached answer is good for this long.
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// A candidate this far from the playing track's length is another
/// recording, however well the names match.
const MAX_DRIFT_SECS: f64 = 30.0;

/// Track metadata used for lyrics lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_ms: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// When the line starts; `None` in lyrics that carry no timing.
    pub at_ms: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    /// Every line has a time, so the panel can follow the song.
    pub synced: bool,
    /// The database knows there are no words.
    pub instrumental: bool,
}

impl Lyrics {
    /// The line being sung at `position_ms`, or `None` before the first one
    /// starts and for lyrics without timing.
    pub fn active_line(&self, position_ms: u32) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let started = self
            .lines
            .partition_point(|line| line.at_ms.is_some_and(|at| at <= position_ms));
        started.checked_sub(1)
    }

    /// How long until the next line starts, from `position_ms`. A line
    /// that starts at exactly `position_ms` has started, so it is not it.
    pub fn until_next_line(&self, position_ms: u32) -> Option<u32> {
        if !self.synced {
            return None;
        }
        let started = self
            .lines
            .partition_point(|line| line.at_ms.is_some_and(|at| at <= position_ms));
        let at = self.lines.get(started)?.at_ms?;
        Some(at - position_ms)
    }
}

/// The position the lyrics follow: what the speakers play, which is the
/// decoder's `position_ms` less the sound still queued and in the device.
pub fn heard_position_ms(position_ms: u32, output_delay_ms: u32) -> u32 {
    position_ms.saturating_sub(output_delay_ms)
}

/// The cached answer at `path`, while it is fresh.
pub fn cached(path: &Path) -> Option<Option<Lyrics>> {
    read_cache(&path.to_path_buf())
}

/// Remember an answer at `path`, `None` included.
pub fn store(path: &Path, found: &Option<Lyrics>) {
    write_cache(path, found);
}

/// Fetches lyrics for `query`, using the disk cache when available.
pub async fn fetch(
    http: &reqwest::Client,
    cache_dir: &Path,
    query: &Query,
) -> Result<Option<Lyrics>> {
    fetch_from(http, API, cache_dir, query, &RETRY_DELAYS).await
}

/// What the interface shows, in place of a status, when LRCLIB is busy.
pub const BUSY: &str = "lyrics-service-busy";
/// What the interface shows when LRCLIB cannot be reached at all.
pub const UNREACHABLE: &str = "lyrics-service-unreachable";

/// How long to wait before each automatic retry of a transient failure.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(5)];
/// A `Retry-After` longer than this is not waited for.
const RETRY_AFTER_MAX: Duration = Duration::from_secs(10);

/// A failure that may pass: LRCLIB busy (429, 500, 502, 503, 504) or not
/// reachable (timeout, connection error).
#[derive(Debug)]
enum Transient {
    Busy {
        status: reqwest::StatusCode,
        retry_after: Option<Duration>,
    },
    Unreachable(reqwest::Error),
}

impl std::fmt::Display for Transient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy { status, .. } => write!(f, "LRCLIB answered {status}"),
            Self::Unreachable(error) => write!(f, "cannot reach LRCLIB: {error}"),
        }
    }
}

impl std::error::Error for Transient {}

/// The text for `Event::Lyrics`: a code the interface turns into a plain
/// message for a transient failure, the technical detail otherwise.
pub fn failure_detail(error: &anyhow::Error) -> String {
    match error.downcast_ref::<Transient>() {
        Some(Transient::Busy { .. }) => BUSY.to_owned(),
        Some(Transient::Unreachable(_)) => UNREACHABLE.to_owned(),
        None => format!("{error:#}"),
    }
}

/// [`fetch`] against `api`, retrying a transient failure after each of
/// `delays`. A transient failure is never cached.
async fn fetch_from(
    http: &reqwest::Client,
    api: &str,
    cache_dir: &Path,
    query: &Query,
    delays: &[Duration],
) -> Result<Option<Lyrics>> {
    let cache_path = cache_dir.join(format!("{}.json", cache_key(query)));
    if let Some(cached) = read_cache(&cache_path) {
        return Ok(cached);
    }
    let mut delays = delays.iter();
    let found = loop {
        match lookup(http, api, query).await {
            Ok(found) => break found,
            Err(error) => {
                let Some(transient) = error.downcast_ref::<Transient>() else {
                    return Err(error);
                };
                log::info!("lyrics: {transient}");
                let Some(&delay) = delays.next() else {
                    return Err(error);
                };
                let wait = match transient {
                    Transient::Busy {
                        retry_after: Some(after),
                        ..
                    } if *after <= RETRY_AFTER_MAX => *after,
                    _ => delay,
                };
                tokio::time::sleep(wait).await;
            }
        }
    };
    write_cache(&cache_path, &found);
    Ok(found)
}

async fn lookup(http: &reqwest::Client, api: &str, query: &Query) -> Result<Option<Lyrics>> {
    let title = clean_title(&query.title);
    let artist = clean_artist(&query.artist);
    if title.is_empty() || artist.is_empty() {
        return Ok(None);
    }
    let duration = (query.duration_ms / 1000).to_string();
    let mut exact = vec![
        ("artist_name", artist.as_str()),
        ("track_name", title.as_str()),
    ];
    if !query.album.trim().is_empty() {
        exact.push(("album_name", query.album.trim()));
    }
    if query.duration_ms > 0 {
        exact.push(("duration", duration.as_str()));
    }
    if let Some(record) = get::<Record>(http, api, "/get", &exact).await?
        && let Some(lyrics) = record.lyrics()
    {
        return Ok(Some(lyrics));
    }
    let candidates = get::<Vec<Record>>(
        http,
        api,
        "/search",
        &[
            ("artist_name", artist.as_str()),
            ("track_name", title.as_str()),
        ],
    )
    .await?
    .unwrap_or_default();
    Ok(pick(candidates, query).and_then(|record| record.lyrics()))
}

/// One LRCLIB request. A 404 is "nobody has transcribed this" and a 400 is
/// "that is not a question I can answer"; neither is a fault worth showing.
async fn get<T: DeserializeOwned>(
    http: &reqwest::Client,
    api: &str,
    path: &str,
    params: &[(&str, &str)],
) -> Result<Option<T>> {
    let response = match http
        .get(format!("{api}{path}"))
        .query(params)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) if error.is_timeout() || error.is_connect() => {
            return Err(Transient::Unreachable(error).into());
        }
        Err(error) => return Err(error).context("cannot reach LRCLIB"),
    };
    let status = response.status();
    if matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504) {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        return Err(Transient::Busy {
            status,
            retry_after,
        }
        .into());
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::BAD_REQUEST {
        return Ok(None);
    }
    if !status.is_success() {
        anyhow::bail!("LRCLIB answered {status}");
    }
    Ok(Some(
        response
            .json()
            .await
            .context("unexpected answer from LRCLIB")?,
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    track_name: String,
    artist_name: String,
    duration: Option<f64>,
    instrumental: bool,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

impl Record {
    fn synced(&self) -> Option<&str> {
        self.synced_lyrics
            .as_deref()
            .filter(|text| !text.trim().is_empty())
    }

    fn plain(&self) -> Option<&str> {
        self.plain_lyrics
            .as_deref()
            .filter(|text| !text.trim().is_empty())
    }

    fn lyrics(&self) -> Option<Lyrics> {
        if self.instrumental {
            return Some(Lyrics {
                instrumental: true,
                ..Lyrics::default()
            });
        }
        if let Some(synced) = self.synced() {
            let lines = parse_lrc(synced);
            if !lines.is_empty() {
                return Some(Lyrics {
                    lines,
                    synced: true,
                    instrumental: false,
                });
            }
        }
        let plain = self.plain()?;
        Some(Lyrics {
            lines: plain
                .lines()
                .map(|line| Line {
                    at_ms: None,
                    text: line.trim_end().to_string(),
                })
                .collect(),
            synced: false,
            instrumental: false,
        })
    }
}

/// How well a search result fits what is playing; `None` rules it out.
fn score(record: &Record, query: &Query) -> Option<i64> {
    if !loose_match(&record.track_name, &clean_title(&query.title)) {
        return None;
    }
    let mut score = 0;
    if loose_match(&record.artist_name, &clean_artist(&query.artist)) {
        score += 1000;
    }
    if query.duration_ms > 0
        && let Some(duration) = record.duration.filter(|duration| *duration > 0.0)
    {
        let drift = (duration - f64::from(query.duration_ms) / 1000.0).abs();
        if drift > MAX_DRIFT_SECS {
            return None;
        }
        score += ((MAX_DRIFT_SECS - drift) * 10.0) as i64;
    }
    // Timing is the point, so a synced upload wins a tie.
    if record.synced().is_some() {
        score += 200;
    } else if record.plain().is_some() {
        score += 50;
    }
    Some(score)
}

fn pick(records: Vec<Record>, query: &Query) -> Option<Record> {
    let mut best: Option<(i64, Record)> = None;
    for record in records {
        let Some(score) = score(&record, query) else {
            continue;
        };
        if best.as_ref().is_none_or(|(held, _)| score > *held) {
            best = Some((score, record));
        }
    }
    best.map(|(_, record)| record)
}

// ---- names ----------------------------------------------------------------

/// Words a title carries in brackets that a lyrics database does not.
const BRACKET_NOISE: &[&str] = &[
    "remaster",
    "remastered",
    "remix",
    "live",
    "acoustic",
    "version",
    "edit",
    "mix",
    "mono",
    "stereo",
    "deluxe",
    "bonus",
    "expanded",
    "explicit",
    "anniversary",
    "feat",
    "featuring",
    "with",
];

/// What a " - " suffix says when it is not part of the title.
const SUFFIX_NOISE: &[&str] = &[
    "remaster",
    "remastered",
    "radio edit",
    "single version",
    "album version",
    "live",
    "mono",
    "stereo",
    "rerecorded",
    "re-recorded",
];

/// Invisible characters that carry no meaning: a zero-width space, the word
/// joiner and the invisible operators, and a byte order mark. Not the
/// zero-width joiner or non-joiner, which hold emoji sequences and Indic
/// and Persian words together.
fn is_disposable(c: char) -> bool {
    matches!(c, '\u{200b}' | '\u{2060}'..='\u{2064}' | '\u{feff}')
}

/// Whether `text` contains `phrase` as whole words, case-insensitively.
fn has_phrase(text: &str, phrase: &str) -> bool {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|word| !word.is_empty())
        .map(|word| word.trim_matches('-').to_string())
        .collect();
    let wanted: Vec<&str> = phrase.split(' ').collect();
    words
        .windows(wanted.len())
        .any(|window| window.iter().zip(&wanted).all(|(a, b)| a == b))
}

/// Strips what a player adds to a title and a database leaves out:
/// "(Remastered 2011)", " - Live at Wembley", "feat. Someone".
pub fn clean_title(title: &str) -> String {
    let original: String = title.chars().filter(|c| !is_disposable(*c)).collect();
    let mut cleaned = String::with_capacity(original.len());
    let mut rest = original.as_str();
    while let Some(open) = rest.find(['(', '[']) {
        let closer = if rest[open..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(close) = rest[open..].find(closer) else {
            break;
        };
        let inner = &rest[open + 1..open + close];
        let noisy = BRACKET_NOISE.iter().any(|word| has_phrase(inner, word));
        if noisy {
            cleaned.push_str(rest[..open].trim_end());
        } else {
            cleaned.push_str(&rest[..open + close + 1]);
        }
        rest = &rest[open + close + 1..];
    }
    cleaned.push_str(rest);
    let mut cleaned = cleaned.trim().to_string();
    // " - Remastered 2009" and friends, from the first dash whose tail is
    // noise; a dash inside a real title stays.
    let mut cut = None;
    for (index, _) in cleaned.match_indices(" - ") {
        let tail = &cleaned[index + 3..];
        if SUFFIX_NOISE.iter().any(|phrase| has_phrase(tail, phrase))
            || tail
                .split_whitespace()
                .next()
                .is_some_and(|word| word.len() == 4 && word.chars().all(|c| c.is_ascii_digit()))
                && has_phrase(tail, "version")
        {
            cut = Some(index);
            break;
        }
    }
    if let Some(index) = cut {
        cleaned.truncate(index);
    }
    let cleaned = strip_featuring(cleaned.trim());
    if cleaned.is_empty() {
        original.trim().to_string()
    } else {
        cleaned
    }
}

/// Everything from a standalone "feat", "ft", or "featuring" on.
fn strip_featuring(text: &str) -> String {
    // The marker is looked for in a lower-cased copy and the cut is made in
    // `text`, and lower casing does not preserve length: Turkish 'İ' becomes
    // two characters and one byte longer, capital sharp s one byte shorter.
    // An offset measured in the copy therefore means somewhere else in the
    // title -- letters kept that should have gone, or, when it lands inside a
    // character, a panic. `starts` carries every offset in the copy back to
    // the same place in `text`, so no offset is ever used in the string it
    // was not measured in.
    let mut lower = String::with_capacity(text.len());
    let mut starts: Vec<usize> = Vec::with_capacity(text.len() + 1);
    for (at, character) in text.char_indices() {
        for lowered in character.to_lowercase() {
            for _ in 0..lowered.len_utf8() {
                starts.push(at);
            }
            lower.push(lowered);
        }
    }
    starts.push(text.len());
    let mut cut = None;
    for marker in ["featuring", "feat", "ft"] {
        let mut from = 0;
        while let Some(found) = lower[from..].find(marker) {
            let start = from + found;
            let end = start + marker.len();
            let before = lower[..start].trim_end_matches(['-', '(']).trim_end();
            let preceded = before.len() < start && !before.is_empty();
            let followed = lower[end..]
                .strip_prefix('.')
                .unwrap_or(&lower[end..])
                .starts_with(' ');
            if preceded && followed {
                let at = before.len();
                cut = Some(cut.map_or(at, |held: usize| held.min(at)));
                break;
            }
            from = end;
        }
    }
    match cut {
        Some(at) => text[..starts[at]].trim().to_string(),
        None => text.trim().to_string(),
    }
}

/// Players report collaborations in ways a database does not file them, and
/// LRCLIB itself sometimes stores "TOOL;Tool" for one artist.
pub fn clean_artist(artist: &str) -> String {
    let original: String = artist.chars().filter(|c| !is_disposable(*c)).collect();
    let cleaned = strip_featuring(&original);
    let cleaned = cleaned.split(';').next().unwrap_or_default().trim();
    if cleaned.is_empty() {
        original.trim().to_string()
    } else {
        cleaned.to_string()
    }
}

/// Lowercase, accents folded, punctuation gone, one space between words.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        let folded = fold(c);
        if folded == '\'' || folded == '’' || folded == '`' {
            continue;
        }
        if folded == '&' {
            for word in [' ', 'a', 'n', 'd', ' '] {
                push_normalized(&mut out, &mut space, word);
            }
            continue;
        }
        push_normalized(&mut out, &mut space, folded);
    }
    out.trim().to_string()
}

fn push_normalized(out: &mut String, space: &mut bool, c: char) {
    if c.is_alphanumeric() {
        out.extend(c.to_lowercase());
        *space = false;
    } else if !*space {
        out.push(' ');
        *space = true;
    }
}

/// The plain letter behind the Latin accents titles most often carry.
fn fold(c: char) -> char {
    match c {
        'À'..='Å' | 'à'..='å' => 'a',
        'Ç' | 'ç' => 'c',
        'È'..='Ë' | 'è'..='ë' => 'e',
        'Ì'..='Ï' | 'ì'..='ï' => 'i',
        'Ñ' | 'ñ' => 'n',
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' => 'o',
        'Ù'..='Ü' | 'ù'..='ü' => 'u',
        'Ý' | 'ý' | 'ÿ' => 'y',
        'ß' => 's',
        _ => c,
    }
}

fn loose_match(left: &str, right: &str) -> bool {
    let a = normalize(left);
    let b = normalize(right);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

// ---- LRC ------------------------------------------------------------------

/// Parses `[mm:ss.xx]` lines. A line may open with several stamps when the
/// same words repeat; tags such as `[ar:...]` carry no digits and are
/// skipped. The result is sorted by time.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim_start();
        let mut times = Vec::new();
        while let Some(stamp) = leading_stamp(rest) {
            times.push(stamp.0);
            rest = &rest[stamp.1..];
        }
        if times.is_empty() {
            continue;
        }
        let body = rest.trim();
        for at_ms in times {
            lines.push(Line {
                at_ms: Some(at_ms),
                text: body.to_string(),
            });
        }
    }
    lines.sort_by_key(|line| line.at_ms);
    lines
}

/// A timestamp at the head of `text`: its milliseconds and its length.
fn leading_stamp(text: &str) -> Option<(u32, usize)> {
    let inner = text.strip_prefix('[')?;
    let close = inner.find(']')?;
    let stamp = &inner[..close];
    let (minutes, seconds) = stamp.split_once(':')?;
    let (seconds, fraction) = match seconds.find(['.', ':']) {
        Some(dot) => (&seconds[..dot], Some(&seconds[dot + 1..])),
        None => (seconds, None),
    };
    let minutes: u32 = minutes.parse().ok()?;
    let seconds: u32 = seconds.parse().ok()?;
    let fraction_ms = match fraction {
        Some(digits) if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            let value: u32 = digits.chars().take(3).collect::<String>().parse().ok()?;
            value * 10u32.pow(3u32.saturating_sub(digits.len().min(3) as u32))
        }
        Some(_) => return None,
        None => 0,
    };
    Some((minutes * 60_000 + seconds * 1_000 + fraction_ms, close + 2))
}

// ---- cache ------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Cached {
    found: Option<Lyrics>,
}

fn cache_key(query: &Query) -> String {
    let digest = Sha1::digest(
        format!(
            "{}|{}|{}|{}",
            query.artist, query.title, query.album, query.duration_ms
        )
        .as_bytes(),
    );
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_cache(path: &PathBuf) -> Option<Option<Lyrics>> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    if modified.elapsed().unwrap_or(CACHE_LIFETIME) >= CACHE_LIFETIME {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Cached>(&text)
        .ok()
        .map(|cached| cached.found)
}

fn write_cache(path: &Path, found: &Option<Lyrics>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&Cached {
        found: found.clone(),
    }) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_featuring_is_cut_where_it_starts_whatever_the_case_costs() {
        // The cut is found in a lower-cased copy and applied to the original,
        // and lower casing is not always the same number of bytes. Turkish
        // 'İ' grows by one byte, so every offset past it points one byte too
        // far into the title, and the letters between are kept.
        assert_eq!(
            clean_title("\u{130}\u{130} feat. Someone"),
            "\u{130}\u{130}"
        );
        assert_eq!(clean_artist("\u{130}zel feat. Someone"), "\u{130}zel");

        // Capital sharp s shrinks by one, so the offset points one byte SHORT
        // -- and one byte short of the end of a character is a panic.
        assert_eq!(
            clean_title("\u{1e9e} caf\u{e9} feat. Someone"),
            "\u{1e9e} caf\u{e9}"
        );

        // The ASCII case, which already worked, still does.
        assert_eq!(clean_title("Song feat. Someone"), "Song");
        assert_eq!(clean_title("Song ft. Someone"), "Song");
        assert_eq!(clean_artist("Artist featuring Other"), "Artist");
    }

    #[test]
    fn titles_lose_what_a_database_leaves_out() {
        assert_eq!(clean_title("Song (Remastered 2011)"), "Song");
        assert_eq!(clean_title("Song - Live at Wembley"), "Song");
        assert_eq!(clean_title("Song - 2009 Remaster"), "Song");
        assert_eq!(clean_title("Song (feat. Someone)"), "Song");
        assert_eq!(clean_title("Song feat. Someone"), "Song");
        assert_eq!(clean_title("Song ft. Someone"), "Song");
        assert_eq!(clean_title("Song (Part One)"), "Song (Part One)");
        assert_eq!(clean_title("Hyphen - Ated"), "Hyphen - Ated");
        assert_eq!(clean_title("(Remastered)"), "(Remastered)", "never empty");
        assert_eq!(clean_title("Feat\u{200b}ure"), "Feature");
    }

    #[test]
    fn artists_keep_their_first_name() {
        assert_eq!(clean_artist("TOOL;Tool"), "TOOL");
        assert_eq!(clean_artist("Artist feat. Guest"), "Artist");
        assert_eq!(clean_artist("Artist (feat. Guest)"), "Artist");
        assert_eq!(clean_artist("Beyoncé"), "Beyoncé");
    }

    #[test]
    fn matching_is_loose_about_case_accents_and_punctuation() {
        assert!(loose_match("Beyoncé", "beyonce"));
        assert!(loose_match("Rock & Roll", "rock and roll"));
        assert!(loose_match("Don't Stop", "dont stop"));
        assert!(loose_match("Song (Live)", "Song"));
        assert!(!loose_match("Something", "Else"));
        assert!(!loose_match("", "Else"));
    }

    fn record(track: &str, artist: &str, duration: f64, synced: bool) -> Record {
        Record {
            track_name: track.into(),
            artist_name: artist.into(),
            duration: Some(duration),
            instrumental: false,
            synced_lyrics: synced.then(|| "[00:01.00] a".to_string()),
            plain_lyrics: Some("a".to_string()),
        }
    }

    #[test]
    fn the_closest_length_wins_and_synced_breaks_ties() {
        let query = Query {
            artist: "Artist".into(),
            title: "Song".into(),
            album: String::new(),
            duration_ms: 200_000,
        };
        let picked = pick(
            vec![
                record("Song", "Artist", 260.0, true),
                record("Song", "Artist", 201.0, false),
                record("Song", "Artist", 202.0, true),
                record("Other", "Artist", 200.0, true),
            ],
            &query,
        )
        .unwrap();
        assert_eq!(picked.duration, Some(202.0));
        assert!(pick(vec![record("Song", "Artist", 400.0, true)], &query).is_none());
    }

    #[test]
    fn lrc_lines_parse_and_sort() {
        let lines = parse_lrc(
            "[ar:Someone]\n[00:12.50]First\n[00:05]Early\n[01:00.1][02:00.123]Twice\n\nNo stamp\n",
        );
        let times: Vec<u32> = lines.iter().filter_map(|line| line.at_ms).collect();
        assert_eq!(times, vec![5_000, 12_500, 60_100, 120_123]);
        assert_eq!(lines[0].text, "Early");
        assert_eq!(lines[2].text, "Twice");
        assert_eq!(lines[3].text, "Twice");
    }

    #[test]
    fn the_active_line_is_the_last_one_started() {
        let lyrics = Lyrics {
            lines: parse_lrc("[00:05]a\n[00:10]b\n[00:15]c"),
            synced: true,
            instrumental: false,
        };
        assert_eq!(lyrics.active_line(0), None);
        assert_eq!(lyrics.active_line(5_000), Some(0));
        assert_eq!(lyrics.active_line(12_000), Some(1));
        assert_eq!(lyrics.active_line(99_000), Some(2));
        let plain = Lyrics {
            lines: vec![Line::default()],
            synced: false,
            instrumental: false,
        };
        assert_eq!(plain.active_line(5_000), None);
    }

    #[test]
    fn stamps_keep_their_fractions_whatever_the_digits() {
        let at = |text: &str| parse_lrc(text)[0].at_ms;
        assert_eq!(at("[02:06.5]a"), Some(126_500));
        assert_eq!(at("[02:06.50]a"), Some(126_500));
        assert_eq!(at("[02:06.500]a"), Some(126_500));
        assert_eq!(at("[02:06.057]a"), Some(126_057));
        assert_eq!(at("[02:06.05]a"), Some(126_050));
        assert_eq!(at("[02:06]a"), Some(126_000));
        assert_eq!(at("[02:06.1234]a"), Some(126_123));
    }

    #[test]
    fn a_line_is_active_from_its_own_start_not_the_next_ones() {
        let lyrics = Lyrics {
            lines: parse_lrc("[00:05.00]a\n[00:10.50]b\n[00:15.000]c"),
            synced: true,
            instrumental: false,
        };
        assert_eq!(lyrics.active_line(4_999), None);
        assert_eq!(lyrics.active_line(5_000), Some(0));
        assert_eq!(lyrics.active_line(10_499), Some(0));
        assert_eq!(lyrics.active_line(10_500), Some(1));
        assert_eq!(lyrics.active_line(14_999), Some(1));
        assert_eq!(lyrics.active_line(15_000), Some(2));
        // The wake-up is for the next line's own time.
        assert_eq!(lyrics.until_next_line(0), Some(5_000));
        assert_eq!(lyrics.until_next_line(5_000), Some(5_500));
        assert_eq!(lyrics.until_next_line(10_499), Some(1));
        assert_eq!(lyrics.until_next_line(15_000), None);
    }

    #[test]
    fn the_lyrics_follow_what_is_heard_not_what_is_decoded() {
        let lyrics = Lyrics {
            lines: parse_lrc("[02:06.00]a\n[02:10.00]b"),
            synced: true,
            instrumental: false,
        };
        // Decoded 126.3 s with 400 ms still queued: the line is not sung yet.
        assert_eq!(lyrics.active_line(126_300), Some(0));
        assert_eq!(lyrics.active_line(heard_position_ms(126_300, 400)), None);
        assert_eq!(heard_position_ms(126_500, 400), 126_100);
        // The delay never takes the position below the start.
        assert_eq!(heard_position_ms(100, 400), 0);
    }

    #[test]
    fn records_turn_into_lyrics() {
        let mut instrumental = record("Song", "Artist", 100.0, false);
        instrumental.instrumental = true;
        assert!(instrumental.lyrics().unwrap().instrumental);
        let synced = record("Song", "Artist", 100.0, true).lyrics().unwrap();
        assert!(synced.synced);
        let plain = record("Song", "Artist", 100.0, false).lyrics().unwrap();
        assert!(!plain.synced);
        assert_eq!(plain.lines.len(), 1);
        let nothing = Record::default();
        assert!(nothing.lyrics().is_none());
    }

    // ---- transient failures ------------------------------------------------

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A LRCLIB stand-in: answers each request with the next of `script`
    /// (the last one repeats), `(status line, extra header, body)`.
    async fn fake_lrclib(
        script: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let n = counter.fetch_add(1, Ordering::SeqCst);
                let (status, header, body) = script[n.min(script.len() - 1)];
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                let reply = format!(
                    "HTTP/1.1 {status}\r\n{header}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (api, hits)
    }

    const FOUND: &str =
        r#"{"trackName":"Song","artistName":"Artist","duration":100,"plainLyrics":"hello"}"#;

    fn song() -> Query {
        Query {
            artist: "Artist".into(),
            title: "Song".into(),
            album: String::new(),
            duration_ms: 100_000,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tidalfast-lyrics-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const FAST: [Duration; 2] = [Duration::from_millis(5), Duration::from_millis(5)];

    #[tokio::test]
    async fn a_busy_answer_is_retried_and_the_lyrics_arrive() {
        let (api, hits) = fake_lrclib(vec![
            ("503 Service Unavailable", "", "busy"),
            ("503 Service Unavailable", "Retry-After: 0\r\n", "busy"),
            ("200 OK", "", FOUND),
        ])
        .await;
        let dir = scratch("retry");
        let http = reqwest::Client::new();
        let found = fetch_from(&http, &api, &dir, &song(), &FAST).await.unwrap();
        assert_eq!(found.unwrap().lines[0].text, "hello");
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn three_busy_answers_give_the_plain_message_and_are_not_cached() {
        let (api, hits) = fake_lrclib(vec![("503 Service Unavailable", "", "busy")]).await;
        let dir = scratch("busy");
        let http = reqwest::Client::new();
        let error = fetch_from(&http, &api, &dir, &song(), &FAST)
            .await
            .unwrap_err();
        assert_eq!(failure_detail(&error), BUSY);
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        let path = dir.join(format!("{}.json", cache_key(&song())));
        assert!(read_cache(&path).is_none(), "a failure must not be cached");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_missing_song_is_no_lyrics_without_a_retry() {
        let (api, hits) = fake_lrclib(vec![("404 Not Found", "", "{}")]).await;
        let dir = scratch("missing");
        let http = reqwest::Client::new();
        let found = fetch_from(&http, &api, &dir, &song(), &FAST).await.unwrap();
        assert!(found.is_none());
        // The exact lookup, then the search; neither is repeated.
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_service_that_cannot_be_reached_gives_the_offline_message() {
        let dir = scratch("down");
        let http = reqwest::Client::new();
        // Nothing listens on port 1.
        let error = fetch_from(&http, "http://127.0.0.1:1", &dir, &song(), &FAST)
            .await
            .unwrap_err();
        assert_eq!(failure_detail(&error), UNREACHABLE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn another_song_cancels_the_pending_retries() {
        let (api, hits) = fake_lrclib(vec![("503 Service Unavailable", "", "busy")]).await;
        let dir = scratch("cancel");
        let long = [Duration::from_secs(30), Duration::from_secs(30)];
        let task = tokio::spawn({
            let dir = dir.clone();
            async move {
                let http = reqwest::Client::new();
                let _ = fetch_from(&http, &api, &dir, &song(), &long).await;
            }
        });
        while hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let seen = hits.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hits.load(Ordering::SeqCst), seen);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
