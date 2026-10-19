//! TIDAL's "My Activity" listening statistics.
//!
//! Two endpoints of the private v2 API are known
//! (docs/_reference/tidal-activity.md, section 1): `my-activity/timeline`,
//! the months that have a report, and `my-activity/top-artists`, one
//! month's most played artists with stream counts. Their JSON is only known
//! from one open-source client's models, so every model here is tolerant:
//! unknown fields are ignored, every field has a default, `null` reads as
//! empty, ids may be numbers or strings, and enum-like values stay strings.
//! The `_raw` variants return TIDAL's JSON untouched, for checking the shapes
//! against a live account.

use reqwest::Method;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{Call, LOCALE, TidalClient};
use crate::api::client::Result;

fn nullable<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// A number that may come as a JSON number or a numeric string; anything
/// else reads as `None`.
fn loose_number<'de, D>(deserializer: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float as i64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

fn loose_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => text,
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    })
}

/// One month that has a report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ActivityMonth {
    #[serde(deserialize_with = "loose_number")]
    pub year: Option<i64>,
    #[serde(deserialize_with = "loose_number")]
    pub month: Option<i64>,
    /// The month's name, localized by TIDAL.
    #[serde(deserialize_with = "loose_string")]
    pub title: String,
}

/// `GET my-activity/timeline`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ActivityTimeline {
    #[serde(deserialize_with = "nullable")]
    pub timeline: Vec<ActivityMonth>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActivityArtistSummary {
    #[serde(deserialize_with = "loose_string")]
    pub id: String,
    #[serde(deserialize_with = "loose_string")]
    pub name: String,
    #[serde(deserialize_with = "loose_number")]
    pub nr_of_streams: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ActivityImage {
    #[serde(deserialize_with = "loose_string")]
    pub id: String,
    #[serde(deserialize_with = "loose_string")]
    pub url: String,
}

/// The month's summary card.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActivityCard {
    /// TIDAL's own type tag, kept as text: its values are not known.
    #[serde(deserialize_with = "loose_string")]
    pub activity_type: String,
    #[serde(deserialize_with = "nullable")]
    pub artists: Vec<ActivityArtistSummary>,
    #[serde(deserialize_with = "nullable")]
    pub images: Vec<ActivityImage>,
    #[serde(deserialize_with = "loose_number")]
    pub year: Option<i64>,
    #[serde(deserialize_with = "loose_number")]
    pub month: Option<i64>,
    /// Days until the month's report is final, while it is still running.
    #[serde(deserialize_with = "loose_number")]
    pub total_nr_of_days_left: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub title: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub subtitle: Option<String>,
}

/// An artist as the report lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActivityArtist {
    #[serde(deserialize_with = "loose_string")]
    pub id: String,
    #[serde(deserialize_with = "loose_string")]
    pub name: String,
    /// Image uuid; see [`super::convert::image_url`].
    #[serde(deserialize_with = "nullable")]
    pub picture: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub selected_album_cover_fallback: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub vibrant_color: Option<String>,
    #[serde(deserialize_with = "loose_number")]
    pub popularity: Option<i64>,
    /// Mix type ("ARTIST_MIX", ...) to mix id.
    #[serde(deserialize_with = "nullable")]
    pub mixes: std::collections::BTreeMap<String, Value>,
}

