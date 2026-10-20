//! My Activity: TIDAL's monthly top artists, and Tidalfast's own count of
//! this month's listening beside them.
//!
//! The numbers on TIDAL's cards are TIDAL's own, from the private v2
//! `my-activity` endpoints (docs/_reference/tidal-activity.md). This module
//! keeps what has been read, decides what to ask for next, and turns
//! TIDAL's tolerant models into what the pages draw. Views mark what they
//! show; the app sends [`ActivityState::take_commands`] after drawing.

use std::collections::{HashMap, HashSet};

use jiff::civil::Date;

use crate::api::models::Image;
use crate::api::tidal::activity::{ActivityMonth, TopArtistsReport};
use crate::backend::{ActivityQuery, ActivityReply, Command, StatsQuery, StatsRange, StatsReport};
use crate::model::Loadable;

/// How many artists TIDAL's list shows.
pub const TOP_ARTISTS: usize = 25;
/// How many songs the Tidalfast section lists.
pub const LOCAL_TOP_TRACKS: usize = 20;

/// A calendar month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonthKey {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
}

impl MonthKey {
    pub fn new(year: i32, month: u32) -> Option<Self> {
        (1..=12).contains(&month).then_some(Self { year, month })
    }

    pub fn of(date: Date) -> Self {
        Self {
            year: i32::from(date.year()),
            month: date.month() as u32,
        }
    }

    fn from_tidal(month: &ActivityMonth) -> Option<Self> {
        let year = i32::try_from(month.year?).ok()?;
        let number = u32::try_from(month.month?).ok()?;
        Self::new(year, number)
    }

    /// The month before.
    pub fn previous(self) -> Self {
        if self.month == 1 {
            Self {
                year: self.year - 1,
                month: 12,
            }
        } else {
            Self {
                year: self.year,
                month: self.month - 1,
            }
        }
    }

    pub fn days(self) -> u32 {
        Date::new(self.year as i16, self.month as i8, 1)
            .map_or(30, |date| date.days_in_month() as u32)
    }
}

/// Days left in `today`'s month after today, and how far through the month
/// today is, 0 to 1: TIDAL's "17 DAYS LEFT" and its bar.
pub fn month_progress(today: Date) -> (u32, f32) {
    let days = MonthKey::of(today).days().max(1);
    let day = (today.day() as u32).min(days);
    (days - day, day as f32 / days as f32)
}

/// An artist in a month's ranking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RankedArtist {
    pub id: String,
    pub name: String,
    pub streams: u64,
    /// The artist's own picture only: the cards show a microphone without
    /// one, as TIDAL does.
    pub picture: Vec<Image>,
    /// The picture, or an album cover when the artist has none: the list.
    pub thumbnail: Vec<Image>,
    /// TIDAL's vibrant colour for the picture, which tints the cards.
    pub color: Option<[u8; 3]>,
}

impl RankedArtist {
    pub fn uri(&self) -> String {
        crate::api::tidal::convert::uri("artist", &self.id)
    }
}

/// One month's report as the pages draw it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonthReport {
    pub artists: Vec<RankedArtist>,
    /// TIDAL's count of days until the month closes, while it runs.
    pub days_left: Option<u32>,
    /// TIDAL's note under the list and its link.
    pub disclaimer: Option<(String, String)>,
}

/// `#rrggbb` or `#rgb`.
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(hex.get(range)?, 16).ok();
    match hex.len() {
        6 | 8 => Some([channel(0..2)?, channel(2..4)?, channel(4..6)?]),
        3 => {
            let [r, g, b] = [channel(0..1)?, channel(1..2)?, channel(2..3)?];
            Some([r * 17, g * 17, b * 17])
        }
        _ => None,
    }
}

