//! Turns the engine's state reports into listening sessions.
//!
//! The engine reports a [`LocalState`] whenever something changes and once
//! a second while playing. [`ListenTracker`] follows those reports and, per
//! played track (one `track_sequence`), keeps the PLAYBACK_START /
//! PLAYBACK_STOP actions TIDAL's `playback_session` event carries: a start
//! when sound begins or resumes, a stop on pause, a stop and a start around
//! each seek, and a final stop when the track ends, is skipped, or playback
//! stops. Listened time is the sum of the played stretches, so paused time
//! and seeked-over music never count.

use serde::{Deserialize, Serialize};

use crate::tidal_engine::{LocalState, LocalTrack, Playback, Quality};

/// A play counts as a stream after 30 seconds, or half of a shorter track
/// (tidal-activity.md 2.3; the same rule as [`crate::history`]).
pub const STREAM_AFTER_MS: u64 = 30_000;

/// Whether `listened_ms` of a `duration_ms` track is a stream.
pub fn is_stream(listened_ms: u64, duration_ms: u32) -> bool {
    listened_ms > 0
        && (listened_ms >= STREAM_AFTER_MS
            || (duration_ms > 0 && listened_ms * 2 >= u64::from(duration_ms)))
}

/// A track ended this close to its end played to the end: a crossfade of
/// up to 12 s starts the next one early.
fn completes(end_ms: u32, duration_ms: u32) -> bool {
    if duration_ms == 0 {
        return false;
    }
    let slack = (duration_ms / 10).clamp(1_500, 13_000);
    end_ms.saturating_add(slack) >= duration_ms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    #[serde(rename = "PLAYBACK_START")]
    Start,
    #[serde(rename = "PLAYBACK_STOP")]
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionType,
    pub position_ms: u32,
    /// Unix milliseconds.
    pub timestamp_ms: i64,
}

/// One finished listen of one track.
#[derive(Clone, Debug, PartialEq)]
pub struct Listen {
    /// The `playbackSessionId`.
    pub session_id: String,
    pub track: LocalTrack,
    pub context_uri: Option<String>,
    pub quality: Option<Quality>,
    pub actions: Vec<Action>,
    pub listened_ms: u64,
    /// Played to (about) the end rather than skipped or stopped.
    pub completed: bool,
}

impl Listen {
    pub fn start_ms(&self) -> i64 {
        self.actions.first().map_or(0, |action| action.timestamp_ms)
    }

    pub fn end_ms(&self) -> i64 {
        self.actions.last().map_or(0, |action| action.timestamp_ms)
    }

    pub fn start_position_ms(&self) -> u32 {
        self.actions.first().map_or(0, |action| action.position_ms)
    }

    pub fn end_position_ms(&self) -> u32 {
        self.actions.last().map_or(0, |action| action.position_ms)
    }

    pub fn is_stream(&self) -> bool {
        is_stream(self.listened_ms, self.track.duration_ms)
    }
}

struct Session {
    sequence: u64,
    seek_sequence: u64,
    session_id: String,
    track: LocalTrack,
    context_uri: Option<String>,
    quality: Option<Quality>,
    actions: Vec<Action>,
    playing: bool,
    /// Position at the last report, and when it was seen.
    position_ms: u32,
    observed_ms: i64,
    segment_start_ms: u32,
    listened_ms: u64,
}

impl Session {
    /// Where the music is now, from the last report.
    fn position_at(&self, now_ms: i64) -> u32 {
        if !self.playing {
            return self.position_ms;
        }
        let elapsed = u32::try_from((now_ms - self.observed_ms).max(0)).unwrap_or(u32::MAX);
        let position = self.position_ms.saturating_add(elapsed);
        match self.track.duration_ms {
            0 => position,
            duration => position.min(duration),
        }
    }

    fn start(&mut self, position_ms: u32, now_ms: i64) {
        self.actions.push(Action {
            kind: ActionType::Start,
            position_ms,
            timestamp_ms: now_ms,
        });
        self.segment_start_ms = position_ms;
        self.playing = true;
    }

    fn stop(&mut self, position_ms: u32, now_ms: i64) {
        self.actions.push(Action {
            kind: ActionType::Stop,
            position_ms,
            timestamp_ms: now_ms,
        });
        self.listened_ms += u64::from(position_ms.saturating_sub(self.segment_start_ms));
        self.playing = false;
    }

