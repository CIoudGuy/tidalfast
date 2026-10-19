//! Local listening statistics.
//!
//! TIDAL's My Activity only gives monthly top artists with stream counts
//! (docs/_reference/tidal-activity.md, 1.2). Listening hours, top tracks,
//! albums, streaks and the hours of the day come from here: every listen
//! made in Tidalfast is appended to a compact JSON-lines file in the state
//! folder, one record per line, behind a version header. A torn last line
//! (a crash mid-write) is skipped when read. The file is bounded: past
//! [`MAX_RECORDS`] (plus some slack) the oldest records are dropped by an
//! atomic rewrite.
//!
//! A *stream* follows TIDAL's rule, [`crate::play_report::listen::is_stream`]:
//! at least 30 s heard, or half of a shorter track. Listening time counts
//! every listen, streams or not.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::play_report::listen::{Listen, is_stream};

const HEADER_KEY: &str = "tidalfast_listens";
const VERSION: u32 = 1;
/// The most listens kept.
pub const MAX_RECORDS: usize = 200_000;
/// Records past [`MAX_RECORDS`] before the file is compacted.
const SLACK: usize = 10_000;

/// One listen, as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenRecord {
    /// When it started, Unix ms.
    #[serde(rename = "t")]
    pub start_ms: i64,
    /// Music heard, ms.
    #[serde(rename = "l")]
    pub listened_ms: u64,
    #[serde(rename = "d", default)]
    pub duration_ms: u32,
    /// Played to the end, rather than skipped or stopped.
    #[serde(rename = "c", default)]
    pub completed: bool,
    #[serde(rename = "tr")]
    pub track_id: String,
    #[serde(rename = "tn", default)]
    pub track_name: String,
    #[serde(rename = "al", default, skip_serializing_if = "String::is_empty")]
    pub album_id: String,
    #[serde(rename = "an", default, skip_serializing_if = "String::is_empty")]
    pub album_name: String,
    /// `(id, name)`; the id is empty when unknown.
    #[serde(rename = "ar", default)]
    pub artists: Vec<(String, String)>,
    #[serde(rename = "cx", default, skip_serializing_if = "Option::is_none")]
    pub context_uri: Option<String>,
}

impl ListenRecord {
    pub fn from_listen(listen: &Listen) -> Self {
        let track = &listen.track;
        Self {
            start_ms: listen.start_ms(),
            listened_ms: listen.listened_ms,
            duration_ms: track.duration_ms,
            completed: listen.completed,
            track_id: crate::util::uri_id(&track.uri)
                .unwrap_or_default()
                .to_string(),
            track_name: track.title.clone(),
            album_id: track
                .album_uri
                .as_deref()
                .and_then(crate::util::uri_id)
                .unwrap_or_default()
                .to_string(),
            album_name: track.album.clone(),
            artists: track
                .artists
                .iter()
                .map(|artist| (artist.id.clone().unwrap_or_default(), artist.name.clone()))
                .collect(),
            context_uri: listen.context_uri.clone(),
        }
    }

    pub fn is_stream(&self) -> bool {
        is_stream(self.listened_ms, self.duration_ms)
    }
}

/// The listens file. Appends and reads are serialized by the caller (the
/// backend keeps it behind a mutex).
pub struct ListenLog {
    path: PathBuf,
    /// Records in the file, once counted.
    count: Option<usize>,
}