impl MonthReport {
    pub fn from_tidal(report: &TopArtistsReport) -> Self {
        let mut artists: Vec<RankedArtist> = report
            .artists
            .iter()
            .filter(|entry| !entry.artist.name.is_empty())
            .map(|entry| {
                let artist = &entry.artist;
                let picture = artist.picture.as_deref().filter(|uuid| !uuid.is_empty());
                let cover = artist
                    .selected_album_cover_fallback
                    .as_deref()
                    .filter(|uuid| !uuid.is_empty());
                RankedArtist {
                    id: artist.id.clone(),
                    name: artist.name.clone(),
                    streams: entry.nr_of_streams.unwrap_or(0).max(0) as u64,
                    picture: crate::api::tidal::convert::artist_images(picture, None),
                    thumbnail: crate::api::tidal::convert::artist_images(picture, cover),
                    color: artist.vibrant_color.as_deref().and_then(parse_color),
                }
            })
            .collect();
        // No full list: the card's summary still names the leaders.
        if artists.is_empty()
            && let Some(card) = &report.activity
        {
            artists = card
                .artists
                .iter()
                .filter(|artist| !artist.name.is_empty())
                .map(|artist| RankedArtist {
                    id: artist.id.clone(),
                    name: artist.name.clone(),
                    streams: artist.nr_of_streams.unwrap_or(0).max(0) as u64,
                    ..RankedArtist::default()
                })
                .collect();
        }
        // TIDAL ranks them; a stable sort keeps its order among equals.
        artists.sort_by_key(|artist| std::cmp::Reverse(artist.streams));
        artists.truncate(TOP_ARTISTS);
        let days_left = report
            .activity
            .as_ref()
            .and_then(|card| card.total_nr_of_days_left)
            .and_then(|days| u32::try_from(days).ok());
        let disclaimer = report
            .disclaimers
            .iter()
            .find(|note| !note.text.trim().is_empty())
            .map(|note| {
                // "Learn more" opens the address: only a web page.
                let url = note.url.trim();
                let url = if crate::opener::web_address(url).is_some() {
                    url.to_string()
                } else {
                    String::new()
                };
                (note.text.trim().to_string(), url)
            });
        Self {
            artists,
            days_left,
            disclaimer,
        }
    }
}

/// Copying a card's picture: the hero is drawn once without its Share
/// button, captured, and cropped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Share {
    #[default]
    Idle,
    /// Asked for; the next frame that shows the whole hero captures it.
    Requested { since: f64 },
    /// The capture was asked for; the picture comes as an input event.
    Capturing {
        rect: egui::Rect,
        pixels_per_point: f32,
        since: f64,
    },
}

/// `rect` (points) out of a picture of the whole window.
pub fn crop(
    window: &egui::ColorImage,
    rect: egui::Rect,
    pixels_per_point: f32,
) -> Option<egui::ColorImage> {
    let [width, height] = window.size;
    let to_px = |value: f32, limit: usize| {
        ((value * pixels_per_point).round().max(0.0) as usize).min(limit)
    };
    let (left, right) = (to_px(rect.left(), width), to_px(rect.right(), width));
    let (top, bottom) = (to_px(rect.top(), height), to_px(rect.bottom(), height));
    if right <= left || bottom <= top {
        return None;
    }
    let mut pixels = Vec::with_capacity((right - left) * (bottom - top));
    for row in top..bottom {
        pixels.extend_from_slice(&window.pixels[row * width + left..row * width + right]);
    }
    Some(egui::ColorImage::new([right - left, bottom - top], pixels))
}

/// Marks the screenshot this module asked for.
#[derive(Debug)]
pub struct ShareShot;

#[derive(Debug, Default)]
pub struct ActivityState {
    /// The account the data belongs to.
    account: Option<String>,
    /// The day it was read: TIDAL updates it once a day.
    day: Option<Date>,
    /// Past months that have a report, newest first.
    pub timeline: Loadable<Vec<MonthKey>>,
    pub reports: HashMap<MonthKey, Loadable<MonthReport>>,
    /// This month's listening counted by Tidalfast.
    pub local: Loadable<StatsReport>,
    /// The Top Artists page's tab; `None` is This Month.
    pub selected: Option<MonthKey>,
    pub share: Share,
    wanted: HashSet<MonthKey>,
    want_timeline: bool,
    want_local: bool,
    next_request: u64,
    /// Answers to requests below this belong to an earlier account or day.
    first_request: u64,
    local_request: Option<u64>,
}

impl ActivityState {
    /// Forgets everything read for another account or on another day.
    fn keep_for(&mut self, account: Option<&str>, today: Date) {
        if self.account.as_deref() != account || self.day != Some(today) {
            let next_request = self.next_request;
            let selected = self.selected;
            *self = Self {
                account: account.map(str::to_string),
                day: Some(today),
                next_request,
                // Every request sent so far is someone else's.
                first_request: next_request + 1,
                selected: if self.account.as_deref() == account {
                    selected
                } else {
                    None
                },
                ..Self::default()
            };
        }
    }

