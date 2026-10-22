//! A blocking adapter that serves the engine's [`Catalog`] from
//! [`TidalClient`]. The trait and its types belong to the engine
//! (`src/tidal_engine/catalog.rs`, .internal/docs/TIDAL-PORT.md) and are re-exported.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};

use super::models::Track;
use super::tidal::{ReleaseGroup, TidalClient};
use crate::content_filter::ContentFilter;
pub use crate::tidal_engine::{Catalog, LocalTrack, Quality, StreamInfo};

/// A context never grows past this many tracks in the engine's queue.
const MAX_CONTEXT_TRACKS: usize = 10_000;
/// What downloading reads of a collection at most: far past any real one
/// (My Collection may hold tens of thousands), only a stop for a server
/// that never ends its pages.
const MAX_DOWNLOAD_TRACKS: usize = 200_000;
const PAGE: u32 = 100;

/// The songs of a collection TIDAL lists, and how many it left out because
/// they cannot be played (removed, or not offered in this country).
#[derive(Clone, Debug, Default)]
pub struct ContextTracks {
    pub tracks: Vec<LocalTrack>,
    pub unplayable: u32,
}

/// Serves [`Catalog`] from a [`TidalClient`], blocking the calling engine
/// thread on the backend's runtime.
pub struct TidalCatalog {
    client: Arc<TidalClient>,
    runtime: tokio::runtime::Handle,
}

impl TidalCatalog {
    pub fn new(client: Arc<TidalClient>, runtime: tokio::runtime::Handle) -> Self {
        Self { client, runtime }
    }

    fn block<T>(&self, future: impl Future<Output = T>) -> anyhow::Result<T> {
        if tokio::runtime::Handle::try_current().is_ok() {
            // Blocking a runtime worker would deadlock it; the engine calls
            // from its own threads, so this is a wiring mistake.
            bail!("the catalogue was called from inside the async runtime");
        }
        Ok(self.runtime.block_on(future))
    }

    /// Reads a collection page by page, handing each page to `deliver`
    /// as it arrives. `deliver` returns false to stop reading, e.g. once
    /// nobody waits for the rest.
    async fn pages<F, Fut>(
        cap: usize,
        mut fetch: F,
        deliver: &mut (dyn FnMut(Vec<Track>) -> bool + Send),
    ) -> anyhow::Result<()>
    where
        F: FnMut(u32) -> Fut,
        Fut: Future<Output = super::client::Result<super::models::Page<Track>>>,
    {
        let mut count = 0;
        let mut offset = 0;
        loop {
            let page = fetch(offset).await?;
            let next = page.next_offset();
            count += page.items.len();
            if !deliver(page.items) {
                break;
            }
            match next {
                Some(next) if count < cap => offset = next,
                _ => break,
            }
        }
        Ok(())
    }

    /// The songs of an album, playlist, mix, artist or collection, read on
    /// the caller's own task. The blocking [`Catalog`] methods are for the
    /// engine's threads: a thread of tokio's blocking pool still counts as
    /// inside the runtime, so they refuse it.
    ///
    /// Every page is read, however long the collection: a download must not
    /// stop short of the songs TIDAL lists.
    pub async fn context_tracks_async(&self, uri: &str) -> anyhow::Result<ContextTracks> {
        let tracks = self
            .context(uri, MAX_DOWNLOAD_TRACKS)
            .await
            .with_context(|| format!("loading {uri}"))?;
        // A playlist lists its videos too; a download keeps songs only.
        let tracks: Vec<Track> = tracks
            .into_iter()
            .filter(|track| !track.uri.starts_with("tidal:video:"))
            .collect();
        let total = tracks.len();
        let tracks = playable(tracks);
        Ok(ContextTracks {
            unplayable: (total - tracks.len()) as u32,
            tracks,
        })
    }

