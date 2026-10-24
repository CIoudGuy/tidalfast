//! TIDAL's Feed (`v2/feed/activities`): new releases by followed artists
//! and new monthly mixes, as the web player's Feed page lists them.
//!
//! ```json
//! { "activities": [ { "followableActivity": {
//!       "activityType": "NEW_ALBUM_RELEASE" | "NEW_HISTORY_MIX",
//!       "occurredAt": "2026-10-01T00:00:00.000Z",
//!       "album": { ..v1 album.. } | "historyMix": { ..v1 mix.. } },
//!     "seen": true } ],
//!   "stats": { "totalNotSeenActivities": 0 } }
//! ```
//!
//! Every field is read tolerantly: an activity of an unknown kind, or one
//! whose payload does not parse, is skipped rather than failing the page.

use serde::Deserialize;
use serde_json::Value;

use super::convert::{self, RawAlbum, RawMix};
use crate::api::models::{Feed, FeedEntry, FeedItem, ReleaseKind};

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawActivity {
    activity_type: Option<String>,
    occurred_at: Option<String>,
    album: Option<Value>,
    history_mix: Option<Value>,
}

/// The Feed from its JSON answer.
pub(crate) fn parse(value: &Value) -> Feed {
    let entries = value
        .get("activities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(entry)
        .collect();
    Feed {
        entries,
        unseen: unseen(value),
    }
}

/// How many activities the account has not seen, from an answer of
/// `v2/feed/activities` (TIDAL 2.46.0: `stats.totalNotSeenActivities ?? 0`).
/// The sidebar's counter shows it; the light check asks with `limit=9`
/// and reads only this.
pub(crate) fn unseen(value: &Value) -> u32 {
    value
        .get("stats")
        .and_then(|stats| stats.get("totalNotSeenActivities"))
        .and_then(Value::as_u64)
        .map_or(0, |count| u32::try_from(count).unwrap_or(u32::MAX))
}

fn entry(value: &Value) -> Option<FeedEntry> {
    let activity: RawActivity =
        serde_json::from_value(value.get("followableActivity")?.clone()).ok()?;
    let item = match (activity.album, activity.history_mix) {
        (Some(album), _) => {
            let raw: RawAlbum = serde_json::from_value(album).ok()?;
            if raw.id.is_empty() {
                return None;
            }
            let kind = match raw.kind.as_deref() {
                Some("SINGLE") => ReleaseKind::Single,
                Some("EP") => ReleaseKind::Ep,
                _ => ReleaseKind::Album,
            };
            let explicit = raw.explicit.unwrap_or(false);
            FeedItem::Release {
                album: convert::album(raw),
                kind,
                explicit,
            }
        }
        (None, Some(mix)) => {
            let raw: RawMix = serde_json::from_value(mix).ok()?;
            if raw.id.is_empty() {
                return None;
            }
            FeedItem::Mix(convert::mix(raw))
        }
        (None, None) => {
            log::debug!(
                "TIDAL feed: skipped a {} activity",
                activity.activity_type.as_deref().unwrap_or("?")
            );
            return None;
        }
    };
    let date = activity
        .occurred_at
        .as_deref()
        .and_then(|at| at.get(..10))
        .unwrap_or_default()
        .to_string();
    Some(FeedEntry {
        item,
        date,
        seen: value.get("seen").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// Which of the Feed's groups a `YYYY-MM-DD` date falls in, seen from
/// `today` (`YYYY-MM-DD`): 0 this month, 1 last month, 2 older.
pub fn month_group(date: &str, today: &str) -> usize {
    let month = |text: &str| -> Option<i32> {
        let year: i32 = text.get(..4)?.parse().ok()?;
        let month: i32 = text.get(5..7)?.parse().ok()?;
        Some(year * 12 + month - 1)
    };
    match (month(date), month(today)) {
        (Some(date), Some(today)) if date >= today => 0,
        (Some(date), Some(today)) if date == today - 1 => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn releases_and_mixes_parse_and_unknown_kinds_are_skipped() {
        let feed = parse(&json!({
            "activities": [
                { "followableActivity": {
                    "activityType": "NEW_ALBUM_RELEASE",
                    "occurredAt": "2026-10-01T00:00:00.000Z",
                    "album": { "id": 561498456, "title": "Mad", "type": "SINGLE",
                        "explicit": true, "cover": "562d2866-4f10-4912-bb5f-db6b751365b9",
                        "videoCover": null,
                        "artists": [{ "id": 9028244, "name": "Pip Millett", "main": true }] } },
                  "seen": false },
                { "followableActivity": {
                    "activityType": "NEW_HISTORY_MIX",
                    "occurredAt": "2026-09-01T09:14:56.499Z",
                    "historyMix": { "id": "008ddf", "mixType": "HISTORY_MONTHLY_MIX",
                        "title": "August 2026",
                        "subTitle": "Carla's Dreams, Ariana Grande and more",
                        "images": { "SMALL": { "width": 267, "height": 267, "url": "https://images.tidal.com/x" } } } },
                  "seen": true },
                { "followableActivity": { "activityType": "SOMETHING_NEW", "occurredAt": "2026-08-01T00:00:00Z" } },
                { "followableActivity": { "album": { "title": "no id" } } },
                "junk"
            ],
            "stats": { "totalNotSeenActivities": 1 }
        }));
        assert_eq!(feed.unseen, 1);
        assert_eq!(feed.entries.len(), 2);
        let first = &feed.entries[0];
        assert_eq!(first.date, "2026-10-01");
        assert!(!first.seen);
        match &first.item {
            FeedItem::Release {
                album,
                kind,
                explicit,
            } => {
                assert_eq!(album.id, "561498456");
                assert_eq!(album.name, "Mad");
                assert_eq!(album.artists[0].name, "Pip Millett");
                assert_eq!(*kind, ReleaseKind::Single);
                assert!(*explicit);
                assert!(album.video_cover.is_none());
            }
            other => panic!("expected a release, got {other:?}"),
        }
        match &feed.entries[1].item {
            FeedItem::Mix(mix) => {
                assert_eq!(mix.name, "August 2026");
                assert_eq!(
                    mix.subtitle.as_deref(),
                    Some("Carla's Dreams, Ariana Grande and more")
                );
                assert_eq!(mix.images.len(), 1);
            }
            other => panic!("expected a mix, got {other:?}"),
        }
    }

    #[test]
    fn the_unseen_count_is_read_from_the_stats_alone() {
        // The light check's answer (`limit=9`): only the stats count.
        let answer = json!({
            "activities": [{ "followableActivity": { "activityType": "NEW_ALBUM_RELEASE" }, "seen": false }],
            "stats": { "totalNotSeenActivities": 3 }
        });
        assert_eq!(unseen(&answer), 3);
        assert_eq!(
            unseen(&json!({ "stats": { "totalNotSeenActivities": 0 } })),
            0
        );
        // TIDAL's `?? 0`: no stats, or no number, is none unseen.
        assert_eq!(unseen(&json!({ "activities": [] })), 0);
        assert_eq!(
            unseen(&json!({ "stats": { "totalNotSeenActivities": null } })),
            0
        );
        assert_eq!(
            unseen(&json!({ "stats": { "totalNotSeenActivities": "2" } })),
            0
        );
    }

    #[test]
    fn an_empty_or_odd_answer_is_an_empty_feed() {
        assert_eq!(parse(&json!({})), Feed::default());
        assert_eq!(parse(&json!({ "activities": null })), Feed::default());
    }

    #[test]
    fn months_group_as_this_last_and_older() {
        assert_eq!(month_group("2026-10-01", "2026-10-14"), 0);
        assert_eq!(month_group("2026-09-30", "2026-10-14"), 1);
        assert_eq!(month_group("2026-08-28", "2026-10-14"), 2);
        assert_eq!(month_group("2025-12-31", "2026-01-02"), 1);
        assert_eq!(month_group("", "2026-01-02"), 2);
    }
}
