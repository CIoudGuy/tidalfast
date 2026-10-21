//! Music videos and a lost connection. A video's playlist and segments are
//! fetched apart from its stream request, and an outage in any of them
//! waits for the network like a song's does: the video stays the playing
//! one at its place, the queue is untouched, the listener is told once, and
//! it asks again until the network is back. TIDAL's own refusal (403, 404)
//! still skips the video.

use std::sync::atomic::AtomicU8;

use super::*;
use crate::tidal_engine::ts::fixture::playable_segment;

/// How a test server answers for the playlist or a segment.
const UP: u8 = 0;
/// The connection dies as the body starts: no answer, as when the network
/// goes.
const CUT: u8 = 1;
const UNAVAILABLE: u8 = 2;
const BUSY: u8 = 3;
const FORBIDDEN: u8 = 4;
const GONE: u8 = 5;

#[derive(Default)]
struct Net {
    playlist: AtomicU8,
    first: AtomicU8,
    second: AtomicU8,
}

fn set(mode: &AtomicU8, to: u8) {
    mode.store(to, Ordering::SeqCst);
}

fn reply(mode: u8, body: &Arc<Vec<u8>>) -> Reply {
    match mode {
        UP => Reply::Body {
            body: Arc::clone(body),
            stall: None,
        },
        CUT => Reply::Body {
            body: Arc::clone(body),
            stall: Some(0),
        },
        UNAVAILABLE => Reply::Status(503),
        BUSY => Reply::Status(429),
        FORBIDDEN => Reply::Status(403),
        _ => Reply::Status(404),
    }
}

struct Rig {
    engine: Engine,
    states: Receiver<LocalState>,
    net: Arc<Net>,
    requests: Requests,
    hub: Arc<crate::tidal_engine::video::VideoHub>,
    /// Each different error the engine reported, in order.
    errors: Vec<String>,
    _song: Server,
}

impl Rig {
    /// A video of two one second segments, and a one second song, behind a
    /// server that answers as `net` says.
    fn new() -> Self {
        let net = Arc::new(Net::default());
        let first = Arc::new(playable_segment(900_000, 25));
        let second = Arc::new(playable_segment(990_000, 25));
        let playlist = Arc::new(
            b"#EXTM3U\n#EXTINF:1,\nseg1.ts\n#EXTINF:1,\nseg2.ts\n#EXT-X-ENDLIST\n".to_vec(),
        );
        let modes = Arc::clone(&net);
        let (base, requests) = serve_with(move |path, _, _| match path {
            "/list.m3u8" => reply(modes.playlist.load(Ordering::SeqCst), &playlist),
            "/seg1.ts" => reply(modes.first.load(Ordering::SeqCst), &first),
            _ => reply(modes.second.load(Ordering::SeqCst), &second),
        });
        let song = serve(wav(decode::RATE, 1.0), None);
        let hub = Arc::new(crate::tidal_engine::video::VideoHub::default());
        let config = EngineConfig {
            autoplay: false,
            video: crate::tidal_engine::video::VideoOptions {
                quality: crate::tidal_engine::hls::VideoQuality::Auto,
                hub: Arc::clone(&hub),
            },
            ..config()
        };
        let catalog = Arc::new(Videos {
            song: song.url.clone(),
            playlist: format!("{base}/list.m3u8"),
        });
        let (engine, states) = quiet_engine_with(config, catalog);
        Self {
            engine,
            states,
            net,
            requests,
            hub,
            errors: Vec::new(),
            _song: song,
        }
    }

    /// Loads the video, then a song, at the video.
    fn load(&self, play: bool) {
        self.engine
            .command(PlayerCommand::Load(LoadSpec {
                uris: vec!["tidal:video:5".into(), "tidal:track:1".into()],
                offset_uri: Some("tidal:video:5".into()),
                play,
                ..LoadSpec::default()
            }))
            .unwrap();
    }

    /// Follows the engine to a state that passes `test`.
    fn until(&mut self, test: impl Fn(&LocalState) -> bool) -> LocalState {
        loop {
            let state = self
                .states
                .recv_timeout(ENGINE_SILENCE)
                .unwrap_or_else(|_| panic!("never got there; errors {:?}", self.errors));
            if let Some(error) = &state.error
                && self.errors.last() != Some(error)
            {
                self.errors.push(error.clone());
            }
            if test(&state) {
                return state;
            }
        }
    }

    fn asked(&self, path: &str) -> usize {
        lock(&self.requests)
            .iter()
            .filter(|(asked, _)| asked == path)
            .count()
    }

    /// The video is waiting at its place, with the queue as it was.
    fn waits(&mut self) -> LocalState {
        let waiting = self.until(|state| state.error.as_deref() == Some(NETWORK_WAIT));
        assert!(waiting.loading, "it shows as waiting");
        assert_eq!(waiting.track.as_ref().unwrap().uri, "tidal:video:5");
        let queue = self.engine.queue(10);
        assert_eq!(queue.current.unwrap().uri, "tidal:video:5");
        assert_eq!(
            queue
                .next_up
                .iter()
                .map(|t| t.uri.as_str())
                .collect::<Vec<_>>(),
            ["tidal:track:1"],
            "nothing skipped"
        );
        waiting
    }
}