    /// An artist's top tracks, every page, then the songs of their own
    /// albums, EPs and singles (not the releases they only appear on).
    /// A song listed twice, by id or by ISRC, comes once. The caller reads
    /// at its own pace, and stops by returning false.
    async fn artist_catalogue(
        client: &TidalClient,
        id: &str,
        cap: usize,
        deliver: &mut (dyn FnMut(Vec<Track>) -> bool + Send),
    ) -> anyhow::Result<()> {
        let mut seen = Seen::default();
        let mut count = 0usize;
        let mut open = true;
        Self::pages(
            cap,
            |offset| client.artist_top_tracks_page(id, offset, PAGE),
            &mut |page| {
                let page = seen.fresh(id, page, false);
                count += page.len();
                open = page.is_empty() || deliver(page);
                open
            },
        )
        .await?;
        for group in [ReleaseGroup::Albums, ReleaseGroup::EpsAndSingles] {
            let mut offset = 0;
            while open && count < cap {
                let releases = match client.artist_releases(id, group, offset, PAGE).await {
                    Ok(releases) => releases,
                    Err(error) => {
                        log::warn!("the releases of artist {id} could not be read: {error}");
                        return Ok(());
                    }
                };
                let next = releases.next_offset();
                for album in releases.items {
                    if !open || count >= cap {
                        break;
                    }
                    let read = Self::pages(
                        cap,
                        |offset| client.album_tracks(&album.id, offset, PAGE),
                        &mut |page| {
                            let page = seen.fresh(id, page, true);
                            count += page.len();
                            open = page.is_empty() || deliver(page);
                            open
                        },
                    )
                    .await;
                    if let Err(error) = read {
                        log::warn!("album {} could not be read: {error}", album.id);
                    }
                }
                match next {
                    Some(next) => offset = next,
                    None => break,
                }
            }
        }
        Ok(())
    }

    async fn context(&self, uri: &str, cap: usize) -> anyhow::Result<Vec<Track>> {
        let mut tracks = Vec::new();
        self.context_into(uri, cap, false, &mut |page| {
            tracks.extend(page);
            true
        })
        .await?;
        Ok(tracks)
    }

    async fn context_into(
        &self,
        uri: &str,
        cap: usize,
        catalogue: bool,
        deliver: &mut (dyn FnMut(Vec<Track>) -> bool + Send),
    ) -> anyhow::Result<()> {
        let client = &self.client;
        let mut parts = uri.splitn(3, ':');
        let (scheme, kind, id) = (parts.next(), parts.next(), parts.next());
        if scheme != Some("tidal") {
            bail!("not a TIDAL context: {uri}");
        }
        let id = id.unwrap_or_default();
        match kind {
            Some("album") => {
                Self::pages(cap, |offset| client.album_tracks(id, offset, PAGE), deliver).await?
            }
            Some("playlist") => {
                Self::pages(
                    cap,
                    |offset| client.playlist_tracks(id, offset, PAGE),
                    deliver,
                )
                .await?
            }
            Some("mix") => {
                Self::pages(cap, |offset| client.mix_tracks(id, offset, PAGE), deliver).await?
            }
            // Playing an artist goes on past their top tracks, which TIDAL
            // stops at some number of, into the rest of their catalogue.
            Some("artist") if catalogue => Self::artist_catalogue(client, id, cap, deliver).await?,
            // An artist plays their top tracks, every page of them, so a
            // shuffle stays on the artist's own songs.
            Some("artist" | "toptracks") => {
                Self::pages(
                    cap,
                    |offset| client.artist_top_tracks_page(id, offset, PAGE),
                    deliver,
                )
                .await?
            }
            Some("collection") => {
                Self::pages(
                    cap,
                    |offset| async move {
                        let page = client.saved_tracks(offset, PAGE).await?;
                        Ok(super::models::Page {
                            items: page.items.into_iter().map(|saved| saved.track).collect(),
                            total: page.total,
                            limit: page.limit,
                            offset: page.offset,
                            next: page.next,
                        })
                    },
                    deliver,
                )
                .await?
            }
            Some("track") => {
                deliver(vec![client.track(id).await?]);
            }
            Some("station") => {
                deliver(
                    radio_tracks(client, &format!("tidal:{id}"), PAGE)
                        .await
                        .map_err(|error| anyhow!(error))?,
                );
            }
            _ => bail!("TIDAL cannot play this kind of context: {uri}"),
        }
        Ok(())
    }
}

