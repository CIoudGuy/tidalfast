//! TIDAL's "Allow explicit content" and "Allow AI content" settings: which
//! tracks the account has chosen not to play. A track the filter refuses is
//! greyed out in lists and left out of playback and the queue, like a track
//! TIDAL cannot play. Videos are not included, as in TIDAL.
//!
//! This is a separate predicate from every other reason a track is skipped
//! (availability, blocked artists); each caller combines them.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::api::models::Track;
use crate::settings::Settings;

/// What the account allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentFilter {
    pub explicit: bool,
    pub ai: bool,
}

impl Default for ContentFilter {
    /// Everything allowed, as TIDAL starts.
    fn default() -> Self {
        Self {
            explicit: true,
            ai: true,
        }
    }
}

static ALLOW_EXPLICIT: AtomicBool = AtomicBool::new(true);
static ALLOW_AI: AtomicBool = AtomicBool::new(true);

impl ContentFilter {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            explicit: settings.allow_explicit,
            ai: settings.allow_ai,
        }
    }

    /// Whether the account lets this track play.
    pub fn allows(self, track: &Track) -> bool {
        (self.explicit || !track.explicit) && (self.ai || !track.ai)
    }

    /// The filter the playback engine's catalog applies to the tracks it
    /// loads; the app sets it whenever the settings change.
    pub fn engine() -> Self {
        Self {
            explicit: ALLOW_EXPLICIT.load(Ordering::Relaxed),
            ai: ALLOW_AI.load(Ordering::Relaxed),
        }
    }

    pub fn set_engine(self) {
        ALLOW_EXPLICIT.store(self.explicit, Ordering::Relaxed);
        ALLOW_AI.store(self.ai, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(explicit: bool, ai: bool) -> Track {
        Track {
            explicit,
            ai,
            ..Track::default()
        }
    }

    #[test]
    fn explicit_plays_and_ai_is_skipped_by_default() {
        let filter = ContentFilter::from_settings(&Settings::default());
        assert!(filter.allows(&track(true, false)));
        assert!(!filter.allows(&track(false, true)));
    }

    #[test]
    fn explicit_off_refuses_only_explicit_tracks() {
        let filter = ContentFilter {
            explicit: false,
            ai: true,
        };
        assert!(!filter.allows(&track(true, false)));
        assert!(filter.allows(&track(false, false)));
        assert!(filter.allows(&track(false, true)));
    }

    #[test]
    fn ai_off_refuses_only_ai_tracks() {
        let filter = ContentFilter {
            explicit: true,
            ai: false,
        };
        assert!(!filter.allows(&track(false, true)));
        assert!(!filter.allows(&track(true, true)));
        assert!(filter.allows(&track(true, false)));
    }
}