/// The playlist cannot be fetched because the network has gone: the video
/// waits (it is not skipped as "can't be played"), says so once, and plays
/// when the network is back.
#[test]
fn a_video_whose_playlist_gets_no_answer_waits_and_resumes() {
    let mut rig = Rig::new();
    set(&rig.net.playlist, CUT);
    rig.load(false);
    rig.waits();
    set(&rig.net.playlist, UP);
    rig.engine.network_back();
    let back = rig.until(paused_on("tidal:video:5"));
    assert!(back.error.is_none(), "the wait is over: {:?}", back.error);
    assert_eq!(rig.errors, [NETWORK_WAIT], "said once");
    assert!(rig.hub.segment_at("tidal:video:5", 0).is_some());
}

/// TIDAL's servers failing is an outage too, and with no word that the
/// network is back the video asks again on its own after the pause.
#[test]
fn a_video_whose_playlist_server_fails_asks_again_by_itself() {
    let mut rig = Rig::new();
    set(&rig.net.playlist, UNAVAILABLE);
    rig.load(false);
    rig.waits();
    let asked = rig.asked("/list.m3u8");
    set(&rig.net.playlist, UP);
    let back = rig.until(paused_on("tidal:video:5"));
    assert!(back.error.is_none(), "{:?}", back.error);
    assert!(rig.asked("/list.m3u8") > asked, "it asked again");
    assert_eq!(rig.errors, [NETWORK_WAIT], "said once");
}

/// The playlist is fine but the first segment is out of reach (rate
/// limited): the same wait.
#[test]
fn a_video_whose_first_segment_is_out_of_reach_waits_and_resumes() {
    let mut rig = Rig::new();
    set(&rig.net.first, BUSY);
    rig.load(false);
    rig.waits();
    set(&rig.net.first, UP);
    rig.engine.network_back();
    let back = rig.until(paused_on("tidal:video:5"));
    assert!(back.error.is_none(), "{:?}", back.error);
    assert_eq!(rig.errors, [NETWORK_WAIT], "said once");
}

/// The network goes after the first segment: that second of the video
/// plays, then the video waits (not skipped, the song after it not
/// started), and once the network is back it carries on from where it was,
/// and the queue plays on.
#[test]
fn a_video_cut_off_part_way_waits_and_carries_on() {
    let mut rig = Rig::new();
    set(&rig.net.second, CUT);
    rig.load(true);
    let waiting = rig.waits();
    assert!(waiting.position_at.is_none(), "the position is held");
    assert!(waiting.position_ms >= 500, "at its place: {waiting:?}");
    set(&rig.net.second, UP);
    rig.engine.network_back();
    let back = rig.until(|state| {
        state.playback == Playback::Playing
            && !state.loading
            && state.error.is_none()
            && state
                .track
                .as_ref()
                .is_some_and(|t| t.uri == "tidal:video:5")
    });
    assert!(back.position_ms >= 500, "from where it was: {back:?}");
    // Then the song the queue held for after it.
    rig.until(|state| {
        state.playback == Playback::Playing
            && !state.loading
            && state
                .track
                .as_ref()
                .is_some_and(|t| t.uri == "tidal:track:1")
    });
    assert_eq!(rig.errors, [NETWORK_WAIT], "said once");
    assert!(
        rig.asked("/seg2.ts") > 1,
        "the second segment was asked for again"
    );
}

/// TIDAL's own refusal of the video's playlist or segment is not an outage:
/// the video is skipped, with TIDAL-like words, on to the song.
#[test]
fn a_video_tidal_refuses_is_still_skipped() {
    for (playlist, first) in [(FORBIDDEN, UP), (GONE, UP), (UP, FORBIDDEN), (UP, GONE)] {
        let mut rig = Rig::new();
        set(&rig.net.playlist, playlist);
        set(&rig.net.first, first);
        rig.load(false);
        let song = rig.until(paused_on("tidal:track:1"));
        let error = song.error.expect("the skipped video says why");
        assert!(
            error.contains(crate::tidal_engine::video::UNPLAYABLE),
            "{playlist} {first}: {error}"
        );
        assert!(!rig.errors.iter().any(|error| error == NETWORK_WAIT));
        assert!(rig.hub.info("tidal:video:5").is_none());
    }
}

/// A video segment that goes missing part way (404) skips the video; only
/// its first second played.
#[test]
fn a_video_whose_later_segment_is_gone_is_skipped() {
    let mut rig = Rig::new();
    set(&rig.net.second, GONE);
    rig.load(true);
    let song = rig.until(|state| {
        state
            .track
            .as_ref()
            .is_some_and(|t| t.uri == "tidal:track:1")
            && state.error.is_some()
    });
    let error = song.error.unwrap();
    assert!(error.contains("so it was skipped"), "{error}");
    assert!(!rig.errors.iter().any(|error| error == NETWORK_WAIT));
}