fn ends_with_newline(path: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

fn header() -> String {
    format!("{{\"{HEADER_KEY}\":{VERSION}}}")
}

impl ListenLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path, count: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Adds one listen.
    pub fn append(&mut self, record: &ListenRecord) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let count = match self.count {
            Some(count) => count,
            None => self.read()?.len(),
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut text = String::new();
        if file.metadata()?.len() == 0 {
            text.push_str(&header());
            text.push('\n');
        } else if !ends_with_newline(&self.path)? {
            // Close a line torn by a crash, so this record stands alone.
            text.push('\n');
        }
        text.push_str(&serde_json::to_string(record).map_err(std::io::Error::other)?);
        text.push('\n');
        // One write: a crash leaves at most a torn last line.
        file.write_all(text.as_bytes())?;
        self.count = Some(count + 1);
        if count + 1 > MAX_RECORDS + SLACK {
            self.compact()?;
        }
        Ok(())
    }

    /// Every stored listen, oldest first. A missing file is no listens; a
    /// file of a newer version is refused rather than misread.
    pub fn read(&mut self) -> std::io::Result<Vec<ListenRecord>> {
        let file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.count = Some(0);
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let mut records = Vec::new();
        for (index, line) in std::io::BufReader::new(file).lines().enumerate() {
            let Ok(line) = line else { continue };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if index == 0
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(version) = value.get(HEADER_KEY)
            {
                if version.as_u64() != Some(u64::from(VERSION)) {
                    return Err(std::io::Error::other(format!(
                        "listens file version {version} is newer than this app"
                    )));
                }
                continue;
            }
            if let Ok(record) = serde_json::from_str::<ListenRecord>(line) {
                records.push(record);
            }
        }
        self.count = Some(records.len());
        Ok(records)
    }

    /// Keeps the newest [`MAX_RECORDS`], rewriting the file atomically.
    pub fn compact(&mut self) -> std::io::Result<()> {
        let mut records = self.read()?;
        if records.len() > MAX_RECORDS {
            let excess = records.len() - MAX_RECORDS;
            records.drain(..excess);
        }
        let mut text = header();
        text.push('\n');
        for record in &records {
            text.push_str(&serde_json::to_string(record).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        fastframe_file::write(&self.path, text)?;
        self.count = Some(records.len());
        Ok(())
    }
}

// ---- queries -------------------------------------------------------------------

/// Which listens a query covers, in the computer's time zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatsRange {
    #[default]
    AllTime,
    Today,
    /// Since Monday.
    ThisWeek,
    ThisMonth,
    ThisYear,
    /// The last `n` days, today included.
    LastDays(u32),
    /// Unix ms, end exclusive.
    Between {
        from_ms: i64,
        to_ms: i64,
    },
}

/// The size of the time buckets in a report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Granularity {
    #[default]
    Day,
    /// Weeks starting on Monday.
    Week,
    Month,
    Year,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsQuery {
    pub range: StatsRange,
    pub granularity: Granularity,
    /// How many entries each top list holds.
    pub top: usize,
}

impl Default for StatsQuery {
    fn default() -> Self {
        Self {
            range: StatsRange::AllTime,
            granularity: Granularity::Month,
            top: 50,
        }
    }
}

/// Listening time and streams over some stretch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub listened_ms: u64,
    pub streams: u64,
    /// Every listen, streams or not.
    pub listens: u64,
    /// Listens left before the end.
    pub skips: u64,
}

impl Totals {
    fn add(&mut self, record: &ListenRecord) {
        self.listened_ms += record.listened_ms;
        self.listens += 1;
        self.streams += u64::from(record.is_stream());
        self.skips += u64::from(!record.completed);
    }
}

/// One time bucket; `start` is its first day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodTotals {
    pub start: Date,
    pub totals: Totals,
}

/// A track, artist or album in a top list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopEntry {
    /// `tidal:track:<id>`, `tidal:artist:<id>`, `tidal:album:<id>`; empty
    /// when the id is unknown.
    pub uri: String,
    pub name: String,
    /// The artists, for tracks and albums.
    pub subtitle: String,
    /// Streams.
    pub plays: u64,
    pub listened_ms: u64,
}

/// One listen of `StatsReport::recent`.
pub type RecentListen = ListenRecord;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatsReport {
    /// What the range resolved to, Unix ms; `None` for all time.
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub totals: Totals,
    /// Per [`StatsQuery::granularity`], oldest first, only buckets with
    /// listens.
    pub periods: Vec<PeriodTotals>,
    pub top_tracks_by_plays: Vec<TopEntry>,
    pub top_tracks_by_time: Vec<TopEntry>,
    pub top_artists_by_plays: Vec<TopEntry>,
    pub top_artists_by_time: Vec<TopEntry>,
    pub top_albums_by_plays: Vec<TopEntry>,
    pub top_albums_by_time: Vec<TopEntry>,
    /// By local hour of day, 0 to 23.
    pub hours: [Totals; 24],
    /// By weekday, Monday first.
    pub weekdays: [Totals; 7],
    /// Days with at least one stream in a row, ending today or yesterday.
    /// All time, whatever the range.
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    /// Distinct days with a stream in the range.
    pub active_days: u32,
}

fn local(ms: i64, tz: &TimeZone) -> Option<jiff::Zoned> {
    Some(
        jiff::Timestamp::from_millisecond(ms)
            .ok()?
            .to_zoned(tz.clone()),
    )
}