    fn finish(mut self, now_ms: i64) -> Option<Listen> {
        if self.playing {
            let position = self.position_at(now_ms);
            self.stop(position, now_ms);
        }
        if self.actions.is_empty() {
            return None;
        }
        let end = self.actions.last().map_or(0, |action| action.position_ms);
        Some(Listen {
            completed: completes(end, self.track.duration_ms),
            session_id: self.session_id,
            track: self.track,
            context_uri: self.context_uri,
            quality: self.quality,
            actions: self.actions,
            listened_ms: self.listened_ms,
        })
    }
}

/// Follows the engine's reports; see the module docs.
#[derive(Default)]
pub struct ListenTracker {
    current: Option<Session>,
}

impl ListenTracker {
    /// Takes one engine report seen at `now_ms` (Unix milliseconds). Returns
    /// the listen it finished, if any.
    pub fn observe(&mut self, state: &LocalState, now_ms: i64) -> Option<Listen> {
        let active = state.track.is_some() && state.playback != Playback::Stopped;
        let same = self
            .current
            .as_ref()
            .is_some_and(|session| active && session.sequence == state.track_sequence);
        let finished = if same {
            None
        } else {
            self.current
                .take()
                .and_then(|session| session.finish(now_ms))
        };
        if !active {
            return finished;
        }
        let position = state.position_now();
        let session = self.current.get_or_insert_with(|| Session {
            sequence: state.track_sequence,
            seek_sequence: state.seek_sequence,
            session_id: super::uuid_v4(),
            track: state.track.clone().unwrap_or_default(),
            context_uri: state.context_uri.clone(),
            quality: None,
            actions: Vec::new(),
            playing: false,
            position_ms: position,
            observed_ms: now_ms,
            segment_start_ms: position,
            listened_ms: 0,
        });
        if let Some(quality) = state.format.as_ref().and_then(|format| format.quality) {
            session.quality = Some(quality);
        }
        if let Some(track) = &state.track
            && session.track.duration_ms == 0
        {
            session.track.duration_ms = track.duration_ms;
        }
        let playing = state.playback == Playback::Playing && !state.loading;
        let expected = session.position_at(now_ms);
        // A jump back without a seek report (a restart) is a seek too.
        let seeked = state.seek_sequence != session.seek_sequence
            || (session.playing && playing && position.saturating_add(3_000) < expected);
        if seeked {
            if session.playing {
                session.stop(expected, now_ms);
            }
            if playing {
                session.start(position, now_ms);
            }
        } else if playing && !session.playing {
            session.start(position, now_ms);
        } else if !playing && session.playing {
            session.stop(position.max(session.segment_start_ms), now_ms);
        }
        session.seek_sequence = state.seek_sequence;
        session.position_ms = position;
        session.observed_ms = now_ms;
        finished
    }