/// What an artist's catalogue has delivered already, by track id and by
/// ISRC, so a remaster or a single repeated on an album comes once.
#[derive(Default)]
struct Seen {
    ids: HashSet<String>,
    isrcs: HashSet<String>,
}

impl Seen {
    /// The songs of `page` not seen before. With `own_only`, a song the
    /// artist is not credited on is left out.
    fn fresh(&mut self, artist: &str, page: Vec<Track>, own_only: bool) -> Vec<Track> {
        page.into_iter()
            .filter(|track| {
                !own_only
                    || track.artists.is_empty()
                    || track
                        .artists
                        .iter()
                        .any(|credited| credited.id.as_deref() == Some(artist))
            })
            .filter(|track| {
                let isrc = track
                    .external_ids
                    .isrc
                    .as_deref()
                    .filter(|isrc| !isrc.is_empty())
                    .map(str::to_ascii_uppercase);
                let new_id = track
                    .id
                    .as_ref()
                    .is_none_or(|id| self.ids.insert(id.clone()));
                let new_isrc = isrc.is_none_or(|isrc| self.isrcs.insert(isrc));
                new_id && new_isrc
            })
            .collect()
    }
}

/// The songs of TIDAL's radio for a seed (`tidal:track:..`,
/// `tidal:artist:..`, `tidal:album:..` or `tidal:playlist:..`): the seed's
/// mix when TIDAL has one, so the radio plays the same songs it showed,
/// else TIDAL's track or artist radio. An album or playlist is seeded by
/// its first song.
pub async fn radio_tracks(
    client: &TidalClient,
    seed: &str,
    limit: u32,
) -> anyhow::Result<Vec<Track>> {
    let mut parts = seed.splitn(3, ':');
    let (scheme, kind, id) = (parts.next(), parts.next(), parts.next());
    if scheme != Some("tidal") {
        bail!("not a TIDAL radio seed: {seed}");
    }
    let id = id.unwrap_or_default().to_string();
    let (kind, id) = match kind {
        Some("album") => (
            "track",
            first_track_id(client.album_tracks(&id, 0, 1).await?)?,
        ),
        Some("playlist") => (
            "track",
            first_track_id(client.playlist_tracks(&id, 0, 1).await?)?,
        ),
        Some(kind @ ("track" | "artist")) => (kind, id),
        _ => bail!("TIDAL has no radio for {seed}"),
    };
    let mix = match kind {
        "track" => client.track_mix(&id).await.ok().flatten(),
        _ => client.artist_mix(&id).await.ok().flatten(),
    };
    if let Some(mix) = mix {
        let page = client.mix_tracks(&mix, 0, limit).await?;
        if !page.items.is_empty() {
            return Ok(page.items);
        }
    }
    Ok(match kind {
        "track" => client.track_radio(&id, limit).await?,
        _ => client.artist_radio(&id, limit).await?,
    })
}

fn first_track_id(page: super::models::Page<Track>) -> anyhow::Result<String> {
    page.items
        .into_iter()
        // A playlist's video is no seed for a song radio.
        .filter(|track| !track.uri.starts_with("tidal:video:"))
        .find_map(|track| track.id)
        .context("there are no songs to start a radio from")
}

