//! Self-update from the Tidalfast update host, through fastframe-update.
//!
//! Releases live on a private files.devalt.cloud resource. Each channel has
//! a signed manifest (`channels/<channel>/latest.json` plus a detached
//! Ed25519 signature, `latest.json.sig`, base64) that names the version and
//! every download with its size and SHA-256. Every request carries the
//! updater key the build was compiled with (`TIDALFAST_UPDATE_KEY`); a build
//! without one does not update itself.
//!
//! fastframe-update still does the work that matters on the machine:
//! detecting how this copy was installed (package-manager installs are never
//! replaced), verifying `checksums.txt` and its signature, staging, the
//! helper handoff and the rollback. It talks to GitHub releases, so
//! [`DevaltTransport`] answers its GitHub requests from the signed manifest
//! and the host's permanent download links. Nothing it fetches is trusted
//! before the manifest's signature checks out against the bundled key, and
//! every download is checked against the manifest's size and digest as it
//! streams, on top of the crate's own checksum check.
//!
//! The previous release's helper installs the first release built on the
//! crate, so what it relies on stays the same: `--version` prints
//! `<command> <version>`, the new app accepts `--update-receipt` and
//! `--update-error` (stripped by [`fastframe_update::intercept`] before the
//! argument parser), and the handoff and receipt files keep their format.

use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

pub use crate::settings::UpdateChannel;
pub use fastframe_update::{
    CHECK_INTERVAL, DownloadState, Installation, Kind, Prepared, Release, Source, Unsupported,
};
use fastframe_update::{MacConfig, Prereleases, Request, Response, Transport, UpdateConfig};

/// The update host. The updater key is only ever sent here, over HTTPS.
pub const UPDATE_HOST: &str = "files.devalt.cloud";
/// The resource every feed and download lives under.
const RESOURCE_BASE: &str = "https://files.devalt.cloud/api/res/cloud-guy/tidalfast";
/// The public releases page, shown as the release link. The update host's
/// own page is private, and a tag page would not exist for a dev build.
const RELEASE_PAGE: &str = "https://github.com/CIoudGuy/tidalfast/releases";
/// The header that carries the updater key.
const KEY_HEADER: &str = "X-Update-Key";
/// The header that carries the random install id (update host only).
const INSTALL_HEADER: &str = "X-Install-Id";
/// The header that carries the plain OS name (update host only).
const CLIENT_HEADER: &str = "X-Client";

/// `windows`, `linux` or `macos`, nothing more specific.
fn client_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

/// What fastframe-update believes is a GitHub repository. It only ever
/// reaches [`DevaltTransport`], which answers for it.
const REPOSITORY: &str = "tidalfast/tidalfast";

/// The bundled publisher key: the manifest and `checksums.txt` must be
/// signed with its private half.
const PUBLIC_KEY: &str = include_str!("../assets/update-public-key.hex");

pub const CONFIG: UpdateConfig = UpdateConfig {
    macos: MacConfig {
        bundle_ids: &["rocks.tidalfast.Tidalfast"],
        executable_names: &["Tidalfast"],
        legacy_bundle_names: &[],
    },
    // Releases publish an Ed25519 signature for checksums.txt, made with
    // the same key as the channel manifests.
    publisher_key: Some(PUBLIC_KEY),
    ..UpdateConfig::new(
        REPOSITORY,
        "Tidalfast",
        "tidalfast",
        env!("CARGO_PKG_VERSION"),
    )
};

/// The updater key this build was compiled with, if any. It comes from the
/// build environment and is never part of the source.
const BUILD_UPDATE_KEY: Option<&str> = option_env!("TIDALFAST_UPDATE_KEY");

/// Whether this build carries an update channel at all.
pub const SELF_UPDATE_ENABLED: bool = BUILD_UPDATE_KEY.is_some();

/// Shown when a build has no updater key.
pub const NO_UPDATE_CHANNEL: &str = "This build doesn't include an update channel";

fn update_key() -> Option<&'static str> {
    BUILD_UPDATE_KEY
        .map(str::trim)
        .filter(|key| !key.is_empty())
}

static CHANNEL: AtomicU8 = AtomicU8::new(0);

/// Follows `channel` from the next check or download on.
pub fn set_channel(channel: UpdateChannel) {
    let index = UpdateChannel::ALL
        .iter()
        .position(|candidate| *candidate == channel)
        .unwrap_or(0);
    CHANNEL.store(index as u8, AtomicOrdering::Relaxed);
}

/// The channel checks and downloads follow.
pub fn channel() -> UpdateChannel {
    UpdateChannel::ALL
        .get(usize::from(CHANNEL.load(AtomicOrdering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

static LATEST_NOTES: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The release notes of the last release a check found, with its version.
pub fn latest_notes() -> Option<(String, String)> {
    LATEST_NOTES.lock().ok()?.clone()
}

static LATEST_HISTORY: Mutex<Vec<HistoryEntry>> = Mutex::new(Vec::new());

/// The earlier releases' notes of the last release a check found, as its
/// manifest listed them (newest first); empty for an older feed.
pub fn latest_history() -> Vec<HistoryEntry> {
    LATEST_HISTORY
        .lock()
        .map(|history| history.clone())
        .unwrap_or_default()
}

/// The version the update dialog counts missed releases from.
#[cfg(any(test, feature = "demo"))]
static DEMO_RUNNING: Mutex<Option<String>> = Mutex::new(None);

pub fn running_version() -> String {
    #[cfg(any(test, feature = "demo"))]
    if let Some(version) = DEMO_RUNNING.lock().ok().and_then(|version| version.clone()) {
        return version;
    }
    env!("CARGO_PKG_VERSION").to_owned()
}

/// `--demo-show update`: pretends the running copy is `running` and a check
/// found `offered` with these notes behind it.
#[cfg(any(test, feature = "demo"))]
pub fn demo_offer(running: &str, offered: &str, notes: &str, history: Vec<HistoryEntry>) {
    if let Ok(mut value) = DEMO_RUNNING.lock() {
        *value = Some(running.to_owned());
    }
    if let Ok(mut value) = LATEST_NOTES.lock() {
        *value = Some((offered.to_owned(), notes.to_owned()));
    }
    if let Ok(mut value) = LATEST_HISTORY.lock() {
        *value = history;
    }
}

/// One release in the manifest's `history`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub version: String,
    #[serde(default)]
    pub date: String,
    pub notes: String,
}

/// The most releases, and the most bytes of notes per release, read from a
/// manifest's `history`.
const HISTORY_ENTRIES: usize = 50;
const HISTORY_NOTES_LIMIT: usize = 32 * 1024;

/// Reads the optional `history` leniently: an entry that is malformed, has an
/// invalid version or oversized notes is skipped, and a `history` that is not
/// a list reads as empty, so it can never make a manifest unusable.
fn lenient_history<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<HistoryEntry>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let serde_json::Value::Array(items) = value else {
        return Ok(Vec::new());
    };
    Ok(items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<HistoryEntry>(item).ok())
        .filter(|entry| {
            Semver::parse(&entry.version).is_some() && entry.notes.len() <= HISTORY_NOTES_LIMIT
        })
        .take(HISTORY_ENTRIES)
        .collect())
}

/// What the update dialog lists: every release newer than `current` up to
/// `offered`, newest first. The manifest's own `notes` stand for the offered
/// release when the history lacks it, so an older feed (no history) gives
/// that release's notes alone.
pub fn missed_notes(
    offered: &str,
    notes: &str,
    history: &[HistoryEntry],
    current: &str,
) -> Vec<HistoryEntry> {
    let (Some(top), Some(floor)) = (Semver::parse(offered), Semver::parse(current)) else {
        return Vec::new();
    };
    let mut missed: Vec<(Semver, HistoryEntry)> = history
        .iter()
        .filter_map(|entry| Some((Semver::parse(&entry.version)?, entry.clone())))
        .filter(|(version, _)| *version > floor && *version <= top)
        .collect();
    missed.sort_by(|a, b| b.0.cmp(&a.0));
    missed.dedup_by(|a, b| a.0 == b.0);
    if !missed.iter().any(|(version, _)| *version == top) && !notes.trim().is_empty() {
        missed.insert(
            0,
            (
                top,
                HistoryEntry {
                    version: offered.to_owned(),
                    date: String::new(),
                    notes: notes.to_owned(),
                },
            ),
        );
    }
    missed.into_iter().map(|(_, entry)| entry).collect()
}

// ---- the signed manifest ---------------------------------------------------

/// A channel's signed `latest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub notes: String,
    /// Earlier releases' notes, newest first, so the dialog can show every
    /// release a person skipped. Optional: older feeds have none.
    #[serde(default, deserialize_with = "lenient_history")]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub published_at: String,
    pub assets: Vec<ManifestAsset>,
}

/// One download of a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestAsset {
    /// `windows-x86_64`, `windows-x86_64-installer`, `linux-x86_64-tar`,
    /// `linux-x86_64-appimage`, `macos-universal`, `checksums`, …
    pub platform: String,
    pub filename: String,
    /// The permanent path relative to the resource: `v/<version>/<filename>`.
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// The largest manifest accepted, the host's own limit.
const MANIFEST_LIMIT: u64 = 256 * 1024;
/// The largest signature file accepted.
const SIGNATURE_LIMIT: u64 = 4096;

fn decode_public_key(hex: &str) -> Result<[u8; 32]> {
    let hex = hex.trim();
    ensure!(
        hex.len() == 64 && hex.is_ascii(),
        "Invalid embedded update public key"
    );
    let mut key = [0; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .context("Invalid embedded update public key")?;
    }
    Ok(key)
}

/// The detached signature: base64 text (as the host stores it) or the raw
/// 64 bytes.
fn decode_signature(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() == 64 {
        return Ok(bytes.to_vec());
    }
    let text = std::str::from_utf8(bytes).context("Invalid update signature")?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .context("Invalid update signature")?;
    ensure!(signature.len() == 64, "Invalid update signature");
    Ok(signature)
}

/// Checks the signature over the exact manifest bytes, then parses and
/// validates the manifest for `channel`.
pub fn verify_manifest(
    bytes: &[u8],
    signature: &[u8],
    public_key: &[u8; 32],
    channel: UpdateChannel,
) -> Result<Manifest> {
    let signature = decode_signature(signature)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(bytes, &signature)
        .map_err(|_| anyhow::anyhow!("The update manifest's signature could not be verified"))?;
    let manifest: Manifest = serde_json::from_slice(bytes).context("Unexpected update manifest")?;
    ensure!(
        manifest.channel == channel.as_str(),
        "The update manifest belongs to another channel"
    );
    ensure!(
        Semver::parse(&manifest.version).is_some(),
        "The update manifest has an invalid version"
    );
    for asset in &manifest.assets {
        ensure!(
            safe_name(&asset.filename),
            "The update manifest names an invalid file"
        );
        ensure!(
            asset_url_allowed(&asset.url, &manifest.version, &asset.filename),
            "The update manifest points outside its release"
        );
        ensure!(
            asset.sha256.len() == 64 && asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "The update manifest has an invalid checksum"
        );
        ensure!(asset.size > 0, "The update manifest has an empty download");
    }
    let mut names: Vec<_> = manifest.assets.iter().map(|a| &a.filename).collect();
    names.sort();
    names.dedup();
    ensure!(
        names.len() == manifest.assets.len(),
        "The update manifest lists a file twice"
    );
    Ok(manifest)
}

