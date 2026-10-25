//! The app side of plugins: the Luna runtime's snapshot and events, the cancelable-action gate and the
//! requests plugins make of the app.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::api::models::{SearchResults, TopHit};
use crate::luna::{
    HostCommand, LunaEvent, NotifyLevel, PlaybackSnapshot, PlaybackState as LunaState, QueueEntry,
    QueueSource, RepeatMode as LunaRepeat, WindowState,
};
use crate::plugins::gate::{self, GateEnv, Outcome};
use crate::plugins::luna_host::HostRequest;
use crate::tidal_engine::{AudioOrigin, SourceUrl};

/// Demo builds only: `TIDALFAST_DEMO_LUNA` runs Luna in the (offline) demo
/// and installs the comma separated plugin URLs it lists (`1` for none), so
/// plugins can be checked by eye in an off-screen demo window.
pub(super) fn demo_luna() -> Option<&'static [String]> {
    #[cfg(feature = "demo")]
    {
        static URLS: std::sync::OnceLock<Option<Vec<String>>> = std::sync::OnceLock::new();
        URLS.get_or_init(|| {
            let value = std::env::var("TIDALFAST_DEMO_LUNA").ok()?;
            Some(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|url| url.starts_with("http") || url.starts_with("file:"))
                    .map(str::to_string)
                    .collect(),
            )
        })
        .as_deref()
    }
    #[cfg(not(feature = "demo"))]
    None
}

/// Whether the window must keep its mode: the off-screen development
/// window (`TIDALFAST_WINDOW_POS`) and the Luna harness
/// (`TIDALFAST_DEMO_LUNA`) never fill a real display, whatever a test, a
/// scripted click or a plugin asks. A plugin's request for fullscreen is
/// emulated there instead (plugins see the window fullscreen in the window
/// as it is) and logged, so its fullscreen view can be checked off the
/// screen. Always false in release builds.
pub(super) fn window_held() -> bool {
    #[cfg(feature = "demo")]
    {
        demo_luna().is_some() || std::env::var_os("TIDALFAST_WINDOW_POS").is_some()
    }
    #[cfg(not(feature = "demo"))]
    false
}

fn tidal_id(uri: &str, kind: &str) -> Option<String> {
    tidal_id_str(uri, kind).map(str::to_string)
}

/// [`tidal_id`], borrowed from the URI: the part after `tidal:<kind>:`, up
/// to the next `:`. Every pass reads the queue's ids through it (2,000
/// queued songs: 0.28 ms with a `split` iterator).
fn tidal_id_str<'a>(uri: &'a str, kind: &str) -> Option<&'a str> {
    let rest = uri
        .strip_prefix("tidal:")?
        .strip_prefix(kind)?
        .strip_prefix(':')?;
    Some(rest.split_once(':').map_or(rest, |(id, _)| id))
}

fn track_uri(id: &str) -> String {
    format!("tidal:track:{id}")
}