/// The engine's track: art at a large and a small size.
pub fn local_track(track: Track) -> LocalTrack {
    let (art_url, art_small_url) = track
        .album
        .as_ref()
        .map(|album| {
            (
                super::models::pick_image(&album.images, 640).map(str::to_string),
                super::models::pick_image(&album.images, 160).map(str::to_string),
            )
        })
        .unwrap_or_default();
    let album_uri = track
        .album
        .as_ref()
        .map(|album| album.uri.clone())
        .filter(|uri| !uri.is_empty());
    let isrc = track
        .external_ids
        .isrc
        .as_deref()
        .filter(|isrc| !isrc.is_empty())
        .map(str::to_ascii_uppercase);
    LocalTrack {
        uri: track.uri,
        title: track.name,
        isrc,
        album_uri,
        album: track.album.map(|album| album.name).unwrap_or_default(),
        artists: track.artists,
        art_url,
        art_small_url,
        duration_ms: track.duration_ms,
    }
}

/// The engine's track for a music video: its 16:9 thumbnail as the art.
pub fn local_video(video: super::models::Video) -> LocalTrack {
    LocalTrack {
        uri: video.uri(),
        art_url: super::models::pick_image(&video.images, 750).map(str::to_string),
        art_small_url: super::models::pick_image(&video.images, 160).map(str::to_string),
        title: video.name,
        artists: video.artists,
        album: String::new(),
        duration_ms: video.duration.saturating_mul(1000),
        album_uri: None,
        isrc: None,
    }
}

fn playable(tracks: Vec<Track>) -> Vec<LocalTrack> {
    tracks.into_iter().filter(plays).map(local_track).collect()
}

/// Whether the engine can play `track`: TIDAL has not marked it
/// unavailable, and it has an id to ask for its stream by.
fn plays(track: &Track) -> bool {
    track.is_playable != Some(false) && track.id.is_some()
}

/// [`playable`] without the tracks the account's content settings refuse
/// (explicit, AI): what the engine plays, so they are skipped in playback
/// and the queue. Downloads read the whole list through [`playable`].
fn playable_by_settings(
    tracks: Vec<Track>,
    filter: crate::content_filter::ContentFilter,
) -> Vec<LocalTrack> {
    playable(
        tracks
            .into_iter()
            .filter(|track| filter.allows(track))
            .collect(),
    )
}

/// A request's failure as the engine reads it. Only TIDAL's own answer
/// about the song (403 not in this country or plan, 404 gone, 401 subStatus
/// 4005 not ready, a protected stream) makes it unplayable. No answer at
/// all (no network, a timeout, a connection refused or reset, a TLS error
/// from a captive portal), a server error and a rate limit are outages:
/// [`crate::tidal_engine::catalog::unreachable`], so the song waits for
/// the network instead of being skipped. Offline mode's refusal is kept as
/// it is: the app's setting, not an outage.
fn engine_error(error: super::ApiError) -> anyhow::Error {
    use super::ApiError;
    let outage = match &error {
        ApiError::Network(why) => !why.contains(crate::http::OFFLINE_MODE_ERROR),
        ApiError::Status { status, .. } => *status >= 500,
        ApiError::RateLimited => true,
        _ => false,
    };
    if outage {
        crate::tidal_engine::catalog::unreachable(error)
    } else {
        anyhow!("{error}")
    }
}

impl Catalog for TidalCatalog {
    fn context_pages(
        &self,
        context_uri: &str,
        deliver: &mut (dyn FnMut(Vec<LocalTrack>) -> bool + Send),
    ) -> anyhow::Result<()> {
        self.block(
            self.context_into(context_uri, MAX_CONTEXT_TRACKS, true, &mut |page| {
                deliver(playable_by_settings(page, ContentFilter::engine()))
            }),
        )?
        .with_context(|| format!("loading {context_uri}"))
    }