/// An asset's `url`: the host's permanent path for this release and file
/// (`v/<version>/<filename>`), or an absolute HTTPS address (a mirror). The
/// signed size and digest are checked either way.
fn asset_url_allowed(url: &str, version: &str, filename: &str) -> bool {
    if url.starts_with("https://") {
        return Url::parse(url).is_ok_and(|parsed| {
            parsed.scheme() == "https"
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.fragment().is_none()
        });
    }
    url == format!("v/{version}/{filename}")
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

// ---- versions --------------------------------------------------------------

/// A semantic version without build metadata, ordered by semver precedence.
#[derive(Debug, PartialEq, Eq)]
pub struct Semver {
    core: [u64; 3],
    pre: Vec<Identifier>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Numeric(u64),
    Alphanumeric(String),
}

impl Semver {
    /// `major.minor.patch[-pre.release]`, numbers without leading zeros.
    pub fn parse(version: &str) -> Option<Self> {
        if version.len() > 64 {
            return None;
        }
        let (core, pre) = match version.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (version, None),
        };
        let number = |part: &str| {
            (!part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0')))
            .then(|| part.parse::<u64>().ok())
            .flatten()
        };
        let mut parts = core.split('.');
        let core = [
            number(parts.next()?)?,
            number(parts.next()?)?,
            number(parts.next()?)?,
        ];
        if parts.next().is_some() {
            return None;
        }
        let mut identifiers = Vec::new();
        if let Some(pre) = pre {
            for part in pre.split('.') {
                if part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                {
                    return None;
                }
                identifiers.push(if part.bytes().all(|byte| byte.is_ascii_digit()) {
                    Identifier::Numeric(number(part)?)
                } else {
                    Identifier::Alphanumeric(part.to_owned())
                });
            }
        }
        Some(Self {
            core,
            pre: identifiers,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

/// Whether `candidate` should be offered to a copy running `current` on
/// `channel`: strictly newer by semver precedence, and on the stable channel
/// never a pre-release.
pub fn is_offered(candidate: &str, current: &str, channel: UpdateChannel) -> bool {
    match (Semver::parse(candidate), Semver::parse(current)) {
        (Some(candidate), Some(current)) => {
            (channel != UpdateChannel::Stable || !candidate.is_prerelease()) && candidate > current
        }
        _ => false,
    }
}

// ---- platforms -------------------------------------------------------------

/// The manifest platform prefix for an operating system and processor:
/// `windows-x86_64`, `linux-aarch64`, `macos-universal`.
pub fn platform_prefix(os: &str, arch: &str) -> Option<String> {
    match (os, arch) {
        ("windows" | "linux", "x86_64" | "aarch64") => Some(format!("{os}-{arch}")),
        ("macos", "x86_64" | "aarch64") => Some("macos-universal".into()),
        _ => None,
    }
}

/// The downloads in `manifest` meant for this operating system and
/// processor (a portable archive, an installer, an AppImage, …).
pub fn platform_assets<'a>(manifest: &'a Manifest, os: &str, arch: &str) -> Vec<&'a ManifestAsset> {
    let Some(prefix) = platform_prefix(os, arch) else {
        return Vec::new();
    };
    manifest
        .assets
        .iter()
        .filter(|asset| {
            asset.platform == prefix
                || asset
                    .platform
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
        .collect()
}

// ---- the host --------------------------------------------------------------

/// One GET the update code needs answered. Redirects are never followed by
/// the client; `Host` follows them itself.
pub struct HostRequest<'a> {
    pub url: &'a str,
    pub accept: &'a str,
    pub user_agent: &'a str,
    /// Sent as `X-Update-Key`; set only for the update host over HTTPS.
    pub update_key: Option<&'a str>,
    /// Sent as `X-Install-Id`; set only where the key is.
    pub install_id: Option<&'a str>,
    /// Sent as `X-Client`; set only where the key is.
    pub client: Option<&'a str>,
    /// Sent as `If-None-Match`.
    pub if_none_match: Option<&'a str>,
    /// Sent as `Range: bytes=<n>-`, to resume a download.
    pub range_from: Option<u64>,
}

/// What the server answered.
pub struct HostResponse {
    pub status: u16,
    pub location: Option<String>,
    pub etag: Option<String>,
    /// The first byte of a `206 Partial Content` answer.
    pub range_start: Option<u64>,
    /// `Retry-After`, in seconds.
    pub retry_after: Option<Duration>,
    pub body: Box<dyn Read + Send>,
}

impl HostResponse {
    pub fn new(status: u16, body: impl Read + Send + 'static) -> Self {
        Self {
            status,
            location: None,
            etag: None,
            range_start: None,
            retry_after: None,
            body: Box::new(body),
        }
    }
}

/// The HTTP client, so tests can stand in for the host.
pub trait Fetch: Send + Sync {
    /// Sends one GET and returns the answer, whatever its status. A
    /// connection failure or timeout is an `Err`.
    fn get(&self, request: &HostRequest<'_>) -> Result<HostResponse>;
}

struct ReqwestFetch(reqwest::blocking::Client);

impl Fetch for ReqwestFetch {
    fn get(&self, request: &HostRequest<'_>) -> Result<HostResponse> {
        use reqwest::header;
        let mut builder = self
            .0
            .get(request.url)
            .header(header::ACCEPT, request.accept)
            .header(header::USER_AGENT, request.user_agent);
        if let Some(key) = request.update_key {
            builder = builder.header(KEY_HEADER, key);
        }
        if let Some(id) = request.install_id {
            builder = builder.header(INSTALL_HEADER, id);
        }
        if let Some(client) = request.client {
            builder = builder.header(CLIENT_HEADER, client);
        }
        if let Some(etag) = request.if_none_match {
            builder = builder.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(from) = request.range_from {
            builder = builder.header(header::RANGE, format!("bytes={from}-"));
        }
        let response = builder.send()?;
        let text = |name: header::HeaderName| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let range_start = text(header::CONTENT_RANGE).and_then(|range| {
            range
                .strip_prefix("bytes ")?
                .split_once('-')?
                .0
                .trim()
                .parse()
                .ok()
        });
        Ok(HostResponse {
            status: response.status().as_u16(),
            location: text(header::LOCATION),
            etag: text(header::ETAG),
            range_start,
            retry_after: text(header::RETRY_AFTER)
                .and_then(|value| value.trim().parse().ok())
                .map(Duration::from_secs),
            body: Box::new(response),
        })
    }
}

/// The host answered 404: no such file, or a key it does not accept (a
/// private resource looks empty without one).
#[derive(Debug)]
pub struct NotFound;

impl std::fmt::Display for NotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("The update server has no such file")
    }
}

impl std::error::Error for NotFound {}

/// Nothing is published on the channel yet (the host answered 404). That is
/// not a failure: there is simply no newer release.
#[derive(Debug)]
pub struct NotPublished;

impl std::fmt::Display for NotPublished {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Nothing is published on the update channel yet")
    }
}

impl std::error::Error for NotPublished {}

/// The manifest did not verify against this build's update key.
#[derive(Debug)]
pub struct KeyRejected;

/// What a failed check says when the key was rejected; the app shows its
/// own wording for it.
pub const KEY_REJECTED: &str = "This build's update key was rejected";

impl std::fmt::Display for KeyRejected {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(KEY_REJECTED)
    }
}

impl std::error::Error for KeyRejected {}

/// The first wait after a busy answer (429, 5xx) or a failed connection.
const BACKOFF_BASE: Duration = Duration::from_secs(60);
/// The longest wait, however often the server fails.
const BACKOFF_CAP: Duration = Duration::from_secs(6 * 60 * 60);
/// The longest `Retry-After` honoured.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(24 * 60 * 60);

/// Exponential backoff shared by every request to the update host: after
/// a failure nothing is sent until the wait is over, so a struggling origin
/// never sees tight retries.
#[derive(Debug, Default)]
pub struct Backoff(Mutex<BackoffState>);

#[derive(Debug, Default)]
struct BackoffState {
    failures: u32,
    not_before: Option<Instant>,
}

impl Backoff {
    /// The wait after `failures` failures in a row, before jitter.
    pub fn delay(failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1).min(16);
        BACKOFF_BASE.saturating_mul(1 << doublings).min(BACKOFF_CAP)
    }

    /// Whether a request may go out at `now`.
    pub fn ready(&self, now: Instant) -> Result<()> {
        let state = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Update state is unavailable"))?;
        if let Some(not_before) = state.not_before
            && now < not_before
        {
            let minutes = (not_before - now).as_secs().div_ceil(60).max(1);
            bail!(
                "The update server is busy. Tidalfast will try again in about {minutes} minute{}.",
                if minutes == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }

    /// Records a failure at `now`. `jitter` in `[0, 1)` adds up to 20%.
    pub fn failure(&self, now: Instant, retry_after: Option<Duration>, jitter: f64) {
        if let Ok(mut state) = self.0.lock() {
            state.failures = state.failures.saturating_add(1);
            let delay = Self::delay(state.failures).mul_f64(1.0 + 0.2 * jitter.clamp(0.0, 1.0));
            let delay = retry_after
                .map(|after| after.min(RETRY_AFTER_CAP))
                .map_or(delay, |after| delay.max(after));
            state.not_before = Some(now + delay);
        }
    }

    pub fn success(&self) {
        if let Ok(mut state) = self.0.lock() {
            *state = BackoffState::default();
        }
    }
}

/// A random number in `[0, 1)`.
pub fn random_unit() -> f64 {
    use ring::rand::SecureRandom;
    let mut bytes = [0; 8];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
        return 0.5;
    }
    (u64::from_le_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64
}

/// A manifest the host sent with an ETag, to ask again with
/// `If-None-Match`.
#[derive(Clone, Debug)]
struct CachedManifest {
    etag: String,
    bytes: Vec<u8>,
    signature: Vec<u8>,
}

/// What survives from one check to the next: the backoff and the last
/// manifest of each channel.
#[derive(Debug, Default)]
pub struct HostState {
    backoff: Backoff,
    manifests: Mutex<std::collections::HashMap<String, CachedManifest>>,
}

fn host_state() -> Arc<HostState> {
    static STATE: OnceLock<Arc<HostState>> = OnceLock::new();
    STATE.get_or_init(Arc::default).clone()
}

/// An answer the update code can use.
struct Fetched {
    status: u16,
    /// Where the body came from, after redirects.
    url: String,
    etag: Option<String>,
    range_start: Option<u64>,
    body: Box<dyn Read + Send>,
}

/// The update host and where it redirects: the key only ever goes to the
/// update host itself, every request respects the backoff.
#[derive(Clone)]
struct Host {
    http: Arc<dyn Fetch>,
    key: String,
    install_id: Option<String>,
    state: Arc<HostState>,
    /// How long a resumed download waits before each new attempt.
    resume_pause: Duration,
}

impl Host {
    fn key_for(&self, url: &Url) -> Option<&str> {
        (url.scheme() == "https" && url.host_str() == Some(UPDATE_HOST) && url.port().is_none())
            .then_some(self.key.as_str())
    }

    /// The install id goes wherever the key does and nowhere else.
    fn install_id_for(&self, url: &Url) -> Option<&str> {
        self.key_for(url).and(self.install_id.as_deref())
    }

    /// GETs `url`, following at most five redirects. 2xx and 304 are
    /// answers; 404 is [`NotFound`]; 429, 5xx and failed connections start
    /// or extend the backoff.
    fn get(
        &self,
        url: &str,
        accept: &str,
        user_agent: &str,
        if_none_match: Option<&str>,
        range_from: Option<u64>,
    ) -> Result<Fetched> {
        let mut current = Url::parse(url).context("Invalid update address")?;
        for _ in 0..=5 {
            ensure!(
                current.scheme() == "https"
                    && current.username().is_empty()
                    && current.password().is_none(),
                "Update redirect is not allowed"
            );
            self.state.backoff.ready(Instant::now())?;
            let answer = self.http.get(&HostRequest {
                url: current.as_str(),
                accept,
                user_agent,
                update_key: self.key_for(&current),
                install_id: self.install_id_for(&current),
                client: self.key_for(&current).map(|_| client_name()),
                if_none_match,
                range_from,
            });
            let response = match answer {
                Ok(response) => response,
                Err(error) => {
                    self.state
                        .backoff
                        .failure(Instant::now(), None, random_unit());
                    return Err(error.context("Could not reach the update server"));
                }
            };
            match response.status {
                200..=299 | 304 => {
                    self.state.backoff.success();
                    return Ok(Fetched {
                        status: response.status,
                        url: current.to_string(),
                        etag: response.etag,
                        range_start: response.range_start,
                        body: response.body,
                    });
                }
                300..=399 => {
                    let location = response
                        .location
                        .context("The update server sent a redirect without a location")?;
                    current = current
                        .join(&location)
                        .context("The update server sent an invalid redirect")?;
                }
                404 => {
                    self.state.backoff.success();
                    return Err(NotFound.into());
                }
                status @ (429 | 500..=599) => {
                    self.state
                        .backoff
                        .failure(Instant::now(), response.retry_after, random_unit());
                    bail!("The update server is busy ({status}). Try again later.");
                }
                status => bail!("The update server answered {status}"),
            }
        }
        bail!("The update server redirected too many times")
    }
}

/// A download that picks up where it stopped (`Range`) when the connection
/// drops, a few times, with a growing pause in between.
struct Resumable {
    host: Host,
    url: String,
    accept: String,
    user_agent: String,
    body: Box<dyn Read + Send>,
    received: u64,
    attempts: u32,
}

const RESUME_ATTEMPTS: u32 = 3;

impl Read for Resumable {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.body.read(buffer) {
                Ok(count) => {
                    self.received += count as u64;
                    return Ok(count);
                }
                Err(error)
                    if error.kind() != std::io::ErrorKind::InvalidData
                        && self.attempts < RESUME_ATTEMPTS =>
                {
                    self.attempts += 1;
                    std::thread::sleep(self.host.resume_pause * (1 << self.attempts));
                    log::info!(
                        "update download interrupted at {} bytes ({error}); resuming",
                        self.received
                    );
                    let fetched = self
                        .host
                        .get(
                            &self.url,
                            &self.accept,
                            &self.user_agent,
                            None,
                            Some(self.received),
                        )
                        .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
                    if fetched.status != 206 || fetched.range_start != Some(self.received) {
                        return Err(std::io::Error::other(
                            "The update download was interrupted and can't be resumed",
                        ));
                    }
                    self.body = fetched.body;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// Answers fastframe-update's GitHub requests from the update host.
pub struct DevaltTransport {
    host: Host,
    channel: UpdateChannel,
    public_key: [u8; 32],
    base: String,
    /// The manifest the last request verified.
    manifest: Mutex<Option<Manifest>>,
    /// Where verified downloads are kept, `<version>/<filename>`.
    kept: Option<PathBuf>,
}

impl DevaltTransport {
    pub fn new(
        http: impl Fetch + 'static,
        key: &str,
        channel: UpdateChannel,
        public_key: [u8; 32],
        state: Arc<HostState>,
    ) -> Self {
        Self {
            host: Host {
                http: Arc::new(http),
                key: key.to_owned(),
                install_id: None,
                state,
                resume_pause: Duration::from_secs(1),
            },
            channel,
            public_key,
            base: RESOURCE_BASE.to_owned(),
            manifest: Mutex::new(None),
            kept: None,
        }
    }

    /// Sends this anonymous install id to the update host with every request.
    pub fn with_install_id(mut self, id: Option<String>) -> Self {
        self.host.install_id = id;
        self
    }

    /// Keeps each verified download in `dir`. fastframe-update stages every
    /// download afresh and never installs one an earlier run staged, so
    /// without this a restart before installing downloads the release again.
    pub fn with_kept_downloads(mut self, dir: PathBuf) -> Self {
        self.kept = Some(dir);
        self
    }

    fn kept_path(&self, version: &str, asset: &ManifestAsset) -> Option<PathBuf> {
        let dir = self.kept.as_ref()?;
        (safe_name(version) && safe_name(&asset.filename))
            .then(|| dir.join(version).join(&asset.filename))
    }

    /// The download kept for `asset`, when it still has the signed size and
    /// digest. Anything else there is removed. The bytes are checked again
    /// as they are read, like a download.
    fn kept_download(&self, version: &str, asset: &ManifestAsset) -> Option<File> {
        let path = self.kept_path(version, asset)?;
        let size = fs::metadata(&path).ok()?.len();
        if size == asset.size
            && file_sha256(&path).is_ok_and(|digest| digest == asset.sha256.to_ascii_lowercase())
            && let Ok(file) = File::open(&path)
        {
            return Some(file);
        }
        let _ = fs::remove_file(&path);
        None
    }

    /// Where a fresh download of `asset` is written as it streams. Kept
    /// downloads of other versions go first: only one release is waiting.
    fn keeper(&self, version: &str, asset: &ManifestAsset) -> Option<(File, PathBuf, PathBuf)> {
        let path = self.kept_path(version, asset)?;
        let dir = path.parent()?;
        if let Some(root) = dir.parent()
            && let Ok(entries) = fs::read_dir(root)
        {
            for entry in entries.flatten() {
                if entry.file_name().to_str() != Some(version) {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        fs::create_dir_all(dir).ok()?;
        let part = dir.join(format!("{}.part", asset.filename));
        let file = File::create(&part).ok()?;
        Some((file, part, path))
    }

    fn read_limited(&self, mut body: Box<dyn Read + Send>, limit: u64) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        (&mut body)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .context("Could not read the update manifest")?;
        ensure!(
            bytes.len() as u64 <= limit,
            "The update manifest is too large"
        );
        Ok(bytes)
    }

    /// The channel's manifest, signature verified. Asks with the last
    /// ETag; `304 Not Modified` reuses the manifest already held.
    pub fn latest(&self, user_agent: &str) -> Result<Manifest> {
        let channel = self.channel.as_str();
        let url = format!("{}/channels/{channel}/latest.json", self.base);
        let not_published = |error: anyhow::Error| {
            if error.is::<NotFound>() {
                anyhow::Error::new(NotPublished)
            } else {
                error
            }
        };
        let cached = self
            .host
            .state
            .manifests
            .lock()
            .ok()
            .and_then(|cached| cached.get(channel).cloned());
        let fetched = self
            .host
            .get(
                &url,
                "application/json",
                user_agent,
                cached.as_ref().map(|cached| cached.etag.as_str()),
                None,
            )
            .map_err(not_published)?;
        let (bytes, signature) = match (fetched.status, cached) {
            (304, Some(cached)) => (cached.bytes, cached.signature),
            (304, None) => bail!("The update server answered 304 to a fresh request"),
            _ => {
                let etag = fetched.etag;
                let bytes = self.read_limited(fetched.body, MANIFEST_LIMIT)?;
                let signature = self.host.get(
                    &format!("{url}.sig"),
                    "application/octet-stream",
                    user_agent,
                    None,
                    None,
                );
                let signature =
                    self.read_limited(signature.map_err(not_published)?.body, SIGNATURE_LIMIT)?;
                if let Some(etag) = etag
                    && let Ok(mut manifests) = self.host.state.manifests.lock()
                {
                    manifests.insert(
                        channel.to_owned(),
                        CachedManifest {
                            etag,
                            bytes: bytes.clone(),
                            signature: signature.clone(),
                        },
                    );
                }
                (bytes, signature)
            }
        };
        let manifest = verify_manifest(&bytes, &signature, &self.public_key, self.channel)
            .map_err(|_| anyhow::Error::new(KeyRejected))?;
        if let Ok(mut held) = self.manifest.lock() {
            *held = Some(manifest.clone());
        }
        Ok(manifest)
    }

    fn manifest_for(&self, version: &str, user_agent: &str) -> Result<Manifest> {
        let held = self.manifest.lock().ok().and_then(|held| held.clone());
        let manifest = match held {
            Some(manifest) if manifest.version == version => manifest,
            _ => self.latest(user_agent)?,
        };
        ensure!(
            manifest.version == version,
            "The release changed. Check for updates again."
        );
        Ok(manifest)
    }

    /// Where an asset is downloaded from: the host's permanent link, or the
    /// manifest's absolute mirror address.
    fn asset_address(&self, asset: &ManifestAsset) -> String {
        if asset.url.starts_with("https://") {
            asset.url.clone()
        } else {
            format!("{}/{}", self.base, asset.url)
        }
    }

    fn json(value: &serde_json::Value) -> Response {
        Response::ok(std::io::Cursor::new(value.to_string().into_bytes()))
    }

    fn github_asset_url(version: &str, name: &str) -> String {
        format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}")
    }

    fn metadata(manifest: &Manifest) -> serde_json::Value {
        let assets: Vec<_> = manifest
            .assets
            .iter()
            .map(|asset| {
                serde_json::json!({
                    "name": asset.filename,
                    "size": asset.size,
                    "browser_download_url":
                        Self::github_asset_url(&manifest.version, &asset.filename),
                })
            })
            .collect();
        serde_json::json!({
            "tag_name": format!("v{}", manifest.version),
            "html_url": RELEASE_PAGE,
            "draft": false,
            "prerelease": false,
            "body": manifest.notes,
            "assets": assets,
        })
    }
}

impl Transport for DevaltTransport {
    fn get(&self, request: &Request<'_>) -> Result<Response> {
        let not_found = || Response {
            status: 404,
            location: None,
            body: Box::new(std::io::empty()),
        };
        let api = format!("https://api.github.com/repos/{REPOSITORY}/releases");
        let downloads = format!("https://github.com/{REPOSITORY}/releases/download/v");
        let url = request.url;
        if url == format!("{api}/latest") {
            let manifest = self.latest(request.user_agent)?;
            return Ok(Self::json(&serde_json::json!({
                "tag_name": format!("v{}", manifest.version),
                "html_url": RELEASE_PAGE,
            })));
        }
        if url.starts_with(&format!("{api}?")) {
            let manifest = self.latest(request.user_agent)?;
            return Ok(Self::json(&serde_json::Value::Array(vec![Self::metadata(
                &manifest,
            )])));
        }
        if let Some(version) = url.strip_prefix(&format!("{api}/tags/v")) {
            let manifest = self.manifest_for(version, request.user_agent)?;
            return Ok(Self::json(&Self::metadata(&manifest)));
        }
        if let Some(rest) = url.strip_prefix(&downloads) {
            let Some((version, name)) = rest.split_once('/') else {
                return Ok(not_found());
            };
            let manifest = self.manifest_for(version, request.user_agent)?;
            let Some(asset) = manifest.assets.iter().find(|asset| asset.filename == name) else {
                return Ok(not_found());
            };
            if let Some(file) = self.kept_download(&manifest.version, asset) {
                log::info!(
                    "update: {} was downloaded before, not again",
                    asset.filename
                );
                return Ok(Response::ok(Verified::new(Box::new(file), asset)));
            }
            let fetched = self.host.get(
                &self.asset_address(asset),
                request.accept,
                request.user_agent,
                None,
                None,
            )?;
            let body = Resumable {
                host: self.host.clone(),
                url: fetched.url,
                accept: request.accept.to_owned(),
                user_agent: request.user_agent.to_owned(),
                body: fetched.body,
                received: 0,
                attempts: 0,
            };
            let verified = Verified::new(Box::new(body), asset);
            return Ok(match self.keeper(&manifest.version, asset) {
                Some((file, part, path)) => Response::ok(Kept {
                    inner: verified,
                    file: Some(file),
                    part,
                    path,
                }),
                None => Response::ok(verified),
            });
        }
        Ok(not_found())
    }
}

/// A download checked against the manifest as it streams: never more than
/// the listed size, and at the end exactly that size and digest.
pub struct Verified {
    inner: Box<dyn Read + Send>,
    hash: Sha256,
    received: u64,
    size: u64,
    sha256: String,
    done: bool,
}

impl Verified {
    fn new(inner: Box<dyn Read + Send>, asset: &ManifestAsset) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
            received: 0,
            size: asset.size,
            sha256: asset.sha256.to_ascii_lowercase(),
            done: false,
        }
    }
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_owned())
}

impl Read for Verified {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        let count = self.inner.read(buffer)?;
        if count == 0 {
            self.done = true;
            if self.received != self.size {
                return Err(invalid("The update download was interrupted"));
            }
            let digest = std::mem::take(&mut self.hash).finalize();
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            if hex != self.sha256 {
                return Err(invalid(
                    "The download couldn't be verified. Try downloading it again.",
                ));
            }
            return Ok(0);
        }
        self.received += count as u64;
        if self.received > self.size {
            return Err(invalid("Update download exceeds its published size"));
        }
        self.hash.update(&buffer[..count]);
        Ok(count)
    }
}

/// A download written to the kept-downloads folder as it streams. The file
/// takes its name only once [`Verified`] has checked the whole download;
/// an error or an unfinished download leaves nothing behind.
struct Kept {
    inner: Verified,
    file: Option<File>,
    part: PathBuf,
    path: PathBuf,
}

impl Kept {
    fn abandon(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.part);
        }
    }
}

impl Read for Kept {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = match self.inner.read(buffer) {
            Ok(count) => count,
            Err(error) => {
                self.abandon();
                return Err(error);
            }
        };
        if count == 0 {
            if let Some(file) = self.file.take() {
                let kept = file.sync_all().is_ok() && {
                    drop(file);
                    fs::rename(&self.part, &self.path).is_ok()
                };
                if !kept {
                    let _ = fs::remove_file(&self.part);
                }
            }
            return Ok(0);
        }
        if let Some(file) = self.file.as_mut()
            && file.write_all(&buffer[..count]).is_err()
        {
            self.abandon();
        }
        Ok(count)
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        self.abandon();
    }
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

// ---- leftovers -------------------------------------------------------------

/// How long an update that was handed to the helper but has no result may
/// still be installing.
const UNFINISHED_UPDATE: Duration = Duration::from_secs(24 * 60 * 60);

/// At startup, in the only running copy: removes what updates left behind.
/// Kept downloads of this version or older go, and so do the staging
/// folders beside the app that no install needs any more. `receipt` is the
/// `handoff.json` of the update that just relaunched this copy; its folder
/// stays until the next start, while the helper may still use it.
pub fn tidy(dirs: &crate::paths::AppDirs, receipt: Option<&Path>) {
    prune_kept(&dirs.update_cache_dir(), env!("CARGO_PKG_VERSION"));
    // An AppImage is replaced in place and a macOS bundle stages beside the
    // bundle, not the executable; neither leaves folders here.
    if cfg!(target_os = "macos") || crate::appimage::current().is_some() {
        return;
    }
    let Some(dir) = std::env::current_exe()
        .and_then(|executable| executable.canonicalize())
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
    else {
        return;
    };
    for folder in tidy_staging(&dir, receipt, std::time::SystemTime::now()) {
        log::info!(
            "update: removed the finished update folder {}",
            folder.display()
        );
    }
}

/// Removes the kept downloads of `current` and older versions.
fn prune_kept(dir: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let current = Semver::parse(current);
    for entry in entries.flatten() {
        let newer = entry
            .file_name()
            .to_str()
            .and_then(Semver::parse)
            .zip(current.as_ref())
            .is_some_and(|(version, current)| version > *current);
        if !newer {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Whether `name` is one of fastframe-update's staging folders for Tidalfast.
fn is_staging_folder(name: &str) -> bool {
    name.strip_prefix(".tidalfast-update-")
        .is_some_and(|suffix| {
            suffix.len() == 16
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// Removes the staging folders in `dir` that nothing needs: an update that
/// finished (it has `result.txt`), a download that was never installed (no
/// `handoff.json`: what a download prepares lives only in the run that made
/// it), and a handoff without a result older than a day. The receipt's
/// folder and any folder with a mounted disk image stay. Returns the
/// folders removed.
fn tidy_staging(dir: &Path, receipt: Option<&Path>, now: std::time::SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let receipt_folder = receipt.and_then(Path::parent).map(|folder| {
        folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf())
    });
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let folder = entry.path();
        if !entry.file_name().to_str().is_some_and(is_staging_folder)
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let canonical = folder.canonicalize().unwrap_or_else(|_| folder.clone());
        if receipt_folder.as_ref() == Some(&canonical) {
            continue;
        }
        // A download this run started while the tidy ran is no leftover:
        // removing it failed that download.
        if entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified > now)
        {
            continue;
        }
        let mounted = fs::read_dir(&folder).is_ok_and(|files| {
            files
                .flatten()
                .any(|file| file.file_name().to_string_lossy().starts_with("mounted-"))
        });
        if mounted {
            continue;
        }
        let job = folder.join("handoff.json");
        let unneeded = folder.join("result.txt").exists()
            || !job.exists()
            || fs::metadata(&job)
                .and_then(|job| job.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age > UNFINISHED_UPDATE)
                });
        if unneeded && fs::remove_dir_all(&folder).is_ok() {
            removed.push(folder);
        }
    }
    removed
}

// ---- when to check ---------------------------------------------------------

/// The first automatic check runs 5 to 30 seconds after startup, so a new
/// release is found on launch without every copy asking the host at once.
const STARTUP_DELAY_MIN: Duration = Duration::from_secs(5);
const STARTUP_DELAY_SPREAD: Duration = Duration::from_secs(25);

/// How long after startup the first automatic check waits, `unit` in `[0, 1)`.
pub fn first_check_delay(unit: f64) -> Duration {
    STARTUP_DELAY_MIN + STARTUP_DELAY_SPREAD.mul_f64(unit.clamp(0.0, 1.0))
}

/// `interval` moved by up to ±20%, `unit` in `[0, 1)`.
pub fn jittered(interval: Duration, unit: f64) -> Duration {
    interval.mul_f64(0.8 + 0.4 * unit.clamp(0.0, 1.0))
}

/// How long automatic checks stay quiet after `failures` failed ones in a
/// row: an hour, doubling to a day, jittered, so a broken host is never
/// asked in a loop.
pub fn retry_delay(failures: u32, unit: f64) -> Duration {
    let hour = Duration::from_secs(60 * 60);
    let base = hour.saturating_mul(1u32 << failures.saturating_sub(1).min(5));
    jittered(base.min(Duration::from_secs(24 * 60 * 60)), unit)
}

/// Whether the automatic check is due, given when the last one started.
/// The first waits a random 5–30 seconds after startup and each later one
/// [`CHECK_INTERVAL`] ±20%, so copies do not all ask the host at once.
pub fn check_due(last_check: Option<Instant>) -> bool {
    struct Schedule {
        first: Instant,
        interval: Option<(Instant, Duration)>,
    }
    static SCHEDULE: Mutex<Option<Schedule>> = Mutex::new(None);
    let Ok(mut schedule) = SCHEDULE.lock() else {
        return last_check.is_none_or(|at| at.elapsed() >= CHECK_INTERVAL);
    };
    let schedule = schedule.get_or_insert_with(|| Schedule {
        // Tests drive the check directly and expect it at once.
        first: Instant::now()
            + if cfg!(test) {
                Duration::ZERO
            } else {
                first_check_delay(random_unit())
            },
        interval: None,
    });
    match last_check {
        None => Instant::now() >= schedule.first,
        Some(at) => {
            let interval = match schedule.interval {
                Some((started, interval)) if started == at => interval,
                _ => {
                    let interval = jittered(CHECK_INTERVAL, random_unit());
                    schedule.interval = Some((at, interval));
                    interval
                }
            };
            at.elapsed() >= interval
        }
    }
}

// ---- the app's handle ------------------------------------------------------

struct Shared(Arc<DevaltTransport>);

impl Transport for Shared {
    fn get(&self, request: &Request<'_>) -> Result<Response> {
        self.0.get(request)
    }
}

/// The running version, as a pre-release when it is not one: lets
/// fastframe-update download and stage a beta or dev release, which it only
/// does on its pre-release channel. Only used while downloading.
fn download_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let current = env!("CARGO_PKG_VERSION");
        if Semver::parse(current).is_some_and(|version| version.is_prerelease()) {
            current.to_owned()
        } else {
            format!("{current}-0")
        }
    })
}