    /// The My Activity page opened: this month, the timeline (for the
    /// previous month's card) and the local count.
    pub fn open_overview(&mut self, account: Option<&str>, today: Date) {
        self.keep_for(account, today);
        self.want_timeline = true;
        self.wanted.insert(MonthKey::of(today));
        if let Some(previous) = self.previous_month(today) {
            self.wanted.insert(previous);
        }
        // Read afresh each visit: it is local and cheap.
        self.want_local = true;
        if self.local.get().is_none() {
            self.local = Loadable::Loading;
        }
    }

    /// The Top Artists page opened, or its tab changed.
    pub fn open_top_artists(&mut self, account: Option<&str>, today: Date) {
        self.keep_for(account, today);
        self.want_timeline = true;
        self.wanted
            .insert(self.selected.unwrap_or_else(|| MonthKey::of(today)));
    }

    /// Shows `month` (`None`: This Month) on the Top Artists page.
    pub fn select(&mut self, month: Option<MonthKey>, today: Date) {
        self.selected = month.filter(|month| *month < MonthKey::of(today));
        self.wanted
            .insert(self.selected.unwrap_or_else(|| MonthKey::of(today)));
    }

    /// The newest past month with a report.
    pub fn previous_month(&self, today: Date) -> Option<MonthKey> {
        self.past_months(today).first().copied()
    }

