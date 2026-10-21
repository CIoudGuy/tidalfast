//! The TIDAL API client.
//!
//! Async methods mirror the `ApiClient` shape the backend calls, with the
//! same names and `api::models` return types, so call sites barely change.
//! TIDAL JSON is mapped in [`convert`]; pages in [`pages`]; playback
//! manifests in [`manifest`].
//!
//! Every request goes through one helper that adds the bearer token, the
//! account's `countryCode` and TIDAL's client-version header; caps
//! concurrency; refreshes an expired token once and retries; waits out 429s
//! (`Retry-After`, else exponential backoff); retries idempotent reads on
//! 5xx; and turns TIDAL's `subStatus` errors into readable messages. It uses
//! the process-wide [`Http`] client, so proxy settings apply.
//!
//! Internal URIs are `tidal:<kind>:<id>` (.internal/docs/TIDAL-PORT.md).

pub mod activity;
pub mod convert;
pub mod events;
pub mod feed;
pub mod manifest;
pub mod pages;
mod playlist_v2;
pub mod save_later;
pub mod search;
pub mod uploads;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Semaphore;

use self::convert::{RawAlbum, RawArtist, RawMix, RawPage, RawPlaylist, RawTrack, Wrapped};
use super::ApiSource;
use super::client::{ActivityGuard, ApiError, NetActivity, Result};
use super::models::*;
use super::tidal_catalog::{Quality, StreamInfo};
use crate::artist_blocks::{BlockedArtist, BlockedTrack};
use crate::auth::tidal::{AuthError, TidalTokens};
use crate::http::Http;

pub const API_BASE: &str = "https://api.tidal.com/v1";
pub const V2_BASE: &str = "https://api.tidal.com/v2";
/// TIDAL's OpenAPI host, for `trackManifests`.
pub const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";
/// The web player's version, which TIDAL's newer endpoints expect.
const CLIENT_VERSION: &str = "2025.7.16";
const LOCALE: &str = "en_US";
const MAX_IN_FLIGHT: usize = 6;
const RETRIES: u32 = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
/// TIDAL lists playlist and album items 100 at a time.
const ITEMS_LIMIT: u32 = 100;
/// An artist page module's "View all" lists 50 at a time (100 answers 400).
const MODULE_LIMIT: u32 = 50;
const SEARCH_LIMIT: u32 = 25;
/// Playlist item removals are sent in chunks this size.
const REMOVE_CHUNK: usize = 50;
/// Pages of the v2 home feed read at most.
const HOME_FEED_PAGES: usize = 4;
/// Shelf lists read at once while filling a page.
pub const PENDING_AT_ONCE: usize = 4;

/// What `TIDALFAST_DUMP_HOME=1` saves: TIDAL's raw Home answers and how
/// long each read took. Never a token or header.
#[derive(Debug, Default, serde::Serialize)]
pub struct HomeDump {
    pub v2: Vec<Value>,
    pub v1: Option<Value>,
    pub timings: Vec<HomeTiming>,
}

#[derive(Debug, serde::Serialize)]
pub struct HomeTiming {
    pub what: String,
    pub ms: u128,
    pub ok: bool,
}

impl HomeDump {
    pub fn time(&mut self, what: &str, started: Instant, ok: bool) {
        self.timings.push(HomeTiming {
            what: what.to_string(),
            ms: started.elapsed().as_millis(),
            ok,
        });
    }
}

/// The local UTC offset as TIDAL's feed takes it (`+02:00`).
fn time_offset() -> String {
    let seconds = jiff::Zoned::now().offset().seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Profiles asked for at once when naming playlist creators.
const USER_NAME_CONCURRENCY: usize = 6;

/// One user id with the lookup's answer: a name, no public profile
/// (`Ok(None)`), or a failure to retry later.
pub type UserNameAnswer = (String, Result<Option<String>>);

/// Runs futures together and returns their outputs in order.
pub(crate) async fn join_all<F: std::future::Future + Unpin>(
    mut futures: Vec<F>,
) -> Vec<F::Output> {
    let mut outputs: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(|context| {
        let mut done = true;
        for (future, output) in futures.iter_mut().zip(outputs.iter_mut()) {
            if output.is_none() {
                match std::pin::Pin::new(future).poll(context) {
                    std::task::Poll::Ready(value) => *output = Some(value),
                    std::task::Poll::Pending => done = false,
                }
            }
        }
        if done {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

/// A TIDAL error body: `{status, subStatus, userMessage}`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ErrorBody {
    sub_status: Option<u32>,
    user_message: Option<String>,
}

impl ErrorBody {
    fn parse(body: &str) -> Self {
        serde_json::from_str(body).unwrap_or_default()
    }

    /// The access token ran out. Never every 401: 4005 is an availability
    /// error that happens to use 401.
    fn token_expired(&self) -> bool {
        matches!(self.sub_status, Some(11002 | 11003))
            || self
                .user_message
                .as_deref()
                .is_some_and(|message| message.starts_with("The token has expired"))
    }

    /// The token is not valid at all (revoked, or for another client).
    fn token_invalid(&self) -> bool {
        matches!(self.sub_status, Some(11001 | 11004 | 11005 | 1001))
    }
}

/// A readable message for a TIDAL error.
fn readable(status: u16, error: &ErrorBody) -> String {
    let known = match (status, error.sub_status) {
        (_, Some(4010)) => Some("You have reached your monthly streaming limit on TIDAL."),
        (_, Some(4032 | 4035)) => {
            Some("TIDAL does not offer this in the country you are connecting from.")
        }
        (_, Some(4033)) => Some("Your TIDAL subscription does not include this."),
        (_, Some(4005)) => Some("This track is not available for playback on TIDAL right now."),
        (404, Some(2001)) => Some("TIDAL has no radio for this."),
        (404, _) => Some("TIDAL could not find this."),
        (412, _) => Some("The playlist changed elsewhere. Try again."),
        (403, _) => Some("TIDAL does not allow this."),
        _ => None,
    };
    if let Some(known) = known {
        return known.to_string();
    }
    match error
        .user_message
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        Some(message) => format!("TIDAL: {message}"),
        None => format!("TIDAL answered HTTP {status}."),
    }
}

fn stream_error(error: manifest::StreamError) -> ApiError {
    match error {
        manifest::StreamError::Malformed(_) | manifest::StreamError::UnknownManifest(_) => {
            ApiError::Decode(error.to_string())
        }
        _ => ApiError::Status {
            status: 403,
            message: error.to_string(),
        },
    }
}

fn auth_error(error: AuthError) -> ApiError {
    match error {
        AuthError::Unreachable(why) => ApiError::Network(why),
        _ => ApiError::SignInExpired {
            api_source: ApiSource::Shared,
        },
    }
}

fn decode<T: DeserializeOwned>(body: &str) -> Result<T> {
    serde_json::from_str(body).map_err(|error| ApiError::Decode(error.to_string()))
}

/// The id of a `tidal:<kind>:<id>` URI of the wanted kind.
fn uri_id<'a>(uri: &'a str, kind: &str) -> Option<&'a str> {
    let mut parts = uri.splitn(3, ':');
    (parts.next() == Some("tidal") && parts.next() == Some(kind))
        .then(|| parts.next())
        .flatten()
        .filter(|id| !id.is_empty())
}

fn backoff(attempt: u32) -> Duration {
    FIRST_BACKOFF * 2u32.pow(attempt.saturating_sub(1))
}

#[derive(Clone)]
struct Call {
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    form: Option<Vec<(String, String)>>,
    /// A JSON:API body (`application/vnd.api+json`).
    json_api: Option<String>,
    if_none_match: Option<String>,
    /// Extra request headers (the web player's `x-tidal-authenticated-*`).
    headers: Vec<(String, String)>,
}

impl Call {
    fn new(method: Method, url: String) -> Self {
        Self {
            method,
            url,
            query: Vec::new(),
            form: None,
            json_api: None,
            if_none_match: None,
            headers: Vec::new(),
        }
    }

    /// Only the demo builds' API probe sends extra headers.
    #[cfg(feature = "demo")]
    fn header(mut self, key: &str, value: impl ToString) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    fn json_api(mut self, body: &Value) -> Self {
        self.json_api = Some(body.to_string());
        self
    }

    fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    fn form(mut self, key: &str, value: impl ToString) -> Self {
        self.form
            .get_or_insert_with(Vec::new)
            .push((key.to_string(), value.to_string()));
        self
    }

    fn etag(mut self, etag: String) -> Self {
        self.if_none_match = Some(etag);
        self
    }

    fn idempotent(&self) -> bool {
        self.method == Method::GET
    }
}

struct Reply {
    /// The HTTP status: 2xx always, unless the call kept errors.
    status: u16,
    etag: Option<String>,
    body: String,
}

/// Who is signed in, from `sessions`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionInfo {
    #[serde(deserialize_with = "convert::id_string")]
    pub session_id: String,
    pub user_id: u64,
    pub country_code: String,
}

/// The account's plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subscription {
    /// "HIFI", "HIFI_PLUS", "PREMIUM", ...
    pub kind: String,
    /// "ACTIVE", ...
    pub status: String,
    pub valid_until: Option<String>,
    /// Stale since the MQA era; never trust it as the quality ceiling.
    pub highest_sound_quality: Option<String>,
}

/// Which of an artist's releases to list.
/// A TIDAL biography without its markup: `[wimpLink artistId="1"]Name[/wimpLink]`
/// becomes `Name`, `<br/>` a line break, other tags go.
/// A user picture's sizes, largest first. TIDAL serves user pictures at
/// 80, 160, 320, 750 and 1080 square (100, 210 and 600 are refused).
pub(crate) fn user_images(uuid: &str) -> Vec<Image> {
    [750u32, 320, 160, 80]
        .iter()
        .map(|&size| Image {
            url: convert::image_url(uuid, &format!("{size}x{size}")),
            width: Some(size),
            height: Some(size),
        })
        .collect()
}

/// The picture a v2 profile names, as the sizes the avatar is drawn from.
fn profile_images(profile: &Value) -> Option<Vec<Image>> {
    ["picture", "profileImage", "image", "imageUrl"]
        .iter()
        .find_map(|key| {
            let value = &profile[*key];
            let found = value
                .as_str()
                .or_else(|| value["url"].as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())?
                .to_string();
            Some(if found.starts_with("http") {
                vec![Image {
                    url: found,
                    width: None,
                    height: None,
                }]
            } else {
                user_images(&found)
            })
        })
}

/// A profile from TIDAL's v2 profile and public-playlists answers, with the
/// account's own name and picture when the profile has none.
/// The display name in a `v2/profiles/{id}` answer; a name that is only
/// the id is no name.
pub(crate) fn parse_user_name(user_id: &str, profile: &Value) -> Option<String> {
    ["name", "displayName"]
        .iter()
        .filter_map(|key| profile[*key].as_str())
        .map(str::trim)
        .find(|name| !name.is_empty() && *name != user_id)
        .map(str::to_string)
}

pub(crate) fn parse_profile(
    user_id: &str,
    profile: &Value,
    playlists: &Value,
    account: Option<User>,
) -> crate::api::models::Profile {
    let text = |value: &Value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let images = profile_images(profile)
        .or_else(|| account.as_ref().map(|user| user.images.clone()))
        .unwrap_or_default();
    let count = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| profile[*key].as_u64())
            .map(|count| count.min(u64::from(u32::MAX)) as u32)
    };
    let playlists = playlists["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let raw = if item.get("playlist").is_some() {
                &item["playlist"]
            } else {
                item
            };
            serde_json::from_value::<convert::RawPlaylist>(raw.clone())
                .ok()
                .map(convert::playlist)
        })
        .filter(|playlist| !playlist.id.is_empty())
        .collect();
    crate::api::models::Profile {
        id: user_id.to_string(),
        name: text(&profile["name"])
            .or_else(|| account.and_then(|user| user.display_name))
            .unwrap_or_default(),
        images,
        followers: count(&["numberOfFollowers", "followers", "followersCount"]),
        following: count(&["numberOfFollows", "following", "followingCount"]),
        playlists,
        // A number on the v2 profile; 0 or missing is no artist.
        artist_id: match &profile["artistId"] {
            Value::Number(id) if id.as_u64() != Some(0) => Some(id.to_string()),
            Value::String(id) if !id.is_empty() && id != "0" => Some(id.clone()),
            _ => None,
        },
    }
}

/// `data.attributes.numberOfFollowers` of an OpenAPI artist answer.
pub(crate) fn followers_of(document: &Value) -> Option<u64> {
    document["data"]["attributes"]["numberOfFollowers"].as_u64()
}