fn config_for(version: &str) -> UpdateConfig {
    if Semver::parse(version).is_some_and(|version| version.is_prerelease()) {
        UpdateConfig {
            prereleases: Prereleases::WhenRunningPrerelease,
            current_version: download_version(),
            ..CONFIG
        }
    } else {
        CONFIG
    }
}

/// The configuration `fastframe_update::intercept` checks a launch with.
///
/// As the update helper (`--apply-update <job>`) it must accept the staged
/// version, which on the beta and dev channels is a pre-release: fastframe
/// accepts one only on its pre-release channel, so the helper reports this
/// copy as a pre-release, as the download does. Otherwise it reports the
/// real version, which the relaunched app's receipt is compared with, and
/// accepts pre-releases when it is one.
pub fn launch_config(arguments: &[std::ffi::OsString]) -> UpdateConfig {
    let helper = arguments
        .iter()
        .any(|argument| argument == fastframe_update::APPLY_UPDATE_FLAG);
    if helper {
        UpdateConfig {
            prereleases: Prereleases::WhenRunningPrerelease,
            current_version: download_version(),
            ..CONFIG
        }
    } else {
        config_for(env!("CARGO_PKG_VERSION"))
    }
}

/// Checks for, downloads and installs updates. Every call blocks.
#[derive(Clone)]
pub struct Updater {
    transport: Arc<DevaltTransport>,
    source: Source,
}