/// The words the app shows for song `id`, as TIDAL's lyrics JSON
/// (`subtitles` in LRC when they are timed).
fn luna_lyrics(id: &str, lyrics: &crate::lyrics::Lyrics) -> Value {
    let plain: Vec<&str> = lyrics.lines.iter().map(|x| x.text.as_str()).collect();
    let subtitles = lyrics.synced.then(|| {
        lyrics
            .lines
            .iter()
            .map(|x| {
                let at = x.at_ms.unwrap_or(0);
                format!(
                    "[{:02}:{:02}.{:02}] {}",
                    at / 60_000,
                    at / 1000 % 60,
                    at % 1000 / 10,
                    x.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    json!({
        "trackId": id,
        "lyricsProvider": "Tidalfast",
        "lyrics": plain.join("\n"),
        "subtitles": subtitles,
        "isRightToLeft": false,
    })
}

#[cfg(test)]
thread_local! {
    /// Tracks made into TIDAL JSON on this thread ([`track_json`]).
    static TRACK_JSONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Queue rows made for plugins on this thread (`App::luna_queue`).
    static QUEUE_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Times the favourites were made for plugins on this thread
    /// (`App::luna_favorites`).
    static FAVORITE_LISTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A TIDAL v1-shaped track for plugins, from what the app already knows.
fn track_json(track: &Track) -> Value {
    #[cfg(test)]
    TRACK_JSONS.with(|n| n.set(n.get() + 1));
    let id = track
        .id
        .clone()
        .or_else(|| tidal_id(&track.uri, "track"))
        .unwrap_or_default();
    let numeric = |id: &str| id.parse::<u64>().map_or(json!(id), |n| json!(n));
    let artists: Vec<Value> = track
        .artists
        .iter()
        .map(|a| {
            json!({
                "id": a.id.as_deref().map(numeric),
                "name": a.name,
                "type": "MAIN",
            })
        })
        .collect();
    let album = track.album.as_ref().map(|album| {
        // TIDAL covers are `resources.tidal.com/images/<uuid with />/...`.
        let url = pick_image(&album.images, 640);
        let cover = url.and_then(|url| {
            let rest = url.split("/images/").nth(1)?;
            let parts: Vec<&str> = rest.split('/').collect();
            (parts.len() > 1).then(|| parts[..parts.len() - 1].join("-"))
        });
        // Art not on TIDAL's image server goes as a plain URL
        // (`MediaItem.coverUrl()` falls back to it).
        let other = url.filter(|_| cover.is_none());
        json!({ "id": numeric(&album.id), "title": album.name, "cover": cover, "_coverUrl": other })
    });
    json!({
        "id": numeric(&id),
        "title": track.name,
        "isrc": track.external_ids.isrc,
        "duration": track.duration_ms / 1000,
        "explicit": track.explicit,
        "trackNumber": track.track_number,
        "volumeNumber": track.disc_number,
        "artist": artists.first().cloned(),
        "artists": artists,
        "album": album,
        "url": format!("http://www.tidal.com/track/{id}"),
        "streamReady": true,
        "allowStreaming": true,
        // What TIDAL said of its quality: Luna's `bestQuality` and
        // `qualityTags` (Tidal Tags' tags and column colours, star-ratings'
        // gold for HiRes).
        "audioQuality": track.audio_quality,
        "mediaMetadata": { "tags": track.quality_tags },
    })
}

/// The tier a song plays at before its stream has said: its own best
/// (HiRes when TIDAL's tags name it, else its `audioQuality`), no higher
/// than the quality asked of TIDAL. What `playbackContext` names while the
/// song loads, so that a plugin colouring by it at the song's change is
/// right for a HiRes song too.
fn expected_quality(
    track: Option<&Track>,
    asked: crate::tidal_engine::Quality,
) -> crate::tidal_engine::Quality {
    use crate::tidal_engine::Quality;
    let own = track.and_then(|track| {
        if track.quality_tags.iter().any(|tag| tag == "HIRES_LOSSLESS") {
            Some(Quality::HiResLossless)
        } else {
            track.audio_quality.as_deref().and_then(Quality::from_api)
        }
    });
    own.map_or(asked, |own| own.min(asked))
}

/// TIDAL's name of a now-playing tab (`settings.nowPlayingActiveView`).
fn tidal_view(tab: Option<crate::model::NowPlayingTab>) -> Value {
    use crate::model::NowPlayingTab as T;
    match tab {
        Some(T::Lyrics) => json!("lyrics"),
        Some(T::Credits) => json!("credits"),
        Some(T::SimilarTracks) => json!("similar"),
        None => Value::Null,
    }
}

/// TIDAL's `settings/SET_NOW_PLAYING_ACTIVE_VIEW` when the now-playing tab
/// is not the one plugins were last `told` of (none until one is chosen).
fn view_event(
    told: &mut Option<Option<crate::model::NowPlayingTab>>,
    tab: Option<crate::model::NowPlayingTab>,
) -> Option<LunaEvent> {
    if *told == Some(tab) {
        return None;
    }
    let first = told.is_none();
    *told = Some(tab);
    (!first || tab.is_some()).then(|| LunaEvent::Action {
        kind: "settings/SET_NOW_PLAYING_ACTIVE_VIEW".into(),
        payload: tidal_view(tab),
    })
}

fn luna_repeat(mode: RepeatMode) -> LunaRepeat {
    match mode {
        RepeatMode::Off => LunaRepeat::Off,
        RepeatMode::Context => LunaRepeat::All,
        RepeatMode::Track => LunaRepeat::One,
    }
}

fn app_repeat(mode: LunaRepeat) -> RepeatMode {
    match mode {
        LunaRepeat::Off => RepeatMode::Off,
        LunaRepeat::All => RepeatMode::Context,
        LunaRepeat::One => RepeatMode::Track,
    }
}

impl App {
    /// The quality the song `id` is expected at before its stream says:
    /// what plugins see in the badge and the playback context until then.
    pub(crate) fn expected_stream_quality(&self, id: Option<&str>) -> crate::tidal_engine::Quality {
        expected_quality(
            id.and_then(|id| self.track_cache.get(id)),
            self.settings.quality(),
        )
    }

    /// Offer a user action to Luna plugins first; `true` when the caller
    /// should go on and apply it now.
    pub(super) fn gate_action(&mut self, action: &Action, ctx: &egui::Context) -> bool {
        if std::mem::take(&mut self.plugins.gate_cleared) {
            return true;
        }
        if self.plugins.luna.is_none() && self.plugins.gate.is_idle() {
            return true;
        }
        let env = self.gate_env(action);
        let outcomes = self
            .plugins
            .gate
            .offer(self.plugins.luna.as_ref(), action.clone(), &env);
        self.apply_gate_outcomes(outcomes, ctx);
        if !self.plugins.gate.is_idle() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        false
    }

    fn apply_gate_outcomes(&mut self, outcomes: Vec<Outcome>, ctx: &egui::Context) {
        for outcome in outcomes {
            match outcome {
                Outcome::Go(action) => {
                    // A track is about to start: plugins that swap it (a
                    // better version) get to see it first.
                    if let (Some(id), Some(luna)) = (
                        gate::starts_track(&action, &GateEnv::default()),
                        &self.plugins.luna,
                    ) {
                        luna.event(LunaEvent::Action {
                            kind: "playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION".into(),
                            payload: json!({
                                "mediaProduct": { "productId": id, "productType": "track" },
                            }),
                        });
                    }
                    if let (
                        Action::AddToPlaylist {
                            playlist_id, items, ..
                        },
                        Some(luna),
                    ) = (&action, &self.plugins.luna)
                    {
                        luna.event(LunaEvent::Action {
                            kind: "content/ADD_MEDIA_ITEMS_TO_PLAYLIST_SUCCESS".into(),
                            payload: json!({
                                "playlistUUID": playlist_id,
                                "mediaItemIdsToAdd": items
                                    .iter()
                                    .filter_map(|i| tidal_id(i.uri(), "track"))
                                    .collect::<Vec<_>>(),
                            }),
                        });
                    }
                    self.apply_cleared(action, ctx)
                }
                Outcome::Cancelled(action) => {
                    log::info!("a Luna plugin cancelled {action:?}");
                }
            }
        }
    }

    /// What TIDAL's actions for `action` carry that only the app knows.
    fn gate_env(&self, action: &Action) -> GateEnv {
        let mut env = GateEnv::default();
        if self.plugins.luna.is_none() {
            return env;
        }
        if let Some(uri) = gate::needs_list(action)
            && let Some((ids, items)) = self.context_list(&uri)
        {
            self.plugins
                .host
                .set_track_list(&gate::track_list_name(&uri), ids.clone(), items);
            env.lists.insert(uri, ids);
        }
        if gate::needs_queue(action) {
            let now = self.now_playing();
            env.queue = self
                .luna_queue(now.as_ref().map(|n| n.uri.as_str()))
                .into_iter()
                .map(|e| (e.uid, e.id))
                .collect();
        }
        if matches!(action, Action::ToggleSaved(_)) {
            env.saved = self
                .saved
                .iter()
                .filter(|(_, saved)| **saved)
                .filter_map(|(uri, _)| tidal_id(uri, "track"))
                .collect();
        }
        env
    }

    /// The tracks of a context that are loaded in the app.
    fn context_tracks(&self, uri: &str) -> Option<Vec<&Track>> {
        let mut parts = uri.split(':');
        let _ = parts.next();
        let kind = parts.next()?;
        let id = parts.next()?;
        Some(match kind {
            "album" => self.album_pages.get(id)?.tracks.items.iter().collect(),
            "mix" => self.mix_pages.get(id)?.tracks.items.iter().collect(),
            "toptracks" => self.artist_top_pages.get(id)?.tracks.items.iter().collect(),
            "playlist" => self
                .playlist_pages
                .get(id)?
                .items
                .items
                .iter()
                .filter_map(|it| it.item.as_ref().or(it.track.as_ref()))
                .map(|p| {
                    let PlayableItem::Track(t) = p;
                    t
                })
                .collect(),
            _ => return None,
        })
    }

    /// The tracks of a context that are loaded in the app, as ids and
    /// TIDAL-shaped JSON.
    #[allow(clippy::type_complexity)]
    pub(super) fn context_list(&self, uri: &str) -> Option<(Vec<String>, Vec<(String, Value)>)> {
        let mut ids = Vec::new();
        let mut items = Vec::new();
        for t in self.context_tracks(uri)? {
            let Some(id) = t.id.clone().or_else(|| tidal_id(&t.uri, "track")) else {
                continue;
            };
            items.push((id.clone(), track_json(t)));
            ids.push(id);
        }
        (!ids.is_empty()).then_some((ids, items))
    }

    /// How many tracks [`Self::context_list`] lists for `uri`, counted
    /// without making their JSON.
    fn context_list_len(&self, uri: &str) -> usize {
        self.context_tracks(uri).map_or(0, |tracks| {
            tracks
                .iter()
                .filter(|t| t.id.is_some() || tidal_id(&t.uri, "track").is_some())
                .count()
        })
    }

    /// TIDAL keeps the open album's, playlist's or mix's track list in
    /// `content.trackLists` while its page shows: scroll-to-playing finds
    /// the playing row's place in it to scroll to a row the list has not
    /// laid out. Given again when the page or its length changes; on every
    /// other pass only the length is counted (making every song's JSON to
    /// compare its length cost a 2,000-song playlist 15 ms a pass).
    fn luna_page_list(&mut self) {
        let uri = match self.page() {
            Page::Album(id) => format!("tidal:album:{id}"),
            Page::Playlist(id) => format!("tidal:playlist:{id}"),
            Page::Mix(id) => format!("tidal:mix:{id}"),
            _ => {
                self.plugins.seen.page_list = None;
                return;
            }
        };
        let len = self.context_list_len(&uri);
        if len == 0 {
            return;
        }
        let key = (uri, len);
        if self.plugins.seen.page_list.as_ref() == Some(&key) {
            return;
        }
        let Some((ids, items)) = self.context_list(&key.0) else {
            return;
        };
        self.plugins
            .host
            .set_track_list(&gate::track_list_name(&key.0), ids, items);
        self.plugins.seen.page_list = Some(key);
    }

    /// The play queue as plugins see it: the playing track, then Next up.
    /// Row `uid`s are the track id and which copy of it this is, so a row
    /// keeps its uid when others before it go.
    fn luna_queue(&self, playing_uri: Option<&str>) -> Vec<QueueEntry> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        self.luna_queue_ids(playing_uri)
            .map(|id| {
                #[cfg(test)]
                QUEUE_ROWS.with(|n| n.set(n.get() + 1));
                let n = seen.entry(id).or_default();
                let row = QueueEntry {
                    uid: format!("{id}:{n}"),
                    id: id.to_string(),
                    kind: "track".into(),
                };
                *n += 1;
                row
            })
            .collect()
    }

    /// The track ids of [`Self::luna_queue`]'s rows in order, read from the
    /// app's queue without making anything: a pass compares them with the
    /// rows plugins have.
    fn luna_queue_ids<'a>(
        &'a self,
        playing_uri: Option<&'a str>,
    ) -> impl Iterator<Item = &'a str> + 'a {
        let loaded = self.queue.get().filter(|q| !q.queue.is_empty());
        // The remembered song, shown paused before anything plays: what
        // follows it is the queue it was saved with (songs queued by hand,
        // then the rest of its context or radio), as Next plays it.
        let resumed = (loaded.is_none() && self.resume_only())
            .then(|| {
                self.resume_queue
                    .iter()
                    .chain(&self.resume_upcoming)
                    .map(String::as_str)
            })
            .into_iter()
            .flatten();
        let next_up = loaded
            .into_iter()
            .flat_map(|q| q.queue.iter().map(PlayableItem::uri));
        playing_uri
            .into_iter()
            .chain(next_up)
            .chain(resumed)
            .filter_map(|uri| tidal_id_str(uri, "track"))
    }

    /// What plugins read of the queue and of the library, kept from pass to
    /// pass and shared with every snapshot ([`crate::plugins::Seen`]): the
    /// queue's rows, its songs as TIDAL JSON and the favourites are made
    /// again only when what they are made of changes. Every pass made all of
    /// them again and copied the songs' JSON into the snapshot, which was
    /// then compared whole with the one plugins had.
    fn luna_shared(&mut self, now: Option<&NowPlaying>) {
        // The rows are the queue's only while something plays.
        let playing_uri = now.map(|now| now.uri.as_str());
        let rows = &self.plugins.seen.queue_rows;
        let same = if now.is_some() {
            rows.iter()
                .map(|row| row.id.as_str())
                .eq(self.luna_queue_ids(playing_uri))
        } else {
            rows.is_empty()
        };
        if !same {
            let rows = if now.is_some() {
                self.luna_queue(playing_uri)
            } else {
                Vec::new()
            };
            self.plugins.seen.queue_rows = Arc::new(rows);
            self.plugins.seen.queue_made += 1;
        }
        let items_key = (self.plugins.seen.queue_made, self.track_cache.len());
        if self.plugins.seen.items_key != Some(items_key) {
            let items = self.luna_queue_items(&self.plugins.seen.queue_rows);
            self.plugins.seen.items = Arc::new(items);
            self.plugins.seen.items_key = Some(items_key);
        }
        let print = self.favorites_print();
        if self.plugins.seen.favorites_key != Some(print) {
            self.plugins.seen.favorite_lists = self.luna_favorites().map(Arc::new);
            self.plugins.seen.favorites_key = Some(print);
        }
    }

    /// Favourite ids as TIDAL's `favorites` slice: the tracks, albums and
    /// artists.
    fn luna_favorites(&self) -> [Vec<String>; 3] {
        #[cfg(test)]
        FAVORITE_LISTS.with(|n| n.set(n.get() + 1));
        let mut tracks: Vec<String> = self
            .saved
            .iter()
            .filter(|(_, saved)| **saved)
            .filter_map(|(uri, _)| tidal_id(uri, "track"))
            .collect();
        tracks.sort();
        // Saved albums and followed artists: what the library lists have
        // loaded plus what was saved or followed here, as TIDAL's
        // `favorites.albums` / `favorites.artists` (LuckyLP picks a random
        // saved album from the first).
        let mut albums: std::collections::BTreeSet<String> = self
            .library
            .albums
            .items
            .iter()
            .map(|saved| saved.album.id.clone())
            .collect();
        let mut artists: std::collections::BTreeSet<String> = self
            .library
            .artists
            .items
            .iter()
            .map(|artist| artist.id.clone())
            .collect();
        for (uri, saved) in &self.saved {
            if let Some(id) = tidal_id(uri, "album") {
                if *saved {
                    albums.insert(id);
                } else {
                    albums.remove(&id);
                }
            } else if let Some(id) = tidal_id(uri, "artist") {
                if *saved {
                    artists.insert(id);
                } else {
                    artists.remove(&id);
                }
            }
        }
        [
            tracks,
            albums.into_iter().filter(|id| !id.is_empty()).collect(),
            artists.into_iter().filter(|id| !id.is_empty()).collect(),
        ]
    }

    /// What [`Self::luna_favorites`] is made of, as one number: the saved
    /// flags' revision and the library's albums and artists. Read every
    /// pass; nothing is made.
    fn favorites_print(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.saved.revision().hash(&mut h);
        self.library.albums.items.len().hash(&mut h);
        for saved in &self.library.albums.items {
            saved.album.id.hash(&mut h);
        }
        self.library.artists.items.len().hash(&mut h);
        for artist in &self.library.artists.items {
            artist.id.hash(&mut h);
        }
        h.finish()
    }

    /// The queue's songs as TIDAL JSON, for `content.mediaItems`: from the
    /// app's cache, else from the queue's own rows, which carry the whole
    /// song. A plugin reading a row's length (Total Play Time sums the whole
    /// queue every second) then finds it at once; a song the store lacks is
    /// fetched from TIDAL one by one, and those answers arrive late and out
    /// of order, so the sum jumps between the queue as it was and as it is.
    fn luna_queue_items(&self, queue: &[QueueEntry]) -> std::collections::BTreeMap<String, Value> {
        let rows: HashMap<String, &Track> = self
            .queue
            .get()
            .into_iter()
            .flat_map(|q| q.queue.iter())
            .filter_map(|item| {
                let PlayableItem::Track(track) = item;
                tidal_id(&track.uri, "track").map(|id| (id, track))
            })
            .collect();
        queue
            .iter()
            .filter_map(|e| {
                self.track_cache
                    .get(&e.id)
                    .or_else(|| rows.get(&e.id).copied())
                    .map(|t| (e.id.clone(), track_json(t)))
            })
            .collect()
    }

    /// Apply an action plugins have already seen (or that came from one).
    fn apply_cleared(&mut self, action: Action, ctx: &egui::Context) {
        self.plugins.gate_cleared = true;
        self.apply(action, ctx);
        self.plugins.gate_cleared = false;
    }

    /// The plugins' work for this frame.
    pub(super) fn plugins_tick(&mut self, ctx: &egui::Context) {
        // Luna on or off follows the master switch.
        let wanted = (self.settings.plugins.luna_enabled && !self.offline) || demo_luna().is_some();
        // In TIDAL plugins load after the sign-in: they read the account
        // (`session.userId`) as they start. Hold Luna until the account is
        // known, or for 15 s when TIDAL does not answer.
        let signing_in = !self.offline
            && self.user.is_none()
            && matches!(
                self.auth,
                crate::backend::AuthStatus::Starting
                    | crate::backend::AuthStatus::Connecting
                    | crate::backend::AuthStatus::Connected { .. }
            );
        let hold = wanted
            && self.plugins.luna.is_none()
            && signing_in
            && self
                .plugins
                .held_since
                .get_or_insert_with(std::time::Instant::now)
                .elapsed()
                < Duration::from_secs(15);
        if hold {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if wanted && !hold && self.plugins.luna.is_none() && self.plugins.luna_error.is_none() {
            // Plugins read the state as they load: the account and what is
            // playing are there before the first sync.
            let now = self.now_playing();
            let snap = self.luna_playback(now.as_ref());
            self.plugins.host.set_snapshot(snap);
            self.plugins.start_luna(self.dirs.luna_dir());
            if let (Some(luna), Some(urls)) = (&self.plugins.luna, demo_luna()) {
                // Installed ones load from disk; installing again would reload them.
                let installed = std::fs::read_to_string(self.dirs.luna_dir().join("plugins.json"))
                    .unwrap_or_default();
                for url in urls.iter().filter(|url| !installed.contains(url.as_str())) {
                    if url.ends_with(".css") {
                        luna.install_theme(url);
                    } else {
                        luna.install(url);
                    }
                }
            }
        } else if !wanted && self.plugins.luna.is_some() {
            self.plugins.stop_luna();
        }
        if !wanted {
            self.plugins.luna_error = None;
        }

        let outcomes = self.plugins.gate.poll();
        self.apply_gate_outcomes(outcomes, ctx);
        if !self.plugins.gate.is_idle() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        for request in self.plugins.host.take_requests() {
            self.handle_host_request(request, ctx);
        }

        let now = self.now_playing();

        if self.plugins.luna.is_some() {
            let window = ctx.input(|i| {
                let v = i.viewport();
                WindowState {
                    fullscreen: v.fullscreen.unwrap_or(false)
                        || crate::ui::plugin_dom::fullscreen_emulated(),
                    maximized: v.maximized.unwrap_or(false),
                    minimized: v.minimized.unwrap_or(false),
                    focused: v.focused.unwrap_or(true),
                }
            });
            self.plugins.host.set_window(window);
            self.plugins
                .host
                .set_next_pass(self.next_pass().map(|due| std::time::Instant::now() + due));
            let hidden = self.luna_hidden(ctx, window);
            self.plugins.host.set_hidden(hidden);
            self.luna_visibility(hidden);
            let screens = crate::luna::screen::changes();
            if self.plugins.seen.screens != screens {
                self.plugins.seen.screens = screens;
                if let Some(luna) = &self.plugins.luna {
                    luna.screens_changed();
                }
            }
            if self.plugins.seen.focused != Some(window.focused) {
                self.plugins.seen.focused = Some(window.focused);
                if let Some(luna) = &self.plugins.luna {
                    luna.set_focused(window.focused);
                }
            }
            self.luna_sync(now.as_ref(), window);
            self.luna_background(ctx);
            #[cfg(feature = "demo")]
            if demo_luna().is_some() || std::env::var_os("TIDALFAST_LUNA_DUMP").is_some() {
                self.demo_luna_dump();
            }
        }
    }

    /// Tell plugins when the window is hidden (minimized, closed to the tray)
    /// or shown again; their timers and animation frames slow down while it
    /// is, as in a browser's background tab.
    /// Whether plugins see the page hidden (`document.hidden`: frames
    /// paused, timers at most once a second): nobody can see the window,
    /// because it is minimised, closed to the tray or covered by other
    /// windows.
    pub(super) fn luna_hidden(&self, ctx: &egui::Context, window: WindowState) -> bool {
        window.minimized || self.window_hidden || crate::occlusion::covered(ctx)
    }

    /// After drawing: tells plugins whether the Plugins tab, where their
    /// Settings are mounted, is on screen, when that changed.
    pub(crate) fn luna_settings_view(&mut self) {
        let open = std::mem::take(&mut self.plugins.settings_drawn);
        if self.plugins.seen.settings_open == Some(open) {
            return;
        }
        if let Some(luna) = &self.plugins.luna {
            self.plugins.seen.settings_open = Some(open);
            luna.set_settings_open(open);
        }
    }

    pub(crate) fn luna_visibility(&mut self, hidden: bool) {
        if self.plugins.seen.hidden == Some(hidden) {
            return;
        }
        self.plugins.seen.hidden = Some(hidden);
        if let Some(luna) = &self.plugins.luna {
            luna.set_hidden(hidden);
        }
    }

    /// Luna's background work the user should hear about, and its
    /// emergency theme shortcut (Ctrl+Shift+T turns every theme off).
    fn luna_background(&mut self, ctx: &egui::Context) {
        let Some(luna) = self.plugins.luna.clone() else {
            return;
        };
        for u in luna.background_updates_since(self.plugins.updates_seen) {
            self.plugins.updates_seen = self.plugins.updates_seen.max(u.seq);
            let text = crate::i18n::gettext(self.locale, "{name} updated to {version}")
                .replace("{name}", &u.name)
                .replace("{version}", u.version.as_deref().unwrap_or("a new build"));
            self.toast(text);
        }
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::T)) {
            luna.disable_themes();
        }
    }

    /// Demo builds with `TIDALFAST_DEMO_LUNA` (or `TIDALFAST_LUNA_DUMP` in a
    /// real, signed-in session): every two seconds, the plugins'
    /// status and logs and the DOM overlay go to `luna-dump.json` in the
    /// state directory, for checking plugins in an off-screen window.
    ///
    /// The JSON is made and written on a thread of its own: with a theme's
    /// overlay it took tens of milliseconds of the window's thread, every
    /// two seconds, which showed in the demo bench's slowest frames. A dump
    /// still being written when the next is due skips that one.
    #[cfg(feature = "demo")]
    fn demo_luna_dump(&self) {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};
        static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        static WRITING: AtomicBool = AtomicBool::new(false);
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(std::time::Instant::now());
        let Some(luna) = &self.plugins.luna else {
            return;
        };
        if WRITING.swap(true, Ordering::AcqRel) {
            return;
        }
        let snap = luna.snapshot();
        let overlay = luna.dom_overlay();
        // `TIDALFAST_DEMO_LUNA_DUMP_LOGS=<n>`: how many log lines per plugin.
        let keep = std::env::var("TIDALFAST_DEMO_LUNA_DUMP_LOGS")
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(15);
        let path = self.dirs.luna_dir().with_file_name("luna-dump.json");
        demo_off_thread(move || {
            let plugins: Vec<Value> = snap
                .plugins
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "status": format!("{:?}", p.status),
                        "logs": p.logs.iter().rev().take(keep).map(|l| format!("{:?} {}", l.level, l.message)).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let dump = json!({
                "plugins": plugins,
                "runtime": snap.runtime_log.iter().rev().take(15).map(|l| l.message.clone()).collect::<Vec<_>>(),
                "overlay": &*overlay,
            });
            let _ = std::fs::write(path, serde_json::to_vec_pretty(&dump).unwrap_or_default());
            WRITING.store(false, Ordering::Release);
        });
    }

    /// The state Luna plugins read.
    pub(super) fn luna_playback(&mut self, now: Option<&NowPlaying>) -> PlaybackSnapshot {
        self.luna_shared(now);
        let seen = &self.plugins.seen;
        let [favorite_tracks, favorite_albums, favorite_artists] = seen.favorite_lists.clone();
        let mut snap = PlaybackSnapshot {
            // The account is there whether or not a song is: plugins read
            // `session.userId` as they load, before anything plays
            // (playlist-tools: "Not logged in").
            user_id: self.user_id().map(str::to_string),
            // `session.countryCode` (TidalApi.queryArgs, catalogue reads).
            country_code: self.user.as_ref().and_then(|u| u.country.clone()),
            profile_name: self.user.as_ref().and_then(|u| u.display_name.clone()),
            // Exclusive output exists on Windows (WASAPI) and Linux (ALSA).
            exclusive_mode: cfg!(any(windows, target_os = "linux"))
                .then_some(self.settings.bit_perfect),
            favorite_tracks,
            favorite_albums,
            favorite_artists,
            // The queue's songs, for `content.mediaItems` (empty while
            // nothing plays, as the queue is).
            known_items: Arc::clone(&seen.items),
            ..PlaybackSnapshot::default()
        };
        let Some(now) = now else {
            snap.volume = f64::from(volume_to_percent(self.local.volume));
            return snap;
        };
        snap.state = if now.resuming {
            LunaState::Paused
        } else if now.loading {
            LunaState::Stalled
        } else if now.playing {
            LunaState::Playing
        } else {
            LunaState::Paused
        };
        snap.desired_state = if now.playing {
            LunaState::Playing
        } else {
            LunaState::Paused
        };
        // What is heard, as TIDAL's player reports it and the app's own
        // lyrics follow: the engine's position runs ahead of the speakers
        // by the sound the output has queued.
        snap.position = f64::from(self.lyrics_position_ms(now)) / 1000.0;
        snap.duration = Some(f64::from(now.duration_ms) / 1000.0);
        snap.volume = f64::from(now.volume_percent);
        snap.muted = now.volume_percent == 0;
        snap.shuffle = now.shuffle;
        snap.repeat = luna_repeat(now.repeat);
        if tidal_id_str(&now.uri, "track").is_some() {
            snap.current_index = Some(0);
        }
        snap.queue = Arc::clone(&self.plugins.seen.queue_rows);
        // On this computer's queue a plugin takes out any row (the songs the
        // user queued, and the rest of a context for this pass of it, queue.md
        // rule 12); its clear takes out only the queued ones (rule 7).
        let local = self.queue_locally_reorderable();
        snap.removable_rows = if local { None } else { Some(0) };
        snap.clearable_rows = Some(if local { self.queued_rows_len() } else { 0 });
        snap.source = self.playing_context_uri().and_then(|uri| {
            let mut parts = uri.split(':');
            let _ = parts.next();
            let kind = parts.next()?.to_string();
            let id = parts.next().unwrap_or_default().to_string();
            let kind = if kind == "collection" {
                "favoriteTracks".to_string()
            } else {
                kind
            };
            let name = Page::from_uri(&uri).and_then(|_| self.station_name(&uri));
            Some(QueueSource {
                url: Some(format!("/{kind}/{id}")),
                kind,
                id,
                name,
            })
        });
        snap.current_item = now
            .id
            .as_ref()
            .and_then(|id| self.track_cache.get(id))
            .map(track_json);
        // The stream's format is the playing song's only once the engine
        // plays that song: a song just double-clicked is shown at once,
        // while the engine still reports the one before it, whose format
        // plugins took for the new song's (Tidal Tags' pill kept the
        // previous song's figures). Until the stream says, the context
        // names the tier the song is expected at and no figures.
        let format = self
            .local
            .format
            .as_ref()
            .filter(|_| self.local.track.as_ref().is_some_and(|t| t.uri == now.uri));
        if now.local
            && let Some(format) = format
        {
            snap.audio_quality = format.quality.map(|q| q.api_name().to_string());
            snap.bit_depth = format.bit_depth;
            snap.sample_rate = Some(format.sample_rate);
            snap.codec = Some(format.codec.clone());
            snap.bitrate = format.bitrate;
        } else if now.local {
            let track = now.id.as_ref().and_then(|id| self.track_cache.get(id));
            snap.audio_quality = Some(
                expected_quality(track, self.settings.quality())
                    .api_name()
                    .to_string(),
            );
        }
        if now.local
            && self
                .local
                .track
                .as_ref()
                .is_some_and(|track| track.uri == now.uri)
        {
            snap.replace_ended = self.local.replace_ended;
            snap.replaced_duration = self.local.source_ms.map(|ms| f64::from(ms) / 1000.0);
            snap.replaced_audio = if self.local.replace_failed {
                Some(false)
            } else {
                self.local
                    .format
                    .as_ref()
                    .is_some_and(|f| f.origin == AudioOrigin::Plugin)
                    .then_some(true)
            };
        }
        snap
    }

    /// Refresh what plugins read and tell them what changed.
    pub(super) fn luna_sync(&mut self, now: Option<&NowPlaying>, window: WindowState) {
        // The queue, its songs and the favourites are the ones plugins have
        // until they change ([`Self::luna_shared`]).
        let snap = self.luna_playback(now);
        let now_playing_view = self.now_playing_view;
        let now_playing_tab = self.now_playing_tab;
        let lyrics_state = {
            let id = now.and_then(|n| n.id.clone());
            // The words the app holds are the playing song's only when it
            // looked them up for it (it does for its Lyrics tab, or when a
            // plugin asks): after a song change they are the song before's.
            let current = now.is_some_and(|n| self.lyrics_uri.as_deref() == Some(n.uri.as_str()));
            let status = match &self.lyrics {
                _ if !current => 0u8,
                Loadable::NotLoaded => 0u8,
                Loadable::Loading => 1,
                Loadable::Loaded(Some(l)) if !l.lines.is_empty() => 2,
                Loadable::Loaded(_) | Loadable::Failed(_) => 3,
            };
            id.map(|id| (id, status))
        };
        let lyrics_event = match (&lyrics_state, &self.plugins.seen.lyrics) {
            (Some((id, status)), prev) if prev.as_ref() != lyrics_state.as_ref() => {
                // What `MediaItem.lyrics()` answers when TIDAL has no words
                // for the song: the app's, once its lookup ends.
                match (status, &self.lyrics) {
                    (1, _) => self.plugins.host.set_app_lyrics(id, None),
                    (2, Loadable::Loaded(Some(l))) => {
                        self.plugins
                            .host
                            .set_app_lyrics(id, Some(Some(luna_lyrics(id, l))));
                    }
                    (3, _) => self.plugins.host.set_app_lyrics(id, Some(None)),
                    _ => {}
                }
                match (status, &self.lyrics) {
                    (1, _) => Some(("content/LOAD_ITEM_LYRICS", json!({ "trackId": id }))),
                    (2, Loadable::Loaded(Some(l))) => {
                        Some(("content/LOAD_ITEM_LYRICS_SUCCESS", luna_lyrics(id, l)))
                    }
                    (3, _) => Some(("content/LOAD_ITEM_LYRICS_FAIL", json!({ "trackId": id }))),
                    _ => None,
                }
            }
            _ => None,
        };
        self.luna_page_list();
        let page = crate::plugins::gate::page_path(self.page());
        let search = crate::plugins::gate::page_search(self.page());
        let track_sequence = self.local.track_sequence;
        let seek_sequence = self.local.seek_sequence;
        let mut events = Vec::new();
        let seen = &mut self.plugins.seen;
        if let Some(now) = now {
            let occurrence = (now.uri.clone(), track_sequence);
            if seen.track.as_ref() != Some(&occurrence) {
                seen.track = Some(occurrence);
                if let Some(id) = tidal_id(&now.uri, "track") {
                    events.push(LunaEvent::TrackChanged { id });
                }
                seen.position_s = None;
            }
            if seen.seek_sequence != Some(seek_sequence) {
                if seen.seek_sequence.is_some() {
                    events.push(LunaEvent::Seek(snap.position));
                }
                seen.seek_sequence = Some(seek_sequence);
            }
            let second = snap.position as u32;
            if seen.position_s != Some(second) {
                seen.position_s = Some(second);
                events.push(LunaEvent::Position(snap.position));
            }
        }
        if seen.state != Some(snap.state) {
            seen.state = Some(snap.state);
            events.push(LunaEvent::PlaybackState(snap.state));
        }
        let volume = snap.volume.round() as u8;
        if seen.volume != Some(volume) {
            if seen.volume.is_some() {
                events.push(LunaEvent::Volume(snap.volume));
            }
            seen.volume = Some(volume);
        }
        if seen.shuffle != Some(snap.shuffle) {
            if seen.shuffle.is_some() {
                events.push(LunaEvent::Shuffle(snap.shuffle));
            }
            seen.shuffle = Some(snap.shuffle);
        }
        if seen.repeat != Some(snap.repeat) {
            if seen.repeat.is_some() {
                events.push(LunaEvent::Repeat(snap.repeat));
            }
            seen.repeat = Some(snap.repeat);
        }
        // The rows are made again exactly when the queue's songs change.
        if seen.queue != Some(seen.queue_made) {
            if seen.queue.is_some() {
                events.push(LunaEvent::QueueChanged);
            }
            seen.queue = Some(seen.queue_made);
        }
        let favorites =
            snap.favorite_tracks.len() + snap.favorite_albums.len() + snap.favorite_artists.len();
        if seen.favorites != Some(favorites) {
            if seen.favorites.is_some() {
                events.push(LunaEvent::FavoritesChanged);
            }
            seen.favorites = Some(favorites);
        }
        let route = format!("{page}{search}");
        if seen.page.as_deref() != Some(route.as_str()) {
            events.push(LunaEvent::Action {
                kind: "router/NAVIGATED".into(),
                payload: json!({ "pathname": page, "path": page, "search": search, "hash": "" }),
            });
            events.push(LunaEvent::Action {
                kind: "page/SET_PAGE_ID".into(),
                payload: json!({ "pageId": page.trim_start_matches('/').replace('/', ".") }),
            });
            seen.page = Some(route);
        }
        if seen.now_playing_view != Some(now_playing_view) {
            if seen.now_playing_view.is_some() || now_playing_view {
                events.push(LunaEvent::Action {
                    kind: if now_playing_view {
                        "view/ENTERED_NOWPLAYING"
                    } else {
                        "view/EXIT_NOWPLAYING"
                    }
                    .into(),
                    payload: Value::Null,
                });
            }
            seen.now_playing_view = Some(now_playing_view);
        }
        events.extend(view_event(&mut seen.now_playing_tab, now_playing_tab));
        if seen.fullscreen != Some(window.fullscreen) {
            if seen.fullscreen.is_none() {
                // Once, as TIDAL's player reports it when it starts.
                events.push(LunaEvent::Action {
                    kind: "view/FULLSCREEN_ALLOWED".into(),
                    payload: json!(true),
                });
                events.push(LunaEvent::Action {
                    kind: "locale/BUNDLE_SWITCH_SUCCESS".into(),
                    payload: json!({ "bundleName": "en" }),
                });
            }
            if seen.fullscreen.is_some() || window.fullscreen {
                events.push(LunaEvent::Action {
                    kind: if window.fullscreen {
                        "view/REQUEST_FULLSCREEN"
                    } else {
                        "view/EXIT_FULLSCREEN"
                    }
                    .into(),
                    payload: Value::Null,
                });
            }
            seen.fullscreen = Some(window.fullscreen);
        }
        if let Some((kind, payload)) = lyrics_event {
            events.push(LunaEvent::Action {
                kind: kind.into(),
                payload,
            });
        }
        seen.lyrics = lyrics_state;
        // The Bit-perfect setting is `player.activeDeviceMode`: announced
        // once as the store starts and again whenever it changes here.
        if seen.exclusive != snap.exclusive_mode {
            if let Some(on) = snap.exclusive_mode {
                events.push(LunaEvent::Action {
                    kind: "player/SET_DEVICE_MODE".into(),
                    payload: json!(if on { "exclusive" } else { "shared" }),
                });
            }
            seen.exclusive = snap.exclusive_mode;
        }
        self.plugins.host.set_snapshot(snap);
        if let Some(luna) = &self.plugins.luna {
            for event in events {
                luna.event(event);
            }
        }
    }

    fn handle_host_request(&mut self, request: HostRequest, ctx: &egui::Context) {
        match request {
            HostRequest::Command(HostCommand::ReplaceAudio { track, url, own }) => {
                // The engine checks the song is still the playing one.
                let uri = track_uri(&track);
                log::debug!("a plugin gave {uri} a source (own song {own})");
                let carrier = (own && self.local.track.is_none())
                    .then(|| self.cached_item(&uri).cloned())
                    .flatten()
                    .map(|track| Box::new(crate::api::tidal_catalog::local_track(track)));
                self.backend.player(PlayerCommand::ReplaceAudio {
                    uri,
                    url: url.map(|u| SourceUrl(u.0)),
                    own,
                    carrier,
                });
            }
            HostRequest::Command(HostCommand::SetExclusiveMode(on)) => {
                // As the Bit-perfect switch in Settings does: the setting,
                // saved, and the engine started again with it.
                if cfg!(any(windows, target_os = "linux")) && self.settings.bit_perfect != on {
                    self.settings.bit_perfect = on;
                    self.apply_cleared(Action::SettingsChanged, ctx);
                    self.apply_cleared(Action::RestartEngine, ctx);
                }
            }
            HostRequest::Command(command) => {
                if self.window_command(&command, ctx) {
                    return;
                }
                if let HostCommand::RetitleTracks(titles) = command {
                    self.retitle_tracks(titles);
                    ctx.request_repaint();
                    return;
                }
                if let HostCommand::SetNowPlayingView(view) = command {
                    use crate::model::NowPlayingTab as T;
                    let tab = match view.as_deref() {
                        Some("lyrics") => Some(T::Lyrics),
                        Some("credits") => Some(T::Credits),
                        Some("similar") => Some(T::SimilarTracks),
                        _ => None,
                    };
                    self.now_playing_tab = tab;
                    if tab == Some(T::Lyrics) {
                        self.lyrics_following = true;
                    }
                    // The plugin's own dispatch is not told back to it.
                    self.plugins.seen.now_playing_tab = Some(tab);
                    ctx.request_repaint();
                    return;
                }
                if let HostCommand::StoreLyrics { track_id, lyrics } = command {
                    self.plugins.store_lyrics = track_id.and_then(|id| {
                        let l = lyrics.as_ref()?;
                        let text = |k: &str| l.get(k).and_then(Value::as_str).unwrap_or("");
                        let words =
                            crate::lyrics::Lyrics::from_texts(text("subtitles"), text("lyrics"))?;
                        Some((track_uri(&id), Loadable::Loaded(Some(words))))
                    });
                    ctx.request_repaint();
                    return;
                }
                for action in self.host_command_actions(command) {
                    self.apply_cleared(action, ctx);
                }
            }
            HostRequest::Notify {
                plugin,
                level,
                message,
            } => {
                let text = match plugin {
                    Some(p) if !p.starts_with("@luna/") => format!("{p}: {message}"),
                    _ => message,
                };
                match level {
                    NotifyLevel::Error => self.toast_error(text),
                    NotifyLevel::Info | NotifyLevel::Warn => self.toast(text),
                }
            }
            HostRequest::OpenUrl(url) => {
                if url.starts_with("tidal://")
                    || url.starts_with("https://tidal.com/")
                    || url.starts_with("https://listen.tidal.com/")
                {
                    self.apply_cleared(Action::OpenLink(url), ctx);
                } else if url.starts_with("https://") || url.starts_with("http://") {
                    self.apply_cleared(Action::OpenUrl(url), ctx);
                }
            }
            HostRequest::Clipboard(text) => {
                // Demo builds: `TIDALFAST_DEMO_CLIPBOARD=<file>` keeps an
                // off-screen check's copies (copy-lyrics) out of the
                // computer's clipboard.
                #[cfg(feature = "demo")]
                let text = match std::env::var_os("TIDALFAST_DEMO_CLIPBOARD") {
                    Some(path) => {
                        let _ = std::fs::write(path, &text);
                        None
                    }
                    None => Some(text),
                };
                #[cfg(not(feature = "demo"))]
                let text = Some(text);
                if let Some(text) = text {
                    ctx.copy_text(text);
                }
            }
            HostRequest::PlaylistChanged(id) => self.plugin_changed_playlist(id),
            HostRequest::Lyrics => self.request_lyrics(),
        }
    }

    /// A plugin changed a playlist of the account (playlist-tools removing
    /// duplicates): what the app holds of it is out of date. Several changes
    /// in a row (one request per removed song) are read again once, after a
    /// short quiet ([`App::refresh_plugin_playlists`]).
    pub(super) fn plugin_changed_playlist(&mut self, id: Option<String>) {
        let (stale, at) = self
            .plugin_playlists_stale
            .get_or_insert_with(|| (Vec::new(), Instant::now()));
        if !stale.contains(&id) {
            stale.push(id);
        }
        *at = Instant::now() + PLUGIN_PLAYLIST_QUIET;
    }

    /// Reads again the playlists plugins changed, once they are quiet.
    pub(super) fn refresh_plugin_playlists(&mut self, ctx: &egui::Context) {
        let Some((_, due)) = &self.plugin_playlists_stale else {
            return;
        };
        if Instant::now() < *due {
            ctx.request_repaint_after((*due - Instant::now()).max(Duration::from_millis(50)));
            return;
        }
        let Some((stale, _)) = self.plugin_playlists_stale.take() else {
            return;
        };
        for id in stale.into_iter().flatten() {
            // The user's own edit in flight keeps its page; it reads the
            // answer when it lands.
            if self
                .playlist_pages
                .get(&id)
                .is_some_and(|page| page.pending_writes > 0)
            {
                continue;
            }
            self.playlist_pages.remove(&id);
            if matches!(self.page(), Page::Playlist(current) if *current == id) {
                self.ensure_loaded(Page::Playlist(id));
            }
        }
        self.load_playlists();
    }

    /// Window requests from plugins (`BrowserWindow`, `requestFullscreen`);
    /// `true` when `command` was one.
    fn window_command(&mut self, command: &HostCommand, ctx: &egui::Context) -> bool {
        match command {
            // An off-screen test window never fills a real display: the
            // fullscreen a plugin asks for is emulated in the window as it is.
            HostCommand::SetFullscreen(on) if self.window_held => {
                log::info!(
                    "a plugin asked for fullscreen {on}: emulated, the window keeps its mode"
                );
                crate::ui::plugin_dom::emulate_fullscreen(*on);
                ctx.request_repaint();
            }
            HostCommand::SetMaximized(on) if self.window_held => {
                log::info!("a plugin asked for maximized {on}: the window keeps its mode");
            }
            HostCommand::SetFullscreen(on) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(*on));
            }
            HostCommand::SetMaximized(on) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(*on));
            }
            HostCommand::SetMinSize(limit) => crate::ui::set_plugin_min_size(ctx, *limit),
            HostCommand::Minimize => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            HostCommand::CloseWindow => self.apply_cleared(Action::CloseWindow, ctx),
            HostCommand::OpenEditor {
                window,
                title,
                language,
                text,
            } => {
                self.plugins.editor = Some(crate::plugins::PluginEditor {
                    window: *window,
                    title: title.clone(),
                    language: language.clone(),
                    text: text.clone(),
                });
                ctx.request_repaint();
            }
            HostCommand::CloseEditor(window) => {
                if self
                    .plugins
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.window == *window)
                {
                    self.plugins.editor = None;
                }
            }
            _ => return false,
        }
        true
    }

    /// A track the app has loaded anywhere: the track cache, else the
    /// album, mix, top-track and playlist pages it holds. Plugins name
    /// tracks by id only (quick-queue's row button), and a queue row made
    /// from the bare id would show no title until it plays.
    /// TIDAL's answer to a search, as its redux action: plugins read
    /// (and may retitle) the songs in it.
    pub(super) fn luna_search_answer(&self, query: &str, results: &SearchResults) {
        let Some(luna) = &self.plugins.luna else {
            return;
        };
        let page =
            |total: u32, items: Vec<Value>| json!({ "items": items, "totalNumberOfItems": total });
        let tracks = results.tracks.as_ref();
        let named = |id: &str, name: &str, key: &str| {
            let numeric = id.parse::<u64>().map_or(json!(id), |n| json!(n));
            json!({ "id": numeric, key: name })
        };
        let mut payload = json!({
            "query": query,
            "tracks": page(
                tracks.map_or(0, |p| p.total),
                tracks.map_or_else(Vec::new, |p| p.items.iter().map(track_json).collect()),
            ),
            "albums": page(
                results.albums.as_ref().map_or(0, |p| p.total),
                results.albums.iter().flat_map(|p| &p.items).map(|a| named(&a.id, &a.name, "title")).collect(),
            ),
            "artists": page(
                results.artists.as_ref().map_or(0, |p| p.total),
                results.artists.iter().flat_map(|p| &p.items).map(|a| named(&a.id, &a.name, "name")).collect(),
            ),
            "playlists": page(
                results.playlists.as_ref().map_or(0, |p| p.total),
                results.playlists.iter().flat_map(|p| &p.items).map(|a| named(&a.id, &a.name, "title")).collect(),
            ),
        });
        if let Some(TopHit::Track(track)) = &results.top_hit {
            payload["topHits"] = json!({ "type": "TRACKS", "value": track_json(track) });
        }
        luna.event(LunaEvent::Action {
            kind: "search/SEARCH_RESULT_SUCCESS".into(),
            payload,
        });
    }

    /// A plugin retitled songs in a TIDAL answer: remember the titles the
    /// track rows show. A title equal to the song's own clears it.
    fn retitle_tracks(&mut self, titles: Vec<(String, String)>) {
        /// More than any list on screen; stops a plugin growing the map.
        const MAX_RETITLES: usize = 5000;
        for (id, title) in titles {
            let same = self.known_track(&id).is_some_and(|t| t.name == title);
            if same || title.trim().is_empty() {
                self.plugins.retitles.remove(&id);
            } else if self.plugins.retitles.len() < MAX_RETITLES
                || self.plugins.retitles.contains_key(&id)
            {
                self.plugins.retitles.insert(id, title);
            }
        }
    }

    /// The title a track row shows: the one a plugin gave the song, else
    /// its own.
    pub fn row_title<'a>(&'a self, item: &'a PlayableItem) -> &'a str {
        if self.plugins.retitles.is_empty() {
            return item.name();
        }
        item.id()
            .or_else(|| item.uri().rsplit(':').next())
            .and_then(|id| self.plugins.retitles.get(id))
            .map_or_else(|| item.name(), String::as_str)
    }

    /// `track` as a copy that carries the title a plugin gave it.
    pub fn retitled(&self, track: &Track) -> Track {
        let mut copy = track.clone();
        if !self.plugins.retitles.is_empty()
            && let Some(title) = track
                .id
                .as_deref()
                .or_else(|| track.uri.rsplit(':').next())
                .and_then(|id| self.plugins.retitles.get(id))
        {
            copy.name = title.clone();
        }
        copy
    }

    pub(super) fn known_track(&self, id: &str) -> Option<Track> {
        if let Some(t) = self.track_cache.get(id) {
            return Some(t.clone());
        }
        let is = |t: &&Track| {
            t.id.as_deref() == Some(id) || tidal_id(&t.uri, "track").as_deref() == Some(id)
        };
        let lists = self
            .album_pages
            .values()
            .flat_map(|p| p.tracks.items.iter())
            .chain(self.mix_pages.values().flat_map(|p| p.tracks.items.iter()))
            .chain(
                self.artist_top_pages
                    .values()
                    .flat_map(|p| p.tracks.items.iter()),
            );
        if let Some(t) = lists.clone().find(is) {
            return Some(t.clone());
        }
        self.playlist_pages
            .values()
            .flat_map(|p| p.items.items.iter())
            .filter_map(|it| it.item.as_ref().or(it.track.as_ref()))
            .map(|p| {
                let PlayableItem::Track(t) = p;
                t
            })
            .find(is)
            .cloned()
    }

    /// The row a plugin's song takes (queued, or added to a playlist):
    /// TIDAL's row where the app holds one ([`App::known_track`]), else the
    /// fullest copy it holds ([`App::known_item`]), else its address alone,
    /// which the queue then asks TIDAL to complete.
    fn queue_item(&self, id: &str) -> PlayableItem {
        let uri = track_uri(id);
        let track = self
            .known_track(id)
            .or_else(|| self.known_item(&uri))
            .unwrap_or_else(|| Track {
                id: Some(id.to_string()),
                uri: uri.clone(),
                ..Track::default()
            });
        PlayableItem::Track(track)
    }

    /// The actions that carry out a plugin's command.
    fn host_command_actions(&self, command: HostCommand) -> Vec<Action> {
        let playing = self.believed_playing();
        let upcoming = |index: usize| -> Option<(usize, String)> {
            let i = index.checked_sub(1)?;
            let uri = self.queue.get()?.queue.get(i)?.uri().to_string();
            Some((i, uri))
        };
        match command {
            HostCommand::Play if !playing => vec![Action::TogglePlay],
            HostCommand::Pause | HostCommand::Stop if playing => vec![Action::TogglePlay],
            HostCommand::Play | HostCommand::Pause | HostCommand::Stop => Vec::new(),
            HostCommand::TogglePlayback => vec![Action::TogglePlay],
            HostCommand::Seek(seconds) => vec![Action::Seek((seconds.max(0.0) * 1000.0) as u32)],
            HostCommand::Next => vec![Action::Next],
            HostCommand::Previous => vec![Action::Previous],
            HostCommand::SetVolume(v) => vec![Action::SetVolume(v.clamp(0.0, 100.0).round() as u8)],
            HostCommand::SetMuted(muted) => {
                let silent = self
                    .now_playing()
                    .map_or(self.local.volume == 0, |n| n.volume_percent == 0);
                if muted != silent {
                    vec![Action::ToggleMute]
                } else {
                    Vec::new()
                }
            }
            HostCommand::SetShuffle(on) => vec![Action::SetShuffle(on)],
            HostCommand::SetRepeat(mode) => vec![Action::SetRepeat(app_repeat(mode))],
            HostCommand::MoveTo(index) => upcoming(index)
                .map(|(i, uri)| Action::PlayFromRow {
                    context: RowContext::Queue,
                    uri,
                    index: i as u32,
                })
                .into_iter()
                .collect(),
            HostCommand::PlayNow(ids) if !ids.is_empty() => vec![Action::PlayUris {
                uris: ids.iter().map(|id| track_uri(id)).collect(),
                index: 0,
            }],
            HostCommand::PlayNow(_) => Vec::new(),
            HostCommand::PlayNowFrom { ids, index } if !ids.is_empty() => {
                vec![Action::PlayUris {
                    uris: ids.iter().map(|id| track_uri(id)).collect(),
                    index: index.min(ids.len() - 1) as u32,
                }]
            }
            HostCommand::PlayNowFrom { .. } => Vec::new(),
            HostCommand::Search(phrase) => vec![Action::Search(phrase)],
            HostCommand::RetitleTracks(_)
            | HostCommand::StoreLyrics { .. }
            | HostCommand::SetNowPlayingView(_) => Vec::new(),
            // Luna's `Page.open()`: TIDAL's not-found route with the page's
            // name as its search.
            HostCommand::Navigate(path) if path.starts_with("/not-found?") => {
                vec![Action::Open(Page::PluginPage(
                    path["/not-found?".len()..].to_string(),
                ))]
            }
            HostCommand::Navigate(path) => {
                vec![Action::OpenPluginLink(format!("https://tidal.com{path}"))]
            }
            HostCommand::RemoveUid(uid) => {
                let now = self.now_playing();
                self.luna_queue(now.as_ref().map(|n| n.uri.as_str()))
                    .iter()
                    .position(|e| e.uid == uid)
                    .and_then(&upcoming)
                    .map(|row| Action::PluginRemoveFromQueue { rows: vec![row] })
                    .into_iter()
                    .collect()
            }
            HostCommand::SetFullscreen(_)
            | HostCommand::SetExclusiveMode(_)
            | HostCommand::SetMaximized(_)
            | HostCommand::SetMinSize(_)
            | HostCommand::Minimize
            | HostCommand::CloseWindow
            | HostCommand::OpenEditor { .. }
            | HostCommand::CloseEditor(_)
            | HostCommand::ReplaceAudio { .. } => Vec::new(),
            HostCommand::PlayNext(ids) => vec![Action::InsertInQueue {
                items: ids.iter().map(|id| self.queue_item(id)).collect(),
                position: 0,
            }],
            HostCommand::AddToQueue(ids) => vec![Action::QueueMany {
                songs: ids
                    .iter()
                    .map(|id| {
                        let name = self.known_track(id).map(|t| t.name).unwrap_or_default();
                        (track_uri(id), name)
                    })
                    .collect(),
            }],
            HostCommand::AddAt { ids, index } => vec![Action::InsertInQueue {
                items: ids.iter().map(|id| self.queue_item(id)).collect(),
                position: index.saturating_sub(1),
            }],
            HostCommand::RemoveAt(index) => upcoming(index)
                .map(|row| Action::PluginRemoveFromQueue { rows: vec![row] })
                .into_iter()
                .collect(),
            HostCommand::MoveTrack { from, to } => match (from.checked_sub(1), to.checked_sub(1)) {
                (Some(from), Some(to)) => vec![Action::MoveInQueue { from, to }],
                _ => Vec::new(),
            },
            HostCommand::ClearQueue => vec![Action::ClearQueue],
            HostCommand::AddFavorites { kind, ids }
            | HostCommand::RemoveFavorites { kind, ids }
                if ids.is_empty() || kind.is_empty() =>
            {
                Vec::new()
            }
            HostCommand::AddFavorites { kind, ids } => vec![Action::SetSavedMany {
                uris: favorite_uris(&kind, &ids),
                saved: true,
            }],
            HostCommand::RemoveFavorites { kind, ids } => vec![Action::SetSavedMany {
                uris: favorite_uris(&kind, &ids),
                saved: false,
            }],
            HostCommand::AddToPlaylist { playlist, ids } => {
                let name = self
                    .known_playlist(&playlist)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                vec![Action::AddToPlaylist {
                    playlist_id: playlist,
                    playlist_name: name,
                    items: ids.iter().map(|id| self.queue_item(id)).collect(),
                }]
            }
        }
    }
}