    /// A playlist's songs keep their own entries, as its page numbers its
    /// rows; the songs left out (unavailable, or refused by the content
    /// settings) leave their numbers unused. Only a playlist's rows are
    /// asked for: anything else is numbered as it is delivered.
    fn context_rows(
        &self,
        context_uri: &str,
        deliver: &mut (dyn FnMut(Vec<(u32, LocalTrack)>) -> bool + Send),
    ) -> anyhow::Result<()> {
        let Some(id) = context_uri.strip_prefix("tidal:playlist:") else {
            return crate::tidal_engine::catalog::numbered(deliver, |pages| {
                self.context_pages(context_uri, pages)
            });
        };
        let filter = ContentFilter::engine();
        let client = &self.client;
        self.block(async move {
            let mut offset = 0;
            let mut count = 0usize;
            loop {
                let page = client.playlist_track_rows(id, offset, PAGE).await?;
                let next = page.next_offset();
                count += page.items.len();
                let rows = page
                    .items
                    .into_iter()
                    .filter(|(_, track)| filter.allows(track) && plays(track))
                    .map(|(row, track)| (row, local_track(track)))
                    .collect();
                if !deliver(rows) {
                    break;
                }
                match next {
                    Some(next) if count < MAX_CONTEXT_TRACKS => offset = next,
                    _ => break,
                }
            }
            anyhow::Ok(())
        })?
        .with_context(|| format!("loading {context_uri}"))
    }

    fn context_tracks(&self, context_uri: &str) -> anyhow::Result<Vec<LocalTrack>> {
        let tracks = self
            .block(self.context(context_uri, MAX_CONTEXT_TRACKS))?
            .with_context(|| format!("loading {context_uri}"))?;
        Ok(playable_by_settings(tracks, ContentFilter::engine()))
    }

    fn tracks(&self, uris: &[String]) -> anyhow::Result<Vec<LocalTrack>> {
        // Songs and music videos, which play alike.
        let ids: Vec<(bool, String)> = uris
            .iter()
            .filter_map(|uri| {
                uri.strip_prefix("tidal:track:")
                    .map(|id| (false, id.to_string()))
                    .or_else(|| {
                        uri.strip_prefix("tidal:video:")
                            .map(|id| (true, id.to_string()))
                    })
            })
            .collect();
        let client = self.client.clone();
        let found = self.block(async move {
            let mut set = tokio::task::JoinSet::new();
            for (index, (video, id)) in ids.into_iter().enumerate() {
                let client = client.clone();
                set.spawn(async move {
                    let found = if video {
                        client.video(&id).await.map(local_video)
                    } else {
                        client.track(&id).await.map(local_track)
                    };
                    (index, found)
                });
            }
            let mut found = Vec::new();
            while let Some(joined) = set.join_next().await {
                match joined {
                    Ok((index, Ok(track))) => found.push((index, track)),
                    Ok((_, Err(error))) => log::debug!("TIDAL track lookup failed: {error}"),
                    Err(_) => {}
                }
            }
            found.sort_by_key(|(index, _)| *index);
            found
                .into_iter()
                .map(|(_, track)| track)
                .collect::<Vec<_>>()
        })?;
        Ok(found)
    }

    fn stream(&self, track_id: &str, quality: Quality) -> anyhow::Result<StreamInfo> {
        self.block(self.client.playback_info(track_id, quality))?
            .map_err(engine_error)
    }

    fn video_stream(&self, video_id: &str) -> anyhow::Result<StreamInfo> {
        self.block(self.client.video_playback_info(video_id))?
            .map_err(engine_error)
    }

    fn autoplay(&self, seed_track_id: &str) -> anyhow::Result<Vec<LocalTrack>> {
        let tracks = self
            .block(self.client.track_radio(seed_track_id, 50))?
            .map_err(engine_error)?;
        let seed = format!("tidal:track:{seed_track_id}");
        Ok(playable_by_settings(tracks, ContentFilter::engine())
            .into_iter()
            .filter(|track| track.uri != seed)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiError;
    use crate::tidal_engine::catalog::is_unreachable;

    fn outage(error: ApiError) -> bool {
        is_unreachable(&format!("{:#}", engine_error(error)))
    }

    /// A catalogue against `base`, signed in, on its own runtime.
    fn catalog_at(base: &str) -> (TidalCatalog, tokio::runtime::Runtime) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let client = TidalClient::with_bases(
            http,
            Arc::new(crate::api::NetActivity::default()),
            base,
            base,
            base,
        );
        let far = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let refresher = Arc::new(crate::auth::tidal::tests::FakeRefresher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail: false,
        });
        client.set_tokens(Some(crate::auth::tidal::TidalTokens::new(
            crate::auth::tidal::tests::grant("token", far),
            refresher,
            None,
        )));
        let catalog = TidalCatalog::new(Arc::new(client), runtime.handle().clone());
        (catalog, runtime)
    }