impl Updater {
    pub fn new(transport: DevaltTransport) -> Self {
        Self {
            transport: Arc::new(transport),
            source: Source::github(),
        }
    }

    /// Takes releases from a local demo feed instead of the update host.
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    fn inner(&self, config: UpdateConfig) -> fastframe_update::Updater {
        fastframe_update::Updater::new(config, Shared(self.transport.clone()))
            .with_source(self.source.clone())
    }

    /// The channel's release, when it is newer than this copy and has a
    /// download for this computer.
    pub fn check(&self) -> Result<Option<Release>> {
        if !self.source.is_github() {
            return self.inner(CONFIG).check();
        }
        let user_agent = format!("Tidalfast/{}", env!("CARGO_PKG_VERSION"));
        let manifest = match self.transport.latest(&user_agent) {
            Ok(manifest) => manifest,
            Err(error) if error.is::<NotPublished>() => return Ok(None),
            Err(error) => return Err(error),
        };
        log::info!(
            "update check: current {}, channel {}, latest {}",
            env!("CARGO_PKG_VERSION"),
            self.transport.channel.as_str(),
            manifest.version
        );
        Ok(offer(
            &manifest,
            env!("CARGO_PKG_VERSION"),
            self.transport.channel,
            std::env::consts::OS,
            std::env::consts::ARCH,
        ))
    }