fn favorite_uris(kind: &str, ids: &[String]) -> Vec<String> {
    let kind = match kind.trim_end_matches('s') {
        "album" => "album",
        "artist" => "artist",
        "playlist" => "playlist",
        "video" => "video",
        "mix" => "mix",
        _ => "track",
    };
    ids.iter().map(|id| format!("tidal:{kind}:{id}")).collect()
}

/// Runs `job` on the demo's own writer thread (`demo-dump`), in order, or
/// here when that thread cannot start.
#[cfg(feature = "demo")]
fn demo_off_thread(job: impl FnOnce() + Send + 'static) {
    type Job = Box<dyn FnOnce() + Send>;
    static WRITER: std::sync::OnceLock<Option<std::sync::mpsc::Sender<Job>>> =
        std::sync::OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("demo-dump".into())
            .spawn(move || rx.into_iter().for_each(|job| job()))
            .ok()
            .map(|_| tx)
    });
    let job: Job = Box::new(job);
    match writer {
        Some(tx) => {
            if let Err(std::sync::mpsc::SendError(job)) = tx.send(job) {
                job();
            }
        }
        None => job(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "demo")]
    #[test]
    fn the_demo_luna_dump_is_made_on_a_thread_of_its_own() {
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..3 {
            let tx = tx.clone();
            demo_off_thread(move || {
                let _ = tx.send((i, std::thread::current().name().map(str::to_string)));
            });
        }
        let ran: Vec<_> = (0..3)
            .map(|_| {
                rx.recv_timeout(Duration::from_secs(10))
                    .expect("the job ran")
            })
            .collect();
        assert_eq!(
            ran,
            (0..3)
                .map(|i| (i, Some("demo-dump".to_string())))
                .collect::<Vec<_>>(),
            "each job on the dump thread, in the order given"
        );
    }

    #[test]
    fn favourite_kinds_become_uris() {
        assert_eq!(
            favorite_uris("tracks", &["1".into()]),
            vec!["tidal:track:1".to_string()]
        );
        assert_eq!(
            favorite_uris("ALBUM".to_lowercase().as_str(), &["2".into()]),
            vec!["tidal:album:2".to_string()]
        );
    }

    #[test]
    fn plugin_tracks_carry_tidal_shapes() {
        let track = Track {
            id: Some("5".into()),
            name: "Song".into(),
            uri: "tidal:track:5".into(),
            duration_ms: 61_000,
            ..Track::default()
        };
        let j = track_json(&track);
        assert_eq!(j["id"], 5);
        assert_eq!(j["duration"], 61);
        assert_eq!(j["title"], "Song");
    }

    /// Luna's `bestQuality` and `qualityTags` read a track's `audioQuality`
    /// and `mediaMetadata.tags`. Tidalfast's tracks for plugins had
    /// neither, so every song was the lowest quality: Tidal Tags drew no
    /// HiRes tag and coloured its columns grey, star-ratings' stars never
    /// turned gold for HiRes.
    #[test]
    fn plugin_tracks_carry_tidals_quality() {
        let track = Track {
            id: Some("5".into()),
            uri: "tidal:track:5".into(),
            audio_quality: Some("LOSSLESS".into()),
            quality_tags: vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()],
            ..Track::default()
        };
        let j = track_json(&track);
        assert_eq!(j["audioQuality"], "LOSSLESS");
        assert_eq!(
            j["mediaMetadata"]["tags"],
            json!(["LOSSLESS", "HIRES_LOSSLESS"])
        );
    }

    /// The open page's list (`content.trackLists`) is made again when the
    /// page or its length changes, and only then: every pass made each
    /// song's JSON to compare the length, 15 ms of every frame on a
    /// 2,000-song playlist while it scrolled.
    #[test]
    fn the_page_list_is_made_again_only_when_the_page_or_its_length_changes() {
        use crate::luna::LunaHost as _;
        let mut app = app("page-list-changes");
        crate::demo::populate(&mut app);
        app.open(Page::Playlist("pl1".into()));
        let window = WindowState::default();
        let made = || TRACK_JSONS.with(std::cell::Cell::get);
        let listed = |app: &App, name: &str| {
            app.plugins
                .host
                .playback()
                .track_lists
                .get(name)
                .map(|ids| ids.len())
        };
        app.luna_sync(None, window);
        let len = listed(&app, "playlist/pl1").expect("the open playlist's list");
        assert!(len > 5, "{len}");
        let before = made();
        app.luna_sync(None, window);
        app.luna_sync(None, window);
        assert_eq!(made(), before, "passes with nothing changed made song JSON");

        // More rows loaded: the longer list is given at once.
        let row = PlaylistItem {
            item: Some(PlayableItem::Track(Track {
                id: Some("987654".into()),
                uri: "tidal:track:987654".into(),
                name: "One more".into(),
                ..Track::default()
            })),
            ..PlaylistItem::default()
        };
        app.playlist_pages
            .get_mut("pl1")
            .expect("loaded")
            .items
            .items
            .push(row);
        app.luna_sync(None, window);
        assert_eq!(listed(&app, "playlist/pl1"), Some(len + 1));
        assert!(made() > before);

        // Another page with a list: its own, at once.
        app.open(Page::Album("alb0".into()));
        app.luna_sync(None, window);
        assert!(
            listed(&app, "album/alb0").is_some_and(|n| n > 0),
            "the album's list"
        );
        app.backend.shutdown();
    }

    /// What plugins read of the queue and the library is made again only
    /// when it changes, and every change reaches them on the next pass: a
    /// song added, one swapped for another in its place (the queue as long
    /// as before), the next song playing, a pause, and a liked song swapped
    /// for another in one pass (as many favourites as before). Every pass
    /// made each queue row and the favourites again, and copied every queue
    /// song's JSON into the snapshot, compared whole with the last one:
    /// milliseconds of every frame with a long queue.
    #[test]
    fn plugins_read_the_queue_as_it_is_and_it_is_made_only_when_it_changes() {
        use crate::luna::LunaHost as _;
        use std::cell::Cell;
        let mut app = app("plugin-queue-shared");
        let song = |n: usize| {
            PlayableItem::Track(Track {
                id: Some(n.to_string()),
                name: format!("Song {n}"),
                uri: format!("tidal:track:{n}"),
                duration_ms: 100_000 + n as u32,
                ..Track::default()
            })
        };
        let queue = |playing: usize, next: std::ops::Range<usize>| {
            Loadable::Loaded(crate::api::models::Queue {
                currently_playing: Some(song(playing)),
                queue: next.map(song).collect(),
            })
        };
        let now = |n: usize, playing: bool| NowPlaying {
            local: true,
            device_name: None,
            device_kind: None,
            uri: format!("tidal:track:{n}"),
            id: Some(n.to_string()),
            title: String::new(),
            artists: Vec::new(),
            subtitle: String::new(),
            album_name: String::new(),
            album_id: None,
            art_url: None,
            art_small: None,
            duration_ms: 200_000,
            position_ms: 1_000,
            playing,
            loading: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume_percent: 50,
            can_control: true,
            can_set_volume: true,
            resuming: false,
        };
        let window = WindowState::default();
        let ids = |app: &App| -> Vec<String> {
            let snap = app.plugins.host.playback();
            snap.queue.iter().map(|row| row.id.clone()).collect()
        };
        let has_song =
            |app: &App, id: &str| app.plugins.host.playback().known_items.contains_key(id);
        app.queue = queue(0, 1..500);
        app.luna_sync(Some(&now(0, true)), window);
        assert_eq!(ids(&app).len(), 500);
        assert!(has_song(&app, "499"));
        // Passes with nothing changed make none of it again.
        let made = || {
            (
                QUEUE_ROWS.with(Cell::get),
                TRACK_JSONS.with(Cell::get),
                FAVORITE_LISTS.with(Cell::get),
            )
        };
        let before = made();
        app.luna_sync(Some(&now(0, true)), window);
        app.luna_sync(Some(&now(0, true)), window);
        assert_eq!(made(), before, "(rows, songs, favourites) made again");

        // A song added at the end.
        app.queue = queue(0, 1..501);
        app.luna_sync(Some(&now(0, true)), window);
        assert_eq!(ids(&app).last().map(String::as_str), Some("500"));
        assert!(has_song(&app, "500"));
        // A song swapped for another in its place.
        if let Loadable::Loaded(q) = &mut app.queue {
            q.queue[9] = song(900);
        }
        app.luna_sync(Some(&now(0, true)), window);
        assert_eq!(ids(&app)[10], "900");
        assert_eq!(ids(&app).len(), 501);
        assert!(has_song(&app, "900"));
        // The next song plays, then pauses.
        app.queue = queue(1, 2..501);
        app.luna_sync(Some(&now(1, true)), window);
        let snap = app.plugins.host.playback();
        assert_eq!(snap.queue.first().map(|row| row.id.as_str()), Some("1"));
        assert_eq!((snap.queue.len(), snap.current_index), (500, Some(0)));
        assert_eq!(snap.state, LunaState::Playing);
        app.luna_sync(Some(&now(1, false)), window);
        assert_eq!(app.plugins.host.playback().state, LunaState::Paused);
        // A liked song, then swapped for another in one pass.
        let liked = |app: &App| app.plugins.host.playback().favorite_tracks.to_vec();
        app.saved.insert("tidal:track:7".into(), true);
        app.saved.insert("tidal:track:8".into(), false);
        app.luna_sync(Some(&now(1, false)), window);
        assert_eq!(liked(&app), ["7"]);
        app.saved.insert("tidal:track:7".into(), false);
        app.saved.insert("tidal:track:8".into(), true);
        app.luna_sync(Some(&now(1, false)), window);
        assert_eq!(liked(&app), ["8"]);
        // Nothing plays: no queue.
        app.luna_sync(None, window);
        assert!(ids(&app).is_empty());
        app.backend.shutdown();
    }

    fn app(name: &str) -> App {
        let root = crate::paths::unique_test_dir(&format!("{name}-test"));
        let _ = std::fs::remove_dir_all(&root);
        App::new(
            &crate::backend::Waker::default(),
            crate::app::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        )
    }

    /// A window that keeps its mode (the off-screen test window) never
    /// goes fullscreen or maximized for a plugin: betterFullscreen's button
    /// (`view/REQUEST_FULLSCREEN`) took the off-screen window onto the
    /// owner's display. The fullscreen is emulated instead, so plugins see
    /// the window fullscreen (`.is-fullscreen`) and draw their view in the
    /// window as it is, and leaving it ends the emulation.
    #[test]
    fn a_held_window_emulates_the_fullscreen_a_plugin_asks_for() {
        let mut app = app("held-window");
        app.window_held = true;
        let ctx = egui::Context::default();
        let mut sent = Vec::new();
        for (command, fullscreen) in [
            (HostCommand::SetFullscreen(true), true),
            (HostCommand::SetMaximized(true), true),
            (HostCommand::SetFullscreen(false), false),
        ] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(app.window_command(&command, ui.ctx()));
            });
            output.textures_delta.clear();
            sent.extend(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .iter()
                    .filter(|c| {
                        matches!(
                            c,
                            egui::ViewportCommand::Fullscreen(_)
                                | egui::ViewportCommand::Maximized(_)
                        )
                    })
                    .cloned(),
            );
            assert_eq!(
                crate::ui::plugin_dom::window_fullscreen(&ctx),
                fullscreen,
                "after {command:?}"
            );
        }
        assert!(sent.is_empty(), "the window was asked to change: {sent:?}");
        app.backend.shutdown();
    }

    /// TIDAL's own fullscreen (the now-playing view's full screen button)
    /// is the one plugins see in a real window: in a held test window it
    /// is emulated too, as a plugin's request is, and ends with the view's
    /// fullscreen. Plugins never saw it there (betterFullscreen's player
    /// waits for TIDAL's `.is-fullscreen`).
    #[test]
    fn a_held_window_emulates_the_apps_own_fullscreen_for_plugins() {
        let mut app = app("held-window-lyrics");
        app.window_held = true;
        let ctx = egui::Context::default();
        for (action, fullscreen) in [
            (Action::SetLyricsFullscreen(true), true),
            (Action::SetLyricsFullscreen(false), false),
        ] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.apply(action.clone(), ui.ctx());
            });
            output.textures_delta.clear();
            assert!(
                !output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&egui::ViewportCommand::Fullscreen(true)),
                "the window was asked to fill the display"
            );
            assert_eq!(
                crate::ui::plugin_dom::window_fullscreen(&ctx),
                fullscreen,
                "after {action:?}"
            );
        }
        app.backend.shutdown();
    }

    /// A song just started is shown at once while the engine still reports
    /// the song before it. The playbackContext plugins read named the new
    /// song with the old one's format (Tidal Tags' pill kept "16-bit
    /// 44.1kHz 569kb/s" on a 24-bit song), and the "LOSSLESS" it fell back
    /// to before the stream said coloured a HiRes song as lossless. Until
    /// the engine plays the song, the context names the tier it is expected
    /// at and no figures; then the stream's own.
    #[test]
    fn the_playing_songs_context_never_carries_the_song_befores_format() {
        use crate::tidal_engine::{LocalTrack, Quality, StreamFormat};
        let mut app = app("plugin-format");
        app.settings.tidal_quality = "HI_RES_LOSSLESS".into();
        app.local.track = Some(LocalTrack {
            uri: "tidal:track:1".into(),
            ..LocalTrack::default()
        });
        app.local.format = Some(StreamFormat {
            codec: "flac".into(),
            sample_rate: 44_100,
            bit_depth: Some(16),
            quality: Some(Quality::Lossless),
            bitrate: Some(569_000),
            ..StreamFormat::default()
        });
        app.track_cache.insert(
            "2".into(),
            Track {
                id: Some("2".into()),
                uri: "tidal:track:2".into(),
                audio_quality: Some("LOSSLESS".into()),
                quality_tags: vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()],
                ..Track::default()
            },
        );
        let now = NowPlaying {
            local: true,
            device_name: None,
            device_kind: None,
            uri: "tidal:track:2".into(),
            id: Some("2".into()),
            title: String::new(),
            artists: Vec::new(),
            subtitle: String::new(),
            album_name: String::new(),
            album_id: None,
            art_url: None,
            art_small: None,
            duration_ms: 200_000,
            position_ms: 0,
            playing: true,
            loading: true,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume_percent: 50,
            can_control: true,
            can_set_volume: true,
            resuming: false,
        };
        let snap = app.luna_playback(Some(&now));
        assert_eq!(snap.audio_quality.as_deref(), Some("HI_RES_LOSSLESS"));
        assert_eq!(
            (snap.sample_rate, snap.bit_depth, snap.bitrate),
            (None, None, None),
            "none of the song before's figures"
        );
        // No higher than the quality asked of TIDAL.
        app.settings.tidal_quality = "LOSSLESS".into();
        let snap = app.luna_playback(Some(&now));
        assert_eq!(snap.audio_quality.as_deref(), Some("LOSSLESS"));
        // The engine plays it: the stream's own figures.
        app.local.track = Some(LocalTrack {
            uri: "tidal:track:2".into(),
            ..LocalTrack::default()
        });
        app.local.format = Some(StreamFormat {
            codec: "flac".into(),
            sample_rate: 96_000,
            bit_depth: Some(24),
            quality: Some(Quality::HiResLossless),
            bitrate: Some(1_960_000),
            ..StreamFormat::default()
        });
        let snap = app.luna_playback(Some(&now));
        assert_eq!(snap.audio_quality.as_deref(), Some("HI_RES_LOSSLESS"));
        assert_eq!(
            (snap.sample_rate, snap.bit_depth, snap.bitrate),
            (Some(96_000), Some(24), Some(1_960_000))
        );
        app.backend.shutdown();
    }

    /// Plugins read where the song is from `playbackControls` and TIDAL's
    /// player signal, which in TIDAL follow what is heard. The engine's
    /// position runs ahead of the speakers by the sound the output has
    /// queued; plugins were given it, so lyric plugins (lyrics-everywhere's
    /// line above the bar) showed each line before it was sung, a line ahead
    /// of the app's own lyrics. They get the heard position, as the app's
    /// lyrics use.
    #[test]
    fn plugins_get_the_position_that_is_heard() {
        let mut app = app("plugin-heard-position");
        app.local.output_delay_ms = 1_800;
        let now = NowPlaying {
            local: true,
            device_name: None,
            device_kind: None,
            uri: "tidal:track:2".into(),
            id: Some("2".into()),
            title: String::new(),
            artists: Vec::new(),
            subtitle: String::new(),
            album_name: String::new(),
            album_id: None,
            art_url: None,
            art_small: None,
            duration_ms: 200_000,
            position_ms: 116_500,
            playing: true,
            loading: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume_percent: 50,
            can_control: true,
            can_set_volume: true,
            resuming: false,
        };
        let snap = app.luna_playback(Some(&now));
        assert!(
            (snap.position - 114.7).abs() < 1e-6,
            "the heard position, not the engine's: {}",
            snap.position
        );
        assert_eq!(
            f64::from(app.lyrics_position_ms(&now)) / 1000.0,
            snap.position,
            "the same position the app's own lyrics follow"
        );
        app.backend.shutdown();
    }

    /// The words plugins get for the playing song are the ones the app
    /// looked up for it. The app keeps a song's words after it ends (it
    /// looks again only for its Lyrics tab or a plugin), and they were
    /// handed on as the next song's.
    #[test]
    fn plugins_get_the_playing_songs_own_words() {
        let mut app = app("plugin-own-words");
        let song = |id: &str| NowPlaying {
            local: true,
            device_name: None,
            device_kind: None,
            uri: format!("tidal:track:{id}"),
            id: Some(id.into()),
            title: String::new(),
            artists: Vec::new(),
            subtitle: String::new(),
            album_name: String::new(),
            album_id: None,
            art_url: None,
            art_small: None,
            duration_ms: 200_000,
            position_ms: 1_000,
            playing: true,
            loading: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume_percent: 50,
            can_control: true,
            can_set_volume: true,
            resuming: false,
        };
        app.lyrics_uri = Some("tidal:track:1".into());
        app.lyrics = Loadable::Loaded(Some(crate::lyrics::Lyrics {
            lines: vec![crate::lyrics::Line {
                at_ms: Some(1_000),
                text: "first song".into(),
            }],
            synced: true,
            instrumental: false,
        }));
        let window = WindowState::default();
        let first = song("1");
        app.luna_sync(Some(&first), window);
        let words = app
            .plugins
            .host
            .app_lyrics("1", std::time::Duration::ZERO)
            .expect("the playing song's words");
        assert_eq!(words["subtitles"], "[00:01.00] first song");
        // The next song: the words held are the song before's.
        let second = song("2");
        app.luna_sync(Some(&second), window);
        assert_eq!(
            app.plugins.host.app_lyrics("2", std::time::Duration::ZERO),
            None,
            "the song before's words are not this song's"
        );
        app.backend.shutdown();
    }

    /// A song TIDAL and the app have no words for, which a plugin gives
    /// words in TIDAL's store: the app shows those words (its Lyrics tab and
    /// views read them), as TIDAL's page does; what plugins are told the app
    /// found stays its own lookup's (TIDAL's lyrics answer stays empty), and
    /// the words go once the plugin gives none.
    #[test]
    fn words_a_plugin_gives_the_store_show_where_the_app_has_none() {
        let mut app = app("plugin-store-lyrics");
        let ctx = egui::Context::default();
        app.lyrics_uri = Some("tidal:track:77".into());
        app.lyrics = Loadable::Loaded(None);
        assert!(matches!(app.shown_lyrics(), Loadable::Loaded(None)));
        app.handle_host_request(
            HostRequest::Command(HostCommand::StoreLyrics {
                track_id: Some("77".into()),
                lyrics: Some(json!({
                    "trackId": "77",
                    "lyrics": "One\nTwo",
                    "subtitles": "[00:01.00] One\n[00:02.00] Two",
                    "isRightToLeft": false,
                })),
            }),
            &ctx,
        );
        match app.shown_lyrics() {
            Loadable::Loaded(Some(l)) => {
                assert!(l.synced);
                assert_eq!(
                    l.lines.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
                    ["One", "Two"]
                );
            }
            other => panic!("{other:?}"),
        }
        // The app's own lookup is unchanged: what plugins are told.
        assert!(matches!(app.lyrics, Loadable::Loaded(None)));
        // Another song's lookup does not show them.
        app.lyrics_uri = Some("tidal:track:78".into());
        assert!(matches!(app.shown_lyrics(), Loadable::Loaded(None)));
        app.lyrics_uri = Some("tidal:track:77".into());
        app.handle_host_request(
            HostRequest::Command(HostCommand::StoreLyrics {
                track_id: None,
                lyrics: None,
            }),
            &ctx,
        );
        assert!(matches!(app.shown_lyrics(), Loadable::Loaded(None)));
        app.backend.shutdown();
    }

    /// Words a plugin gave a song in TIDAL's store go with the runtime when
    /// plugins are switched off (no word that there are none comes from a
    /// runtime that stopped): the app shows its own lookup's at once.
    #[test]
    fn words_a_plugin_gave_the_store_go_when_plugins_stop() {
        let mut app = app("plugin-store-lyrics-stop");
        let ctx = egui::Context::default();
        app.lyrics_uri = Some("tidal:track:77".into());
        app.lyrics = Loadable::Loaded(None);
        app.handle_host_request(
            HostRequest::Command(HostCommand::StoreLyrics {
                track_id: Some("77".into()),
                lyrics: Some(json!({
                    "trackId": "77",
                    "lyrics": "One\nTwo",
                    "subtitles": "",
                    "isRightToLeft": false,
                })),
            }),
            &ctx,
        );
        assert!(matches!(app.shown_lyrics(), Loadable::Loaded(Some(_))));
        app.plugins.stop_luna();
        assert!(matches!(app.shown_lyrics(), Loadable::Loaded(None)));
        app.backend.shutdown();
    }

    /// The now-playing tab is TIDAL's `settings.nowPlayingActiveView`: the
    /// user's switch is told to plugins; a plugin's switch changes the tab
    /// without opening the view, and is not told back to it.
    #[test]
    fn the_now_playing_tab_and_tidals_active_view_follow_each_other() {
        use crate::model::NowPlayingTab;
        let mut app = app("plugin-np-view");
        let ctx = egui::Context::default();
        let views = |app: &mut App| -> Vec<Value> {
            view_event(&mut app.plugins.seen.now_playing_tab, app.now_playing_tab)
                .into_iter()
                .map(|e| match e {
                    LunaEvent::Action { kind, payload } => {
                        assert_eq!(kind, "settings/SET_NOW_PLAYING_ACTIVE_VIEW");
                        payload
                    }
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        assert!(views(&mut app).is_empty(), "no tab chosen yet");
        app.now_playing_tab = Some(NowPlayingTab::Credits);
        assert_eq!(views(&mut app), [json!("credits")]);
        assert!(views(&mut app).is_empty());
        app.handle_host_request(
            HostRequest::Command(HostCommand::SetNowPlayingView(Some("lyrics".into()))),
            &ctx,
        );
        assert_eq!(app.now_playing_tab, Some(NowPlayingTab::Lyrics));
        assert!(!app.now_playing_view, "the view stays closed");
        assert!(views(&mut app).is_empty(), "not told back");
        app.now_playing_tab = Some(NowPlayingTab::SimilarTracks);
        assert_eq!(views(&mut app), [json!("similar")]);
        app.backend.shutdown();
    }

    /// quick-queue's row button queues a track by id. The queue row shows
    /// the song the album page already has (it showed an empty row with no
    /// title until the song played).
    #[test]
    fn a_plugins_device_mode_switches_the_bit_perfect_setting_and_the_snapshot_follows() {
        let mut app = app("plugin-exclusive");
        let ctx = egui::Context::default();
        assert!(!app.settings.bit_perfect);
        if !cfg!(any(windows, target_os = "linux")) {
            // macOS has no exclusive output: plugins are told so, and the
            // command changes nothing.
            assert_eq!(app.luna_playback(None).exclusive_mode, None);
            app.handle_host_request(
                HostRequest::Command(HostCommand::SetExclusiveMode(true)),
                &ctx,
            );
            assert!(!app.settings.bit_perfect);
            app.backend.shutdown();
            return;
        }
        assert_eq!(app.luna_playback(None).exclusive_mode, Some(false));
        app.handle_host_request(
            HostRequest::Command(HostCommand::SetExclusiveMode(true)),
            &ctx,
        );
        assert!(app.settings.bit_perfect);
        assert_eq!(app.luna_playback(None).exclusive_mode, Some(true));
        app.handle_host_request(
            HostRequest::Command(HostCommand::SetExclusiveMode(false)),
            &ctx,
        );
        assert!(!app.settings.bit_perfect);
        app.backend.shutdown();
    }

    #[test]
    fn tracks_a_plugin_queues_by_id_come_with_what_the_app_knows() {
        let mut app = app("plugin-queue-names");
        app.album_pages.insert(
            "1".into(),
            crate::model::AlbumPage {
                tracks: crate::model::PagedList {
                    items: vec![Track {
                        id: Some("7".into()),
                        name: "Happily Losing My Mind".into(),
                        uri: "tidal:track:7".into(),
                        ..Track::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let actions = app.host_command_actions(HostCommand::PlayNext(vec!["7".into()]));
        let Some(Action::InsertInQueue { items, .. }) = actions.first() else {
            panic!("{actions:?}");
        };
        assert_eq!(items[0].name(), "Happily Losing My Mind");
        let actions = app.host_command_actions(HostCommand::AddToQueue(vec!["7".into()]));
        let Some(Action::QueueMany { songs }) = actions.first() else {
            panic!("{actions:?}");
        };
        assert_eq!(songs[0].1, "Happily Losing My Mind");
        app.backend.shutdown();
    }

    /// A song a plugin queues takes TIDAL's row from the page the app holds
    /// it on, with its explicit mark, album and ISRC, not the player's copy
    /// of it in Next up, which has none of those.
    #[test]
    fn a_song_a_plugin_queues_keeps_tidals_row_over_the_players_copy() {
        let mut app = app("plugin-queue-tidal-row");
        let row = Track {
            id: Some("9".into()),
            name: "Song 9".into(),
            uri: "tidal:track:9".into(),
            explicit: true,
            external_ids: crate::api::models::ExternalIds {
                isrc: Some("USRC10009".into()),
            },
            artists: vec![crate::api::models::ArtistRef {
                name: "Artist".into(),
                ..Default::default()
            }],
            album: Some(crate::api::models::Album {
                id: "A9".into(),
                uri: "tidal:album:A9".into(),
                images: vec![crate::api::models::Image {
                    url: "https://resources.tidal.com/images/9.jpg".into(),
                    width: Some(640),
                    height: Some(640),
                }],
                ..Default::default()
            }),
            ..Track::default()
        };
        app.album_pages.insert(
            "A9".into(),
            crate::model::AlbumPage {
                tracks: crate::model::PagedList {
                    items: vec![row.clone()],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let players_copy = PlayableItem::Track(crate::backend::track_from_local(
            &crate::api::tidal_catalog::local_track(row.clone()),
        ));
        app.queue = crate::model::Loadable::Loaded(crate::api::models::Queue {
            currently_playing: None,
            queue: vec![players_copy],
        });
        let actions = app.host_command_actions(HostCommand::PlayNext(vec!["9".into()]));
        let Some(Action::InsertInQueue { items, .. }) = actions.first() else {
            panic!("{actions:?}");
        };
        assert_eq!(items.as_slice(), [PlayableItem::Track(row)]);
        app.backend.shutdown();
    }

    /// A song a plugin queues by its id has the row of wherever the app
    /// holds it (an artist's Popular tracks here, which the plugins' own
    /// lookup used to miss), and one the app holds nothing of is asked of
    /// TIDAL, whose answer completes its row.
    #[test]
    fn a_song_a_plugin_queues_has_its_whole_row() {
        let mut app = app("plugin-queue-rows");
        let song = |id: &str| Track {
            id: Some(id.into()),
            name: format!("Song {id}"),
            uri: format!("tidal:track:{id}"),
            artists: vec![crate::api::models::ArtistRef {
                name: "Artist".into(),
                ..Default::default()
            }],
            album: Some(crate::api::models::Album {
                images: vec![crate::api::models::Image {
                    url: format!("https://resources.tidal.com/images/{id}.jpg"),
                    width: Some(640),
                    height: Some(640),
                }],
                ..Default::default()
            }),
            ..Track::default()
        };
        app.artist_pages.insert(
            "1".into(),
            crate::model::ArtistPage {
                top_tracks: crate::model::Loadable::Loaded(Arc::new(vec![song("7")])),
                ..Default::default()
            },
        );
        app.queue = crate::model::Loadable::Loaded(crate::api::models::Queue::default());
        let actions = app.host_command_actions(HostCommand::PlayNext(vec!["7".into(), "8".into()]));
        let Some(Action::InsertInQueue { items, .. }) = actions.first() else {
            panic!("{actions:?}");
        };
        assert_eq!(items[0], PlayableItem::Track(song("7")));
        let ctx = egui::Context::default();
        for action in actions {
            app.apply(action, &ctx);
        }
        assert!(
            app.track_requests.contains("8"),
            "the unknown song is asked for"
        );
        app.handle_api(crate::backend::ApiResponse::Track {
            id: "8".into(),
            result: Ok(song("8")),
        });
        let rows = &app.queue.get().unwrap().queue;
        assert_eq!(
            rows.as_slice(),
            [
                PlayableItem::Track(song("7")),
                PlayableItem::Track(song("8"))
            ]
        );
        app.backend.shutdown();
    }

    /// A plugin moving the page (`router/PUSH`: AltPlay's artist links,
    /// track-radio) opens it like a link but never brings the window
    /// forward, which took focus from whatever the user was doing.
    #[test]
    fn a_plugin_navigating_does_not_raise_the_window() {
        let mut app = app("plugin-navigate");
        let actions = app.host_command_actions(HostCommand::Navigate("/album/5".into()));
        assert!(
            matches!(actions.as_slice(), [Action::OpenPluginLink(a)] if a == "https://tidal.com/album/5"),
            "{actions:?}"
        );
        let ctx = egui::Context::default();
        for action in actions {
            app.apply(action, &ctx);
        }
        assert!(
            !app.actions.iter().any(|a| matches!(a, Action::ShowWindow)),
            "{:?}",
            app.actions
        );
        assert_eq!(app.pending_link.as_deref(), Some("tidal:album:5"));
        app.backend.shutdown();
    }

    /// A title a plugin gave a song in a TIDAL answer is what its row shows;
    /// the song's own title comes back once the plugin hands it back, and
    /// stopping the runtime forgets them.
    #[test]
    fn a_title_a_plugin_gave_a_song_is_what_its_row_shows() {
        let mut app = app("plugin-retitle");
        let track = Track {
            id: Some("7".into()),
            name: "Song".into(),
            uri: "tidal:track:7".into(),
            ..Track::default()
        };
        let song = PlayableItem::Track(track.clone());
        assert_eq!(app.row_title(&song), "Song");
        app.retitle_tracks(vec![("7".into(), "노래".into()), ("8".into(), "x".into())]);
        assert_eq!(app.row_title(&song), "노래");
        app.track_cache.insert("7".into(), track);
        app.retitle_tracks(vec![("7".into(), "Song".into())]);
        assert_eq!(app.row_title(&song), "Song");
        app.retitle_tracks(vec![("7".into(), "노래".into())]);
        app.plugins.stop_luna();
        assert_eq!(app.row_title(&song), "Song");
        app.backend.shutdown();
    }

    /// Luna's `Page.open()` pushes TIDAL's not-found route with the page's
    /// name as its search: the app opens that plugin page, tells plugins
    /// the route as TIDAL's router shows it, and does not bring it back after
    /// a restart (the plugin may be gone).
    #[test]
    fn a_plugins_page_opens_as_the_not_found_route_with_its_name() {
        let mut app = app("plugin-page-route");
        let actions = app.host_command_actions(HostCommand::Navigate("/not-found?Mine".into()));
        assert!(
            matches!(actions.as_slice(), [Action::Open(Page::PluginPage(n))] if n == "Mine"),
            "{actions:?}"
        );
        let ctx = egui::Context::default();
        for action in actions {
            app.apply(action, &ctx);
        }
        assert_eq!(app.page(), &Page::PluginPage("Mine".into()));
        assert_eq!(crate::plugins::gate::page_path(app.page()), "/not-found");
        assert_eq!(crate::plugins::gate::page_search(app.page()), "?Mine");
        assert_eq!(crate::plugins::gate::page_search(&Page::Home), "");
        let name = Page::PluginPage("a:b".into());
        assert_eq!(Page::decode(&name.encode()), Some(name));
        app.backend.shutdown();
    }

    /// The remembered song, shown paused after a restart: plugins see the
    /// queue it was saved with after it (Total Play Time counted only the
    /// paused song, as if nothing followed it on a radio or album).
    #[test]
    fn the_remembered_songs_queue_reaches_plugins_before_playback() {
        let mut app = app("plugin-resume-queue");
        app.resume_track = Some("tidal:track:10".into());
        app.resume_queue = vec!["tidal:track:9".into()];
        app.resume_upcoming = vec!["tidal:track:11".into(), "tidal:track:12".into()];
        let ids: Vec<String> = app
            .luna_queue(Some("tidal:track:10"))
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["10", "9", "11", "12"]);
        app.backend.shutdown();
    }

    /// Total Play Time sums the length of every queue row each second. The
    /// rows' lengths are in the store at once (from the queue's own rows),
    /// not fetched from TIDAL one song at a time, whose late answers made
    /// the sum jump between the queue as it was and as it is.
    #[test]
    fn the_queues_songs_are_in_the_store_with_their_lengths() {
        let mut app = app("plugin-queue-items");
        let song = |id: &str, ms: u32| {
            PlayableItem::Track(Track {
                id: Some(id.into()),
                name: format!("Song {id}"),
                uri: format!("tidal:track:{id}"),
                duration_ms: ms,
                ..Track::default()
            })
        };
        app.queue = Loadable::Loaded(crate::api::models::Queue {
            currently_playing: Some(song("1", 100_000)),
            queue: vec![song("2", 200_000), song("3", 300_000)],
        });
        let queue = app.luna_queue(Some("tidal:track:1"));
        let items = app.luna_queue_items(&queue);
        let seconds: Vec<_> = ["2", "3"]
            .iter()
            .map(|id| items.get(*id).map(|item| item["duration"].clone()))
            .collect();
        assert_eq!(seconds, [Some(json!(200)), Some(json!(300))], "{items:?}");
        app.backend.shutdown();
    }

    /// With plugins on, the queue plugins read follows each new song as the
    /// open panel's does: after a new context or autoplay they saw the
    /// previous rows (or none) for up to a minute.
    #[test]
    fn the_queue_plugins_read_is_asked_again_when_a_song_starts() {
        let mut app = app("plugin-queue-refresh");
        app.auth = crate::backend::AuthStatus::Connected {
            username: "alice".into(),
        };
        app.local.track = Some(crate::player::LocalTrack {
            uri: "tidal:track:5".into(),
            ..Default::default()
        });
        app.local.playback = crate::player::Playback::Playing;
        let before = app.queue_seq;
        app.on_now_playing_changed();
        assert_eq!(app.queue_seq, before, "Luna off: nothing is asked");
        app.plugins
            .start_luna(crate::paths::unique_test_dir("plugin-queue-refresh-luna"));
        assert!(app.plugins.luna.is_some());
        app.last_now_playing_uri = None;
        app.on_now_playing_changed();
        assert!(app.queue_seq > before, "Luna on: the queue is asked for");
        app.plugins.stop_luna();
        app.backend.shutdown();
    }
}