fn start_of(date: Date, granularity: Granularity) -> Date {
    match granularity {
        Granularity::Day => date,
        Granularity::Week => {
            let back = i64::from(date.weekday().to_monday_zero_offset());
            date.checked_sub(jiff::Span::new().days(back))
                .unwrap_or(date)
        }
        Granularity::Month => date.first_of_month(),
        Granularity::Year => Date::new(date.year(), 1, 1).unwrap_or(date),
    }
}

fn day_start_ms(date: Date, tz: &TimeZone) -> Option<i64> {
    Some(date.to_zoned(tz.clone()).ok()?.timestamp().as_millisecond())
}

/// The range as Unix ms, end exclusive.
fn resolve(range: StatsRange, now_ms: i64, tz: &TimeZone) -> (Option<i64>, Option<i64>) {
    let Some(today) = local(now_ms, tz).map(|now| now.date()) else {
        return (None, None);
    };
    let since = |date: Date| (day_start_ms(date, tz), None);
    match range {
        StatsRange::AllTime => (None, None),
        StatsRange::Today => since(today),
        StatsRange::ThisWeek => since(start_of(today, Granularity::Week)),
        StatsRange::ThisMonth => since(start_of(today, Granularity::Month)),
        StatsRange::ThisYear => since(start_of(today, Granularity::Year)),
        StatsRange::LastDays(days) => since(
            today
                .checked_sub(jiff::Span::new().days(i64::from(days.max(1)) - 1))
                .unwrap_or(today),
        ),
        StatsRange::Between { from_ms, to_ms } => (Some(from_ms), Some(to_ms)),
    }
}

#[derive(Default)]
struct Tally {
    name: String,
    subtitle: String,
    plays: u64,
    listened_ms: u64,
}

fn tally(
    map: &mut HashMap<String, Tally>,
    key: String,
    name: &str,
    subtitle: &str,
    record: &ListenRecord,
) {
    let entry = map.entry(key).or_default();
    if entry.name.is_empty() {
        entry.name = name.to_string();
        entry.subtitle = subtitle.to_string();
    }
    entry.plays += u64::from(record.is_stream());
    entry.listened_ms += record.listened_ms;
}

fn ranked(map: &HashMap<String, Tally>, top: usize, by_time: bool) -> Vec<TopEntry> {
    let mut entries: Vec<TopEntry> = map
        .iter()
        .filter(|(_, tally)| tally.listened_ms > 0 || tally.plays > 0)
        .map(|(key, tally)| TopEntry {
            uri: key
                .strip_prefix('#')
                .map_or_else(|| key.clone(), |_| String::new()),
            name: tally.name.clone(),
            subtitle: tally.subtitle.clone(),
            plays: tally.plays,
            listened_ms: tally.listened_ms,
        })
        .collect();
    entries.sort_by(|a, b| {
        let (first, second) = if by_time {
            ((a.listened_ms, a.plays), (b.listened_ms, b.plays))
        } else {
            ((a.plays, a.listened_ms), (b.plays, b.listened_ms))
        };
        second.cmp(&first).then_with(|| a.name.cmp(&b.name))
    });
    entries.truncate(top);
    entries
}