impl ActivityArtist {
    pub fn uri(&self) -> String {
        super::convert::uri("artist", &self.id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActivityArtistEntry {
    #[serde(deserialize_with = "nullable")]
    pub artist: ActivityArtist,
    #[serde(deserialize_with = "loose_number")]
    pub nr_of_streams: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ActivityDisclaimer {
    #[serde(deserialize_with = "loose_string")]
    pub text: String,
    #[serde(deserialize_with = "loose_string")]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ActivityMetadata {
    #[serde(deserialize_with = "nullable")]
    pub timeline: Vec<ActivityMonth>,
}

/// `GET my-activity/top-artists?year=&month=`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TopArtistsReport {
    #[serde(deserialize_with = "nullable")]
    pub activity: Option<ActivityCard>,
    #[serde(deserialize_with = "nullable")]
    pub artists: Vec<ActivityArtistEntry>,
    #[serde(deserialize_with = "nullable")]
    pub disclaimers: Vec<ActivityDisclaimer>,
    #[serde(deserialize_with = "nullable")]
    pub metadata: ActivityMetadata,
}

/// Endpoints that might exist beside the two known ones. Probed once by the
/// activity dump, never used otherwise (tidal-activity.md, 1.4).
pub const GUESSED_ACTIVITY_PATHS: [&str; 5] = [
    "my-activity/top-tracks",
    "my-activity/top-albums",
    "my-activity/top-genres",
    "my-activity/listening-time",
    "my-activity/summary",
];

impl TidalClient {
    fn activity_call(&self, path: &str) -> Call {
        Call::new(Method::GET, self.v2(path)).query("locale", LOCALE)
    }

    fn month_call(&self, path: &str, year: i32, month: u32) -> Call {
        self.activity_call(path)
            .query("year", year)
            .query("month", month)
    }

    /// The months that have a report, as TIDAL sends them.
    pub async fn my_activity_timeline_raw(&self) -> Result<Value> {
        self.get(self.activity_call("my-activity/timeline")).await
    }

    /// The months that have a report.
    pub async fn my_activity_timeline(&self) -> Result<ActivityTimeline> {
        self.get(self.activity_call("my-activity/timeline")).await
    }

    /// One month's most played artists, as TIDAL sends them.
    pub async fn my_activity_top_artists_raw(&self, year: i32, month: u32) -> Result<Value> {
        self.get(self.month_call("my-activity/top-artists", year, month))
            .await
    }

    /// One month's most played artists with their stream counts.
    pub async fn my_activity_top_artists(&self, year: i32, month: u32) -> Result<TopArtistsReport> {
        self.get(self.month_call("my-activity/top-artists", year, month))
            .await
    }

    /// Any v2 path for one month, raw; the answer's status on failure.
    /// Only for the activity dump's probes.
    pub async fn my_activity_probe(&self, path: &str, year: i32, month: u32) -> Result<Value> {
        self.get(self.month_call(path, year, month)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeline_reads_tidlers_shape_and_odd_values() {
        let timeline: ActivityTimeline = serde_json::from_str(
            r#"{"timeline":[{"year":2026,"month":9,"title":"September"},
                {"year":"2026","month":"8","title":null,"extra":true}],"more":1}"#,
        )
        .unwrap();
        assert_eq!(timeline.timeline.len(), 2);
        assert_eq!(timeline.timeline[0].month, Some(9));
        assert_eq!(timeline.timeline[1].year, Some(2026));
        assert_eq!(timeline.timeline[1].title, "");
        let empty: ActivityTimeline = serde_json::from_str(r#"{"timeline":null}"#).unwrap();
        assert!(empty.timeline.is_empty());
    }

    #[test]
    fn top_artists_read_with_unknown_fields_and_nulls() {
        let report: TopArtistsReport = serde_json::from_str(
            r##"{
              "activity": {"@class":"x.y.Z","activityType":"SOMETHING_NEW",
                "artists":[{"id":123,"name":"A","nrOfStreams":42}],
                "images":[{"id":"i","url":"https://x/y.jpg"}],
                "month":9,"year":2026,"totalNrOfDaysLeft":null,"title":"T","subtitle":null},
              "artists":[{"artist":{"id":123,"name":"A","picture":null,"popularity":77,
                  "doublePopularity":77.3,"artistRoles":[{"categoryId":-1,"category":"Artist"}],
                  "artistTypes":["ARTIST"],"mixes":{"ARTIST_MIX":"abc"},"vibrantColor":"#aabbcc"},
                "nrOfStreams":"41"},
                {"artist":null,"nrOfStreams":1}],
              "disclaimers":null,
              "metadata":{"timeline":[{"year":2026,"month":9,"title":"September"}]},
              "brandNew":{"a":1}
            }"##,
        )
        .unwrap();
        let card = report.activity.unwrap();
        assert_eq!(card.activity_type, "SOMETHING_NEW");
        assert_eq!(card.artists[0].nr_of_streams, Some(42));
        assert_eq!(card.total_nr_of_days_left, None);
        assert_eq!(report.artists[0].artist.id, "123");
        assert_eq!(report.artists[0].nr_of_streams, Some(41));
        assert_eq!(report.artists[0].artist.uri(), "tidal:artist:123");
        assert_eq!(report.artists[1].artist, ActivityArtist::default());
        assert!(report.disclaimers.is_empty());
        assert_eq!(report.metadata.timeline.len(), 1);
        let empty: TopArtistsReport = serde_json::from_str("{}").unwrap();
        assert!(empty.activity.is_none() && empty.artists.is_empty());
    }
}