    /// Past months with a report, newest first: the tabs after This Month.
    pub fn past_months(&self, today: Date) -> Vec<MonthKey> {
        let current = MonthKey::of(today);
        self.timeline
            .get()
            .map(|months| {
                months
                    .iter()
                    .copied()
                    .filter(|month| *month < current)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn report(&self, month: MonthKey) -> &Loadable<MonthReport> {
        static NOT_LOADED: Loadable<MonthReport> = Loadable::NotLoaded;
        self.reports.get(&month).unwrap_or(&NOT_LOADED)
    }

    /// Whether anything failed, so Retry asks again.
    pub fn retry(&mut self) {
        for report in self.reports.values_mut() {
            if matches!(report, Loadable::Failed(_)) {
                *report = Loadable::NotLoaded;
            }
        }
        if matches!(self.timeline, Loadable::Failed(_)) {
            self.timeline = Loadable::NotLoaded;
        }
    }

    fn request(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }

    /// The requests to send for what the pages want and is not read yet.
    pub fn take_commands(&mut self) -> Vec<Command> {
        let mut commands = Vec::new();
        if std::mem::take(&mut self.want_timeline) && self.timeline.needs_load() {
            self.timeline = Loadable::Loading;
            let request = self.request();
            commands.push(Command::MyActivity {
                request,
                query: ActivityQuery::Timeline,
            });
        }
        let mut wanted: Vec<MonthKey> = self.wanted.drain().collect();
        wanted.sort_unstable_by(|a, b| b.cmp(a));
        for month in wanted {
            let report = self.reports.entry(month).or_default();
            if !report.needs_load() {
                continue;
            }
            *report = Loadable::Loading;
            let request = self.request();
            commands.push(Command::MyActivity {
                request,
                query: ActivityQuery::TopArtists {
                    year: month.year,
                    month: month.month,
                },
            });
        }
        if std::mem::take(&mut self.want_local) {
            let request = self.request();
            self.local_request = Some(request);
            commands.push(Command::Stats {
                request,
                query: StatsQuery {
                    range: StatsRange::ThisMonth,
                    granularity: crate::backend::StatsGranularity::Day,
                    top: LOCAL_TOP_TRACKS,
                },
            });
        }
        commands
    }

    fn merge_timeline(&mut self, months: &[ActivityMonth], replace: bool) {
        let mut keys: Vec<MonthKey> = months.iter().filter_map(MonthKey::from_tidal).collect();
        if !replace && let Some(known) = self.timeline.get() {
            keys.extend(known.iter().copied());
        }
        keys.sort_unstable_by(|a, b| b.cmp(a));
        keys.dedup();
        self.timeline = Loadable::Loaded(keys);
    }

    /// TIDAL's answer to a My Activity request.
    pub fn receive(
        &mut self,
        request: u64,
        query: ActivityQuery,
        result: Result<ActivityReply, String>,
        today: Date,
    ) {
        if request < self.first_request {
            return;
        }
        match (query, result) {
            (ActivityQuery::Timeline, Ok(ActivityReply::Timeline(timeline))) => {
                self.merge_timeline(&timeline.timeline, true);
                // The previous month's card needs its report.
                if let Some(previous) = self.previous_month(today)
                    && self.report(previous).needs_load()
                {
                    self.wanted.insert(previous);
                }
            }
            (ActivityQuery::Timeline, Err(error)) => {
                // A report's own copy of the timeline may already be here.
                if self.timeline.get().is_none() {
                    self.timeline = Loadable::Failed(error);
                }
            }
            (ActivityQuery::TopArtists { year, month }, result) => {
                let Some(key) = MonthKey::new(year, month) else {
                    return;
                };
                let loaded = match result {
                    Ok(ActivityReply::TopArtists(report)) => {
                        if !report.metadata.timeline.is_empty()
                            && !matches!(self.timeline, Loadable::Loaded(_))
                        {
                            self.merge_timeline(&report.metadata.timeline, false);
                        }
                        Loadable::Loaded(MonthReport::from_tidal(&report))
                    }
                    Ok(ActivityReply::Timeline(_)) => return,
                    Err(error) => Loadable::Failed(error),
                };
                self.reports.insert(key, loaded);
            }
            (ActivityQuery::Timeline, Ok(ActivityReply::TopArtists(_))) => {}
        }
    }

    /// The answer to the local statistics request.
    pub fn receive_stats(&mut self, request: u64, result: Result<Box<StatsReport>, String>) {
        if self.local_request != Some(request) {
            return;
        }
        self.local_request = None;
        self.local = match result {
            Ok(report) => Loadable::Loaded(*report),
            Err(error) => Loadable::Failed(error),
        };
    }

    /// The hero cut out of the window's picture taken for Share; Share is
    /// done either way.
    pub fn take_shot(&mut self, window: &egui::ColorImage) -> Option<egui::ColorImage> {
        let Share::Capturing {
            rect,
            pixels_per_point,
            ..
        } = std::mem::take(&mut self.share)
        else {
            return None;
        };
        crop(window, rect, pixels_per_point)
    }

    /// Demo and tests: the data as read.
    pub fn fill(
        &mut self,
        account: Option<&str>,
        today: Date,
        past: Vec<MonthKey>,
        reports: Vec<(MonthKey, MonthReport)>,
        local: StatsReport,
    ) {
        self.keep_for(account, today);
        self.timeline = Loadable::Loaded(past);
        for (month, report) in reports {
            self.reports.insert(month, Loadable::Loaded(report));
        }
        self.local = Loadable::Loaded(local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::tidal::activity::ActivityTimeline;

    fn day(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn report(json: &str) -> TopArtistsReport {
        serde_json::from_str(json).unwrap()
    }

    fn top_artists(commands: &[Command]) -> Vec<(i32, u32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::MyActivity {
                    query: ActivityQuery::TopArtists { year, month },
                    ..
                } => Some((*year, *month)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn days_left_and_progress_follow_the_calendar() {
        assert_eq!(month_progress(day("2026-10-14")).0, 17);
        let (left, progress) = month_progress(day("2026-02-28"));
        assert_eq!(left, 0);
        assert!((progress - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            MonthKey::new(2026, 1).unwrap().previous(),
            MonthKey {
                year: 2025,
                month: 12
            }
        );
        assert_eq!(MonthKey::new(2024, 2).unwrap().days(), 29);
        assert!(MonthKey::new(2026, 13).is_none());
    }

    #[test]
    fn colors_read_as_tidal_writes_them() {
        assert_eq!(parse_color("#aabbcc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_color("#abc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_color("aabbcc"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
    }

    #[test]
    fn a_report_ranks_artists_with_pictures_and_cover_fallbacks() {
        let month = MonthReport::from_tidal(&report(
            r##"{
              "activity": {"totalNrOfDaysLeft": 17, "artists": []},
              "artists": [
                {"artist": {"id": 2, "name": "Second", "picture": "aa-bb", "vibrantColor": "#802020"},
                 "nrOfStreams": 69},
                {"artist": {"id": 1, "name": "First", "picture": null,
                            "selectedAlbumCoverFallback": "cc-dd"}, "nrOfStreams": 84},
                {"artist": null, "nrOfStreams": 3}
              ],
              "disclaimers": [{"text": " We do our best. ", "url": "https://example.com/x"}]
            }"##,
        ));
        assert_eq!(month.days_left, Some(17));
        let names: Vec<_> = month
            .artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect();
        assert_eq!(names, ["First", "Second"]);
        assert!(
            month.artists[0].picture.is_empty(),
            "no picture: the mic disc"
        );
        assert!(month.artists[0].thumbnail[0].url.contains("cc/dd"));
        assert!(month.artists[1].picture[0].url.contains("aa/bb"));
        assert_eq!(month.artists[1].color, Some([0x80, 0x20, 0x20]));
        assert_eq!(month.artists[0].uri(), "tidal:artist:1");
        assert_eq!(
            month.disclaimer,
            Some(("We do our best.".into(), "https://example.com/x".into()))
        );
    }

    /// The note's "Learn more" goes to the system's opener, which runs
    /// whatever the address names: a settings page, a program on a network
    /// share, a file. Only a web address is kept.
    #[test]
    fn a_disclaimer_link_is_kept_only_when_it_is_a_web_address() {
        let link = |url: &str| {
            let text = format!(r#"{{"disclaimers": [{{"text": "Note", "url": {url:?}}}]}}"#);
            MonthReport::from_tidal(&report(&text))
                .disclaimer
                .map(|(_, url)| url)
        };
        assert_eq!(
            link("https://tidal.com/x").as_deref(),
            Some("https://tidal.com/x")
        );
        for url in [
            "ms-settings:privacy",
            "file:///C:/Windows/System32/calc.exe",
            r"\\host\share\run.exe",
            "javascript:alert(1)",
            "tidal://track/1",
        ] {
            assert_eq!(link(url).as_deref(), Some(""), "{url}");
        }
    }

    #[test]
    fn the_card_summary_stands_in_for_a_missing_list() {
        let month = MonthReport::from_tidal(&report(
            r#"{"activity": {"artists": [{"id": 9, "name": "Only", "nrOfStreams": 5}]}}"#,
        ));
        assert_eq!(month.artists.len(), 1);
        assert_eq!(month.artists[0].streams, 5);
        let many = format!(
            r#"{{"artists": [{}]}}"#,
            (0..40)
                .map(|index| format!(
                    r#"{{"artist": {{"id": {index}, "name": "A{index}"}}, "nrOfStreams": {index}}}"#
                ))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            MonthReport::from_tidal(&report(&many)).artists.len(),
            TOP_ARTISTS
        );
    }

    #[test]
    fn the_overview_asks_once_then_follows_the_timeline() {
        let today = day("2026-10-14");
        let mut state = ActivityState::default();
        state.open_overview(Some("u1"), today);
        let commands = state.take_commands();
        assert_eq!(top_artists(&commands), [(2026, 10)]);
        assert!(commands.iter().any(|command| matches!(
            command,
            Command::MyActivity {
                query: ActivityQuery::Timeline,
                ..
            }
        )));
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::Stats { .. }))
        );
        // Nothing twice while in flight.
        state.open_overview(Some("u1"), today);
        assert_eq!(state.take_commands().len(), 1, "only the local count again");

        let timeline: ActivityTimeline = serde_json::from_str(
            r#"{"timeline":[{"year":2026,"month":8},{"year":2026,"month":10},{"year":2026,"month":9}]}"#,
        )
        .unwrap();
        let request = commands
            .iter()
            .find_map(|command| match command {
                Command::MyActivity {
                    request,
                    query: ActivityQuery::Timeline,
                } => Some(*request),
                _ => None,
            })
            .unwrap();
        state.receive(
            request,
            ActivityQuery::Timeline,
            Ok(ActivityReply::Timeline(timeline)),
            today,
        );
        assert_eq!(
            state.past_months(today),
            [
                MonthKey {
                    year: 2026,
                    month: 9
                },
                MonthKey {
                    year: 2026,
                    month: 8
                }
            ]
        );
        assert_eq!(top_artists(&state.take_commands()), [(2026, 9)]);
    }

    #[test]
    fn answers_for_another_account_or_day_are_dropped() {
        let today = day("2026-10-14");
        let mut state = ActivityState::default();
        state.open_top_artists(Some("u1"), today);
        let commands = state.take_commands();
        state.open_top_artists(Some("u2"), today);
        for command in commands {
            if let Command::MyActivity { request, query } = command {
                state.receive(request, query, Err("late".into()), today);
            }
        }
        assert!(
            state
                .reports
                .values()
                .all(|report| !matches!(report, Loadable::Failed(_)))
        );
        // A new day reads everything again.
        let _ = state.take_commands();
        state.reports.insert(
            MonthKey::of(today),
            Loadable::Loaded(MonthReport::default()),
        );
        state.open_top_artists(Some("u2"), day("2026-10-15"));
        assert_eq!(top_artists(&state.take_commands()), [(2026, 10)]);
    }

    #[test]
    fn a_failed_timeline_falls_back_to_the_reports_copy() {
        let today = day("2026-10-14");
        let mut state = ActivityState::default();
        state.open_top_artists(Some("u1"), today);
        let commands = state.take_commands();
        for command in commands {
            if let Command::MyActivity { request, query } = command {
                let result = match query {
                    ActivityQuery::Timeline => Err("404".into()),
                    ActivityQuery::TopArtists { .. } => {
                        Ok(ActivityReply::TopArtists(Box::new(report(
                            r#"{"metadata":{"timeline":[{"year":2026,"month":7,"title":"July"}]}}"#,
                        ))))
                    }
                };
                state.receive(request, query, result, today);
            }
        }
        assert_eq!(
            state.past_months(today),
            [MonthKey {
                year: 2026,
                month: 7
            }]
        );
    }

    #[test]
    fn tabs_select_past_months_only_and_retry_clears_failures() {
        let today = day("2026-10-14");
        let mut state = ActivityState::default();
        state.select(
            Some(MonthKey {
                year: 2026,
                month: 11,
            }),
            today,
        );
        assert_eq!(state.selected, None);
        assert_eq!(
            top_artists(&state.take_commands()),
            [(2026, 10)],
            "This Month"
        );
        state.select(
            Some(MonthKey {
                year: 2026,
                month: 5,
            }),
            today,
        );
        assert_eq!(top_artists(&state.take_commands()), [(2026, 5)]);
        state.reports.insert(
            MonthKey {
                year: 2026,
                month: 5,
            },
            Loadable::Failed("x".into()),
        );
        state.retry();
        state.open_top_artists(None, today);
        assert_eq!(top_artists(&state.take_commands()), [(2026, 5)]);
    }

    #[test]
    fn share_crops_the_hero_in_pixels() {
        let pixels = (0..16u8).map(egui::Color32::from_gray).collect();
        let window = egui::ColorImage::new([4, 4], pixels);
        let mut state = ActivityState {
            share: Share::Capturing {
                rect: egui::Rect::from_min_max(egui::pos2(0.5, 0.5), egui::pos2(1.5, 2.0)),
                pixels_per_point: 2.0,
                since: 0.0,
            },
            ..ActivityState::default()
        };
        let picture = state.take_shot(&window).unwrap();
        assert_eq!(picture.size, [2, 3]);
        assert_eq!(picture.pixels[0], egui::Color32::from_gray(5));
        assert_eq!(state.share, Share::Idle);
        assert!(state.take_shot(&window).is_none(), "only once");
        assert!(
            crop(
                &window,
                egui::Rect::from_min_max(egui::pos2(9.0, 9.0), egui::pos2(10.0, 10.0)),
                1.0
            )
            .is_none()
        );
    }

    #[test]
    fn stats_answers_only_count_for_the_latest_request() {
        let today = day("2026-10-14");
        let mut state = ActivityState::default();
        state.open_overview(None, today);
        let request = state
            .take_commands()
            .into_iter()
            .find_map(|command| match command {
                Command::Stats { request, query } => {
                    assert_eq!(query.range, StatsRange::ThisMonth);
                    Some(request)
                }
                _ => None,
            })
            .unwrap();
        state.receive_stats(request + 100, Ok(Box::default()));
        assert!(state.local.is_loading());
        state.receive_stats(request, Ok(Box::default()));
        assert!(state.local.get().is_some());
    }

    /// The Tidalfast section lists the month's top 20 songs, not only 5.
    #[test]
    fn the_overview_asks_for_twenty_top_songs() {
        let mut state = ActivityState::default();
        state.open_overview(None, day("2026-10-14"));
        let top = state
            .take_commands()
            .into_iter()
            .find_map(|command| match command {
                Command::Stats { query, .. } => Some(query.top),
                _ => None,
            });
        assert_eq!(top, Some(20));
    }
}