    /// How this copy was installed, or why it cannot update itself
    /// (Flatpak, Snap, apt/dnf/pacman, Nix, Homebrew, cargo, …).
    pub fn installation(&self) -> Result<Installation, Unsupported> {
        if let Some(appimage) = crate::appimage::current() {
            return appimage_installation(appimage);
        }
        self.inner(CONFIG).installation()
    }

    /// Downloads and verifies `release` into a staging folder beside the app.
    /// An AppImage is replaced in place instead (see `Updater::replace_appimage`).
    pub fn download(&self, release: &Release, progress: impl FnMut(u64, u64)) -> Result<Prepared> {
        if self.source.is_github()
            && let Some(appimage) = crate::appimage::current()
        {
            return self.replace_appimage(&appimage, release, progress);
        }
        self.inner(config_for(&release.version))
            .download(release, progress)
    }

    /// Starts the helper; quit the app right after.
    pub fn handoff(&self, prepared: Prepared, arguments: Vec<String>) -> Result<()> {
        if let Some(appimage) = crate::appimage::current()
            && prepared.installation().executable == appimage
            && replaced_appimage(prepared.version())
        {
            return crate::appimage::relaunch_after_exit(&appimage, &arguments);
        }
        self.inner(CONFIG).handoff(prepared, arguments)
    }

    /// fastframe-update has no installer for an AppImage, so the AppImage is
    /// replaced here: the release's `linux-<arch>-appimage` download comes
    /// through the same signed manifest and size and SHA-256 check as every
    /// other download, is written beside `appimage`, flushed, made
    /// executable and renamed over it. Nothing is renamed unless the whole
    /// download verified. The new file runs at the next start; the returned
    /// stand-in only tells the interface the restart is ready.
    fn replace_appimage(
        &self,
        appimage: &std::path::Path,
        release: &Release,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Prepared> {
        if let Some(reason) = crate::appimage::replace_blocker(appimage) {
            bail!(reason);
        }
        let user_agent = format!("Tidalfast/{}", env!("CARGO_PKG_VERSION"));
        let manifest = self.transport.manifest_for(&release.version, &user_agent)?;
        let platform = platform_prefix(std::env::consts::OS, std::env::consts::ARCH)
            .map(|prefix| format!("{prefix}-appimage"))
            .context("There is no AppImage for this computer")?;
        let asset = manifest
            .assets
            .iter()
            .find(|asset| asset.platform == platform)
            .with_context(|| format!("This release has no {platform} download"))?;
        let response = self.transport.get(&Request {
            url: &DevaltTransport::github_asset_url(&manifest.version, &asset.filename),
            accept: "application/octet-stream",
            user_agent: &user_agent,
        })?;
        ensure!(
            response.status == 200,
            "The update server answered {} for the AppImage",
            response.status
        );
        let total = asset.size;
        progress(0, total);
        crate::appimage::replace_with(appimage, response.body, |written| progress(written, total))?;
        if let Ok(mut replaced) = REPLACED_APPIMAGE.lock() {
            *replaced = Some(manifest.version.clone());
        }
        Ok(Prepared::sample(
            appimage_installation_of(appimage),
            &manifest.version,
        ))
    }
}

/// The version of the AppImage file this run already replaced.
static REPLACED_APPIMAGE: Mutex<Option<String>> = Mutex::new(None);

fn replaced_appimage(version: &str) -> bool {
    REPLACED_APPIMAGE
        .lock()
        .is_ok_and(|replaced| replaced.as_deref() == Some(version))
}

fn appimage_installation_of(appimage: &std::path::Path) -> Installation {
    Installation {
        executable: appimage.to_path_buf(),
        kind: Kind::Portable,
    }
}

/// An AppImage updates itself when its folder takes a new file; otherwise
/// the interface falls back to telling the user where to download it.
fn appimage_installation(appimage: std::path::PathBuf) -> Result<Installation, Unsupported> {
    match crate::appimage::replace_blocker(&appimage) {
        Some(reason) => Err(Unsupported::Unavailable(reason)),
        None => Ok(appimage_installation_of(&appimage)),
    }
}

/// The release to offer from `manifest`, if any.
fn offer(
    manifest: &Manifest,
    current: &str,
    channel: UpdateChannel,
    os: &str,
    arch: &str,
) -> Option<Release> {
    if !is_offered(&manifest.version, current, channel)
        || platform_assets(manifest, os, arch).is_empty()
    {
        return None;
    }
    if let Ok(mut notes) = LATEST_NOTES.lock() {
        *notes = Some((manifest.version.clone(), manifest.notes.clone()));
    }
    if let Ok(mut history) = LATEST_HISTORY.lock() {
        history.clone_from(&manifest.history);
    }
    Some(Release {
        version: manifest.version.clone(),
        url: RELEASE_PAGE.to_owned(),
    })
}

/// An updater on the app's HTTP client, through the configured proxy, for
/// the channel last set with [`set_channel`].
pub fn updater(proxy: &crate::settings::ProxyConfig) -> Result<Updater> {
    let Some(key) = update_key() else {
        bail!(NO_UPDATE_CHANNEL);
    };
    let client = crate::http::blocking_builder(proxy)
        .map_err(anyhow::Error::msg)?
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(15 * 60))
        .build()?;
    let dirs = crate::paths::AppDirs::discover();
    let settings = crate::settings::Settings::load(&dirs.settings_file());
    let install_id = crate::install_id::current(&dirs, settings.anonymous_install_id);
    Ok(Updater::new(
        DevaltTransport::new(
            ReqwestFetch(client),
            key,
            channel(),
            decode_public_key(PUBLIC_KEY)?,
            host_state(),
        )
        .with_install_id(install_id)
        .with_kept_downloads(dirs.update_cache_dir()),
    ))
}