    /// Through the real client: a server that is not there (stopped, or the
    /// network gone) is out of reach, so the engine waits; TIDAL's refusal
    /// of the song is not, so the engine skips it.
    #[test]
    fn a_stopped_server_is_out_of_reach_and_a_refusal_is_not() {
        let stopped = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", stopped.local_addr().unwrap());
        drop(stopped);
        let (catalog, _runtime) = catalog_at(&base);
        let error = catalog.stream("1", Quality::High).unwrap_err();
        assert!(is_unreachable(&format!("{error:#}")), "{error:#}");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let body = r#"{"status":403,"subStatus":4032,"userMessage":"Asset is not available in your location"}"#;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        let (catalog, _runtime) = catalog_at(&base);
        let error = catalog.stream("1", Quality::High).unwrap_err();
        let error = format!("{error:#}");
        assert!(!is_unreachable(&error), "{error}");
        assert!(error.contains("country"), "TIDAL's reason: {error}");
    }

    #[test]
    fn only_an_answer_about_the_song_makes_it_unplayable() {
        // No answer, or no real one: the song waits for the network.
        for error in [
            ApiError::Network("dns error: no such host is known".into()),
            ApiError::Network("error sending request: connection refused".into()),
            ApiError::Network("connection reset by peer".into()),
            ApiError::Network("operation timed out".into()),
            ApiError::Network("invalid peer certificate: UnknownIssuer".into()),
            ApiError::Status {
                status: 503,
                message: "TIDAL answered HTTP 503.".into(),
            },
            ApiError::Status {
                status: 500,
                message: "TIDAL: Internal Server Error".into(),
            },
            ApiError::RateLimited,
        ] {
            assert!(outage(error.clone()), "{error:?} is an outage");
        }
        // TIDAL's own word about the song: it is skipped.
        for error in [
            ApiError::Status {
                status: 403,
                message: "TIDAL: Asset is not available in your location".into(),
            },
            ApiError::Status {
                status: 404,
                message: "TIDAL: Track not found".into(),
            },
            ApiError::Status {
                status: 401,
                message: "TIDAL: Asset is not ready for playback".into(),
            },
            ApiError::Decode("malformed manifest".into()),
            // Offline mode is the setting, not an outage.
            ApiError::Network(crate::http::OFFLINE_MODE_ERROR.into()),
        ] {
            assert!(!outage(error.clone()), "{error:?} is TIDAL's answer");
        }
    }

    fn track(id: &str, explicit: bool, ai: bool) -> Track {
        Track {
            id: Some(id.into()),
            uri: format!("tidal:track:{id}"),
            explicit,
            ai,
            ..Track::default()
        }
    }

    #[test]
    fn the_engine_skips_the_songs_the_content_settings_refuse() {
        let tracks = || {
            vec![
                track("e", true, false),
                track("ai", false, true),
                track("ok", false, false),
            ]
        };
        let uris = |filter| {
            playable_by_settings(tracks(), filter)
                .into_iter()
                .map(|track| track.uri)
                .collect::<Vec<_>>()
        };
        assert_eq!(uris(ContentFilter::default()).len(), 3);
        assert_eq!(
            uris(ContentFilter {
                explicit: false,
                ai: true
            }),
            ["tidal:track:ai", "tidal:track:ok"]
        );
        assert_eq!(
            uris(ContentFilter {
                explicit: true,
                ai: false
            }),
            ["tidal:track:e", "tidal:track:ok"]
        );
        // Downloads read the whole list.
        assert_eq!(playable(tracks()).len(), 3);
    }
}