pub(crate) fn plain_bio(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(['[', '<']) {
        out.push_str(&rest[..start]);
        let close = if rest[start..].starts_with('[') {
            ']'
        } else {
            '>'
        };
        match rest[start..].find(close) {
            Some(end) => {
                let tag = &rest[start + 1..start + end];
                if tag.eq_ignore_ascii_case("br") || tag.eq_ignore_ascii_case("br/") {
                    out.push('\n');
                }
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod plain_bio_tests {
    #[test]
    fn links_keep_their_names_and_breaks_become_lines() {
        assert_eq!(
            super::plain_bio(
                "Born in Detroit, [wimpLink artistId=\"123\"]Some Artist[/wimpLink] began.<br/>Later work."
            ),
            "Born in Detroit, Some Artist began.
Later work."
        );
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn user_pictures_come_in_the_sizes_tidal_serves() {
        let urls: Vec<String> = user_images("aa-bb")
            .into_iter()
            .map(|image| image.url)
            .collect();
        assert!(urls[0].ends_with("/aa/bb/750x750.jpg"));
        assert!(
            urls.iter()
                .all(|url| !url.contains("600x600") && !url.contains("210x210"))
        );
    }

    #[test]
    fn a_profile_reads_counts_and_public_playlists_and_falls_back_to_the_account() {
        let profile =
            serde_json::json!({"name": "", "numberOfFollowers": 12, "numberOfFollows": 3});
        let playlists = serde_json::json!({"items": [
            {"playlist": {"uuid": "p-1", "title": "Mine", "squareImage": "cc-dd"}},
            {"playlist": {"uuid": "", "title": "Broken"}}
        ]});
        let account = User {
            id: "7".into(),
            display_name: Some("Listener".into()),
            images: user_images("ee-ff"),
            ..Default::default()
        };
        let parsed = parse_profile("7", &profile, &playlists, Some(account));
        assert_eq!(parsed.name, "Listener");
        assert_eq!(parsed.followers, Some(12));
        assert_eq!(parsed.following, Some(3));
        assert_eq!(parsed.playlists.len(), 1);
        assert_eq!(parsed.playlists[0].name, "Mine");
        assert!(parsed.images[0].url.contains("/ee/ff/"));

        let profile =
            serde_json::json!({"name": "Someone", "picture": "https://example.com/p.jpg"});
        let parsed = parse_profile("8", &profile, &serde_json::Value::Null, None);
        assert_eq!(parsed.name, "Someone");
        assert_eq!(parsed.images[0].url, "https://example.com/p.jpg");
        assert_eq!(parsed.followers, None);
        assert_eq!(parsed.artist_id, None);
    }

    /// The v2 profile names the artist the account uploads as (shape seen
    /// live on the account's own profile, 2026-10-19).
    #[test]
    fn a_profile_names_the_accounts_artist() {
        let profile = serde_json::json!({
            "artistId": 12345678, "handle": null, "name": "Example Artist",
            "numberOfFollowers": 0, "numberOfFollows": 0,
            "picture": {"url": "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"},
            "profileType": "MY_PROFILE", "userId": 87654321
        });
        let parsed = parse_profile("87654321", &profile, &serde_json::Value::Null, None);
        assert_eq!(parsed.artist_id.as_deref(), Some("12345678"));
        for none in [
            serde_json::json!(0),
            serde_json::json!(null),
            serde_json::json!(""),
        ] {
            let profile = serde_json::json!({"name": "A", "artistId": none});
            let parsed = parse_profile("1", &profile, &serde_json::Value::Null, None);
            assert_eq!(parsed.artist_id, None);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseGroup {
    Albums,
    EpsAndSingles,
    /// "Appears on".
    Compilations,
}

impl ReleaseGroup {
    fn filter(self) -> &'static str {
        match self {
            Self::Albums => "ALBUMS",
            Self::EpsAndSingles => "EPSANDSINGLES",
            Self::Compilations => "COMPILATIONS",
        }
    }

    /// The modules of TIDAL's artist page (`v2/artist/{id}`) that list the
    /// group, in the page's order, each with the `album_type` its cards are
    /// given (the interface's Compilations and Live albums shelves read the
    /// Albums list by it). TIDAL's lists hold the versions its page shows
    /// (for J. Cole the explicit one of each, both Come Up Mixtapes),
    /// which `artists/{id}/albums` does not: that lists every version,
    /// clean and explicit, in no stable order.
    fn modules(self) -> &'static [(&'static str, Option<&'static str>)] {
        match self {
            Self::Albums => &[
                ("ARTIST_ALBUMS", None),
                ("ARTIST_COMPILATIONS", Some("compilation")),
                ("ARTIST_LIVE_ALBUMS", Some("live")),
            ],
            Self::EpsAndSingles => &[("ARTIST_TOP_SINGLES", None)],
            Self::Compilations => &[("ARTIST_APPEARS_ON", None)],
        }
    }

    /// The `album_group` the interface groups by.
    fn group(self) -> &'static str {
        match self {
            Self::Albums => "album",
            Self::EpsAndSingles => "single",
            Self::Compilations => "appears_on",
        }
    }

    /// The groups an `include_groups` list names, in TIDAL's order.
    pub fn parse(include_groups: &str) -> Vec<Self> {
        let names: Vec<&str> = include_groups.split(',').map(str::trim).collect();
        let mut groups = Vec::new();
        if names.iter().any(|name| name.is_empty() || *name == "album") {
            groups.push(Self::Albums);
        }
        if names.contains(&"single") {
            groups.push(Self::EpsAndSingles);
        }
        if names
            .iter()
            .any(|name| matches!(*name, "compilation" | "appears_on"))
        {
            groups.push(Self::Compilations);
        }
        groups
    }
}

/// One release of each title (ignoring case), the explicit version where
/// the list has one, at the place of the title's first version.
fn one_version_each(albums: Vec<Album>) -> Vec<Album> {
    let mut kept: Vec<Album> = Vec::with_capacity(albums.len());
    let mut at = std::collections::HashMap::new();
    for album in albums {
        let title = album.name.to_lowercase();
        match at.get(&title) {
            Some(&index) => {
                let first: &mut Album = &mut kept[index];
                if album.explicit && !first.explicit {
                    *first = album;
                }
            }
            None => {
                at.insert(title, kept.len());
                kept.push(album);
            }
        }
    }
    kept
}

pub struct TidalClient {
    http: Http,
    tokens: Mutex<Option<Arc<TidalTokens>>>,
    api_base: String,
    v2_base: String,
    openapi_base: String,
    limiter: Semaphore,
    cooldown_until: tokio::sync::Mutex<Instant>,
    activity: Arc<NetActivity>,
    /// Set once TIDAL answered a Max request with a lossy stream: this
    /// sign-in's client cannot have Max, so Lossless is asked from then on.
    max_refused: std::sync::atomic::AtomicBool,
    /// Set once `trackManifests` answered with DRM: this sign-in's client
    /// (the PKCE one) gets Widevine there, so `playbackinfopostpaywall`
    /// is asked directly from then on.
    manifests_protected: std::sync::atomic::AtomicBool,
    /// Playlists read through openapi v2 because v1 fails on their videos.
    v2_playlists: Mutex<playlist_v2::V2Playlists>,
}

impl TidalClient {
    pub fn new(http: impl Into<Http>, activity: Arc<NetActivity>) -> Self {
        Self::with_bases(http, activity, API_BASE, V2_BASE, OPENAPI_BASE)
    }

    /// A client against other hosts (tests).
    pub fn with_bases(
        http: impl Into<Http>,
        activity: Arc<NetActivity>,
        api_base: &str,
        v2_base: &str,
        openapi_base: &str,
    ) -> Self {
        Self {
            http: http.into(),
            tokens: Mutex::new(None),
            api_base: api_base.trim_end_matches('/').to_string(),
            v2_base: v2_base.trim_end_matches('/').to_string(),
            openapi_base: openapi_base.trim_end_matches('/').to_string(),
            limiter: Semaphore::new(MAX_IN_FLIGHT),
            cooldown_until: tokio::sync::Mutex::new(Instant::now()),
            activity,
            max_refused: std::sync::atomic::AtomicBool::new(false),
            manifests_protected: std::sync::atomic::AtomicBool::new(false),
            v2_playlists: Mutex::new(playlist_v2::V2Playlists::default()),
        }
    }

    /// A fresh client for a new sign-in: an old request must never pick up
    /// the new account's token, nor inherit the old cooldown.
    pub fn for_tokens(&self, tokens: Arc<TidalTokens>) -> Self {
        let client = Self::with_bases(
            self.http.clone(),
            self.activity.clone(),
            &self.api_base,
            &self.v2_base,
            &self.openapi_base,
        );
        client.set_tokens(Some(tokens));
        client
    }

    pub fn set_tokens(&self, tokens: Option<Arc<TidalTokens>>) {
        self.max_refused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.manifests_protected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        *self.tokens.lock().unwrap_or_else(|lock| lock.into_inner()) = tokens;
    }

    pub fn tokens(&self) -> Result<Arc<TidalTokens>> {
        self.tokens
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone()
            .ok_or(ApiError::NotSignedIn)
    }

    pub fn user_id(&self) -> Result<u64> {
        Ok(self.tokens()?.user_id())
    }

    fn v1(&self, path: &str) -> String {
        format!("{}/{}", self.api_base, path.trim_start_matches('/'))
    }

    fn v2(&self, path: &str) -> String {
        format!("{}/{}", self.v2_base, path.trim_start_matches('/'))
    }

    fn user_path(&self, rest: &str) -> Result<String> {
        Ok(self.v1(&format!("users/{}/{rest}", self.user_id()?)))
    }

    async fn wait_cooldown(&self) {
        let until = *self.cooldown_until.lock().await;
        let now = Instant::now();
        if until > now {
            tokio::time::sleep(until - now).await;
        }
    }

    async fn cool_down(&self, wait: Duration) {
        let mut until = self.cooldown_until.lock().await;
        *until = (*until).max(Instant::now() + wait);
    }

    async fn send(&self, call: Call) -> Result<Reply> {
        self.send_keeping(call, false).await
    }

    /// [`Self::send`]; with `keep_errors`, TIDAL's own error answer (status
    /// and body as sent) comes back as a reply instead of an error. Expired
    /// tokens are still refreshed, and rate limits still waited out.
    async fn send_keeping(&self, call: Call, keep_errors: bool) -> Result<Reply> {
        let tokens = self.tokens()?;
        let http = self.http.client().map_err(ApiError::Network)?;
        let _activity = ActivityGuard::begin(&self.activity);
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            self.wait_cooldown().await;
            let _permit = self
                .limiter
                .acquire()
                .await
                .map_err(|_| ApiError::NotSignedIn)?;
            let token = tokens.access_token(None).await.map_err(auth_error)?;
            let country = tokens.country_code();
            let mut request = http
                .request(call.method.clone(), &call.url)
                .bearer_auth(&token)
                .header("x-tidal-client-version", CLIENT_VERSION);
            if !country.is_empty() {
                request = request.query(&[("countryCode", country.as_str())]);
            }
            request = request.query(&call.query);
            if let Some(form) = &call.form {
                request = request.form(form);
            }
            if let Some(body) = &call.json_api {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/vnd.api+json")
                    .body(body.clone());
            }
            if let Some(etag) = &call.if_none_match {
                request = request.header(reqwest::header::IF_NONE_MATCH, etag);
            }
            for (key, value) in &call.headers {
                request = request.header(key.as_str(), value.as_str());
            }
            let started = Instant::now();
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if call.idempotent() && attempt < RETRIES => {
                    attempt += 1;
                    log::debug!("TIDAL request failed, retrying: {}", error.without_url());
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                Err(error) => return Err(ApiError::Network(error.without_url().to_string())),
            };
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let body = match response.text().await {
                Ok(body) => body,
                // Cut off on the way (the network went): no answer, as if
                // none had come, never an empty one to misread.
                Err(error) if call.idempotent() && attempt < RETRIES => {
                    attempt += 1;
                    log::debug!("TIDAL answer cut off, retrying: {}", error.without_url());
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                Err(error) => return Err(ApiError::Network(error.without_url().to_string())),
            };
            let path = call.url.split('?').next().unwrap_or_default();
            // Path, status, time and size only: never the query or headers.
            log::debug!(
                "TIDAL {} {path} {status} {}ms {}B",
                call.method,
                started.elapsed().as_millis(),
                body.len()
            );
            match status {
                200..=299 => {
                    return Ok(Reply {
                        status,
                        etag: headers
                            .get(reqwest::header::ETAG)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        body,
                    });
                }
                401 => {
                    let error = ErrorBody::parse(&body);
                    if error.token_expired() || error.token_invalid() {
                        if refreshed {
                            return Err(ApiError::SignInExpired {
                                api_source: ApiSource::Shared,
                            });
                        }
                        refreshed = true;
                        log::debug!("TIDAL token rejected for {path}; refreshing once");
                        tokens
                            .access_token(Some(&token))
                            .await
                            .map_err(auth_error)?;
                        continue;
                    }
                    if keep_errors {
                        return Ok(Reply {
                            status,
                            etag: None,
                            body,
                        });
                    }
                    return Err(ApiError::Status {
                        status,
                        message: readable(status, &error),
                    });
                }
                429 => {
                    if attempt >= RETRIES {
                        return Err(ApiError::RateLimited);
                    }
                    attempt += 1;
                    let wait = headers
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.trim().parse::<u64>().ok())
                        .map(Duration::from_secs)
                        .unwrap_or_else(|| backoff(attempt))
                        .min(MAX_RETRY_AFTER);
                    log::debug!("TIDAL rate limited {path}; waiting {wait:?}");
                    self.cool_down(wait).await;
                    continue;
                }
                500..=599 if call.idempotent() && attempt < RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                _ => {
                    let error = ErrorBody::parse(&body);
                    log::debug!(
                        "TIDAL {path} answered {status} subStatus={:?}",
                        error.sub_status
                    );
                    if keep_errors {
                        return Ok(Reply {
                            status,
                            etag: None,
                            body,
                        });
                    }
                    return Err(ApiError::Status {
                        status,
                        message: readable(status, &error),
                    });
                }
            }
        }
    }

    /// GET and decode, retrying once over a truncated or garbled 200.
    async fn get<T: DeserializeOwned>(&self, call: Call) -> Result<T> {
        let retry = call.clone();
        let reply = self.send(call).await?;
        match decode(&reply.body) {
            Ok(value) => Ok(value),
            Err(_) => decode(&self.send(retry).await?.body),
        }
    }

    /// A GET whose 404 means "none".
    async fn get_optional<T: DeserializeOwned>(&self, call: Call) -> Result<Option<T>> {
        match self.get(call).await {
            Ok(value) => Ok(Some(value)),
            Err(ApiError::Status { status: 404, .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    // ---- identity ------------------------------------------------------------

    /// Who the token belongs to and their store country.
    pub async fn session(&self) -> Result<SessionInfo> {
        self.get(Call::new(Method::GET, self.v1("sessions"))).await
    }

    pub async fn subscription(&self) -> Result<Subscription> {
        let value: Value = self
            .get(Call::new(Method::GET, self.user_path("subscription")?))
            .await?;
        let text = |value: &Value| value.as_str().map(str::to_string);
        Ok(Subscription {
            kind: value["subscription"]["type"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            status: value["status"].as_str().unwrap_or_default().to_string(),
            valid_until: text(&value["validUntil"]),
            highest_sound_quality: text(&value["highestSoundQuality"]),
        })
    }

    /// The signed-in user. `product` is the subscription type, lowercased
    /// ("hifi", "hifi_plus"), or `None` when it could not be read.
    pub async fn me(&self) -> Result<User> {
        let user_id = self.user_id()?;
        let (value, subscription) = tokio::join!(
            self.get::<Value>(Call::new(Method::GET, self.v1(&format!("users/{user_id}")))),
            self.subscription(),
        );
        let value = value?;
        let text = |key: &str| {
            value[key]
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string)
        };
        let full_name = match (text("firstName"), text("lastName")) {
            (Some(first), Some(last)) => Some(format!("{first} {last}")),
            (first, last) => first.or(last),
        };
        // Many accounts carry the picture only on the v2 profile (what TIDAL's
        // own avatar reads), not on `users/{id}`.
        let mut images = text("picture")
            .map(|uuid| user_images(&uuid))
            .unwrap_or_default();
        // The profile also names accounts without a first or last name. The
        // account's `username` is the e-mail address and is never a name.
        let full_name = full_name.filter(|name| crate::api::models::public_name(name).is_some());
        let mut profile_name = None;
        if (images.is_empty() || full_name.is_none())
            && let Ok(Some(profile)) = self
                .get_optional::<Value>(Call::new(
                    Method::GET,
                    self.v2(&format!("profiles/{user_id}")),
                ))
                .await
        {
            if images.is_empty() {
                images = profile_images(&profile).unwrap_or_default();
            }
            profile_name = parse_user_name(&user_id.to_string(), &profile)
                .filter(|name| crate::api::models::public_name(name).is_some());
        }
        Ok(User {
            id: user_id.to_string(),
            display_name: full_name.or(profile_name),
            images,
            product: subscription
                .ok()
                .map(|subscription| subscription.kind.to_ascii_lowercase())
                .filter(|kind| !kind.is_empty()),
            country: text("countryCode").or_else(|| {
                self.tokens()
                    .ok()
                    .map(|tokens| tokens.country_code())
                    .filter(|code| !code.is_empty())
            }),
            uri: Some(convert::uri("user", &user_id.to_string())),
        })
    }

    /// A user's profile (`v2/profiles/{id}`) with their public playlists
    /// (`v2/user-playlists/{id}/public`). The profile's name and picture
    /// fall back to the account's own (`users/{id}`) for the signed-in
    /// user; a missing profile is no failure when the playlists answer.
    pub async fn profile(&self, user_id: &str) -> Result<crate::api::models::Profile> {
        let own = self.user_id().ok().map(|id| id.to_string()) == Some(user_id.to_string());
        let (profile, playlists, account) = tokio::join!(
            self.get_optional::<Value>(Call::new(
                Method::GET,
                self.v2(&format!("profiles/{user_id}"))
            )),
            self.get::<Value>(
                Call::new(
                    Method::GET,
                    self.v2(&format!("user-playlists/{user_id}/public"))
                )
                .query("limit", 50)
            ),
            async { if own { self.me().await.ok() } else { None } },
        );
        let profile = profile.ok().flatten().unwrap_or(Value::Null);
        let playlists = match playlists {
            Ok(value) => value,
            Err(error) if profile.is_null() && account.is_none() => return Err(error),
            Err(_) => Value::Null,
        };
        Ok(parse_profile(user_id, &profile, &playlists, account))
    }

    /// The name behind a user id (`v2/profiles/{id}`), for a playlist's
    /// "Created by". `Ok(None)` is TIDAL saying there is no public profile
    /// (404 deleted, 403 private); any other failure is an error, so the
    /// caller asks again later instead of remembering a guess.
    pub async fn user_name(&self, user_id: &str) -> Result<Option<String>> {
        let found = self
            .get::<Value>(Call::new(
                Method::GET,
                self.v2(&format!("profiles/{user_id}")),
            ))
            .await;
        match found {
            Ok(profile) => Ok(parse_user_name(user_id, &profile)),
            Err(ApiError::Status {
                status: 400 | 403 | 404,
                ..
            }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Names for a batch of user ids, a few profiles at a time so a long
    /// library does not open dozens of connections at once. Each id has its
    /// own answer: one failing lookup does not take the others down.
    pub async fn user_names(&self, ids: &[String]) -> Result<Vec<UserNameAnswer>> {
        let mut answers = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(USER_NAME_CONCURRENCY) {
            let lookups: Vec<_> = chunk
                .iter()
                .map(|id| Box::pin(async move { (id.clone(), self.user_name(id).await) }))
                .collect();
            answers.extend(join_all(lookups).await);
        }
        Ok(answers)
    }

    /// TIDAL's Feed: new releases by followed artists and new monthly
    /// mixes (`v2/feed/activities`, as the web player reads it).
    pub async fn feed(&self) -> Result<Feed> {
        let value: Value = self
            .get(
                Call::new(Method::GET, self.v2("feed/activities"))
                    .query("userId", self.user_id()?)
                    .query("deviceType", "BROWSER")
                    .query("locale", LOCALE),
            )
            .await?;
        Ok(feed::parse(&value))
    }

    // ---- favourites ----------------------------------------------------------

    async fn favourites<R: DeserializeOwned + Default>(
        &self,
        kind: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RawPage<Wrapped<R>>> {
        self.get(
            Call::new(Method::GET, self.user_path(&format!("favorites/{kind}"))?)
                .query("limit", limit)
                .query("offset", offset)
                .query("order", "DATE")
                .query("orderDirection", "DESC"),
        )
        .await
    }

    /// Favourite tracks, newest first. Unavailable tracks stay, marked
    /// unplayable, so positions hold.
    pub async fn saved_tracks(&self, offset: u32, limit: u32) -> Result<Page<SavedTrack>> {
        let page = self.favourites::<RawTrack>("tracks", offset, limit).await?;
        Ok(saved_tracks_page(page, limit))
    }

    pub async fn saved_albums(&self, offset: u32, limit: u32) -> Result<Page<SavedAlbum>> {
        let page = self.favourites::<RawAlbum>("albums", offset, limit).await?;
        Ok(page
            .map(limit, |wrapped| SavedAlbum {
                added_at: wrapped.created,
                album: wrapped.item.map(convert::album).unwrap_or_default(),
            })
            .retain_items(|saved| !saved.album.id.is_empty()))
    }

    pub async fn saved_artists(&self, offset: u32, limit: u32) -> Result<Page<Artist>> {
        let page = self
            .favourites::<RawArtist>("artists", offset, limit)
            .await?;
        Ok(page
            .map(limit, |wrapped| {
                wrapped.item.map(convert::artist).unwrap_or_default()
            })
            .retain_items(|artist| !artist.id.is_empty()))
    }

    /// Favourite videos, newest first.
    pub async fn saved_videos(&self, offset: u32, limit: u32) -> Result<Page<Video>> {
        let page = self
            .favourites::<convert::RawVideo>("videos", offset, limit)
            .await?;
        Ok(page
            .map(limit, |wrapped| {
                wrapped.item.map(convert::video).unwrap_or_default()
            })
            .retain_items(|video| !video.id.is_empty()))
    }

    /// Favourite artists as the cursor page the backend reads;
    /// the cursor is an offset.
    pub async fn followed_artists(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<CursorPage<Artist>> {
        let offset = after.and_then(|after| after.parse().ok()).unwrap_or(0);
        let page = self.saved_artists(offset, limit).await?;
        let next = page.next_offset();
        Ok(CursorPage {
            items: page.items,
            total: Some(page.total),
            next: next.map(|next| next.to_string()),
            cursors: Some(Cursors {
                after: next.map(|next| next.to_string()),
                before: None,
            }),
        })
    }

    /// Favourite mixes (v2), newest first.
    pub async fn favorite_mixes(&self, offset: u32, limit: u32) -> Result<Page<Mix>> {
        let page: RawPage<RawMix> = self
            .get(
                Call::new(Method::GET, self.v2("favorites/mixes"))
                    .query("limit", limit.min(50))
                    .query("offset", offset)
                    .query("order", "DATE")
                    .query("orderDirection", "DESC"),
            )
            .await?;
        Ok(page.map(limit.min(50), convert::mix))
    }

    async fn favourite_ids(&self) -> Result<FavouriteIds> {
        self.get(Call::new(Method::GET, self.user_path("favorites/ids")?))
            .await
    }

    /// Saves tracks, albums, artists, playlists and mixes, by URI.
    pub async fn save(&self, uris: &[String]) -> Result<()> {
        for (kind, plural, singular) in [
            ("track", "trackIds", "trackId"),
            ("album", "albumIds", "albumId"),
            ("artist", "artistIds", "artistId"),
            // Live 2026-10-18: `videoIds` answers 200 and the video is in
            // `favorites/videos` and `favorites/ids` straight away.
            ("video", "videoIds", "videoId"),
        ] {
            let ids: Vec<&str> = uris.iter().filter_map(|uri| uri_id(uri, kind)).collect();
            if ids.is_empty() {
                continue;
            }
            let url = self.user_path(&format!("favorites/{kind}s"))?;
            let joined = ids.join(",");
            let sent = self
                .send(Call::new(Method::POST, url.clone()).form(plural, &joined))
                .await;
            if let Err(ApiError::Status { status: 400, .. }) = sent {
                // Older deployments take the singular name.
                self.send(Call::new(Method::POST, url).form(singular, &joined))
                    .await?;
            } else {
                sent?;
            }
        }
        for id in uris.iter().filter_map(|uri| uri_id(uri, "playlist")) {
            self.follow_playlist(id).await?;
        }
        let mixes: Vec<&str> = uris.iter().filter_map(|uri| uri_id(uri, "mix")).collect();
        if !mixes.is_empty() {
            self.send(
                Call::new(Method::PUT, self.v2("favorites/mixes/add"))
                    .query("mixIds", mixes.join(","))
                    .query("onArtifactNotFound", "FAIL"),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn unsave(&self, uris: &[String]) -> Result<()> {
        for kind in ["track", "album", "artist", "video"] {
            for id in uris.iter().filter_map(|uri| uri_id(uri, kind)) {
                let url = self.user_path(&format!("favorites/{kind}s/{id}"))?;
                match self.send(Call::new(Method::DELETE, url)).await {
                    // Already gone is what was asked for.
                    Ok(_) | Err(ApiError::Status { status: 404, .. }) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        for id in uris.iter().filter_map(|uri| uri_id(uri, "playlist")) {
            self.unfollow_playlist(id).await?;
        }
        let mixes: Vec<&str> = uris.iter().filter_map(|uri| uri_id(uri, "mix")).collect();
        if !mixes.is_empty() {
            self.send(
                Call::new(Method::PUT, self.v2("favorites/mixes/remove"))
                    .query("mixIds", mixes.join(",")),
            )
            .await?;
        }
        Ok(())
    }

    /// "Don't play this" (TIDAL's block list, `users/{id}/blocks/tracks`):
    /// the song is left out of mixes and radios from now on.
    pub async fn block_track(&self, uri: &str) -> Result<()> {
        let Some(id) = uri_id(uri, "track") else {
            return Ok(());
        };
        // Live 2026-10-17: the singular `trackId` is the field; `trackIds`
        // answers 404.
        self.send(Call::new(Method::POST, self.user_path("blocks/tracks")?).form("trackId", id))
            .await?;
        Ok(())
    }

    /// What is on TIDAL's block list of one kind (`artists` or `tracks`, at
    /// `users/{id}/blocks/<kind>`), all pages: the `item` of each entry.
    async fn blocked_items(&self, kind: &str) -> Result<Vec<serde_json::Value>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            items: Vec<Entry>,
            #[serde(default, rename = "totalNumberOfItems")]
            total: usize,
        }
        #[derive(Deserialize)]
        struct Entry {
            item: serde_json::Value,
        }
        const PAGE: usize = 100;
        let mut items = Vec::new();
        loop {
            let page: Page = self
                .get(
                    Call::new(Method::GET, self.user_path(&format!("blocks/{kind}"))?)
                        .query("limit", PAGE.to_string())
                        .query("offset", items.len().to_string()),
                )
                .await?;
            let got = page.items.len();
            items.extend(page.items.into_iter().map(|entry| entry.item));
            if got == 0 || items.len() >= page.total || items.len() >= 5000 {
                return Ok(items);
            }
        }
    }

    /// The artists on TIDAL's block list.
    pub async fn blocked_artists(&self) -> Result<Vec<BlockedArtist>> {
        Ok(self
            .blocked_items("artists")
            .await?
            .iter()
            .filter_map(|item| {
                Some(BlockedArtist {
                    id: json_id(&item["id"])?,
                    name: item["name"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect())
    }

    /// The ids of the artists on TIDAL's block list.
    pub async fn blocked_artist_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .blocked_artists()
            .await?
            .into_iter()
            .map(|artist| artist.id)
            .collect())
    }

    /// The songs on TIDAL's block list.
    pub async fn blocked_tracks(&self) -> Result<Vec<BlockedTrack>> {
        Ok(self
            .blocked_items("tracks")
            .await?
            .iter()
            .filter_map(|item| {
                let artist = item["artist"]["name"]
                    .as_str()
                    .or_else(|| item["artists"][0]["name"].as_str())
                    .unwrap_or_default();
                Some(BlockedTrack {
                    id: json_id(&item["id"])?,
                    title: item["title"].as_str().unwrap_or_default().to_string(),
                    artist: artist.to_string(),
                    hidden: false,
                })
            })
            .collect())
    }

    /// Lifts the block on a song: `DELETE users/{id}/blocks/tracks/{id}`.
    pub async fn unblock_track(&self, uri: &str) -> Result<()> {
        let Some(id) = uri_id(uri, "track") else {
            return Ok(());
        };
        match self
            .send(Call::new(
                Method::DELETE,
                self.user_path(&format!("blocks/tracks/{id}"))?,
            ))
            .await
        {
            // Not blocked any more is what was asked for.
            Ok(_) | Err(ApiError::Status { status: 404, .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Blocks an artist (TIDAL leaves them out of mixes and radios) or lifts
    /// the block: `POST users/{id}/blocks/artists` form `artistId`, and
    /// `DELETE users/{id}/blocks/artists/{id}`.
    pub async fn set_artist_blocked(&self, uri: &str, blocked: bool) -> Result<()> {
        let Some(id) = uri_id(uri, "artist") else {
            return Ok(());
        };
        if blocked {
            self.send(
                Call::new(Method::POST, self.user_path("blocks/artists")?).form("artistId", id),
            )
            .await?;
        } else {
            match self
                .send(Call::new(
                    Method::DELETE,
                    self.user_path(&format!("blocks/artists/{id}"))?,
                ))
                .await
            {
                // Not blocked any more is what was asked for.
                Ok(_) | Err(ApiError::Status { status: 404, .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Whether each URI is a favourite, in the order of `uris`.
    pub async fn contains(&self, uris: &[String]) -> Result<Vec<bool>> {
        let ids = self.favourite_ids().await?;
        let mut mixes = HashSet::new();
        if uris.iter().any(|uri| uri_id(uri, "mix").is_some()) {
            let mut offset = 0;
            loop {
                let page = self.favorite_mixes(offset, 50).await?;
                mixes.extend(page.items.iter().map(|mix| mix.id.clone()));
                match page.next_offset() {
                    Some(next) if next < 1000 => offset = next,
                    _ => break,
                }
            }
        }
        Ok(uris.iter().map(|uri| ids.contains(uri, &mixes)).collect())
    }

    // ---- playlists -------------------------------------------------------------

    /// The account's own and followed playlists.
    pub async fn my_playlists(&self, offset: u32, limit: u32) -> Result<Page<Playlist>> {
        let limit = limit.min(50);
        let combined: Result<RawPage<CombinedPlaylist>> = self
            .get(
                Call::new(
                    Method::GET,
                    self.user_path("playlistsAndFavoritePlaylists")?,
                )
                .query("limit", limit)
                .query("offset", offset)
                .query("order", "DATE")
                .query("orderDirection", "DESC"),
            )
            .await;
        match combined {
            Ok(page) => Ok(page
                .map(limit, |entry| {
                    entry.playlist.map(convert::playlist).unwrap_or_default()
                })
                .retain_items(|playlist| !playlist.id.is_empty())),
            Err(ApiError::Status {
                status: 400 | 404, ..
            }) => self.own_playlists(offset, limit).await,
            Err(error) => Err(error),
        }
    }

    /// Only the playlists the account created.
    pub async fn own_playlists(&self, offset: u32, limit: u32) -> Result<Page<Playlist>> {
        let page: RawPage<RawPlaylist> = self
            .get(
                Call::new(Method::GET, self.user_path("playlists")?)
                    .query("limit", limit.min(50))
                    .query("offset", offset),
            )
            .await?;
        Ok(page.map(limit.min(50), convert::playlist))
    }

    pub async fn playlist(&self, id: &str) -> Result<Playlist> {
        let raw: RawPlaylist = self
            .get(Call::new(Method::GET, self.v1(&format!("playlists/{id}"))))
            .await?;
        Ok(convert::playlist(raw))
    }

    async fn playlist_items_raw(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(RawPage<Wrapped<RawTrack>>, Option<String>)> {
        let limit = limit.min(ITEMS_LIMIT);
        if self.playlist_is_v1_broken(id) {
            let body = self.playlist_items_via_v2(id, offset, limit).await?;
            return Ok((decode(&body)?, None));
        }
        let reply = self
            .send(
                Call::new(Method::GET, self.v1(&format!("playlists/{id}/items")))
                    .query("limit", limit)
                    .query("offset", offset)
                    .query("order", "INDEX")
                    .query("orderDirection", "ASC"),
            )
            .await;
        match reply {
            Ok(reply) => Ok((decode(&reply.body)?, reply.etag)),
            // Live 2026-10-18: v1 answers 500 for a page holding a video.
            Err(error @ ApiError::Status { status, .. }) if status >= 500 => {
                match self.playlist_items_via_v2(id, offset, limit).await {
                    Ok(body) => Ok((decode(&body)?, None)),
                    Err(_) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// A playlist's entries in order. A video keeps its slot as an empty
    /// entry: edits address items by position.
    pub async fn playlist_items(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<PlaylistItem>> {
        let (page, _) = self.playlist_items_raw(id, offset, limit).await?;
        Ok(playlist_items_page(page, limit.min(ITEMS_LIMIT)))
    }

    /// The songs and videos of a playlist page (what the engine plays).
    pub async fn playlist_tracks(&self, id: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        let (page, _) = self.playlist_items_raw(id, offset, limit).await?;
        Ok(playlist_tracks_page(page, limit.min(ITEMS_LIMIT)))
    }

    /// Requested songs already present in a playlist.
    pub async fn playlist_duplicates(&self, id: &str, uris: &[String]) -> Result<Vec<String>> {
        let wanted: HashSet<&str> = uris.iter().map(String::as_str).collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut found = HashSet::new();
        let mut offset = 0;
        loop {
            let page = self.playlist_items(id, offset, ITEMS_LIMIT).await?;
            for uri in page
                .items
                .iter()
                .filter_map(PlaylistItem::playable)
                .map(PlayableItem::uri)
            {
                if wanted.contains(uri) {
                    found.insert(uri.to_string());
                }
            }
            if found.len() == wanted.len() {
                break;
            }
            let Some(next) = page.next_offset() else {
                break;
            };
            offset = next;
        }
        Ok(uris
            .iter()
            .filter(|uri| found.contains(uri.as_str()))
            .cloned()
            .collect())
    }

    /// The playlist's current ETag and length, for an item edit.
    async fn playlist_version(&self, id: &str) -> Result<(String, u32)> {
        self.forget_playlist_read(id);
        if !self.playlist_is_v1_broken(id) {
            let reply = self
                .send(
                    Call::new(Method::GET, self.v1(&format!("playlists/{id}/items")))
                        .query("limit", 1)
                        .query("offset", 0)
                        .query("order", "INDEX")
                        .query("orderDirection", "ASC"),
                )
                .await;
            match reply {
                Ok(reply) => {
                    let page: RawPage<Wrapped<RawTrack>> = decode(&reply.body)?;
                    let etag = reply.etag.ok_or_else(|| {
                        ApiError::Decode("TIDAL sent no ETag for the playlist".into())
                    })?;
                    return Ok((etag, page.total_number_of_items.unwrap_or(0)));
                }
                Err(ApiError::Status { status, .. }) if status >= 500 => {}
                Err(error) => return Err(error),
            }
        }
        // v1 cannot list this playlist (a video is in it; live 2026-10-18):
        // the playlist itself carries the ETag and the length.
        let reply = self
            .send(Call::new(Method::GET, self.v1(&format!("playlists/{id}"))))
            .await?;
        let etag = reply
            .etag
            .clone()
            .ok_or_else(|| ApiError::Decode("TIDAL sent no ETag for the playlist".into()))?;
        let playlist: RawPlaylist = decode(&reply.body)?;
        let total = playlist.number_of_tracks.unwrap_or(0) + playlist.number_of_videos.unwrap_or(0);
        Ok((etag, total))
    }

    /// Sends an item edit with a fresh `If-None-Match`, retrying once when
    /// the playlist changed in between (412).
    async fn playlist_edit(&self, id: &str, make: impl Fn(String, u32) -> Call) -> Result<Reply> {
        let (etag, total) = self.playlist_version(id).await?;
        let reply = match self.send(make(etag, total)).await {
            Err(ApiError::Status { status: 412, .. }) => {
                let (etag, total) = self.playlist_version(id).await?;
                self.send(make(etag, total)).await
            }
            other => other,
        };
        self.forget_playlist_read(id);
        reply
    }

    pub async fn create_playlist(
        &self,
        name: &str,
        public: bool,
        description: &str,
    ) -> Result<Playlist> {
        let reply = self
            .send(
                Call::new(
                    Method::PUT,
                    self.v2("my-collection/playlists/folders/create-playlist"),
                )
                .query("name", name)
                .query("description", description)
                .query("folderId", "root")
                .query("isPublic", public),
            )
            .await?;
        let created: CreatedPlaylist = decode(&reply.body)?;
        let mut playlist = convert::playlist(created.data);
        if playlist.owner.id.is_none() {
            let user = self.user_id()?.to_string();
            playlist.owner.uri = Some(convert::uri("user", &user));
            playlist.owner.id = Some(user);
        }
        Ok(playlist)
    }

    /// Renames, redescribes, or publishes a playlist; `None` keeps a field.
    pub async fn update_playlist(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        public: Option<bool>,
    ) -> Result<()> {
        if name.is_some() || description.is_some() {
            // TIDAL takes both fields together.
            let current = if name.is_none() || description.is_none() {
                Some(self.playlist(id).await?)
            } else {
                None
            };
            let title = name
                .map(str::to_string)
                .or_else(|| current.as_ref().map(|playlist| playlist.name.clone()))
                .unwrap_or_default();
            let text = description
                .map(str::to_string)
                .or_else(|| {
                    current
                        .as_ref()
                        .and_then(|playlist| playlist.description.clone())
                })
                .unwrap_or_default();
            self.send(
                Call::new(Method::POST, self.v1(&format!("playlists/{id}")))
                    .form("title", title)
                    .form("description", text),
            )
            .await?;
        }
        if let Some(public) = public {
            let action = if public { "set-public" } else { "set-private" };
            self.send(Call::new(
                Method::PUT,
                self.v2(&format!("playlists/{id}/{action}")),
            ))
            .await?;
        }
        Ok(())
    }

    /// Adds songs or videos at the end of a playlist through the JSON:API
    /// relationship (`kind` is `tracks` or `videos`), duplicates allowed.
    async fn add_playlist_relationship(&self, id: &str, kind: &str, ids: &[&str]) -> Result<()> {
        let added = self
            .send(
                Call::new(
                    Method::POST,
                    format!("{}/playlists/{id}/relationships/items", self.openapi_base),
                )
                .json_api(&playlist_items_body(kind, ids)),
            )
            .await;
        self.forget_playlist_read(id);
        added?;
        Ok(())
    }

    /// Adds tracks at `position`, or at the end.
    pub async fn add_playlist_items(
        &self,
        id: &str,
        uris: &[String],
        position: Option<u32>,
    ) -> Result<Option<String>> {
        let ids: Vec<&str> = uris.iter().filter_map(|uri| uri_id(uri, "track")).collect();
        let videos: Vec<&str> = uris.iter().filter_map(|uri| uri_id(uri, "video")).collect();
        let mut updated = None;
        if !ids.is_empty() {
            let joined = ids.join(",");
            let url = self.v1(&format!("playlists/{id}/items"));
            let edit = self
                .playlist_edit(id, |etag, total| {
                    Call::new(Method::POST, url.clone())
                        .etag(etag)
                        .form("trackIds", &joined)
                        .form("toIndex", position.unwrap_or(total))
                        .form("onArtifactNotFound", "SKIP")
                        .form("onDupes", "ADD")
                })
                .await;
            match edit {
                Ok(reply) => updated = last_updated(&reply.body),
                // Live 2026-10-18: TIDAL's v1 read of a playlist that holds
                // a video answers 500, and the edit starts with that read.
                // Songs for the end of the list go in through the same
                // relationship a video does.
                Err(ApiError::Status { status, .. }) if status >= 500 && position.is_none() => {
                    self.add_playlist_relationship(id, "tracks", &ids).await?;
                }
                Err(error) => return Err(error),
            }
        }
        if !videos.is_empty() {
            // The v1 `trackIds` field takes songs only. TIDAL's web player
            // adds a video through the JSON:API relationship (live
            // 2026-10-18); the video lands at the end.
            self.add_playlist_relationship(id, "videos", &videos)
                .await?;
        }
        Ok(updated)
    }

    /// Every song of an album, playlist or mix, in order. Artists have no
    /// fixed songs and give none.
    pub async fn context_tracks(&self, context_uri: &str) -> Result<Vec<Track>> {
        let mut tracks = Vec::new();
        let mut offset = 0;
        loop {
            let page = if let Some(album) = uri_id(context_uri, "album") {
                self.album_tracks(album, offset, ITEMS_LIMIT).await?
            } else if let Some(source) = uri_id(context_uri, "playlist") {
                self.playlist_tracks(source, offset, ITEMS_LIMIT).await?
            } else if let Some(mix) = uri_id(context_uri, "mix") {
                self.mix_tracks(mix, offset, ITEMS_LIMIT).await?
            } else {
                return Ok(tracks);
            };
            let next = page.next_offset();
            tracks.extend(page.items);
            let Some(next) = next else {
                break;
            };
            offset = next;
        }
        Ok(tracks)
    }

    /// Removes every occurrence of `uris`. TIDAL edits by position, so the
    /// playlist is read to find them, then they are removed from the end.
    pub async fn remove_playlist_items(
        &self,
        id: &str,
        uris: &[String],
        _snapshot_id: Option<&str>,
    ) -> Result<Option<String>> {
        let wanted: HashSet<&str> = uris.iter().map(String::as_str).collect();
        let mut indices = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.playlist_items(id, offset, ITEMS_LIMIT).await?;
            for (position, item) in page.items.iter().enumerate() {
                if item
                    .playable()
                    .is_some_and(|playable| wanted.contains(playable.uri()))
                {
                    indices.push(page.offset + position as u32);
                }
            }
            let Some(next) = page.next_offset() else {
                break;
            };
            offset = next;
        }
        self.remove_playlist_indices(id, &indices).await
    }

    /// Removes the items at these positions (of the current order).
    pub async fn remove_playlist_indices(
        &self,
        id: &str,
        indices: &[u32],
    ) -> Result<Option<String>> {
        let mut indices = indices.to_vec();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        indices.dedup();
        let mut updated = None;
        // Highest first, so earlier chunks never shift later ones.
        for chunk in indices.chunks(REMOVE_CHUNK) {
            let list = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let url = self.v1(&format!("playlists/{id}/items/{list}"));
            let reply = self
                .playlist_edit(id, |etag, _| {
                    Call::new(Method::DELETE, url.clone())
                        .etag(etag)
                        .query("order", "INDEX")
                        .query("orderDirection", "ASC")
                })
                .await?;
            updated = last_updated(&reply.body).or(updated);
        }
        Ok(updated)
    }

    /// Moves the item at `range_start` to before `insert_before` (TIDAL's
    /// convention; TIDAL's `toIndex` is an insertion point too).
    pub async fn reorder_playlist(
        &self,
        id: &str,
        range_start: u32,
        insert_before: u32,
        _snapshot_id: Option<&str>,
    ) -> Result<Option<String>> {
        let url = self.v1(&format!("playlists/{id}/items/{range_start}"));
        let reply = self
            .playlist_edit(id, |etag, _| {
                Call::new(Method::POST, url.clone())
                    .etag(etag)
                    .form("toIndex", insert_before)
            })
            .await?;
        Ok(last_updated(&reply.body))
    }

    /// Adds someone else's playlist to the collection.
    pub async fn follow_playlist(&self, id: &str) -> Result<()> {
        self.send(
            Call::new(
                Method::PUT,
                self.v2("my-collection/playlists/folders/add-favorites"),
            )
            .query("folderId", "root")
            .query("uuids", id),
        )
        .await?;
        Ok(())
    }

    /// Removes a playlist from the collection without deleting it.
    pub async fn unfollow_playlist(&self, id: &str) -> Result<()> {
        self.send(
            Call::new(
                Method::PUT,
                self.v2("my-collection/playlists/folders/remove"),
            )
            .query("trns", format!("trn:playlist:{id}")),
        )
        .await?;
        Ok(())
    }

    /// Deletes one of the account's own playlists.
    pub async fn delete_playlist(&self, id: &str) -> Result<()> {
        self.send(Call::new(
            Method::DELETE,
            self.v1(&format!("playlists/{id}")),
        ))
        .await?;
        Ok(())
    }

    // ---- playlist folders ------------------------------------------------------

    /// Every folder of My Collection with the playlists in it, parents
    /// before their children. A folder's items are read with its own
    /// `folderId`; the top level is `root`.
    pub async fn playlist_folders(&self) -> Result<Vec<PlaylistFolder>> {
        let mut folders = Vec::new();
        let mut pending = vec![(String::from("root"), None::<String>)];
        // TIDAL nests folders; a cap keeps a cyclic answer from looping.
        let mut reads = 0;
        while let Some((folder_id, parent)) = pending.pop() {
            reads += 1;
            if reads > 200 {
                break;
            }
            let mut cursor: Option<String> = None;
            let mut playlist_ids = Vec::new();
            let mut children = Vec::new();
            loop {
                let mut call = Call::new(Method::GET, self.v2("my-collection/playlists/folders"))
                    .query("folderId", &folder_id)
                    .query("limit", 50)
                    .query("order", "DATE")
                    .query("orderDirection", "DESC");
                if let Some(cursor) = &cursor {
                    call = call.query("cursor", cursor);
                }
                let page: serde_json::Value = self.get(call).await?;
                for item in page["items"].as_array().into_iter().flatten() {
                    let data = &item["data"];
                    match item["itemType"].as_str() {
                        Some(kind) if kind.eq_ignore_ascii_case("FOLDER") => {
                            let id = folder_item_id(item, data);
                            if id.is_empty() {
                                continue;
                            }
                            let name = data["name"]
                                .as_str()
                                .or(item["name"].as_str())
                                .unwrap_or_default()
                                .to_string();
                            children.push((id, name));
                        }
                        Some(kind) if kind.eq_ignore_ascii_case("PLAYLIST") => {
                            if let Some(uuid) = data["uuid"].as_str().or(data["id"].as_str()) {
                                playlist_ids.push(uuid.to_string());
                            }
                        }
                        _ => {}
                    }
                }
                match page["cursor"].as_str().filter(|cursor| !cursor.is_empty()) {
                    Some(next) if Some(next) != cursor.as_deref() => cursor = Some(next.into()),
                    _ => break,
                }
            }
            if folder_id != "root" {
                // Filled in below once its name is known from the parent.
                if let Some(folder) = folders
                    .iter_mut()
                    .find(|folder: &&mut PlaylistFolder| folder.id == folder_id)
                {
                    folder.playlist_ids = playlist_ids;
                }
            }
            for (id, name) in children.into_iter().rev() {
                if folders.iter().any(|folder| folder.id == id) {
                    continue;
                }
                folders.push(PlaylistFolder {
                    id: id.clone(),
                    name,
                    parent_id: (folder_id != "root").then(|| folder_id.clone()),
                    playlist_ids: Vec::new(),
                });
                pending.push((id, Some(folder_id.clone())));
            }
            let _ = parent;
        }
        Ok(folders)
    }

    /// Makes an empty folder at the top level.
    pub async fn create_folder(&self, name: &str) -> Result<()> {
        self.send(
            Call::new(
                Method::PUT,
                self.v2("my-collection/playlists/folders/create-folder"),
            )
            .query("name", name)
            .query("folderId", "root"),
        )
        .await?;
        Ok(())
    }

    pub async fn rename_folder(&self, id: &str, name: &str) -> Result<()> {
        self.send(
            Call::new(
                Method::PUT,
                self.v2("my-collection/playlists/folders/rename"),
            )
            .query("trn", format!("trn:folder:{id}"))
            .query("name", name),
        )
        .await?;
        Ok(())
    }

    /// Deletes a folder. Its playlists go to the top level first, so
    /// removing the folder never removes a playlist from the collection.
    pub async fn delete_folder(&self, id: &str, playlist_ids: &[String]) -> Result<()> {
        if !playlist_ids.is_empty() {
            self.move_playlists_to_folder(playlist_ids, None).await?;
        }
        // The move endpoint is unverified: a 200 proves nothing. Delete
        // only a folder read back as empty (whatever was known before).
        let page: serde_json::Value = self
            .get(
                Call::new(Method::GET, self.v2("my-collection/playlists/folders"))
                    .query("folderId", id)
                    .query("limit", 50),
            )
            .await?;
        if page["items"]
            .as_array()
            .is_none_or(|items| !items.is_empty())
        {
            return Err(ApiError::FolderNotEmpty);
        }
        self.send(
            Call::new(
                Method::PUT,
                self.v2("my-collection/playlists/folders/remove"),
            )
            .query("trns", format!("trn:folder:{id}")),
        )
        .await?;
        Ok(())
    }

    /// Moves playlists into a folder, or to the top level with `None`.
    pub async fn move_playlists_to_folder(
        &self,
        playlist_ids: &[String],
        folder_id: Option<&str>,
    ) -> Result<()> {
        let trns = playlist_ids
            .iter()
            .map(|id| format!("trn:playlist:{id}"))
            .collect::<Vec<_>>()
            .join(",");
        self.send(
            Call::new(Method::PUT, self.v2("my-collection/playlists/folders/move"))
                .query("folderId", folder_id.unwrap_or("root"))
                .query("trns", trns),
        )
        .await?;
        Ok(())
    }

    // ---- catalogue -------------------------------------------------------------

    /// The size in bytes of a stream file TIDAL handed out (the URL of a
    /// BTS manifest), from a HEAD request: what TidaLuna reads to work out
    /// a FLAC's bit rate (Tidal Tags' "951kb/s"). `None` when the server
    /// does not say.
    pub async fn stream_size(&self, url: &str) -> Option<u64> {
        let client = self.http.client().ok()?;
        let response = client
            .head(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }

    /// A v1 catalogue answer as TIDAL sends it (`tracks/{id}`,
    /// `albums/{id}/items`, ...), for the Luna plugin runtime, whose plugins
    /// read TIDAL's own JSON. GET only; `path` is relative to the v1 base.
    pub async fn v1_json(&self, path: &str, query: &[(&str, String)]) -> Result<serde_json::Value> {
        let items = playlist_items_path(path);
        if let Some(id) = items
            && self.playlist_is_v1_broken(id)
            && let Some(body) = self.playlist_items_for_plugin(id, query).await
        {
            return decode(&body);
        }
        let mut call = Call::new(Method::GET, self.v1(path));
        for (key, value) in query {
            call = call.query(key, value);
        }
        let result = self.get(call).await;
        if let (Some(id), Err(ApiError::Status { status, .. })) = (items, &result)
            && *status >= 500
            && let Some(body) = self.playlist_items_for_plugin(id, query).await
        {
            return decode(&body);
        }
        result
    }

    /// A plugin's read of `playlists/{id}/items` when v1 fails on a video:
    /// the page from openapi v2 in v1's shape.
    async fn playlist_items_for_plugin(
        &self,
        id: &str,
        query: &[(&str, String)],
    ) -> Option<String> {
        let number = |name: &str, default: u32| {
            query
                .iter()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(default)
        };
        self.playlist_items_via_v2(
            id,
            number("offset", 0),
            number("limit", 10).min(ITEMS_LIMIT),
        )
        .await
        .ok()
    }

    /// A v1 call a Luna plugin made itself (`fetch` of
    /// `desktop.tidal.com/v1/...` as the signed-in account), answered as TIDAL
    /// answers it: the status, the body and the `ETag` a playlist read gives
    /// (the next write names it in `If-None-Match`). `form` is the urlencoded
    /// body of a write. An error status is an answer, not an error, and its
    /// body is TIDAL's own.
    pub async fn v1_call(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        form: &[(String, String)],
        if_none_match: Option<String>,
    ) -> Result<(u16, String, Option<String>)> {
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|_| ApiError::Decode("the request method".into()))?;
        let method_is_get = method == Method::GET;
        let mut call = Call::new(method, self.v1(path));
        for (key, value) in query {
            call = call.query(key, value);
        }
        for (key, value) in form {
            call = call.form(key, value);
        }
        if let Some(etag) = if_none_match {
            call = call.etag(etag);
        }
        let mut retry = Some(call.clone());
        let items = playlist_items_path(path).filter(|_| method_is_get);
        if let Some(id) = items
            && self.playlist_is_v1_broken(id)
            && let Some(body) = self.playlist_items_for_plugin(id, query).await
        {
            return Ok((200, body, None));
        }
        loop {
            match self.send_keeping(call, true).await {
                Ok(reply) if reply.status >= 500 && items.is_some() => {
                    if let Some(body) = self
                        .playlist_items_for_plugin(items.unwrap_or_default(), query)
                        .await
                    {
                        return Ok((200, body, None));
                    }
                    return Ok((reply.status, reply.body, reply.etag));
                }
                // A truncated or garbled read is asked for once more.
                Ok(reply)
                    if (200..300).contains(&reply.status)
                        && retry.is_some()
                        && retry.as_ref().is_some_and(Call::idempotent)
                        && decode::<serde_json::Value>(&reply.body).is_err() =>
                {
                    call = retry.take().unwrap_or_else(|| unreachable!());
                }
                // TIDAL's answer as it came, an error's body too (`status`,
                // `subStatus`, `userMessage`), as Electron's `fetch` gets it.
                Ok(reply) => return Ok((reply.status, reply.body, reply.etag)),
                Err(ApiError::Status { status, message }) => {
                    let body = serde_json::json!({ "status": status, "userMessage": message });
                    return Ok((status, body.to_string(), None));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// A v2 answer as TIDAL sends it, GET only (demo builds' API probe).
    /// `path` may also be a whole `https://` address on a TIDAL API host
    /// (`openapi.tidal.com/v2/...`).
    #[cfg(feature = "demo")]
    pub async fn v2_json(&self, path: &str, query: &[(&str, String)]) -> Result<serde_json::Value> {
        let url = match path.strip_prefix("https://") {
            Some(rest)
                if rest.starts_with("openapi.tidal.com/")
                    || rest.starts_with("api.tidal.com/")
                    || rest.starts_with("tidal.com/v2/") =>
            {
                path.to_string()
            }
            _ => self.v2(path),
        };
        let mut call = Call::new(Method::GET, url);
        for (key, value) in query {
            // `h:<name>=<value>` sends a header instead of a query value.
            call = match key.strip_prefix("h:") {
                Some(header) => call.header(header, value),
                None => call.query(key, value),
            };
        }
        self.get(call).await
    }

    pub async fn track(&self, id: &str) -> Result<Track> {
        let raw: RawTrack = self
            .get(Call::new(Method::GET, self.v1(&format!("tracks/{id}"))))
            .await?;
        Ok(convert::track(raw))
    }

    /// An album with its first page of tracks in `tracks`.
    pub async fn album(&self, id: &str) -> Result<Album> {
        let (album, tracks) = tokio::join!(
            self.get::<RawAlbum>(Call::new(Method::GET, self.v1(&format!("albums/{id}")))),
            self.album_tracks(id, 0, ITEMS_LIMIT),
        );
        let mut album = convert::album(album?);
        let mut tracks = tracks?;
        // The album's tracks carry a reference back to it; keep it whole.
        for track in &mut tracks.items {
            if let Some(reference) = &mut track.album {
                reference.images.clone_from(&album.images);
            }
        }
        album.tracks = Some(tracks);
        Ok(album)
    }

    pub async fn album_tracks(&self, id: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        let limit = limit.min(ITEMS_LIMIT);
        let page: RawPage<RawTrack> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("albums/{id}/tracks")))
                    .query("limit", limit)
                    .query("offset", offset),
            )
            .await?;
        Ok(page.map(limit, convert::track))
    }

    pub async fn artist(&self, id: &str) -> Result<Artist> {
        let raw: RawArtist = self
            .get(Call::new(Method::GET, self.v1(&format!("artists/{id}"))))
            .await?;
        Ok(convert::artist(raw))
    }

    /// One page of an artist's top tracks, most popular first.
    pub async fn artist_top_tracks_page(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Track>> {
        let limit = limit.clamp(1, ITEMS_LIMIT);
        let page: RawPage<RawTrack> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/toptracks")))
                    .query("limit", limit)
                    .query("offset", offset),
            )
            .await?;
        Ok(page.map(limit, convert::track))
    }

    pub async fn artist_top_tracks(&self, id: &str) -> Result<Vec<Track>> {
        let page: RawPage<RawTrack> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/toptracks")))
                    .query("limit", 20)
                    .query("offset", 0),
            )
            .await?;
        Ok(page.items.into_iter().map(convert::track).collect())
    }

    /// One group of an artist's releases, as TIDAL's artist page lists
    /// them (`ReleaseGroup::modules`). An artist whose page lists nothing
    /// there (an uploader's own artist, which the page answers empty) or a
    /// page that cannot be read falls back to `artists/{id}/albums`.
    pub async fn artist_releases(
        &self,
        id: &str,
        group: ReleaseGroup,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>> {
        match self.artist_module_releases(id, group, offset, limit).await {
            Ok(page) if offset > 0 || !page.items.is_empty() => return Ok(page),
            Ok(_) => {}
            Err(error) => log::debug!("artist {id}'s page lists could not be read: {error}"),
        }
        self.artist_releases_v1(id, group, offset, limit).await
    }

    /// [`Self::artist_releases`] from the artist page's modules. A group of
    /// one module is paged as TIDAL pages it (50 at most); the Albums
    /// group's three short modules are read in full and paged here.
    async fn artist_module_releases(
        &self,
        id: &str,
        group: ReleaseGroup,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>> {
        let limit = limit.clamp(1, MODULE_LIMIT);
        let modules = group.modules();
        if let [(module, kind)] = modules {
            let (items, more) = self
                .artist_module_page(id, module, *kind, group, offset, limit)
                .await?;
            let end = offset + items.len() as u32;
            return Ok(Page {
                next: more.then(|| format!("offset={end}")),
                total: end + u32::from(more),
                items,
                limit,
                offset,
            });
        }
        let mut all = Vec::new();
        for (module, kind) in modules {
            let mut at = 0;
            // A module of more than 500 releases is cut there.
            for _ in 0..10 {
                let (items, more) = self
                    .artist_module_page(id, module, *kind, group, at, MODULE_LIMIT)
                    .await?;
                at += MODULE_LIMIT;
                all.extend(items);
                if !more {
                    break;
                }
            }
        }
        let total = all.len() as u32;
        let items: Vec<Album> = all
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        let end = offset + items.len() as u32;
        Ok(Page {
            next: (end < total).then(|| format!("offset={end}")),
            items,
            total,
            limit,
            offset,
        })
    }

    /// One page of an artist page module's "View all"
    /// (`v2/artist/<MODULE>/view-all`), and whether more may follow.
    async fn artist_module_page(
        &self,
        id: &str,
        module: &str,
        kind: Option<&str>,
        group: ReleaseGroup,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, bool)> {
        let page = self
            .page_value(&format!(
                "v2/artist/{module}/view-all?artistId={id}&limit={limit}&offset={offset}"
            ))
            .await?;
        let listed = page
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let albums = pages::shelf_items(&page)
            .into_iter()
            .filter_map(|item| match item {
                ShelfItem::Album(mut album) => {
                    if let Some(kind) = kind {
                        album.album_type = Some(kind.to_string());
                    }
                    album.album_group = Some(group.group().to_string());
                    Some(album)
                }
                _ => None,
            })
            .collect();
        Ok((albums, listed as u32 >= limit))
    }

    /// One group of an artist's releases from `artists/{id}/albums`, which
    /// lists every version of a release: one of each title is kept, the
    /// explicit one where there is one.
    async fn artist_releases_v1(
        &self,
        id: &str,
        group: ReleaseGroup,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>> {
        let limit = limit.clamp(1, ITEMS_LIMIT);
        let page: RawPage<RawAlbum> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/albums")))
                    .query("filter", group.filter())
                    .query("limit", limit)
                    .query("offset", offset),
            )
            .await?;
        let mut page = page.map(limit, |raw| {
            let mut album = convert::album(raw);
            album.album_group = Some(group.group().to_string());
            album
        });
        page.items = one_version_each(page.items);
        Ok(page)
    }

    /// An artist's releases for an `include_groups` list, as one
    /// list: albums, then EPs and singles, then appearances. `offset`
    /// counts across the groups.
    pub async fn artist_albums(
        &self,
        id: &str,
        include_groups: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>> {
        let limit = limit.clamp(1, ITEMS_LIMIT);
        let mut items = Vec::new();
        let mut before = 0u32;
        for group in ReleaseGroup::parse(include_groups) {
            let wanted = limit - items.len() as u32;
            let local_offset = offset.saturating_sub(before);
            // Once the page is full, each later group is asked only its size.
            let page = self
                .artist_releases(id, group, local_offset, wanted.max(1))
                .await?;
            if wanted > 0 && offset < before + page.total {
                items.extend(page.items.into_iter().take(wanted as usize));
            }
            before += page.total;
        }
        let end = offset + items.len() as u32;
        Ok(Page {
            next: (end < before && !items.is_empty()).then(|| format!("offset={end}")),
            items,
            total: before,
            limit,
            offset,
        })
    }

    /// The artist's biography with TIDAL's `[wimpLink]` markup and its
    /// source; `None` when TIDAL has none.
    pub async fn artist_bio(&self, id: &str) -> Result<Option<crate::api::bio::ArtistBio>> {
        let bio: Option<Value> = self
            .get_optional(Call::new(
                Method::GET,
                self.v1(&format!("artists/{id}/bio")),
            ))
            .await?;
        Ok(bio.as_ref().and_then(bio_from))
    }

    /// The album's review (TiVo) with TIDAL's `[wimpLink]` markup and its
    /// source, as an artist's biography comes; `None` when TIDAL has none
    /// (it answers 404).
    pub async fn album_review(&self, id: &str) -> Result<Option<crate::api::bio::ArtistBio>> {
        let review: Option<Value> = self
            .get_optional(Call::new(
                Method::GET,
                self.v1(&format!("albums/{id}/review")),
            ))
            .await?;
        Ok(review.as_ref().and_then(bio_from))
    }

    /// The album's credits: the release's own roles (`albums/{id}/credits`)
    /// and each song's (`albums/{id}/items/credits`, read in full).
    pub async fn album_credits(&self, id: &str) -> Result<crate::model::AlbumCredits> {
        let album_path = format!("albums/{id}/credits");
        let (album, first) = tokio::join!(
            self.v1_json(&album_path, &[]),
            self.album_items_credits(id, 0),
        );
        let (mut tracks, total) = first?;
        let mut offset = tracks.len() as u32;
        // 100 songs a page; a long compilation takes a few more reads.
        while offset < total && offset < 1000 {
            let (more, _) = self.album_items_credits(id, offset).await?;
            if more.is_empty() {
                break;
            }
            offset += more.len() as u32;
            tracks.extend(more);
        }
        Ok(crate::model::AlbumCredits {
            // The release-wide roles are a bonus: without them the songs'
            // credits still show.
            album: album
                .map(|json| parse_credits(&json).roles)
                .unwrap_or_default(),
            tracks,
        })
    }

    async fn album_items_credits(
        &self,
        id: &str,
        offset: u32,
    ) -> Result<(Vec<crate::model::AlbumTrackCredits>, u32)> {
        let answer = self
            .v1_json(
                &format!("albums/{id}/items/credits"),
                &[("limit", "100".into()), ("offset", offset.to_string())],
            )
            .await;
        match answer {
            Ok(json) => Ok(parse_album_items_credits(&json)),
            // An album TIDAL keeps no credits for.
            Err(ApiError::Status { status: 404, .. }) => Ok((Vec::new(), 0)),
            Err(error) => Err(error),
        }
    }

    /// One page of TIDAL's Recommended songs for a playlist.
    pub async fn playlist_recommendations(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Track>> {
        let limit = limit.clamp(1, ITEMS_LIMIT);
        let page: RawPage<Wrapped<RawTrack>> = self
            .get(
                Call::new(
                    Method::GET,
                    self.v1(&format!("playlists/{id}/recommendations/items")),
                )
                .query("limit", limit)
                .query("offset", offset),
            )
            .await?;
        Ok(tracks_page(page, limit))
    }

    /// How many people follow the artist, from the OpenAPI `artists`
    /// resource; `None` when TIDAL gives no count.
    pub async fn artist_followers(&self, id: &str) -> Result<Option<u64>> {
        let document: Option<Value> = self
            .get_optional(Call::new(
                Method::GET,
                format!("{}/artists/{id}", self.openapi_base),
            ))
            .await?;
        Ok(document.as_ref().and_then(followers_of))
    }

    pub async fn related_artists(&self, id: &str) -> Result<Vec<Artist>> {
        let page: Option<RawPage<RawArtist>> = self
            .get_optional(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/similar")))
                    .query("limit", 50),
            )
            .await?;
        Ok(page
            .map(|page| page.items.into_iter().map(convert::artist).collect())
            .unwrap_or_default())
    }

    /// A mix's title and artwork.
    pub async fn mix(&self, id: &str) -> Result<Mix> {
        let page: Value = self
            .get(
                Call::new(Method::GET, self.v1("pages/mix"))
                    .query("mixId", id)
                    .query("deviceType", "BROWSER")
                    .query("locale", LOCALE),
            )
            .await?;
        let header = page["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["modules"].as_array())
            .flatten()
            .find_map(|module| (module["type"] == "MIX_HEADER").then(|| module["mix"].clone()))
            .ok_or_else(|| ApiError::Decode("TIDAL's mix page has no header".into()))?;
        let mut mix = convert::mix(decode(&header.to_string())?);
        if mix.id.is_empty() {
            mix.id = id.to_string();
            mix.uri = convert::uri("mix", id);
        }
        Ok(mix)
    }

    pub async fn mix_tracks(&self, id: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        let limit = limit.min(ITEMS_LIMIT);
        let page: RawPage<Wrapped<RawTrack>> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("mixes/{id}/items")))
                    .query("limit", limit)
                    .query("offset", offset),
            )
            .await?;
        Ok(tracks_page(page, limit))
    }

    /// Tracks like this one (track radio).
    pub async fn track_radio(&self, id: &str, limit: u32) -> Result<Vec<Track>> {
        let page: Option<RawPage<RawTrack>> = self
            .get_optional(
                Call::new(Method::GET, self.v1(&format!("tracks/{id}/radio")))
                    .query("limit", limit.min(ITEMS_LIMIT)),
            )
            .await?;
        Ok(page
            .map(|page| page.items.into_iter().map(convert::track).collect())
            .unwrap_or_default())
    }

    /// The id of the track's radio mix, if it has one.
    pub async fn track_mix(&self, id: &str) -> Result<Option<String>> {
        let mix: Option<MixId> = self
            .get_optional(Call::new(Method::GET, self.v1(&format!("tracks/{id}/mix"))))
            .await?;
        Ok(mix.map(|mix| mix.id).filter(|id| !id.is_empty()))
    }

    pub async fn artist_radio(&self, id: &str, limit: u32) -> Result<Vec<Track>> {
        let page: Option<RawPage<RawTrack>> = self
            .get_optional(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/radio")))
                    .query("limit", limit.min(ITEMS_LIMIT)),
            )
            .await?;
        Ok(page
            .map(|page| page.items.into_iter().map(convert::track).collect())
            .unwrap_or_default())
    }

    /// The id of the artist's radio mix, if it has one.
    pub async fn artist_mix(&self, id: &str) -> Result<Option<String>> {
        let mix: Option<MixId> = self
            .get_optional(Call::new(
                Method::GET,
                self.v1(&format!("artists/{id}/mix")),
            ))
            .await?;
        Ok(mix.map(|mix| mix.id).filter(|id| !id.is_empty()))
    }

    /// TIDAL's recommendations, from the first seed's radio.
    pub async fn recommendations(
        &self,
        seed_tracks: &[String],
        seed_artists: &[String],
        limit: u32,
    ) -> Result<Vec<Track>> {
        let id_of = |seed: &String, kind| {
            uri_id(seed, kind)
                .map(str::to_string)
                .unwrap_or_else(|| seed.clone())
        };
        if let Some(seed) = seed_tracks.first() {
            return self.track_radio(&id_of(seed, "track"), limit).await;
        }
        if let Some(seed) = seed_artists.first() {
            return self.artist_radio(&id_of(seed, "artist"), limit).await;
        }
        Ok(Vec::new())
    }

    /// Searches TIDAL. `types` uses the interface's names (`track`, `artist`,
    /// `album`, `playlist`); other types are ignored. TIDAL's top hit
    /// is in `top_hit`.
    pub async fn search(&self, query: &str, types: &[&str]) -> Result<SearchResults> {
        let types: Vec<&str> = types
            .iter()
            .filter_map(|kind| match *kind {
                "track" => Some("TRACKS"),
                "artist" => Some("ARTISTS"),
                "album" => Some("ALBUMS"),
                "playlist" => Some("PLAYLISTS"),
                _ => None,
            })
            .collect();
        if types.is_empty() {
            return Ok(SearchResults::default());
        }
        // The whole results page comes from the web player's v2 search
        // (ranked top hits, genres, videos, uploads); v1 stays the
        // fallback and answers single-kind searches.
        if types.len() == 4 {
            match self.search_v2(query).await {
                Ok(results) if !results.is_empty() || !results.genres.is_empty() => {
                    return Ok(results);
                }
                Ok(_) => {}
                Err(error) => log::debug!("TIDAL v2 search failed, using v1: {error}"),
            }
        }
        let raw: RawSearch = self
            .get(
                Call::new(Method::GET, self.v1("search"))
                    .query("query", query)
                    .query("types", types.join(","))
                    .query("limit", SEARCH_LIMIT)
                    .query("offset", 0)
                    .query("includeContributors", true),
            )
            .await?;
        Ok(search_results(raw))
    }

    async fn search_v2(&self, query: &str) -> Result<SearchResults> {
        let mut call = Call::new(Method::GET, self.v2("search/"))
            .query("query", query)
            .query("types", search::TYPES);
        for (key, value) in search::search_params() {
            call = call.query(key, value);
        }
        let value: Value = self.get(call).await?;
        search::dump("search", query, &value);
        Ok(search::results(&value))
    }

    /// What TIDAL suggests for a query being typed: fuller queries and
    /// direct hits (`v2/suggestions/`).
    pub async fn suggestions(&self, query: &str) -> Result<SearchSuggestions> {
        let value: Value = self
            .get(
                Call::new(Method::GET, self.v2("suggestions/"))
                    .query("query", query)
                    .query("explicit", true)
                    .query("hybrid", true),
            )
            .await?;
        search::dump("suggestions", query, &value);
        Ok(search::suggestions(query, &value))
    }

    // ---- pages ---------------------------------------------------------------

    /// A named TIDAL page (`home`, `explore`, `for_you`, `moods`,
    /// `genre_page`, `my_collection_my_mixes`...) as shelves.
    pub async fn page(&self, name: &str) -> Result<Vec<Shelf>> {
        self.page_path(&format!("pages/{name}")).await
    }

    /// TIDAL's Home: the v2 feed the official apps read, else the legacy
    /// `pages/home`. Shelves TIDAL sends without their cards come back
    /// `pending`, for [`Self::shelf_items`].
    pub async fn home(&self) -> Result<Vec<Shelf>> {
        self.home_feed(None).await
    }

    /// [`Self::home`], recording the raw answers and timings in `dump`.
    pub async fn home_feed(&self, mut dump: Option<&mut HomeDump>) -> Result<Vec<Shelf>> {
        let started = Instant::now();
        let feed = self.home_v2("static", dump.as_deref_mut()).await;
        if let Some(dump) = dump.as_deref_mut() {
            dump.time("v2/home/feed/static (all pages)", started, feed.is_ok());
        }
        let feed_error = match feed {
            Ok(shelves) if !shelves.is_empty() => return Ok(shelves),
            Ok(_) => None,
            Err(error) => {
                log::debug!("TIDAL v2 home feed failed, reading pages/home: {error}");
                Some(error)
            }
        };
        let started = Instant::now();
        let page = self.page_value("pages/home").await;
        if let Some(dump) = dump {
            dump.time("pages/home", started, page.is_ok());
            if let Ok(page) = &page {
                dump.v1 = Some(page.clone());
            }
        }
        match page {
            Ok(page) => Ok(pages::shelves(&page)),
            Err(error) => Err(feed_error.unwrap_or(error)),
        }
    }

    /// One of Home's other tabs: Staff Picks (`editorial`) or Uploads
    /// (`uploads`), the v2 feeds TIDAL's web player reads for them.
    pub async fn home_tab(&self, feed: &str) -> Result<Vec<Shelf>> {
        let shelves = self.home_v2(feed, None).await?;
        Ok(self.fill_pending(shelves).await)
    }

    /// A v2 home feed (`static`, `editorial`, `uploads`), following its
    /// cursor a few pages.
    async fn home_v2(&self, feed: &str, mut dump: Option<&mut HomeDump>) -> Result<Vec<Shelf>> {
        let mut shelves = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..HOME_FEED_PAGES {
            let mut call = Call::new(Method::GET, self.v2(&format!("home/feed/{feed}")))
                .query("deviceType", "BROWSER")
                .query("platform", "WEB")
                .query("locale", LOCALE)
                .query("limit", 20)
                .query("timeOffset", time_offset());
            if let Some(cursor) = &cursor {
                call = call.query("cursor", cursor);
            }
            let feed: Value = self.get(call).await?;
            shelves.extend(pages::feed_shelves(&feed));
            cursor = pages::feed_cursor(&feed);
            if let Some(dump) = dump.as_deref_mut() {
                dump.v2.push(feed);
            }
            if cursor.is_none() {
                break;
            }
        }
        Ok(shelves)
    }

    /// The cards of a shelf sent without them, from its `source`.
    pub async fn shelf_items(&self, source: &str) -> Result<Vec<ShelfItem>> {
        let page = self.page_value(source).await?;
        Ok(pages::shelf_items(&page))
    }

    /// Fills the shelves sent without their cards, a few at a time, and
    /// drops those that stay empty.
    async fn fill_pending(&self, shelves: Vec<Shelf>) -> Vec<Shelf> {
        let semaphore = Semaphore::new(PENDING_AT_ONCE);
        let semaphore = &semaphore;
        let tasks: Vec<_> = shelves
            .into_iter()
            .map(|mut shelf| {
                Box::pin(async move {
                    if let (true, Some(source)) = (shelf.pending, shelf.source.clone()) {
                        let _permit = semaphore.acquire().await;
                        match self.shelf_items(&source).await {
                            Ok(items) => shelf.items = items,
                            Err(error) => log::debug!("TIDAL shelf list failed: {error}"),
                        }
                        shelf.pending = false;
                    }
                    shelf
                })
            })
            .collect();
        // Polled together, so the semaphore sets the pace.
        join_all(tasks)
            .await
            .into_iter()
            .filter(|shelf| !shelf.items.is_empty() || !shelf.links.is_empty())
            .collect()
    }

    pub async fn explore(&self) -> Result<Vec<Shelf>> {
        self.page("explore").await
    }

    /// A page by API path, as a shelf's `more` gives it, with the shelves
    /// TIDAL sends without their cards filled in.
    pub async fn page_path(&self, path: &str) -> Result<Vec<Shelf>> {
        let page = self.page_value(path).await?;
        let mut shelves = pages::shelves(&page);
        if shelves.is_empty() && page.get("rows").is_none() {
            shelves = pages::feed_shelves(&page);
        }
        if shelves.is_empty() {
            let items = pages::shelf_items(&page);
            if !items.is_empty() {
                shelves.push(Shelf {
                    title: page
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    items,
                    ..Default::default()
                });
            }
        }
        Ok(self.fill_pending(shelves).await)
    }

    /// The Playlists and Credits modules of an artist's page.
    pub async fn artist_modules(&self, id: &str) -> Result<ArtistModules> {
        let shelves = self
            .page_path(&format!("pages/artist?artistId={id}"))
            .await?;
        Ok(pages::artist_modules(&shelves))
    }

    /// The shelves under an album page's footer: Other versions, More albums
    /// by the artist, Related albums and Related artists.
    pub async fn album_shelves(&self, id: &str) -> Result<Vec<AlbumShelf>> {
        let shelves = self.page_path(&format!("pages/album?albumId={id}")).await?;
        Ok(pages::album_shelves(shelves))
    }

    /// Every card of a module's "View all" page.
    pub async fn module_items(&self, path: &str) -> Result<Vec<ShelfItem>> {
        let shelves = self.page_path(path).await?;
        Ok(shelves.into_iter().flat_map(|shelf| shelf.items).collect())
    }

    /// A page's raw JSON by API path: v1 unless the path starts `v2/`.
    async fn page_value(&self, path: &str) -> Result<Value> {
        let path = pages::normalize_path(path);
        let (path, extra) = path.split_once('?').unwrap_or((path.as_str(), ""));
        let mut call = match path.strip_prefix("v2/") {
            Some(rest) => Call::new(Method::GET, self.v2(rest)).query("platform", "WEB"),
            None => Call::new(Method::GET, self.v1(path)),
        }
        .query("deviceType", "BROWSER")
        .query("locale", LOCALE);
        for pair in extra.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if matches!(key, "countryCode" | "deviceType" | "locale" | "platform") {
                continue;
            }
            let value = urlencoding::decode(value)
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| value.to_string());
            call.query.push((key.to_string(), value));
        }
        self.get(call).await
    }

    // ---- lyrics and playback ---------------------------------------------------

    /// The track's lyrics, synced when TIDAL has timings. `None` when TIDAL
    /// has none.
    /// A track's credits (`tracks/{id}/credits`), with the release year and
    /// the label line from the track itself, as TIDAL's now-playing view
    /// lists them.
    pub async fn track_credits(&self, track_id: &str) -> Result<crate::model::TrackCredits> {
        let include = [("includeContributors", "true".to_string())];
        let credits_path = format!("tracks/{track_id}/credits");
        let track_path = format!("tracks/{track_id}");
        let (credits, track) = tokio::join!(
            self.v1_json(&credits_path, &include),
            self.v1_json(&track_path, &[]),
        );
        let mut parsed = parse_credits(&credits?);
        if let Ok(track) = track {
            let (released, label) = release_and_label(&track);
            parsed.released = released;
            parsed.label = label;
        }
        Ok(parsed)
    }

    pub async fn lyrics(&self, track_id: &str) -> Result<Option<crate::lyrics::Lyrics>> {
        let raw: Option<RawLyrics> = match self
            .get_optional(Call::new(
                Method::GET,
                self.v1(&format!("tracks/{track_id}/lyrics")),
            ))
            .await
        {
            // Rights-restricted lyrics answer 401/403: there are none to show.
            Err(ApiError::Status {
                status: 401 | 403, ..
            }) => None,
            other => other?,
        };
        Ok(raw.and_then(lyrics))
    }

    /// A playable stream for a track. The quality reported is what TIDAL
    /// gave, which may be lower than asked. Protected streams are an error.
    ///
    /// Lossless is asked from `trackManifests`, naming the formats up to
    /// 16-bit FLAC: `playbackinfopostpaywall?audioquality=LOSSLESS` answers
    /// AAC 320 ("HIGH") for the sign-in clients Tidalfast uses, even for a
    /// track TIDAL holds in FLAC (measured 2026-10-14). If `trackManifests`
    /// refuses, or answers with DRM (as it does for the PKCE client, whose
    /// `LOSSLESS` playbackinfo is 16-bit FLAC), the old request is asked.
    ///
    /// Max is asked from `playbackinfopostpaywall`, which gives hi-res. When
    /// it answers lossy (a track with no hi-res master, or a client that
    /// cannot have Max), `trackManifests` is asked with every format up to
    /// hi-res. Only if that is refused too is the request asked again as
    /// Lossless, and later Max requests go straight there.
    pub async fn playback_info(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        use std::sync::atomic::Ordering;
        let wanted =
            if quality == Quality::HiResLossless && self.max_refused.load(Ordering::Relaxed) {
                Quality::Lossless
            } else {
                quality
            };
        if wanted == Quality::Lossless {
            if let Some(stream) = self.track_manifest_or_fallback(track_id, wanted).await? {
                return Ok(stream);
            }
            return self.playback_info_at(track_id, wanted).await;
        }
        let stream = self.playback_info_at(track_id, wanted).await?;
        if wanted != Quality::HiResLossless || stream.quality >= Quality::Lossless {
            return Ok(stream);
        }
        if let Some(manifest) = self.track_manifest_or_fallback(track_id, wanted).await? {
            return Ok(if manifest.quality >= stream.quality {
                manifest
            } else {
                stream
            });
        }
        if let Ok(lossless) = self.playback_info_at(track_id, Quality::Lossless).await
            && lossless.quality >= Quality::Lossless
        {
            log::info!("TIDAL gave a lossy stream for Max; asking for Lossless from now on");
            self.max_refused.store(true, Ordering::Relaxed);
            return Ok(lossless);
        }
        Ok(stream)
    }

    /// `trackManifests` for a lossless tier, or `None` when it was refused
    /// or unreadable and `playbackinfopostpaywall` should be asked instead.
    /// Errors that the old request would meet too are returned as they are.
    async fn track_manifest_or_fallback(
        &self,
        track_id: &str,
        quality: Quality,
    ) -> Result<Option<StreamInfo>> {
        use std::sync::atomic::Ordering;
        if self.manifests_protected.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match self.track_manifest_at(track_id, quality).await {
            Ok(stream) => Ok(Some(stream)),
            Err(error @ (ApiError::Status { .. } | ApiError::Decode(_))) => {
                log::debug!("TIDAL trackManifests refused ({error}); using playbackinfo");
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// The official player's request: the formats it can play, best last.
    async fn track_manifest_at(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let formats: &[&str] = match quality {
            Quality::Low => &["HEAACV1"],
            Quality::High => &["HEAACV1", "AACLC"],
            Quality::Lossless => &["HEAACV1", "AACLC", "FLAC"],
            Quality::HiResLossless => &["HEAACV1", "AACLC", "FLAC", "FLAC_HIRES"],
        };
        let mut call = Call::new(
            Method::GET,
            format!("{}/trackManifests/{track_id}", self.openapi_base),
        )
        .query("manifestType", "MPEG_DASH")
        .query("uriScheme", "DATA")
        .query("usage", "PLAYBACK")
        .query("adaptive", "false");
        for format in formats {
            call = call.query("formats", format);
        }
        let document: manifest::TrackManifestDocument = self.get(call).await?;
        manifest::track_manifest(document).map_err(|error| {
            if matches!(error, manifest::StreamError::Encrypted(_)) {
                log::info!(
                    "TIDAL trackManifests is protected for this sign-in; using playbackinfo"
                );
                self.manifests_protected
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            stream_error(error)
        })
    }

    async fn playback_info_at(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let info: manifest::PlaybackInfo = self
            .get(
                Call::new(
                    Method::GET,
                    self.v1(&format!("tracks/{track_id}/playbackinfopostpaywall")),
                )
                .query("audioquality", quality.api_name())
                .query("playbackmode", "STREAM")
                .query("assetpresentation", "FULL"),
            )
            .await?;
        manifest::stream_info(info).map_err(stream_error)
    }

    /// A music video's metadata.
    pub async fn video(&self, id: &str) -> Result<Video> {
        let raw: convert::RawVideo = self
            .get(Call::new(Method::GET, self.v1(&format!("videos/{id}"))))
            .await?;
        Ok(convert::video(raw))
    }

    /// A music video's stream: the HLS master playlist's URL. Protected
    /// videos are an error; nothing here decrypts.
    pub async fn video_playback_info(&self, video_id: &str) -> Result<StreamInfo> {
        let info: manifest::PlaybackInfo = self
            .get(
                Call::new(
                    Method::GET,
                    self.v1(&format!("videos/{video_id}/playbackinfopostpaywall")),
                )
                .query("videoquality", "HIGH")
                .query("playbackmode", "STREAM")
                .query("assetpresentation", "FULL"),
            )
            .await?;
        manifest::video_stream_info(info).map_err(|error| match error {
            manifest::StreamError::Malformed(_) | manifest::StreamError::UnknownManifest(_) => {
                ApiError::Decode(error.to_string())
            }
            _ => ApiError::Status {
                status: 403,
                message: error.to_string(),
            },
        })
    }

    /// Videos like `video_id`, for the now-playing view's Similar videos:
    /// TIDAL's recommendations for it, else the artist's other videos.
    pub async fn similar_videos(
        &self,
        video_id: &str,
        artist_id: Option<&str>,
    ) -> Result<Vec<Video>> {
        let recommended: Result<Vec<Video>> = async {
            let page: Value = self
                .get(
                    Call::new(
                        Method::GET,
                        self.v1(&format!("videos/{video_id}/recommendations")),
                    )
                    .query("limit", "20"),
                )
                .await?;
            Ok(convert::video_items(&page))
        }
        .await;
        let mut videos = match recommended {
            Ok(items) if !items.is_empty() => items,
            other => {
                if let Err(error) = other {
                    log::debug!("video recommendations: {error}");
                }
                match artist_id {
                    Some(artist) => self.artist_videos_page(artist).await?,
                    None => Vec::new(),
                }
            }
        };
        videos.retain(|video| video.id != video_id && !video.id.is_empty());
        Ok(videos)
    }

    /// One page of an artist's videos.
    pub async fn artist_videos(&self, id: &str, offset: u32, limit: u32) -> Result<Page<Video>> {
        let limit = limit.clamp(1, ITEMS_LIMIT);
        let page: convert::RawPage<convert::RawVideo> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("artists/{id}/videos")))
                    .query("limit", limit)
                    .query("offset", offset),
            )
            .await?;
        Ok(page
            .map(limit, convert::video)
            .retain_items(|video| !video.id.is_empty()))
    }

    async fn artist_videos_page(&self, artist_id: &str) -> Result<Vec<Video>> {
        let page: convert::RawPage<convert::RawVideo> = self
            .get(
                Call::new(Method::GET, self.v1(&format!("artists/{artist_id}/videos")))
                    .query("limit", "20"),
            )
            .await?;
        Ok(page.items.into_iter().map(convert::video).collect())
    }

    /// Ends the session at TIDAL (best effort) and forgets the tokens.
    pub async fn logout(&self) {
        if let Ok(tokens) = self.tokens()
            && let Ok(token) = tokens.access_token(None).await
        {
            crate::auth::tidal::logout(&self.http, &token).await;
        }
        self.set_tokens(None);
    }
}

// ---- response shapes and their mapping ------------------------------------------

trait RetainItems<T> {
    fn retain_items(self, keep: impl FnMut(&T) -> bool) -> Self;
}

impl<T> RetainItems<T> for Page<T> {
    fn retain_items(mut self, keep: impl FnMut(&T) -> bool) -> Self {
        self.items.retain(keep);
        self
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CombinedPlaylist {
    playlist: Option<RawPlaylist>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CreatedPlaylist {
    data: RawPlaylist,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MixId {
    #[serde(deserialize_with = "convert::id_string")]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "SCREAMING_SNAKE_CASE")]
struct FavouriteIds {
    track: Vec<Value>,
    album: Vec<Value>,
    artist: Vec<Value>,
    playlist: Vec<Value>,
    video: Vec<Value>,
}

impl FavouriteIds {
    fn contains(&self, uri: &str, mixes: &HashSet<String>) -> bool {
        let has = |list: &[Value], id: &str| {
            list.iter().any(|value| match value {
                Value::String(text) => text == id,
                Value::Number(number) => number.to_string() == id,
                _ => false,
            })
        };
        if let Some(id) = uri_id(uri, "track") {
            has(&self.track, id)
        } else if let Some(id) = uri_id(uri, "album") {
            has(&self.album, id)
        } else if let Some(id) = uri_id(uri, "artist") {
            has(&self.artist, id)
        } else if let Some(id) = uri_id(uri, "playlist") {
            has(&self.playlist, id)
        } else if let Some(id) = uri_id(uri, "video") {
            has(&self.video, id)
        } else if let Some(id) = uri_id(uri, "mix") {
            mixes.contains(id)
        } else {
            false
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawSearch {
    artists: Option<RawPage<RawArtist>>,
    albums: Option<RawPage<RawAlbum>>,
    playlists: Option<RawPage<RawPlaylist>>,
    tracks: Option<RawPage<RawTrack>>,
    top_hit: Option<RawTopHit>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawTopHit {
    #[serde(rename = "type")]
    kind: String,
    value: Value,
}

fn search_results(raw: RawSearch) -> SearchResults {
    let top_hit = raw.top_hit.and_then(|hit| {
        let value = hit.value.to_string();
        Some(match hit.kind.as_str() {
            "ARTISTS" => TopHit::Artist(convert::artist(decode(&value).ok()?)),
            "ALBUMS" => TopHit::Album(convert::album(decode(&value).ok()?)),
            "TRACKS" => TopHit::Track(convert::track(decode(&value).ok()?)),
            "PLAYLISTS" => TopHit::Playlist(convert::playlist(decode(&value).ok()?)),
            _ => return None,
        })
    });
    SearchResults {
        tracks: raw
            .tracks
            .map(|page| page.map(SEARCH_LIMIT, convert::track)),
        artists: raw
            .artists
            .map(|page| page.map(SEARCH_LIMIT, convert::artist)),
        albums: raw
            .albums
            .map(|page| page.map(SEARCH_LIMIT, convert::album)),
        playlists: raw
            .playlists
            .map(|page| page.map(SEARCH_LIMIT, convert::playlist)),
        top_hits: top_hit.iter().cloned().collect(),
        top_hit,
        ..SearchResults::default()
    }
}

fn saved_tracks_page(page: RawPage<Wrapped<RawTrack>>, limit: u32) -> Page<SavedTrack> {
    page.map(limit, |wrapped| SavedTrack {
        added_at: wrapped.created,
        track: wrapped.item.map(convert::track).unwrap_or_default(),
    })
    .retain_items(|saved| saved.track.id.is_some())
}

/// A playlist entry as a queue row: a song, or a video as the row
/// `Video::as_track` makes of it (`tidal:video:<id>`). Other kinds are none.
fn playlist_entry_track(wrapped: Wrapped<RawTrack>) -> Option<Track> {
    let video = wrapped
        .kind
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("video"));
    let item = wrapped.item?;
    if video {
        Some(convert::video(item.into()).as_track())
    } else if wrapped.kind.is_none()
        || wrapped
            .kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("track"))
    {
        Some(convert::track(item))
    } else {
        None
    }
}

fn playlist_items_page(page: RawPage<Wrapped<RawTrack>>, limit: u32) -> Page<PlaylistItem> {
    page.map(limit, |wrapped| {
        let created = wrapped.created.clone();
        let track = playlist_entry_track(wrapped).map(PlayableItem::Track);
        PlaylistItem {
            added_at: created,
            added_by: None,
            is_local: false,
            item: track,
            track: None,
        }
    })
}

/// A playlist page's songs and videos (a video plays as a queue row, see
/// [`playlist_entry_track`]); `total` and paging count every entry.
fn playlist_tracks_page(page: RawPage<Wrapped<RawTrack>>, limit: u32) -> Page<Track> {
    let mut mapped = page.map(limit, playlist_entry_track);
    let items = std::mem::take(&mut mapped.items)
        .into_iter()
        .flatten()
        .collect();
    Page {
        items,
        total: mapped.total,
        limit: mapped.limit,
        offset: mapped.offset,
        next: mapped.next,
    }
}

/// Only the tracks of a wrapped page; `total` and paging keep counting the
/// videos so the next offset is right.
fn tracks_page(page: RawPage<Wrapped<RawTrack>>, limit: u32) -> Page<Track> {
    let mut mapped = page.map(limit, |wrapped| {
        wrapped
            .is_track()
            .then_some(wrapped.item)
            .flatten()
            .map(convert::track)
    });
    let items = std::mem::take(&mut mapped.items)
        .into_iter()
        .flatten()
        .collect();
    Page {
        items,
        total: mapped.total,
        limit: mapped.limit,
        offset: mapped.offset,
        next: mapped.next,
    }
}

/// A folder entry's id: its own field, the item's, or the end of its TRN.
fn folder_item_id(item: &Value, data: &Value) -> String {
    let text = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    text(&data["id"])
        .or_else(|| text(&item["id"]))
        .or_else(|| {
            item["trn"]
                .as_str()
                .or(data["trn"].as_str())
                .and_then(|trn| trn.rsplit(':').next())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// The body that adds songs or videos to a playlist (`POST
/// playlists/{id}/relationships/items`; `kind` is `tracks` or `videos`).
fn playlist_items_body(kind: &str, ids: &[&str]) -> Value {
    serde_json::json!({
        "data": ids.iter().map(|id| serde_json::json!({"id": id, "type": kind})).collect::<Vec<_>>(),
        "meta": {"onDuplicates": "ADD"},
    })
}

/// The playlist id of a v1 `playlists/{id}/items` path.
fn playlist_items_path(path: &str) -> Option<&str> {
    let id = path
        .trim_start_matches('/')
        .strip_prefix("playlists/")?
        .strip_suffix("/items")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

fn last_updated(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    match &value["lastUpdated"] {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawLyrics {
    lyrics: Option<String>,
    subtitles: Option<String>,
}

/// TIDAL's lyrics in the shape the lyrics panel reads: LRC subtitles when
/// present (sorted, multi-stamp lines expanded), else plain lines.
fn lyrics(raw: RawLyrics) -> Option<crate::lyrics::Lyrics> {
    use crate::lyrics::{Line, Lyrics};
    if let Some(subtitles) = raw
        .subtitles
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        let lines: Vec<Line> = crate::lyrics::parse_lrc(subtitles)
            .into_iter()
            .filter(|line| !line.text.is_empty())
            .collect();
        if !lines.is_empty() {
            return Some(Lyrics {
                lines,
                synced: true,
                instrumental: false,
            });
        }
    }
    let plain = raw.lyrics.filter(|text| !text.trim().is_empty())?;
    let lines: Vec<Line> = plain
        .lines()
        .map(|line| Line {
            at_ms: None,
            text: line.trim().to_string(),
        })
        .collect();
    Some(Lyrics {
        lines,
        synced: false,
        instrumental: false,
    })
}

#[cfg(test)]
mod tests;

/// `tracks/{id}/credits`: `[{type, contributors: [{name, id?}]}]`.
pub(crate) fn parse_credits(json: &serde_json::Value) -> crate::model::TrackCredits {
    let roles = json
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let role = entry.get("type")?.as_str()?.trim().to_string();
            let contributors: Vec<_> = entry
                .get("contributors")?
                .as_array()?
                .iter()
                .filter_map(|person| {
                    let name = person.get("name")?.as_str()?.trim();
                    (!name.is_empty()).then(|| crate::api::models::ArtistRef {
                        id: person.get("id").and_then(|id| match id {
                            serde_json::Value::Number(number) => Some(number.to_string()),
                            serde_json::Value::String(text) if !text.is_empty() => {
                                Some(text.clone())
                            }
                            _ => None,
                        }),
                        name: name.to_string(),
                        ..Default::default()
                    })
                })
                .collect();
            (!role.is_empty() && !contributors.is_empty())
                .then_some(crate::model::Credit { role, contributors })
        })
        .collect();
    crate::model::TrackCredits {
        roles,
        ..Default::default()
    }
}

/// `albums/{id}/items/credits`: `{items: [{item: track, credits: [...]}],
/// totalNumberOfItems}`. A video among the items is left out.
pub(crate) fn parse_album_items_credits(
    json: &serde_json::Value,
) -> (Vec<crate::model::AlbumTrackCredits>, u32) {
    let items = json
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let tracks = items
        .iter()
        .filter(|entry| {
            entry
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|kind| kind.eq_ignore_ascii_case("track"))
        })
        .filter_map(|entry| {
            let track = entry.get("item")?;
            let roles = parse_credits(entry.get("credits")?).roles;
            let mut title = track.get("title")?.as_str()?.trim().to_string();
            if let Some(version) = track
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|version| !version.is_empty())
            {
                title = format!("{title} ({version})");
            }
            let number = |key: &str| {
                track
                    .get(key)
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32
            };
            Some(crate::model::AlbumTrackCredits {
                volume: number("volumeNumber").max(1),
                number: number("trackNumber"),
                title,
                roles,
                track: RawTrack::deserialize(track).ok().map(convert::track),
            })
        })
        .collect();
    let total = json
        .get("totalNumberOfItems")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(items.len() as u64) as u32;
    (tracks, total)
}

/// A review or biography answer, `{text, source}`, unless its text is empty.
fn bio_from(bio: &Value) -> Option<crate::api::bio::ArtistBio> {
    let text = bio["text"].as_str()?.trim();
    (!plain_bio(text).is_empty()).then(|| crate::api::bio::ArtistBio {
        text: text.to_string(),
        source: bio["source"]
            .as_str()
            .map(str::trim)
            .filter(|source| !source.is_empty())
            .map(str::to_string),
    })
}

/// The release year and the copyright line of a v1 track.
fn release_and_label(track: &serde_json::Value) -> (Option<String>, Option<String>) {
    let date = track
        .pointer("/album/releaseDate")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            track
                .get("streamStartDate")
                .and_then(serde_json::Value::as_str)
        });
    let year = date
        .and_then(|date| date.get(..4))
        .filter(|year| year.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string);
    let label = track
        .get("copyright")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string);
    (year, label)
}

/// A TIDAL id in a JSON body, which sends numbers and sometimes strings.
fn json_id(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(id) => Some(id.to_string()),
        serde_json::Value::String(id) if !id.is_empty() => Some(id.clone()),
        _ => None,
    }
}