fn streaks(days: &[Date], today: Date) -> (u32, u32) {
    let mut longest = 0;
    let mut run = 0;
    let mut previous: Option<Date> = None;
    for &day in days {
        run = match previous {
            Some(before) if before.tomorrow().ok() == Some(day) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(day);
    }
    let current = match previous {
        Some(last) if last == today || last.tomorrow().ok() == Some(today) => run,
        _ => 0,
    };
    (current, longest)
}

/// Builds a report from `records` as of `now_ms` in `tz`.
pub fn report(
    records: &[ListenRecord],
    query: &StatsQuery,
    now_ms: i64,
    tz: &TimeZone,
) -> StatsReport {
    let (from_ms, to_ms) = resolve(query.range, now_ms, tz);
    let mut report = StatsReport {
        from_ms,
        to_ms,
        ..StatsReport::default()
    };
    let mut periods: std::collections::BTreeMap<Date, Totals> = Default::default();
    let mut tracks = HashMap::new();
    let mut artists = HashMap::new();
    let mut albums = HashMap::new();
    let mut stream_days = std::collections::BTreeSet::new();
    let mut range_days = std::collections::BTreeSet::new();
    for record in records {
        let Some(when) = local(record.start_ms, tz) else {
            continue;
        };
        if record.is_stream() {
            stream_days.insert(when.date());
        }
        let inside = from_ms.is_none_or(|from| record.start_ms >= from)
            && to_ms.is_none_or(|to| record.start_ms < to);
        if !inside {
            continue;
        }
        if record.is_stream() {
            range_days.insert(when.date());
        }
        report.totals.add(record);
        periods
            .entry(start_of(when.date(), query.granularity))
            .or_default()
            .add(record);
        if let Ok(hour) = usize::try_from(when.hour()) {
            report.hours[hour.min(23)].add(record);
        }
        if let Ok(day) = usize::try_from(when.weekday().to_monday_zero_offset()) {
            report.weekdays[day.min(6)].add(record);
        }
        let artist_names = record
            .artists
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let track_key = if record.track_id.is_empty() {
            format!("#{}", record.track_name)
        } else {
            format!("tidal:track:{}", record.track_id)
        };
        tally(
            &mut tracks,
            track_key,
            &record.track_name,
            &artist_names,
            record,
        );
        for (id, name) in &record.artists {
            let key = if id.is_empty() {
                format!("#{name}")
            } else {
                format!("tidal:artist:{id}")
            };
            tally(&mut artists, key, name, "", record);
        }
        if !record.album_id.is_empty() || !record.album_name.is_empty() {
            let key = if record.album_id.is_empty() {
                let first = record.artists.first().map_or("", |(_, name)| name.as_str());
                format!("#{}\u{1f}{first}", record.album_name)
            } else {
                format!("tidal:album:{}", record.album_id)
            };
            tally(&mut albums, key, &record.album_name, &artist_names, record);
        }
    }
    report.periods = periods
        .into_iter()
        .map(|(start, totals)| PeriodTotals { start, totals })
        .collect();
    report.top_tracks_by_plays = ranked(&tracks, query.top, false);
    report.top_tracks_by_time = ranked(&tracks, query.top, true);
    report.top_artists_by_plays = ranked(&artists, query.top, false);
    report.top_artists_by_time = ranked(&artists, query.top, true);
    report.top_albums_by_plays = ranked(&albums, query.top, false);
    report.top_albums_by_time = ranked(&albums, query.top, true);
    let days: Vec<Date> = stream_days.into_iter().collect();
    if let Some(today) = local(now_ms, tz).map(|now| now.date()) {
        (report.current_streak_days, report.longest_streak_days) = streaks(&days, today);
    }
    report.active_days = range_days.len() as u32;
    report
}

/// Reads the log and reports, in the computer's time zone.
pub fn query(log: &mut ListenLog, query: &StatsQuery) -> std::io::Result<StatsReport> {
    let records = log.read()?;
    let now_ms = jiff::Timestamp::now().as_millisecond();
    Ok(report(&records, query, now_ms, &TimeZone::system()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    /// 2026-10-12 00:00 UTC, a Monday.
    const MONDAY: i64 = 1_791_763_200_000;

    fn record(start_ms: i64, track: &str, listened_s: u64, artist: (&str, &str)) -> ListenRecord {
        ListenRecord {
            start_ms,
            listened_ms: listened_s * 1000,
            duration_ms: 200_000,
            completed: listened_s >= 200,
            track_id: track.into(),
            track_name: format!("Song {track}"),
            album_id: "9".into(),
            album_name: "Album".into(),
            artists: vec![(artist.0.into(), artist.1.into())],
            context_uri: Some("tidal:album:9".into()),
        }
    }

    #[test]
    fn monday_constant_is_a_monday() {
        let date = local(MONDAY, &TimeZone::UTC).unwrap().date();
        assert_eq!(date.to_string(), "2026-10-12");
        assert_eq!(date.weekday(), jiff::civil::Weekday::Monday);
    }

    #[test]
    fn totals_tops_buckets_and_hours() {
        let records = vec![
            record(MONDAY + 9 * 3_600_000, "1", 200, ("10", "A")),
            record(MONDAY + 9 * 3_600_000 + 300_000, "1", 200, ("10", "A")),
            record(MONDAY + DAY + 20 * 3_600_000, "2", 20, ("11", "B")),
            record(MONDAY + DAY + 21 * 3_600_000, "3", 150, ("11", "B")),
            record(MONDAY - 40 * DAY, "4", 60, ("12", "C")),
        ];
        let query = StatsQuery {
            range: StatsRange::ThisWeek,
            granularity: Granularity::Day,
            top: 10,
        };
        let now = MONDAY + DAY + 22 * 3_600_000;
        let report = report(&records, &query, now, &TimeZone::UTC);
        assert_eq!(report.from_ms, Some(MONDAY));
        assert_eq!(report.totals.listens, 4);
        assert_eq!(report.totals.streams, 3);
        assert_eq!(report.totals.skips, 2);
        assert_eq!(report.totals.listened_ms, 570_000);
        assert_eq!(report.periods.len(), 2);
        assert_eq!(report.periods[0].start.to_string(), "2026-10-12");
        assert_eq!(report.periods[1].totals.listened_ms, 170_000);
        assert_eq!(report.top_tracks_by_plays[0].uri, "tidal:track:1");
        assert_eq!(report.top_tracks_by_plays[0].plays, 2);
        assert_eq!(report.top_tracks_by_plays[0].subtitle, "A");
        assert_eq!(report.top_artists_by_time[0].uri, "tidal:artist:10");
        assert_eq!(report.top_artists_by_time[1].listened_ms, 170_000);
        assert_eq!(report.top_artists_by_plays[1].plays, 1);
        assert_eq!(report.top_albums_by_plays[0].uri, "tidal:album:9");
        assert_eq!(report.hours[9].streams, 2);
        assert_eq!(report.hours[20].streams, 0);
        assert_eq!(report.hours[20].listens, 1);
        assert_eq!(report.weekdays[0].listens, 2);
        assert_eq!(report.weekdays[1].listens, 2);
        assert_eq!(report.current_streak_days, 2);
        assert_eq!(report.longest_streak_days, 2);
        assert_eq!(report.active_days, 2);

        let all = super::report(
            &records,
            &StatsQuery {
                granularity: Granularity::Month,
                ..StatsQuery::default()
            },
            now,
            &TimeZone::UTC,
        );
        assert_eq!(all.totals.listens, 5);
        assert_eq!(all.periods.len(), 2);
        assert_eq!(all.periods[0].start.to_string(), "2026-09-01");
        let years = super::report(
            &records,
            &StatsQuery {
                granularity: Granularity::Year,
                ..StatsQuery::default()
            },
            now,
            &TimeZone::UTC,
        );
        assert_eq!(years.periods.len(), 1);
        let weeks = super::report(
            &records,
            &StatsQuery {
                granularity: Granularity::Week,
                ..StatsQuery::default()
            },
            now,
            &TimeZone::UTC,
        );
        assert_eq!(
            weeks.periods.last().unwrap().start.to_string(),
            "2026-10-12"
        );
    }

    #[test]
    fn streaks_need_consecutive_days_up_to_yesterday() {
        let day = |offset: i64| local(MONDAY + offset * DAY, &TimeZone::UTC).unwrap().date();
        let days = [day(0), day(1), day(2), day(5), day(6)];
        assert_eq!(streaks(&days, day(7)), (2, 3));
        assert_eq!(streaks(&days, day(6)), (2, 3));
        assert_eq!(streaks(&days, day(8)), (0, 3));
        assert_eq!(streaks(&[], day(0)), (0, 0));
    }

    #[test]
    fn the_log_appends_skips_torn_lines_and_compacts() {
        let dir = std::env::temp_dir().join(format!(
            "tidalfast-listens-{}-{}",
            std::process::id(),
            crate::play_report::uuid_v4()
        ));
        let path = dir.join("listens.jsonl");
        let mut log = ListenLog::new(path.clone());
        assert!(log.read().unwrap().is_empty());
        log.append(&record(1, "1", 40, ("1", "A"))).unwrap();
        log.append(&record(2, "2", 40, ("1", "A"))).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("{\"tidalfast_listens\":1}\n"));
        assert_eq!(text.lines().count(), 3);
        // A crash mid-line.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"t\":3,\"l\"")
            .unwrap();
        let mut again = ListenLog::new(path.clone());
        assert_eq!(again.read().unwrap().len(), 2);
        again.append(&record(4, "4", 40, ("1", "A"))).unwrap();
        let records = again.read().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].track_id, "4");
        again.compact().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 4);
        std::fs::write(&path, "{\"tidalfast_listens\":99}\n").unwrap();
        assert!(ListenLog::new(path.clone()).read().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