/// How the executable at `executable` was installed, or why it cannot
/// update itself. Needs no network or update key; for packaging checks.
pub fn installation_at(executable: &std::path::Path) -> Result<Installation, Unsupported> {
    struct Offline;
    impl Transport for Offline {
        fn get(&self, _: &Request<'_>) -> Result<Response> {
            bail!("offline")
        }
    }
    fastframe_update::Updater::new(CONFIG, Offline).installation_at(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::collections::HashMap;

    #[test]
    fn a_manifest_history_is_read_leniently() {
        let base = r#"{"version":"7.13.0","channel":"stable","assets":[]"#;
        let none: Manifest = serde_json::from_str(&format!("{base}}}")).unwrap();
        assert!(none.history.is_empty());
        let text = format!(
            r#"{base},"history":[
                {{"version":"7.13.0","date":"2026-10-16","notes":"a"}},
                {{"version":"nonsense","notes":"b"}},
                {{"version":"7.12.0"}},
                7,
                {{"version":"7.11.0","notes":"c"}}]}}"#
        );
        let manifest: Manifest = serde_json::from_str(&text).unwrap();
        let versions: Vec<_> = manifest
            .history
            .iter()
            .map(|e| e.version.as_str())
            .collect();
        assert_eq!(versions, ["7.13.0", "7.11.0"]);
        assert_eq!(manifest.history[1].date, "");
        let wrong: Manifest =
            serde_json::from_str(&format!(r#"{base},"history":"text"}}"#)).unwrap();
        assert!(wrong.history.is_empty());
        let huge = "x".repeat(HISTORY_NOTES_LIMIT + 1);
        let text = format!(r#"{base},"history":[{{"version":"7.1.0","notes":"{huge}"}}]}}"#);
        assert!(
            serde_json::from_str::<Manifest>(&text)
                .unwrap()
                .history
                .is_empty()
        );
        let many: Vec<_> = (0..HISTORY_ENTRIES + 20)
            .map(|n| format!(r#"{{"version":"1.0.{n}","notes":"n"}}"#))
            .collect();
        let text = format!(r#"{base},"history":[{}]}}"#, many.join(","));
        let manifest: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest.history.len(), HISTORY_ENTRIES);
    }

    #[test]
    fn missed_notes_run_from_the_running_version_to_the_offered_one() {
        let entry = |version: &str| HistoryEntry {
            version: version.into(),
            date: String::new(),
            notes: format!("notes {version}"),
        };
        let history = [
            entry("7.14.0"),
            entry("7.12.0"),
            entry("7.13.0"),
            entry("7.10.5"),
        ];
        let versions = |found: Vec<HistoryEntry>| -> Vec<String> {
            found.into_iter().map(|entry| entry.version).collect()
        };
        // Newer than the running copy, no newer than the offer, newest first.
        assert_eq!(
            versions(missed_notes("7.13.0", "own", &history, "7.10.5")),
            ["7.13.0", "7.12.0"]
        );
        // The manifest's own notes stand in when the history lacks the offer.
        let found = missed_notes("7.13.0", "own", &history[3..], "7.10.0");
        assert_eq!(versions(found.clone()), ["7.13.0", "7.10.5"]);
        assert_eq!(found[0].notes, "own");
        // An older feed: the offered release alone.
        assert_eq!(
            versions(missed_notes("7.13.0", "own", &[], "7.10.5")),
            ["7.13.0"]
        );
        assert!(missed_notes("7.13.0", "", &[], "7.10.5").is_empty());
    }

    #[test]
    fn the_first_check_comes_within_thirty_seconds_of_startup() {
        for unit in [0.0, 0.5, 0.999] {
            let delay = first_check_delay(unit);
            assert!(delay >= Duration::from_secs(5) && delay <= Duration::from_secs(30));
        }
    }

    #[test]
    fn update_config_is_valid() {
        CONFIG.validate().unwrap();
        assert_eq!(CONFIG.current_version, env!("CARGO_PKG_VERSION"));
        decode_public_key(PUBLIC_KEY).unwrap();
        config_for("9.9.9-beta.1").validate().unwrap();
    }

    #[test]
    fn updates_require_the_release_signing_key() {
        assert!(
            CONFIG.publisher_key.is_some(),
            "updates must require a publisher signature"
        );
        // Not the key Tidalfast inherited from upstream.
        assert_ne!(
            PUBLIC_KEY.trim(),
            "e44c87b1550484c687947bd927ae333ff431cc7adc6cc33533c628f82016b554"
        );
    }

    fn signer() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap()
    }

    fn public() -> [u8; 32] {
        signer().public_key().as_ref().try_into().unwrap()
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn asset(platform: &str, filename: &str, version: &str, body: &[u8]) -> ManifestAsset {
        ManifestAsset {
            platform: platform.into(),
            filename: filename.into(),
            url: format!("v/{version}/{filename}"),
            size: body.len() as u64,
            sha256: sha(body),
        }
    }

    const PACKAGE: &[u8] = b"a verified package";

    fn manifest(version: &str, channel: &str) -> Manifest {
        Manifest {
            version: version.into(),
            channel: channel.into(),
            notes: "Fixes".into(),
            history: Vec::new(),
            published_at: "2026-10-14T00:00:00Z".into(),
            assets: vec![
                asset(
                    "windows-x86_64",
                    &format!("tidalfast-v{version}-x86_64-pc-windows-msvc.zip"),
                    version,
                    PACKAGE,
                ),
                asset(
                    "linux-x86_64-tar",
                    &format!("tidalfast-v{version}-x86_64-unknown-linux-gnu.tar.gz"),
                    version,
                    PACKAGE,
                ),
                asset(
                    "linux-x86_64-appimage",
                    &format!("tidalfast-v{version}-x86_64.AppImage"),
                    version,
                    PACKAGE,
                ),
            ],
        }
    }

    fn signed(manifest: &Manifest) -> (Vec<u8>, Vec<u8>) {
        let bytes = serde_json::to_vec(manifest).unwrap();
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signer().sign(&bytes).as_ref())
            .into_bytes();
        (bytes, signature)
    }

    #[test]
    fn signatures_verify_and_tampering_is_refused() {
        let (bytes, signature) = signed(&manifest("1.0.0", "stable"));
        let parsed = verify_manifest(&bytes, &signature, &public(), UpdateChannel::Stable).unwrap();
        assert_eq!(parsed.version, "1.0.0");
        // Raw 64-byte signatures and trailing newlines are accepted too.
        let raw = signer().sign(&bytes).as_ref().to_vec();
        verify_manifest(&bytes, &raw, &public(), UpdateChannel::Stable).unwrap();
        let mut newline = signature.clone();
        newline.push(b'\n');
        verify_manifest(&bytes, &newline, &public(), UpdateChannel::Stable).unwrap();
        // A changed byte, a changed signature, another key, garbage.
        let mut tampered = bytes.clone();
        let at = tampered.len() / 2;
        tampered[at] ^= 1;
        assert!(verify_manifest(&tampered, &signature, &public(), UpdateChannel::Stable).is_err());
        let mut bad = raw.clone();
        bad[0] ^= 1;
        assert!(verify_manifest(&bytes, &bad, &public(), UpdateChannel::Stable).is_err());
        let other: [u8; 32] = Ed25519KeyPair::from_seed_unchecked(&[8; 32])
            .unwrap()
            .public_key()
            .as_ref()
            .try_into()
            .unwrap();
        assert!(verify_manifest(&bytes, &signature, &other, UpdateChannel::Stable).is_err());
        assert!(verify_manifest(&bytes, b"not base64!", &public(), UpdateChannel::Stable).is_err());
        assert!(verify_manifest(&bytes, b"", &public(), UpdateChannel::Stable).is_err());
        // A correctly signed manifest for another channel.
        assert!(verify_manifest(&bytes, &signature, &public(), UpdateChannel::Beta).is_err());
    }

    #[test]
    fn signed_manifests_must_stay_inside_their_release() {
        let mut escaping = manifest("1.0.0", "stable");
        escaping.assets[0].url = "v/0.9.0/tidalfast.zip".into();
        let (bytes, signature) = signed(&escaping);
        assert!(verify_manifest(&bytes, &signature, &public(), UpdateChannel::Stable).is_err());
        let mut traversal = manifest("1.0.0", "stable");
        traversal.assets[0].filename = "../x".into();
        traversal.assets[0].url = "v/1.0.0/../x".into();
        let (bytes, signature) = signed(&traversal);
        assert!(verify_manifest(&bytes, &signature, &public(), UpdateChannel::Stable).is_err());
        let (bytes, signature) = signed(&manifest("v1.0", "stable"));
        assert!(verify_manifest(&bytes, &signature, &public(), UpdateChannel::Stable).is_err());
    }

    #[test]
    fn versions_compare_by_semver_precedence() {
        let order = [
            "0.9.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.10.0",
        ];
        for pair in order.windows(2) {
            assert!(
                Semver::parse(pair[0]).unwrap() < Semver::parse(pair[1]).unwrap(),
                "{pair:?}"
            );
        }
        for bad in [
            "",
            "1",
            "1.0",
            "v1.0.0",
            "01.0.0",
            "1.0.0-",
            "1.0.0-a..b",
            "1.0.0+build",
            "1.0.0.0",
        ] {
            assert!(Semver::parse(bad).is_none(), "{bad}");
        }
    }

    /// The helper of any copy, stable included, installs a beta or dev
    /// release: fastframe accepts a pre-release only when the config is on
    /// its pre-release channel (`WhenRunningPrerelease` and a pre-release
    /// current version). 1.0.0's helper used `CONFIG` and refused
    /// 1.0.1-alpha.1 with "Invalid update version".
    #[test]
    fn the_update_helper_accepts_a_prerelease_and_a_launch_reports_the_real_version() {
        let helper = launch_config(&[
            "tidalfast.exe".into(),
            "--apply-update".into(),
            "job".into(),
        ]);
        assert_eq!(helper.prereleases, Prereleases::WhenRunningPrerelease);
        assert!(
            Semver::parse(helper.current_version).is_some_and(|v| v.is_prerelease()),
            "{}",
            helper.current_version
        );
        // A normal launch acknowledges the receipt with its real version.
        let launch = launch_config(&["tidalfast.exe".into()]);
        assert_eq!(launch.current_version, env!("CARGO_PKG_VERSION"));
        let prerelease =
            Semver::parse(env!("CARGO_PKG_VERSION")).is_some_and(|v| v.is_prerelease());
        assert_eq!(
            launch.prereleases == Prereleases::WhenRunningPrerelease,
            prerelease,
            "a pre-release build accepts its own pre-release receipt"
        );
    }

    #[test]
    fn channels_decide_whether_prereleases_are_offered() {
        use UpdateChannel::*;
        assert!(is_offered("1.1.0", "1.0.0", Stable));
        assert!(!is_offered("1.0.0", "1.0.0", Stable));
        assert!(!is_offered("0.9.0", "1.0.0", Beta));
        assert!(!is_offered("1.1.0-beta.1", "1.0.0", Stable));
        assert!(is_offered("1.1.0-beta.1", "1.0.0", Beta));
        assert!(is_offered("1.1.0-dev.3", "1.1.0-dev.2", Dev));
        assert!(is_offered("1.1.0", "1.1.0-beta.4", Stable));
        assert!(!is_offered("garbage", "1.0.0", Dev));
    }

    /// The update window links to a page anyone can open: the update host's
    /// own page is private and answers 404 to everyone but its owner.
    #[test]
    fn the_release_link_opens_the_public_releases_page() {
        let mut manifest = manifest("1.1.0", "stable");
        manifest.assets.push(asset(
            "windows-x86_64",
            "tidalfast-v1.1.0-x86_64-pc-windows-msvc.zip",
            "1.1.0",
            PACKAGE,
        ));
        let release = offer(
            &manifest,
            "1.0.0",
            UpdateChannel::Stable,
            "windows",
            "x86_64",
        )
        .unwrap();
        assert_eq!(
            release.url,
            "https://github.com/CIoudGuy/tidalfast/releases"
        );
        let metadata = DevaltTransport::metadata(&manifest);
        assert_eq!(metadata["html_url"], release.url);
    }

    /// An installed copy updates through the setup program, which the
    /// manifest lists beside the zip as `windows-x86_64-installer`.
    #[test]
    fn the_windows_installer_counts_as_a_download_for_windows() {
        let mut manifest = manifest("1.0.0", "stable");
        manifest.assets.push(asset(
            "windows-x86_64-installer",
            "tidalfast-v1.0.0-x86_64-pc-windows-msvc-setup.exe",
            "1.0.0",
            PACKAGE,
        ));
        let names: Vec<_> = platform_assets(&manifest, "windows", "x86_64")
            .into_iter()
            .map(|asset| asset.platform.as_str())
            .collect();
        assert_eq!(names, ["windows-x86_64", "windows-x86_64-installer"]);
        // Offered even when the installer is all that is published.
        manifest.assets.remove(0);
        let release = offer(
            &manifest,
            "0.9.0",
            UpdateChannel::Stable,
            "windows",
            "x86_64",
        );
        assert_eq!(release.unwrap().version, "1.0.0");
        let metadata = DevaltTransport::metadata(&manifest);
        assert!(
            metadata["assets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|asset| asset["name"] == "tidalfast-v1.0.0-x86_64-pc-windows-msvc-setup.exe")
        );
    }

    #[test]
    fn the_platform_decides_which_downloads_count() {
        let manifest = manifest("1.0.0", "stable");
        let names = |os, arch| {
            platform_assets(&manifest, os, arch)
                .into_iter()
                .map(|asset| asset.platform.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("windows", "x86_64"), ["windows-x86_64"]);
        assert_eq!(
            names("linux", "x86_64"),
            ["linux-x86_64-tar", "linux-x86_64-appimage"]
        );
        assert!(names("windows", "aarch64").is_empty());
        assert!(names("freebsd", "x86_64").is_empty());
        assert_eq!(
            platform_prefix("macos", "aarch64").unwrap(),
            "macos-universal"
        );
        // A release without a download for this computer is not offered.
        assert!(
            offer(
                &manifest,
                "0.9.0",
                UpdateChannel::Stable,
                "windows",
                "aarch64"
            )
            .is_none()
        );
        assert_eq!(
            offer(
                &manifest,
                "0.9.0",
                UpdateChannel::Stable,
                "windows",
                "x86_64"
            )
            .unwrap()
            .version,
            "1.0.0"
        );
    }

    #[test]
    fn a_build_without_a_key_has_no_update_channel() {
        if update_key().is_none() {
            let error = updater(&crate::settings::ProxyConfig::Off).err().unwrap();
            assert_eq!(error.to_string(), NO_UPDATE_CHANNEL);
        }
    }

    #[test]
    fn the_channel_setting_is_remembered() {
        set_channel(UpdateChannel::Dev);
        assert_eq!(channel(), UpdateChannel::Dev);
        set_channel(UpdateChannel::Stable);
        assert_eq!(channel(), UpdateChannel::Stable);
    }

    /// One answer of the fake host.
    #[derive(Clone)]
    struct Answer {
        status: u16,
        body: Vec<u8>,
        location: Option<String>,
        etag: Option<String>,
        /// Fail reading after this many bytes (a dropped connection).
        drop_after: Option<usize>,
    }

    /// What the fake host saw of one request.
    #[derive(Clone, Debug)]
    struct Seen {
        url: String,
        keyed: bool,
        install_id: Option<String>,
        client: Option<String>,
        if_none_match: Option<String>,
        range_from: Option<u64>,
    }

    /// A fake update host: URL → answer. Honours `If-None-Match` and
    /// `Range` on the answers it has, and records every request.
    #[derive(Clone, Default)]
    struct FakeHost {
        files: Arc<Mutex<HashMap<String, Answer>>>,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeHost {
        fn answer(self, url: &str, answer: Answer) -> Self {
            self.files.lock().unwrap().insert(url.into(), answer);
            self
        }
        fn serve(self, url: &str, body: &[u8]) -> Self {
            self.answer(
                url,
                Answer {
                    status: 200,
                    body: body.to_vec(),
                    location: None,
                    etag: None,
                    drop_after: None,
                },
            )
        }
        fn status(self, url: &str, status: u16) -> Self {
            self.answer(
                url,
                Answer {
                    status,
                    body: Vec::new(),
                    location: None,
                    etag: None,
                    drop_after: None,
                },
            )
        }
        fn redirect(self, url: &str, to: &str) -> Self {
            self.answer(
                url,
                Answer {
                    status: 302,
                    body: Vec::new(),
                    location: Some(to.into()),
                    etag: None,
                    drop_after: None,
                },
            )
        }
        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    /// A body that fails after some bytes, like a dropped connection.
    struct Dropping(std::io::Cursor<Vec<u8>>, usize);

    impl Read for Dropping {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let left = self.1.saturating_sub(self.0.position() as usize);
            if left == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "connection reset",
                ));
            }
            let limit = left.min(buffer.len());
            self.0.read(&mut buffer[..limit])
        }
    }

    impl Fetch for FakeHost {
        fn get(&self, request: &HostRequest<'_>) -> Result<HostResponse> {
            self.seen.lock().unwrap().push(Seen {
                url: request.url.into(),
                keyed: request.update_key == Some("k"),
                install_id: request.install_id.map(str::to_owned),
                client: request.client.map(str::to_owned),
                if_none_match: request.if_none_match.map(str::to_owned),
                range_from: request.range_from,
            });
            let Some(answer) = self.files.lock().unwrap().get(request.url).cloned() else {
                return Ok(HostResponse::new(404, std::io::empty()));
            };
            if answer.etag.is_some() && request.if_none_match == answer.etag.as_deref() {
                return Ok(HostResponse::new(304, std::io::empty()));
            }
            let mut response = HostResponse::new(answer.status, std::io::empty());
            response.location = answer.location;
            response.etag = answer.etag;
            let from = request.range_from.unwrap_or(0) as usize;
            let body = answer.body[from.min(answer.body.len())..].to_vec();
            if request.range_from.is_some() {
                response.status = 206;
                response.range_start = Some(from as u64);
                response.body = Box::new(std::io::Cursor::new(body));
            } else if let Some(after) = answer.drop_after {
                response.body = Box::new(Dropping(std::io::Cursor::new(body), after));
            } else {
                response.body = Box::new(std::io::Cursor::new(body));
            }
            Ok(response)
        }
    }

    #[test]
    fn the_install_id_goes_only_to_the_update_host() {
        let id = "0123456789abcdef0123456789abcdef";
        let mut manifest = manifest("1.2.0", "stable");
        manifest.assets[0].url = "https://mirror.invalid/tidalfast.zip".into();
        let name = manifest.assets[0].filename.clone();
        let run = |install_id: Option<&str>| {
            let host = published(&manifest, "stable")
                .serve("https://mirror.invalid/tidalfast.zip", PACKAGE);
            let transport =
                transport(&host, UpdateChannel::Stable).with_install_id(install_id.map(Into::into));
            get(&transport, &download_url("1.2.0", &name)).unwrap();
            host.seen()
        };
        let seen = run(Some(id));
        assert!(seen.iter().any(|s| s.url.contains("mirror.invalid")));
        for s in &seen {
            if s.url.contains("mirror.invalid") {
                assert_eq!(s.install_id, None, "{}", s.url);
                assert_eq!(s.client, None, "{}", s.url);
            } else {
                assert_eq!(s.install_id.as_deref(), Some(id), "{}", s.url);
                assert_eq!(s.client.as_deref(), Some(client_name()), "{}", s.url);
            }
        }
        assert!(run(None).iter().all(|s| s.install_id.is_none()));
    }

    fn host_url(path: &str) -> String {
        format!("{RESOURCE_BASE}/{path}")
    }

    fn published(manifest: &Manifest, channel: &str) -> FakeHost {
        let (bytes, signature) = signed(manifest);
        FakeHost::default()
            .serve(
                &host_url(&format!("channels/{channel}/latest.json")),
                &bytes,
            )
            .serve(
                &host_url(&format!("channels/{channel}/latest.json.sig")),
                &signature,
            )
    }

    fn transport(host: &FakeHost, channel: UpdateChannel) -> DevaltTransport {
        let mut transport = DevaltTransport::new(
            host.clone(),
            "k",
            channel,
            public(),
            Arc::new(HostState::default()),
        );
        transport.host.resume_pause = Duration::ZERO;
        transport
    }

    fn get(transport: &DevaltTransport, url: &str) -> Result<Vec<u8>> {
        let response = transport.get(&Request {
            url,
            accept: "*/*",
            user_agent: "Tidalfast/0.0.0",
        })?;
        ensure!(response.status == 200, "status {}", response.status);
        let mut body = Vec::new();
        response.body.take(1 << 20).read_to_end(&mut body)?;
        Ok(body)
    }

    fn download_url(version: &str, name: &str) -> String {
        format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}")
    }

    #[test]
    fn missing_manifests_mean_nothing_is_published() {
        let host = FakeHost::default();
        let error = transport(&host, UpdateChannel::Beta)
            .latest("Tidalfast/0.0.0")
            .unwrap_err();
        assert!(error.is::<NotPublished>(), "{error:#}");
        // For a check that is "up to date", not a failure.
        let updater = Updater::new(transport(&host, UpdateChannel::Beta));
        assert!(updater.check().unwrap().is_none());
        // Unknown files answer 404 to the crate, which reports it.
        let manifest = manifest("1.0.0", "beta");
        let transport = transport(&published(&manifest, "beta"), UpdateChannel::Beta);
        assert!(get(&transport, &download_url("1.0.0", "nothing.zip")).is_err());
        assert!(get(&transport, "https://example.com/anything").is_err());
    }

    #[test]
    fn the_crate_sees_a_github_release_built_from_the_manifest() {
        let manifest = manifest("1.2.0", "stable");
        let package = &manifest.assets[0];
        let host = published(&manifest, "stable")
            .redirect(
                &host_url(&package.url),
                "https://storage.invalid/blob?sig=1",
            )
            .serve("https://storage.invalid/blob?sig=1", PACKAGE);
        let transport = transport(&host, UpdateChannel::Stable);
        let latest: serde_json::Value = serde_json::from_slice(
            &get(
                &transport,
                &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(latest["tag_name"], "v1.2.0");
        let metadata: serde_json::Value = serde_json::from_slice(
            &get(
                &transport,
                &format!("https://api.github.com/repos/{REPOSITORY}/releases/tags/v1.2.0"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["assets"][0]["name"], package.filename);
        assert_eq!(metadata["prerelease"], false);
        let body = get(
            &transport,
            metadata["assets"][0]["browser_download_url"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body, PACKAGE);
        // The key went to the update host and never to the storage redirect.
        for seen in host.seen() {
            assert_eq!(seen.keyed, seen.url.starts_with(RESOURCE_BASE), "{seen:?}");
        }
        // Another version is not this channel's release.
        assert!(
            get(
                &transport,
                &format!("https://api.github.com/repos/{REPOSITORY}/releases/tags/v1.1.0"),
            )
            .is_err()
        );
    }

    #[test]
    fn mirrored_downloads_are_verified_and_never_see_the_key() {
        let mut manifest = manifest("1.2.0", "stable");
        manifest.assets[0].url = "https://mirror.invalid/tidalfast.zip".into();
        let name = manifest.assets[0].filename.clone();
        let host =
            published(&manifest, "stable").serve("https://mirror.invalid/tidalfast.zip", PACKAGE);
        assert_eq!(
            get(
                &transport(&host, UpdateChannel::Stable),
                &download_url("1.2.0", &name)
            )
            .unwrap(),
            PACKAGE
        );
        let mirror: Vec<_> = host
            .seen()
            .into_iter()
            .filter(|seen| seen.url.contains("mirror.invalid"))
            .collect();
        assert_eq!(mirror.len(), 1);
        assert!(!mirror[0].keyed);
        // A mirror serving something else is refused.
        let host = published(&manifest, "stable").serve(
            "https://mirror.invalid/tidalfast.zip",
            b"a tampered! package",
        );
        assert!(
            get(
                &transport(&host, UpdateChannel::Stable),
                &download_url("1.2.0", &name)
            )
            .is_err()
        );
        // Plain HTTP mirrors are not accepted at all.
        let mut insecure = manifest.clone();
        insecure.assets[0].url = "http://mirror.invalid/tidalfast.zip".into();
        let (bytes, signature) = signed(&insecure);
        assert!(verify_manifest(&bytes, &signature, &public(), UpdateChannel::Stable).is_err());
    }

    #[test]
    fn downloads_that_do_not_match_the_manifest_are_refused() {
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        for served in [
            b"a tampered! package".as_slice(),
            b"a verified packag",
            b"a verified package and more",
        ] {
            let host = published(&manifest, "stable").serve(&host_url(&package.url), served);
            let error = get(
                &transport(&host, UpdateChannel::Stable),
                &download_url("1.2.0", &package.filename),
            )
            .unwrap_err();
            let text = format!("{error:#}");
            assert!(
                text.contains("verified")
                    || text.contains("interrupted")
                    || text.contains("exceeds"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_dropped_download_resumes_with_a_range_request() {
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        let host = published(&manifest, "stable").answer(
            &host_url(&package.url),
            Answer {
                status: 200,
                body: PACKAGE.to_vec(),
                location: None,
                etag: None,
                drop_after: Some(5),
            },
        );
        let body = get(
            &transport(&host, UpdateChannel::Stable),
            &download_url("1.2.0", &package.filename),
        )
        .unwrap();
        assert_eq!(body, PACKAGE);
        let ranges: Vec<_> = host
            .seen()
            .iter()
            .filter_map(|seen| seen.range_from)
            .collect();
        assert_eq!(ranges, [5]);
    }

    fn appimage_release(body: &[u8]) -> (Manifest, FakeHost) {
        let mut manifest = manifest("1.2.0", "stable");
        let platform = platform_prefix(std::env::consts::OS, std::env::consts::ARCH)
            .map(|prefix| format!("{prefix}-appimage"))
            .unwrap_or_else(|| "linux-x86_64-appimage".into());
        manifest.assets[2].platform = platform;
        let url = host_url(&manifest.assets[2].url);
        let host = published(&manifest, "stable").serve(&url, body);
        (manifest, host)
    }

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            url: String::new(),
        }
    }

    #[test]
    fn an_appimage_is_replaced_by_the_verified_download() {
        let (_, host) = appimage_release(PACKAGE);
        let directory = crate::appimage::Scratch::new();
        let appimage = directory.path().join("Tidalfast.AppImage");
        std::fs::write(&appimage, b"old").unwrap();
        let updater = Updater::new(transport(&host, UpdateChannel::Stable));
        let mut progress = Vec::new();
        let prepared = updater
            .replace_appimage(&appimage, &release("1.2.0"), |received, total| {
                progress.push((received, total))
            })
            .unwrap();
        assert_eq!(std::fs::read(&appimage).unwrap(), PACKAGE);
        assert_eq!(prepared.version(), "1.2.0");
        assert_eq!(prepared.installation().executable, appimage);
        assert_eq!(
            progress.last(),
            Some(&(PACKAGE.len() as u64, PACKAGE.len() as u64))
        );
        assert!(replaced_appimage("1.2.0"));
        // The key went to the update host only.
        assert!(host.seen().iter().any(|seen| seen.keyed));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_download_that_does_not_match_the_manifest_leaves_the_appimage_alone() {
        let (_, host) = appimage_release(b"a tampered package!");
        let directory = crate::appimage::Scratch::new();
        let appimage = directory.path().join("Tidalfast.AppImage");
        std::fs::write(&appimage, b"old").unwrap();
        let updater = Updater::new(transport(&host, UpdateChannel::Stable));
        assert!(
            updater
                .replace_appimage(&appimage, &release("1.2.0"), |_, _| {})
                .is_err()
        );
        assert_eq!(std::fs::read(&appimage).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        // Nor does a release the manifest does not name.
        let (_, host) = appimage_release(PACKAGE);
        let updater = Updater::new(transport(&host, UpdateChannel::Stable));
        assert!(
            updater
                .replace_appimage(&appimage, &release("9.9.9"), |_, _| {})
                .is_err()
        );
        assert_eq!(std::fs::read(&appimage).unwrap(), b"old");
    }

    #[test]
    #[cfg(unix)]
    fn an_appimage_in_a_closed_folder_falls_back_to_the_download_page() {
        use std::os::unix::fs::PermissionsExt;
        let directory = crate::appimage::Scratch::new();
        let appimage = directory.path().join("Tidalfast.AppImage");
        std::fs::write(&appimage, b"old").unwrap();
        assert_eq!(
            appimage_installation(appimage.clone()).unwrap().executable,
            appimage
        );
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::File::create(directory.path().join("probe")).is_err() {
            assert!(matches!(
                appimage_installation(appimage),
                Err(Unsupported::Unavailable(_))
            ));
        }
    }

    #[test]
    fn a_tampered_manifest_is_never_used() {
        let manifest = manifest("1.2.0", "stable");
        let (mut bytes, signature) = signed(&manifest);
        let at = bytes.len() - 3;
        bytes[at] ^= 1;
        let host = FakeHost::default()
            .serve(&host_url("channels/stable/latest.json"), &bytes)
            .serve(&host_url("channels/stable/latest.json.sig"), &signature);
        assert!(
            transport(&host, UpdateChannel::Stable)
                .latest("Tidalfast/0.0.0")
                .is_err()
        );
    }

    #[test]
    fn an_unchanged_manifest_is_not_downloaded_again() {
        let manifest = manifest("1.2.0", "stable");
        let (bytes, signature) = signed(&manifest);
        let latest = host_url("channels/stable/latest.json");
        let host = FakeHost::default()
            .answer(
                &latest,
                Answer {
                    status: 200,
                    body: bytes,
                    location: None,
                    etag: Some("\"v1\"".into()),
                    drop_after: None,
                },
            )
            .serve(&format!("{latest}.sig"), &signature);
        let transport = transport(&host, UpdateChannel::Stable);
        assert_eq!(
            transport.latest("Tidalfast/0.0.0").unwrap().version,
            "1.2.0"
        );
        // Asked again with the ETag: 304, the held manifest, no signature
        // download, and it is still verified.
        assert_eq!(
            transport.latest("Tidalfast/0.0.0").unwrap().version,
            "1.2.0"
        );
        let seen = host.seen();
        let manifest_requests: Vec<_> = seen.iter().filter(|seen| seen.url == latest).collect();
        assert_eq!(manifest_requests.len(), 2);
        assert_eq!(manifest_requests[0].if_none_match, None);
        assert_eq!(
            manifest_requests[1].if_none_match.as_deref(),
            Some("\"v1\"")
        );
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.url.ends_with(".sig"))
                .count(),
            1
        );
    }

    #[test]
    fn backoff_grows_to_a_cap_and_resets_on_success() {
        assert_eq!(Backoff::delay(1), BACKOFF_BASE);
        assert_eq!(Backoff::delay(2), BACKOFF_BASE * 2);
        assert_eq!(Backoff::delay(4), BACKOFF_BASE * 8);
        assert_eq!(Backoff::delay(40), BACKOFF_CAP);
        let backoff = Backoff::default();
        let now = Instant::now();
        backoff.ready(now).unwrap();
        backoff.failure(now, None, 0.0);
        assert!(backoff.ready(now + Duration::from_secs(30)).is_err());
        backoff.ready(now + BACKOFF_BASE).unwrap();
        backoff.failure(now, None, 0.0);
        assert!(backoff.ready(now + BACKOFF_BASE).is_err());
        backoff.ready(now + BACKOFF_BASE * 2).unwrap();
        // Jitter adds at most 20%.
        backoff.failure(now, None, 0.999);
        assert!(backoff.ready(now + BACKOFF_BASE * 4).is_err());
        backoff.ready(now + BACKOFF_BASE * 5).unwrap();
        // Retry-After lengthens the wait, within reason.
        backoff.success();
        backoff.failure(now, Some(Duration::from_secs(3600)), 0.0);
        assert!(backoff.ready(now + Duration::from_secs(3599)).is_err());
        backoff.success();
        backoff.ready(now).unwrap();
    }

    #[test]
    fn a_busy_server_is_left_alone_until_the_backoff_ends() {
        let latest = host_url("channels/stable/latest.json");
        for status in [429, 500, 503] {
            let host = FakeHost::default().status(&latest, status);
            let transport = transport(&host, UpdateChannel::Stable);
            let first = transport.latest("Tidalfast/0.0.0").unwrap_err();
            assert!(first.to_string().contains("busy"), "{first:#}");
            // The next check fails at once, without a request.
            let second = transport.latest("Tidalfast/0.0.0").unwrap_err();
            assert!(second.to_string().contains("try again"), "{second:#}");
            assert_eq!(host.seen().len(), 1, "{status}");
        }
    }

    #[test]
    fn failed_checks_back_off() {
        assert!(retry_delay(1, 0.5) >= Duration::from_secs(48 * 60));
        assert!(retry_delay(2, 0.5) > retry_delay(1, 0.5));
        assert!(retry_delay(50, 1.0) <= Duration::from_secs(29 * 60 * 60));
    }

    #[test]
    fn check_times_are_spread_out() {
        assert_eq!(jittered(CHECK_INTERVAL, 0.0), CHECK_INTERVAL.mul_f64(0.8));
        assert_eq!(jittered(CHECK_INTERVAL, 0.5), CHECK_INTERVAL);
        assert!(jittered(CHECK_INTERVAL, 0.999) <= CHECK_INTERVAL.mul_f64(1.2));
        for _ in 0..100 {
            let unit = random_unit();
            assert!((0.0..1.0).contains(&unit));
        }
        assert!(check_due(None));
        assert!(!check_due(Some(Instant::now())));
        let long_ago = Instant::now().checked_sub(CHECK_INTERVAL.mul_f64(1.21));
        if let Some(long_ago) = long_ago {
            assert!(check_due(Some(long_ago)));
        }
    }

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "tidalfast-updates-{name}-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_restart_before_installing_does_not_download_the_release_again() {
        let kept = scratch("kept");
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        let url = download_url("1.2.0", &package.filename);
        let host = published(&manifest, "stable").serve(&host_url(&package.url), PACKAGE);
        let first = transport(&host, UpdateChannel::Stable).with_kept_downloads(kept.0.clone());
        assert_eq!(get(&first, &url).unwrap(), PACKAGE);
        assert_eq!(
            files_in(&kept.0.join("1.2.0")),
            std::slice::from_ref(&package.filename)
        );

        // The next run: the host no longer even has the package, and the
        // kept download is served without asking for it.
        let host = published(&manifest, "stable");
        let next = transport(&host, UpdateChannel::Stable).with_kept_downloads(kept.0.clone());
        assert_eq!(get(&next, &url).unwrap(), PACKAGE);
        assert!(
            host.seen()
                .iter()
                .all(|seen| !seen.url.ends_with(&package.url)),
            "{:?}",
            host.seen()
        );
    }

    #[test]
    fn a_kept_download_that_no_longer_matches_is_downloaded_again() {
        let kept = scratch("tampered");
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        let path = kept.0.join("1.2.0").join(&package.filename);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"a tampered! package").unwrap();
        let host = published(&manifest, "stable").serve(&host_url(&package.url), PACKAGE);
        let transport = transport(&host, UpdateChannel::Stable).with_kept_downloads(kept.0.clone());
        assert_eq!(
            get(&transport, &download_url("1.2.0", &package.filename)).unwrap(),
            PACKAGE
        );
        assert_eq!(fs::read(&path).unwrap(), PACKAGE);
    }

    #[test]
    fn a_download_that_fails_verification_is_not_kept() {
        let kept = scratch("refused");
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        for served in [b"a tampered! package".as_slice(), b"a verified packag"] {
            let host = published(&manifest, "stable").serve(&host_url(&package.url), served);
            let transport =
                transport(&host, UpdateChannel::Stable).with_kept_downloads(kept.0.clone());
            assert!(get(&transport, &download_url("1.2.0", &package.filename)).is_err());
            assert!(files_in(&kept.0.join("1.2.0")).is_empty());
        }
    }

    #[test]
    fn only_the_newest_release_stays_kept() {
        let kept = scratch("versions");
        fs::create_dir_all(kept.0.join("1.1.0")).unwrap();
        fs::write(kept.0.join("1.1.0").join("old.zip"), b"old").unwrap();
        let manifest = manifest("1.2.0", "stable");
        let package = manifest.assets[0].clone();
        let host = published(&manifest, "stable").serve(&host_url(&package.url), PACKAGE);
        let transport = transport(&host, UpdateChannel::Stable).with_kept_downloads(kept.0.clone());
        get(&transport, &download_url("1.2.0", &package.filename)).unwrap();
        assert_eq!(files_in(&kept.0), ["1.2.0"]);

        // Once 1.2.0 runs, its download goes; a newer one waiting stays.
        fs::create_dir_all(kept.0.join("1.3.0")).unwrap();
        fs::create_dir_all(kept.0.join("not-a-version")).unwrap();
        prune_kept(&kept.0, "1.2.0");
        assert_eq!(files_in(&kept.0), ["1.3.0"]);
    }

    /// A download that started after the tidy began is this run's own.
    #[test]
    fn the_tidy_leaves_a_download_this_run_started() {
        let dir = scratch("fresh-download");
        let started = std::time::SystemTime::now() - Duration::from_secs(60);
        let folder = dir.0.join(".tidalfast-update-00000000000000aa");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("tidalfast-setup.exe"), b"half").unwrap();
        assert!(tidy_staging(&dir.0, None, started).is_empty());
        assert!(folder.exists());
    }

    #[test]
    fn startup_removes_update_folders_no_install_needs() {
        let dir = scratch("staging");
        let folder = |suffix: &str, files: &[&str]| {
            let folder = dir.0.join(format!(".tidalfast-update-{suffix}"));
            fs::create_dir_all(&folder).unwrap();
            for file in files {
                fs::write(folder.join(file), b"x").unwrap();
            }
            folder
        };
        folder(
            "0000000000000001",
            &["handoff.json", "helper.exe", "result.txt"],
        );
        folder("0000000000000002", &["tidalfast-setup.exe"]);
        let receipt = folder("0000000000000003", &["handoff.json", "result.txt"]);
        folder("0000000000000004", &["handoff.json", "ready"]);
        folder(
            "0000000000000005",
            &["result.txt", "mounted-0000000000000000"],
        );
        folder("not-a-staging-dir", &["result.txt"]);
        fs::write(dir.0.join("tidalfast.exe"), b"app").unwrap();

        let now = std::time::SystemTime::now();
        let removed = tidy_staging(&dir.0, Some(&receipt.join("handoff.json")), now);
        assert_eq!(removed.len(), 2, "{removed:?}");
        assert_eq!(
            files_in(&dir.0),
            [
                ".tidalfast-update-0000000000000003",
                ".tidalfast-update-0000000000000004",
                ".tidalfast-update-0000000000000005",
                ".tidalfast-update-not-a-staging-dir",
                "tidalfast.exe",
            ]
        );

        // A day later, an install that never finished is given up on, and
        // the receipt's folder goes once another copy starts.
        let later = now + UNFINISHED_UPDATE + Duration::from_secs(60);
        tidy_staging(&dir.0, None, later);
        assert_eq!(
            files_in(&dir.0),
            [
                ".tidalfast-update-0000000000000005",
                ".tidalfast-update-not-a-staging-dir",
                "tidalfast.exe",
            ]
        );
    }
}