    /// Ends the listen in progress (sign-out, quit).
    pub fn finish(&mut self, now_ms: i64) -> Option<Listen> {
        self.current
            .take()
            .and_then(|session| session.finish(now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, duration_ms: u32) -> LocalTrack {
        LocalTrack {
            uri: format!("tidal:track:{id}"),
            title: format!("Song {id}"),
            duration_ms,
            ..LocalTrack::default()
        }
    }

    /// A fake engine timeline: reports as the engine sends them, with the
    /// position as of each report (`position_at` is left unset so the
    /// reported position is exact).
    struct Timeline {
        tracker: ListenTracker,
        state: LocalState,
        listens: Vec<Listen>,
    }

    impl Timeline {
        fn new() -> Self {
            Self {
                tracker: ListenTracker::default(),
                state: LocalState::default(),
                listens: Vec::new(),
            }
        }

        fn report(&mut self, at_s: f64, change: impl FnOnce(&mut LocalState)) {
            change(&mut self.state);
            let now = 1_760_000_000_000 + (at_s * 1000.0) as i64;
            self.listens
                .extend(self.tracker.observe(&self.state.clone(), now));
        }

        fn load(&mut self, at_s: f64, track: LocalTrack, context: Option<&str>) {
            self.report(at_s, |state| {
                state.track = Some(track);
                state.track_sequence += 1;
                state.loading = true;
                state.position_ms = 0;
                state.context_uri = context.map(str::to_string);
                if state.playback == Playback::Stopped {
                    state.playback = Playback::Loading;
                }
            });
        }

        fn play_at(&mut self, at_s: f64, position_ms: u32) {
            self.report(at_s, |state| {
                state.loading = false;
                state.playback = Playback::Playing;
                state.position_ms = position_ms;
            });
        }
    }

    #[test]
    fn a_track_played_through_is_one_complete_stream() {
        let mut timeline = Timeline::new();
        timeline.load(0.0, track("1", 180_000), Some("tidal:album:9"));
        timeline.play_at(0.5, 0);
        for second in 1..180 {
            timeline.play_at(0.5 + f64::from(second), second * 1000);
        }
        timeline.load(180.5, track("2", 200_000), Some("tidal:album:9"));
        assert_eq!(timeline.listens.len(), 1);
        let listen = &timeline.listens[0];
        assert_eq!(listen.track.uri, "tidal:track:1");
        assert_eq!(listen.context_uri.as_deref(), Some("tidal:album:9"));
        assert_eq!(listen.actions.len(), 2);
        assert_eq!(listen.actions[0].kind, ActionType::Start);
        assert_eq!(listen.start_position_ms(), 0);
        assert_eq!(listen.end_position_ms(), 180_000);
        assert_eq!(listen.listened_ms, 180_000);
        assert_eq!(listen.end_ms() - listen.start_ms(), 180_000);
        assert!(listen.completed && listen.is_stream());
    }

    #[test]
    fn pause_resume_and_seek_make_action_pairs_and_skip_counts_only_heard_music() {
        let mut timeline = Timeline::new();
        timeline.load(0.0, track("1", 240_000), None);
        timeline.play_at(1.0, 0);
        timeline.play_at(11.0, 10_000);
        // Pause at 10 s, resume 20 s later.
        timeline.report(11.0, |state| state.playback = Playback::Paused);
        timeline.play_at(31.0, 10_000);
        // Seek from 20 s to 100 s.
        timeline.report(41.0, |state| {
            state.position_ms = 100_000;
            state.seek_sequence += 1;
        });
        timeline.play_at(46.0, 105_000);
        // Skip to the next song.
        timeline.load(46.0, track("2", 200_000), None);
        let listen = &timeline.listens[0];
        let kinds: Vec<_> = listen.actions.iter().map(|action| action.kind).collect();
        use ActionType::{Start, Stop};
        assert_eq!(kinds, [Start, Stop, Start, Stop, Start, Stop]);
        assert_eq!(listen.actions[3].position_ms, 20_000);
        assert_eq!(listen.actions[4].position_ms, 100_000);
        assert_eq!(listen.listened_ms, 10_000 + 10_000 + 5_000);
        assert!(!listen.completed);
        assert!(!listen.is_stream());
        assert_eq!(listen.context_uri, None);
    }

    #[test]
    fn a_track_skipped_while_loading_is_no_listen_and_stop_ends_one() {
        let mut timeline = Timeline::new();
        timeline.load(0.0, track("1", 100_000), None);
        timeline.load(0.2, track("2", 100_000), None);
        assert!(timeline.listens.is_empty());
        timeline.play_at(1.0, 0);
        // Stopped after 40 s, mid-report: the end is interpolated.
        timeline.play_at(40.0, 39_000);
        timeline.report(41.0, |state| {
            state.playback = Playback::Stopped;
        });
        assert_eq!(timeline.listens.len(), 1);
        assert_eq!(timeline.listens[0].track.uri, "tidal:track:2");
        assert_eq!(timeline.listens[0].listened_ms, 40_000);
        assert!(timeline.listens[0].is_stream());
    }

    #[test]
    fn replaying_the_same_track_is_a_new_session_and_finish_closes_the_last() {
        let mut timeline = Timeline::new();
        timeline.load(0.0, track("1", 60_000), None);
        timeline.play_at(0.0, 0);
        timeline.play_at(40.0, 40_000);
        timeline.load(40.0, track("1", 60_000), None);
        timeline.play_at(41.0, 0);
        assert_eq!(timeline.listens.len(), 1);
        let last = timeline.tracker.finish(1_760_000_000_000 + 51_000).unwrap();
        assert_eq!(last.listened_ms, 10_000);
        assert_ne!(last.session_id, timeline.listens[0].session_id);
        assert!(timeline.tracker.finish(0).is_none());
    }

    #[test]
    fn the_stream_rule_is_thirty_seconds_or_half() {
        assert!(is_stream(30_000, 600_000));
        assert!(!is_stream(29_999, 600_000));
        assert!(is_stream(10_000, 20_000));
        assert!(!is_stream(0, 0));
    }
}
