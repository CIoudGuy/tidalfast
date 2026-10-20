//! The engine against a fake catalogue and a local HTTP server.
//!
//! Every load here is paused (`play: false`), so no test opens an audio
//! device: a paused load still resolves, downloads, decodes and reports
//! its first position, which is the part these tests are about. The one
//! bit-perfect test plays, into a capture standing in for the device, and
//! the tests that play a plugin's own song through play into a quiet
//! output ([`quiet_engine_with`]): a machine with no sound device (a CI
//! runner) pauses a real output at its first write.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::Receiver;

use super::*;
use crate::tidal_engine::buffer::{Download, Fetch};
use crate::tidal_engine::decode::tests::wav;

mod adaptive;
mod paged;
mod song_end;

/// Serves `body` at every path, honouring `Range: bytes=N-`, at most
/// `pace` bytes per 10 ms when set. Counts the range requests it served.
pub(crate) struct Server {
    pub url: String,
    pub ranges: Arc<AtomicUsize>,
}

pub(crate) fn serve(body: Vec<u8>, pace: Option<usize>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/track", listener.local_addr().unwrap());
    let body = Arc::new(body);
    let ranges = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&ranges);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let body = Arc::clone(&body);
            let counted = Arc::clone(&counted);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut from = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(range) = lower.strip_prefix("range: bytes=") {
                        from = range.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                let start = from.unwrap_or(0).min(body.len());
                let head = match from {
                    Some(_) => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len() - start,
                            start,
                            body.len() - 1,
                            body.len()
                        )
                    }
                    None => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    ),
                };
                if stream.write_all(head.as_bytes()).is_err() {
                    return;
                }
                let rest = &body[start..];
                match pace {
                    None => {
                        let _ = stream.write_all(rest);
                    }
                    Some(pace) => {
                        for chunk in rest.chunks(pace) {
                            if stream.write_all(chunk).is_err() {
                                return;
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
            });
        }
    });
    Server { url, ranges }
}

fn client() -> reqwest::blocking::Client {
    crate::http::blocking_builder(&crate::settings::ProxyConfig::Off)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn a_download_reads_while_it_arrives_and_jumps_ahead_for_a_far_seek() {
    let body: Vec<u8> = (0..4_000_000u32).map(|i| (i % 251) as u8).collect();
    let server = serve(body.clone(), Some(16 * 1024));
    let client = client();
    let download = thread::spawn(move || {
        let download = Download::start(Fetch::new(client, vec![server.url]));
        let (mut reader, _) = download.reader(true);
        let mut head = [0u8; 1000];
        reader.read_exact(&mut head).unwrap();
        assert_eq!(&head[..], &body[..1000]);

        // At 1.6 MB/s from the start, 3.9 MB is two and a half seconds
        // away; a range request brings it at once.
        let started = Instant::now();
        reader.seek(SeekFrom::Start(3_900_000)).unwrap();
        let mut far = [0u8; 1000];
        reader.read_exact(&mut far).unwrap();
        assert_eq!(&far[..], &body[3_900_000..3_901_000]);
        assert!(started.elapsed() < Duration::from_millis(1500));
        assert!(server.ranges.load(Ordering::SeqCst) >= 1);

        // The gap before it still fills in, and the end reads as the end.
        reader.seek(SeekFrom::Start(2_000_000)).unwrap();
        let mut middle = vec![0u8; 100_000];
        reader.read_exact(&mut middle).unwrap();
        assert_eq!(&middle[..], &body[2_000_000..2_100_000]);
        reader.seek(SeekFrom::End(0)).unwrap();
        assert_eq!(reader.read(&mut far).unwrap(), 0);
    });
    download.join().unwrap();
}

#[test]
fn dash_segments_play_back_to_back_as_one_stream() {
    let first = serve(b"init".to_vec(), None);
    let second = serve(b"-segment-1".to_vec(), None);
    let client = client();
    thread::spawn(move || {
        let download = Download::start(Fetch::new(client, vec![first.url, second.url]));
        let (mut reader, _) = download.reader(false);
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"init-segment-1");
    })
    .join()
    .unwrap();
}

fn track(n: u32) -> LocalTrack {
    LocalTrack {
        uri: format!("tidal:track:{n}"),
        title: format!("Song {n}"),
        duration_ms: 1_000,
        ..LocalTrack::default()
    }
}

/// A catalogue of one album, `tidal:album:1`, of tracks 1 to 4, whose
/// streams are one WAV file; track 3 cannot be streamed.
struct FakeCatalog {
    url: String,
    asked: Mutex<HashMap<&'static str, usize>>,
}

impl FakeCatalog {
    fn new() -> Arc<Self> {
        let server = serve(wav(decode::RATE, 1.0), None);
        Arc::new(Self {
            url: server.url,
            asked: Mutex::default(),
        })
    }

    fn note(&self, what: &'static str) {
        *lock(&self.asked).entry(what).or_default() += 1;
    }
}

impl Catalog for FakeCatalog {
    fn context_tracks(&self, context_uri: &str) -> Result<Vec<LocalTrack>> {
        self.note("context");
        match context_uri {
            "tidal:album:1" => Ok((1..=4).map(track).collect()),
            _ => Err(anyhow!("no such context")),
        }
    }

    fn tracks(&self, uris: &[String]) -> Result<Vec<LocalTrack>> {
        self.note("tracks");
        Ok(uris
            .iter()
            .filter_map(|uri| crate::util::uri_id(uri)?.parse().ok())
            .map(track)
            .collect())
    }

    fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        self.note("stream");
        if track_id == "3" {
            return Err(anyhow!("not streamable"));
        }
        Ok(StreamInfo {
            urls: vec![self.url.clone()],
            mime: "audio/wav".into(),
            codec: "pcm".into(),
            quality,
            track_gain_db: Some(-6.0),
            ..StreamInfo::default()
        })
    }

    fn autoplay(&self, seed_track_id: &str) -> Result<Vec<LocalTrack>> {
        self.note("autoplay");
        let seed: u32 = seed_track_id.parse()?;
        Ok(vec![track(seed + 100), track(seed + 101)])
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        device_name: "Tidalfast".into(),
        quality: Quality::Lossless,
        adaptive: false,
        normalisation: true,
        autoplay: true,
        gapless: true,
        audio_device: None,
        initial_volume: u16::MAX / 2,
        volume_dir: std::path::PathBuf::new(),
        audio_cache_dir: None,
        audio_cache_limit: None,
        crossfade_ms: 0,
        offline_dir: None,
        buffer_ms: crate::sink::DEFAULT_BUFFER_MS,
        eq: crate::eq::shared(),
        proxy: crate::settings::ProxyConfig::Off,
        bit_perfect: false,
        release_device_when_paused: true,
        video: Default::default(),
    }
}

fn engine(catalog: Arc<FakeCatalog>) -> (Engine, Receiver<LocalState>) {
    engine_with(config(), catalog)
}

fn engine_with(config: EngineConfig, catalog: Arc<dyn Catalog>) -> (Engine, Receiver<LocalState>) {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    let notify: Notify = Arc::new(move |event| {
        if let EngineEvent::State(state) = event {
            let _ = lock(&sender).send(state);
        }
    });
    (Engine::new(config, catalog, notify).unwrap(), receiver)
}

/// As [`engine_with`], playing into a quiet output at the music's own
/// speed instead of the sound device: songs play to their end without a
/// sound, and on a machine that has no device at all.
fn quiet_engine_with(
    config: EngineConfig,
    catalog: Arc<dyn Catalog>,
) -> (Engine, Receiver<LocalState>) {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    let notify: Notify = Arc::new(move |event| {
        if let EngineEvent::State(state) = event {
            let _ = lock(&sender).send(state);
        }
    });
    let make_sink: SinkFactory = Arc::new(|| {
        Box::new(song_end::Quiet {
            frames: Arc::default(),
            pace: Some(1),
            stops: Arc::default(),
        }) as Box<dyn crate::sink::AudioSink>
    });
    let opener = exclusive::tests::capture(|_| true, Arc::default());
    let engine = Engine::with_outputs(config, catalog, notify, opener, Some(make_sink)).unwrap();
    (engine, receiver)
}

/// How long the engine may stay silent before a test gives up on it. The
/// wait is for the next report, not for the whole run: a slow machine
/// delays reports, and only an engine that has stopped talking fails.
const ENGINE_SILENCE: Duration = Duration::from_secs(20);

/// Waits for a state that passes `test`.
fn until(states: &Receiver<LocalState>, test: impl Fn(&LocalState) -> bool) -> LocalState {
    loop {
        let state = states
            .recv_timeout(ENGINE_SILENCE)
            .expect("the engine never reached the expected state");
        if test(&state) {
            return state;
        }
    }
}

fn paused_on(uri: &str) -> impl Fn(&LocalState) -> bool + '_ {
    move |state| {
        state.playback == Playback::Paused
            && !state.loading
            && state.track.as_ref().is_some_and(|track| track.uri == uri)
    }
}

fn load_album(engine: &Engine, offset: Option<&str>) {
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:album:1".into()),
            offset_uri: offset.map(str::to_string),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
}

#[test]
fn a_paused_load_resolves_the_context_and_reports_the_track() {
    let catalog = FakeCatalog::new();
    let (engine, states) = engine(Arc::clone(&catalog));
    let first = until(&states, |state| state.connected);
    assert_eq!(first.volume, u16::MAX / 2);

    load_album(&engine, Some("tidal:track:2"));
    // Loading at once, then the chosen track while it resolves.
    until(&states, |state| {
        state.loading && state.playback == Playback::Loading
    });
    let loading = until(&states, |state| state.track.is_some());
    assert!(loading.loading);
    assert_eq!(loading.track.as_ref().unwrap().uri, "tidal:track:2");
    let paused = until(&states, paused_on("tidal:track:2"));
    assert_eq!(paused.position_ms, 0);
    assert_eq!(paused.position_at, None);
    // What the quality badge shows.
    let format = paused.format.clone().expect("the playing format");
    assert_eq!(format.sample_rate, decode::RATE);
    assert_eq!(format.bit_depth, Some(16));
    assert_eq!(format.quality, Some(Quality::Lossless));
    assert_eq!(format.origin, AudioOrigin::Stream);

    let view = engine.queue(10);
    assert_eq!(view.context_uri.as_deref(), Some("tidal:album:1"));
    assert_eq!(view.next_up, vec![track(3), track(4)]);

    // Next keeps it paused; the unstreamable track 3 is skipped with an
    // error, on to track 4.
    let sequence = paused.track_sequence;
    engine.command(PlayerCommand::Next).unwrap();
    let skipped = until(&states, paused_on("tidal:track:4"));
    assert!(skipped.track_sequence >= sequence + 2);
    assert!(skipped.error.is_some(), "the skipped track says why");
}

/// With nothing loaded, or a song paused with its sound ready, nothing can
/// change without a message: the engine's thread sleeps until one comes
/// rather than looking again twice a second.
#[test]
fn an_idle_or_paused_engine_sleeps_until_told() {
    let quiet = |engine: &Engine| {
        thread::sleep(Duration::from_millis(300));
        let before = engine.wakes();
        thread::sleep(Duration::from_millis(1600));
        engine.wakes() - before
    };
    let (engine, states) = engine(FakeCatalog::new());
    until(&states, |state| state.connected);
    assert_eq!(quiet(&engine), 0, "nothing loaded");
    load_album(&engine, None);
    until(&states, paused_on("tidal:track:1"));
    assert_eq!(quiet(&engine), 0, "a paused song");
    // A message still wakes it at once.
    engine.command(PlayerCommand::Seek(400)).unwrap();
    until(&states, |state| state.position_ms == 400);
}

#[test]
fn seeking_reports_a_seek_and_the_new_position() {
    let (engine, states) = engine(FakeCatalog::new());
    load_album(&engine, None);
    let paused = until(&states, paused_on("tidal:track:1"));
    engine.command(PlayerCommand::Seek(400)).unwrap();
    let seeked = until(&states, |state| state.seek_sequence > paused.seek_sequence);
    assert_eq!(seeked.position_ms, 400);
    assert_eq!(seeked.position_at, None, "paused, so not advancing");
}

#[test]
fn queue_shuffle_and_repeat_commands_reach_the_queue_and_the_state() {
    let catalog = FakeCatalog::new();
    let (engine, states) = engine(Arc::clone(&catalog));
    load_album(&engine, None);
    until(&states, paused_on("tidal:track:1"));

    engine
        .command(PlayerCommand::AddToQueue("tidal:track:9".into()))
        .unwrap();
    engine.command(PlayerCommand::Shuffle(true)).unwrap();
    until(&states, |state| state.shuffle);
    engine
        .command(PlayerCommand::Repeat(RepeatMode::Context))
        .unwrap();
    until(&states, |state| state.repeat == RepeatMode::Context);
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine
        .queue(10)
        .queued
        .first()
        .is_none_or(|t| t.title.is_empty())
    {
        assert!(Instant::now() < deadline, "queued metadata never arrived");
        thread::sleep(Duration::from_millis(10));
    }
    let view = engine.queue(10);
    assert_eq!(view.queued, vec![track(9)]);
    assert_eq!(
        view.current,
        Some(track(1)),
        "shuffle keeps the playing song"
    );

    // The queued song plays next, ahead of the album.
    engine.command(PlayerCommand::Next).unwrap();
    until(&states, paused_on("tidal:track:9"));
    engine.command(PlayerCommand::ClearQueue).unwrap();
    assert!(engine.queue(10).queued.is_empty());
}

#[test]
fn next_past_the_end_continues_with_autoplay() {
    let catalog = FakeCatalog::new();
    let (engine, states) = engine(Arc::clone(&catalog));
    load_album(&engine, Some("tidal:track:4"));
    until(&states, paused_on("tidal:track:4"));
    engine.command(PlayerCommand::Next).unwrap();
    // Autoplay's first track, seeded by the album's last.
    until(&states, |state| {
        state
            .track
            .as_ref()
            .is_some_and(|track| track.uri == "tidal:track:104")
            && !state.loading
    });
    assert!(engine.queue(10).autoplay);
}

/// Once the context ends and autoplay plays on, the songs come from the
/// radio of the song it was seeded with, and that is what the queue says
/// it plays from ("<song> Radio", opening that radio): no longer the album
/// or playlist that ended, which the owner saw still named while autoplay
/// played.
#[test]
fn autoplay_plays_from_the_seed_songs_radio_not_the_context_that_ended() {
    let catalog = FakeCatalog::new();
    let (engine, states) = engine(Arc::clone(&catalog));
    load_album(&engine, Some("tidal:track:4"));
    let album = until(&states, paused_on("tidal:track:4"));
    let album_context = album.context_uri.clone();
    assert!(album_context.is_some());
    engine.command(PlayerCommand::Next).unwrap();
    let autoplaying = until(&states, |state| {
        state
            .track
            .as_ref()
            .is_some_and(|track| track.uri == "tidal:track:104")
            && !state.loading
    });
    assert_eq!(
        autoplaying.context_uri.as_deref(),
        Some("tidal:station:track:4"),
        "plays from the last song's radio, not {album_context:?}"
    );
    assert_eq!(
        engine.queue(10).context_uri.as_deref(),
        Some("tidal:station:track:4")
    );
}

#[test]
fn previous_steps_back_and_a_resume_point_keeps_the_queue() {
    let (engine, states) = engine(FakeCatalog::new());
    load_album(&engine, Some("tidal:track:2"));
    until(&states, paused_on("tidal:track:2"));
    engine.command(PlayerCommand::Previous).unwrap();
    until(&states, paused_on("tidal:track:1"));

    let Some(PlaybackResume::Queue(snapshot)) = engine.resume_point() else {
        panic!("a live queue resumes as a queue");
    };
    engine.shutdown();
    let (replacement, states) = engine_with_queue();
    replacement.resume(PlaybackResume::Queue(snapshot)).unwrap();
    until(&states, paused_on("tidal:track:1"));
    assert_eq!(
        replacement.queue(10).next_up,
        vec![track(2), track(3), track(4)]
    );
}

/// A song loading paused (a restored session) reads as paused while it
/// loads: an engine replaced then picks it up paused, not playing, and
/// Play is what starts it. A song loading to play reads as playing.
#[test]
fn a_song_loading_paused_is_not_playing() {
    let (engine, states) = engine(FakeCatalog::new());
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: vec!["tidal:track:1".into()],
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    let loading = until(&states, |state| {
        state.playback == Playback::Loading && state.track.is_some()
    });
    assert!(!loading.will_play());
    assert!(!loading.interrupted().unwrap().playing);
    until(&states, paused_on("tidal:track:1"));
    engine.command(PlayerCommand::Toggle).unwrap();
    let playing = until(&states, |state| state.playback == Playback::Playing);
    assert!(playing.will_play());
    engine.shutdown();

    let (engine, states) = engine_with_queue();
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: vec!["tidal:track:1".into()],
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    let loading = until(&states, |state| {
        state.playback == Playback::Loading && state.track.is_some()
    });
    assert!(loading.will_play());
    assert!(loading.interrupted().unwrap().playing);
    engine.shutdown();
}

fn engine_with_queue() -> (Engine, Receiver<LocalState>) {
    engine(FakeCatalog::new())
}

#[test]
fn a_context_that_fails_to_load_reports_why() {
    let (engine, states) = engine(FakeCatalog::new());
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:playlist:missing".into()),
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    let failed = until(&states, |state| state.error.is_some());
    assert_eq!(failed.playback, Playback::Stopped);
    assert!(!failed.loading);
}

#[test]
fn volume_is_applied_at_once_and_only_a_kept_level_is_reported() {
    let (engine, states) = engine(FakeCatalog::new());
    until(&states, |state| state.connected);
    engine.command(PlayerCommand::VolumePreview(100)).unwrap();
    assert_eq!(engine.heard().level(), 100);
    engine.command(PlayerCommand::Volume(200)).unwrap();
    let reported = until(&states, |_| true);
    assert_eq!(reported.volume, 200, "the preview was not reported");
}

#[test]
fn an_autoplay_load_plays_the_station_seeded_by_the_context() {
    let catalog = FakeCatalog::new();
    let tracks = resolve(
        catalog.as_ref(),
        None,
        &LoadSpec {
            context_uri: Some("tidal:album:1".into()),
            autoplay: true,
            ..LoadSpec::default()
        },
    )
    .unwrap();
    assert_eq!(tracks[0].uri, "tidal:track:104");
    let tracks = resolve(
        catalog.as_ref(),
        None,
        &LoadSpec {
            context_uri: Some("tidal:track:7".into()),
            ..LoadSpec::default()
        },
    )
    .unwrap();
    assert_eq!(tracks, vec![track(7)]);
}

/// A folder of its own for one test, removed when it is dropped.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tidalfast-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How a test server answers one request: `(path, range start, request
/// number)` in, a reply out.
enum Reply {
    Status(u16),
    /// The body from the asked range on; with `stall`, only that many
    /// bytes of it before the connection dies.
    Body {
        body: Arc<Vec<u8>>,
        stall: Option<usize>,
    },
}

type Handler = dyn Fn(&str, Option<usize>, usize) -> Reply + Send + Sync;

/// Each request a test server took: its path and range start.
type Requests = Arc<Mutex<Vec<(String, Option<usize>)>>>;

/// Serves whatever `handler` says, and records each request's path and
/// range start.
fn serve_with(
    handler: impl Fn(&str, Option<usize>, usize) -> Reply + Send + Sync + 'static,
) -> (String, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handler: Arc<Handler> = Arc::new(handler);
    let log: Requests = Arc::default();
    let seen = Arc::clone(&log);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let handler = Arc::clone(&handler);
            let seen = Arc::clone(&seen);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut from = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(range) = lower.strip_prefix("range: bytes=") {
                        from = range.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                let number = {
                    let mut seen = lock(&seen);
                    seen.push((path.clone(), from));
                    seen.len()
                };
                match handler(&path, from, number) {
                    Reply::Status(code) => {
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 {code} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        );
                    }
                    Reply::Body { body, stall } => {
                        let start = from.unwrap_or(0).min(body.len());
                        let head = match from {
                            Some(_) => format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                                body.len() - start,
                                start,
                                body.len().saturating_sub(1),
                                body.len()
                            ),
                            None => format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            ),
                        };
                        if stream.write_all(head.as_bytes()).is_err() {
                            return;
                        }
                        let rest = &body[start..];
                        match stall {
                            None => {
                                let _ = stream.write_all(rest);
                            }
                            Some(sent) => {
                                let _ = stream.write_all(&rest[..sent.min(rest.len())]);
                                let _ = stream.flush();
                                // Dropped here: the connection dies mid-body,
                                // at once, with no timer for a busy machine
                                // to upset.
                            }
                        }
                    }
                }
            });
        }
    });
    (base, log)
}

/// A client for the test servers. Its timeout is only a safety net: the
/// servers fail connections by closing them, never by hanging.
fn test_client() -> reqwest::blocking::Client {
    crate::http::blocking_builder(&crate::settings::ProxyConfig::Off)
        .unwrap()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap()
}

fn read_all(download: &Download) -> Vec<u8> {
    let (mut reader, _) = download.reader(true);
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    all
}

#[test]
fn a_stalled_connection_reconnects_and_resumes_with_a_range() {
    let body: Arc<Vec<u8>> = Arc::new((0..300_000u32).map(|i| (i % 253) as u8).collect());
    let served = Arc::clone(&body);
    let (base, log) = serve_with(move |_, from, number| Reply::Body {
        body: Arc::clone(&served),
        // The first connection hangs a third of the way in.
        stall: (number == 1 && from.is_none()).then_some(100_000),
    });
    let client = test_client();
    thread::spawn(move || {
        let download = Download::start(Fetch::new(client, vec![format!("{base}/track")]));
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        assert_eq!(read_all(&download), *body);
        let log = lock(&log).clone();
        assert!(log.len() >= 2, "{log:?}");
        // It carried on from the first missing byte, not from the start.
        assert!(log[1].1.is_some_and(|from| from > 0), "{log:?}");
    })
    .join()
    .unwrap();
}

/// A network that is gone for a few seconds (here a server answering 503
/// for three) is ridden out: the download carries on where it was rather
/// than failing the song, and every song after it, after two seconds.
#[test]
fn a_few_seconds_without_the_server_are_ridden_out() {
    let body: Arc<Vec<u8>> = Arc::new((0..150_000u32).map(|i| (i % 241) as u8).collect());
    let served = Arc::clone(&body);
    let back_at = Instant::now() + Duration::from_secs(3);
    let (base, _) = serve_with(move |_, _, number| {
        if number == 1 {
            // Part of the track, then the connection drops...
            return Reply::Body {
                body: Arc::clone(&served),
                stall: Some(40_000),
            };
        }
        // ...and the server is out of reach for a while.
        if Instant::now() < back_at {
            return Reply::Status(503);
        }
        Reply::Body {
            body: Arc::clone(&served),
            stall: None,
        }
    });
    let client = test_client();
    thread::spawn(move || {
        let download = Download::start(Fetch::new(client, vec![format!("{base}/track")]));
        assert_eq!(download.wait(Duration::from_secs(30)), Some(Ok(())));
        assert_eq!(read_all(&download), *body);
    })
    .join()
    .unwrap();
}

#[test]
fn an_address_that_expires_mid_track_is_renewed_and_the_download_carries_on() {
    let body: Arc<Vec<u8>> = Arc::new((0..200_000u32).map(|i| (i % 249) as u8).collect());
    let served = Arc::clone(&body);
    let (base, log) = serve_with(move |path, from, _| match (path, from) {
        // The old address serves part of the track and hangs, and then
        // turns every new request away.
        ("/old", None) => Reply::Body {
            body: Arc::clone(&served),
            stall: Some(70_000),
        },
        ("/old", Some(_)) => Reply::Status(403),
        _ => Reply::Body {
            body: Arc::clone(&served),
            stall: None,
        },
    });
    let renewed = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&renewed);
    let fresh = format!("{base}/new");
    let refresh: buffer::Refresh = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(vec![fresh.clone()])
    });
    let client = test_client();
    thread::spawn(move || {
        let download = Download::start(Fetch {
            refresh: Some(refresh),
            ..Fetch::new(client, vec![format!("{base}/old")])
        });
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        assert_eq!(read_all(&download), *body);
        assert_eq!(renewed.load(Ordering::SeqCst), 1);
        let log = lock(&log).clone();
        let resumed = log.iter().find(|(path, _)| path == "/new").unwrap();
        assert!(resumed.1.is_some_and(|from| from > 0), "{log:?}");
    })
    .join()
    .unwrap();
}

/// A long DASH stream holds about its own size, not up to twice it, and the
/// whole file reaches `on_complete` without a second copy staying behind.
#[test]
fn a_finished_dash_stream_holds_about_its_own_size() {
    const SEGMENT: usize = 64 * 1024;
    let (base, _) = serve_with(move |path, _, _| Reply::Body {
        body: Arc::new(if path.ends_with("/init") {
            b"init".to_vec()
        } else {
            vec![7u8; SEGMENT]
        }),
        stall: None,
    });
    let mut urls = vec![format!("{base}/init")];
    urls.extend((0..40).map(|index| format!("{base}/s{index}")));
    let (sent, completed) = std::sync::mpsc::channel();
    let client = client();
    thread::spawn(move || {
        let download = Download::start(Fetch {
            on_complete: Some(Box::new(
                move |stream: &buffer::Finished<'_>, ends: &[u64]| {
                    sent.send((stream.len() as usize, ends.len())).unwrap();
                },
            )),
            ..Fetch::new(client, urls)
        });
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        let len = 4 + 40 * SEGMENT;
        assert_eq!(
            completed.recv_timeout(Duration::from_secs(5)).unwrap(),
            (len, 41)
        );
        let (capacity, held) = download.held();
        assert_eq!(held, len);
        assert!(capacity <= len + SEGMENT, "{capacity} for {len}");
        assert_eq!(read_all(&download).len(), len);
    })
    .join()
    .unwrap();
}

/// A stream given a folder goes to a file there as it arrives: none of
/// it is held in memory, it reads back whole, it is kept by a move once
/// whole, and a stream nobody kept leaves no file behind.
#[test]
fn a_stream_with_a_folder_holds_none_of_itself_in_memory() {
    const SEGMENT: usize = 64 * 1024;
    let (base, _) = serve_with(move |path, _, _| Reply::Body {
        body: Arc::new(if path.ends_with("/init") {
            b"init".to_vec()
        } else {
            vec![path.len() as u8; SEGMENT]
        }),
        stall: None,
    });
    let mut urls = vec![format!("{base}/init")];
    urls.extend((0..20).map(|index| format!("{base}/s{index}")));
    let spill = Scratch::new("spill");
    let kept_at = spill.0.join("kept.bin");
    let client = client();
    let dir = spill.0.clone();
    let target = kept_at.clone();
    let kept_target = kept_at.clone();
    thread::spawn(move || {
        let (sent, completed) = std::sync::mpsc::channel();
        let download = Download::start(Fetch {
            on_complete: Some(Box::new(move |stream: &buffer::Finished<'_>, _: &[u64]| {
                sent.send(stream.keep_at(&target).is_ok()).unwrap();
            })),
            spill: Some(dir.clone()),
            ..Fetch::new(client.clone(), urls.clone())
        });
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        assert!(completed.recv_timeout(Duration::from_secs(5)).unwrap());
        assert_eq!(download.held(), (0, 0), "nothing in memory");
        let whole = read_all(&download);
        assert_eq!(whole.len(), 4 + 20 * SEGMENT);
        assert_eq!(
            std::fs::read(&kept_target).unwrap(),
            whole,
            "kept by a move"
        );
        drop(download);
        assert!(kept_target.exists(), "a kept file stays");

        // Nobody keeps this one: its file goes with it.
        let download = Download::start(Fetch {
            spill: Some(dir.clone()),
            ..Fetch::new(client, urls)
        });
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        drop(download);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let spills = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|item| item.path().extension().is_some_and(|ext| ext == "spill"))
                .count();
            if spills == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "{spills} spill files left");
            thread::sleep(Duration::from_millis(20));
        }
    })
    .join()
    .unwrap();
    assert!(kept_at.exists());
}

/// What has arrived stays readable while the disk is slow to take what
/// comes next: the download writes its file without holding up readers,
/// so a decoder never waits on a write of bytes it is not reading yet
/// (the "audio queue ran dry" gaps on a busy machine). Both a single file
/// and DASH segments.
#[test]
fn arrived_bytes_read_while_a_slow_disk_takes_the_next_ones() {
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let single = serve(body.clone(), None);
    let init = serve(b"init".to_vec(), None);
    let segment = serve(vec![7u8; 100_000], None);
    for (name, urls, first) in [
        ("single", vec![single.url.clone()], body[0]),
        ("dash", vec![init.url.clone(), segment.url.clone()], b'i'),
    ] {
        let spill = Scratch::new(&format!("slow-disk-{name}"));
        buffer::slow_disk::hold(&spill.0, 1);
        let download = Arc::new(Download::start(Fetch {
            spill: Some(spill.0.clone()),
            ..Fetch::new(client(), urls)
        }));
        let (mut reader, _) = download.reader(false);
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte[0], first, "{name}");
        buffer::slow_disk::until_held();
        // A write is stuck on the disk now; a new reader of the bytes that
        // arrived still gets them.
        let (read, answer) = mpsc::channel();
        let reading = Arc::clone(&download);
        thread::spawn(move || {
            let (mut reader, _) = reading.reader(false);
            let mut byte = [0u8; 1];
            let _ = read.send(reader.read_exact(&mut byte).map(|()| byte[0]).ok());
        });
        let got = answer.recv_timeout(Duration::from_secs(10));
        buffer::slow_disk::release();
        assert_eq!(got, Ok(Some(first)), "{name}: a read waited for the disk");
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
    }
}

/// A kept DASH stream read from its file from a later segment is the
/// initialisation segment and that segment on, as a fetch would give it.
#[test]
fn a_kept_file_opens_from_a_later_segment() {
    let dir = Scratch::new("kept-window");
    let path = dir.0.join("track.bin");
    let bytes: Vec<u8> = (0..100u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let download = Download::from_file(&path, &[10, 40, 70, 100], 2).unwrap();
    let never = AtomicBool::new(false);
    assert_eq!(download.end_of(0, &never), Some(10));
    assert_eq!(download.end_of(1, &never), Some(40));
    let expected: Vec<u8> = (0..10u8).chain(70..100u8).collect();
    assert_eq!(read_all(&download), expected);
    let whole = Download::from_file(&path, &[100], 0).unwrap();
    assert_eq!(read_all(&whole), bytes);
    assert_eq!(whole.held(), (0, 0));
}

#[test]
fn a_dash_download_from_a_later_segment_renews_only_its_own_segments() {
    let (base, log) = serve_with(move |path, _, _| {
        if path.starts_with("/a/") {
            return Reply::Status(410);
        }
        Reply::Body {
            body: Arc::new(path.as_bytes().to_vec()),
            stall: None,
        }
    });
    let full = |prefix: &str| -> Vec<String> {
        ["init", "s1", "s2", "s3"]
            .iter()
            .map(|name| format!("{base}/{prefix}/{name}"))
            .collect()
    };
    let old = full("a");
    let fresh = full("b");
    let refresh: buffer::Refresh = Arc::new(move || Ok(fresh.clone()));
    let client = test_client();
    thread::spawn(move || {
        // From the third media segment: the initialisation segment, then s3.
        let download = Download::start(Fetch {
            urls: vec![old[0].clone(), old[3].clone()],
            skipped: 2,
            refresh: Some(refresh),
            ..Fetch::new(client, Vec::new())
        });
        assert_eq!(download.wait(Duration::from_secs(20)), Some(Ok(())));
        assert_eq!(read_all(&download), b"/b/init/b/s3");
        let never = AtomicBool::new(false);
        assert_eq!(download.end_of(0, &never), Some(7));
        let asked: Vec<String> = lock(&log)
            .iter()
            .map(|(path, _)| path.clone())
            .filter(|path| path.starts_with("/b/"))
            .collect();
        assert_eq!(asked, vec!["/b/init", "/b/s3"]);
    })
    .join()
    .unwrap();
}

fn kept_config(cache: Option<&Scratch>, offline: Option<&Scratch>) -> EngineConfig {
    EngineConfig {
        audio_cache_dir: cache.map(|dir| dir.0.clone()),
        audio_cache_limit: Some(64 * 1024 * 1024),
        offline_dir: offline.map(|dir| dir.0.clone()),
        ..config()
    }
}

#[test]
fn a_played_track_is_cached_and_plays_again_without_the_network() {
    let cache = Scratch::new("cache");
    let catalog = FakeCatalog::new();
    let (engine, states) = engine_with(kept_config(Some(&cache), None), catalog.clone());
    load_album(&engine, None);
    let first = until(&states, paused_on("tidal:track:1"));
    assert_eq!(first.format.unwrap().origin, AudioOrigin::Stream);
    // The whole track arrives in the background and is kept. Reading the
    // folder must not sweep it: the download is still spilling files there.
    let kept = ContentCache::peek(cache.0.join("tidal"), None);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !kept.contains("tidal:track:1", Quality::Lossless) {
        assert!(Instant::now() < deadline, "the track was never cached");
        thread::sleep(Duration::from_millis(20));
    }
    engine.shutdown();

    let again = FakeCatalog::new();
    let (engine, states) = engine_with(kept_config(Some(&cache), None), again.clone());
    load_album(&engine, None);
    let second = until(&states, paused_on("tidal:track:1"));
    assert_eq!(second.format.unwrap().origin, AudioOrigin::Cache);
    assert_eq!(
        lock(&again.asked).get("stream"),
        None,
        "a cached track needs no stream"
    );
    engine.shutdown();
}

/// A catalogue with TIDAL out of reach.
struct Unreachable;

impl Catalog for Unreachable {
    fn context_tracks(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Err(anyhow!("offline"))
    }

    fn tracks(&self, _: &[String]) -> Result<Vec<LocalTrack>> {
        Err(anyhow!("offline"))
    }

    fn stream(&self, _: &str, _: Quality) -> Result<StreamInfo> {
        Err(anyhow!("offline"))
    }

    fn autoplay(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Err(anyhow!("offline"))
    }
}

#[test]
fn tracks_kept_offline_play_with_tidal_out_of_reach() {
    let dir = Scratch::new("offline");
    let store = Arc::new(OfflineStore::open(dir.0.clone()));
    let uris: Vec<String> = ["tidal:track:1", "tidal:track:2", "tidal:track:3"]
        .iter()
        .map(|uri| uri.to_string())
        .collect();
    let (events, received) = mpsc::channel();
    let events = Mutex::new(events);
    let job = store.download(
        FakeCatalog::new(),
        client(),
        Quality::Lossless,
        uris.clone(),
        Some("tidal:album:1".into()),
        move |event| {
            let _ = lock(&events).send(event);
        },
    );
    job.join();
    let events: Vec<OfflineEvent> = received.try_iter().collect();
    assert!(events.contains(&OfflineEvent::Done {
        uri: "tidal:track:1".into()
    }));
    assert!(
        events.iter().any(
            |event| matches!(event, OfflineEvent::Failed { uri, .. } if uri == "tidal:track:3")
        ),
        "track 3 cannot be streamed, so it is not kept"
    );
    assert_eq!(events.last(), Some(&OfflineEvent::Finished));
    assert!(store.is_available("tidal:track:1"));
    assert!(!store.is_available("tidal:track:3"));
    let listed: Vec<String> = store.list().into_iter().map(|kept| kept.uri).collect();
    assert_eq!(listed.len(), 2);
    // Opaque names: nothing in the folder looks like music.
    for item in std::fs::read_dir(&dir.0).unwrap() {
        let name = item.unwrap().file_name().into_string().unwrap();
        assert!(name.ends_with(".bin") || name.ends_with(".json"), "{name}");
        assert!(!name.contains("track"), "{name}");
    }

    let (engine, states) = engine_with(kept_config(None, Some(&dir)), Arc::new(Unreachable));
    // The album plays from what is kept of it, in its order.
    load_album(&engine, None);
    let paused = until(&states, paused_on("tidal:track:1"));
    assert_eq!(paused.track.unwrap().title, "Song 1");
    assert_eq!(paused.format.unwrap().origin, AudioOrigin::Offline);
    assert_eq!(engine.queue(10).next_up, vec![track(2)]);
    // So does a single track.
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: vec!["tidal:track:2".into()],
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    until(&states, paused_on("tidal:track:2"));

    store.remove("tidal:track:2").unwrap();
    assert!(!store.is_available("tidal:track:2"));
    engine.shutdown();
}

/// A cancelled download accounts for every track it was asked for and
/// keeps none of them.
#[test]
fn a_cancelled_download_reports_each_track_and_keeps_nothing() {
    let dir = Scratch::new("offline-cancel");
    let store = Arc::new(OfflineStore::open(dir.0.clone()));
    let uris: Vec<String> = ["tidal:track:1", "tidal:track:2"]
        .iter()
        .map(|uri| uri.to_string())
        .collect();
    let (events, received) = mpsc::channel();
    let events = Mutex::new(events);
    let (go, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    let job = store.download(
        Arc::new(Unreachable),
        client(),
        Quality::Lossless,
        uris,
        None,
        move |event| {
            // Hold the job at its first report until it is cancelled.
            let _ = lock(&wait).recv_timeout(Duration::from_secs(5));
            let _ = lock(&events).send(event);
        },
    );
    job.cancel();
    drop(go);
    job.join();
    let events: Vec<OfflineEvent> = received.try_iter().collect();
    assert_eq!(events.last(), Some(&OfflineEvent::Finished));
    for uri in ["tidal:track:1", "tidal:track:2"] {
        assert!(
            events.iter().any(|event| matches!(
                event,
                OfflineEvent::Cancelled { uri: cancelled } | OfflineEvent::Failed { uri: cancelled, .. }
                    if cancelled == uri
            )),
            "{uri} is accounted for: {events:?}"
        );
        assert!(!store.is_available(uri));
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OfflineEvent::Cancelled { .. })),
        "the track after the cancel is reported cancelled: {events:?}"
    );
}

/// A cancel that lands just as a song finishes downloading, while it is
/// being kept or just after, keeps nothing of it and says Cancelled, never
/// Done: a removed download does not come back.
#[test]
fn a_cancel_as_a_song_finishes_keeps_nothing() {
    for stage in ["commit", "kept"] {
        let dir = Scratch::new(&format!("offline-cancel-{stage}"));
        let (reached, at) = mpsc::channel::<()>();
        let (go, wait) = mpsc::channel::<()>();
        let reached = Mutex::new(reached);
        let wait = Mutex::new(wait);
        let store = Arc::new(OfflineStore::open(dir.0.clone()).with_hook(move |step| {
            if step == stage {
                let _ = lock(&reached).send(());
                let _ = lock(&wait).recv_timeout(Duration::from_secs(10));
            }
        }));
        let (events, received) = mpsc::channel();
        let events = Mutex::new(events);
        let job = store.download(
            FakeCatalog::new(),
            client(),
            Quality::Lossless,
            vec!["tidal:track:1".into()],
            None,
            move |event| {
                let _ = lock(&events).send(event);
            },
        );
        at.recv_timeout(Duration::from_secs(10))
            .expect("the download reaches the step");
        job.cancel();
        let _ = go.send(());
        job.join();
        let events: Vec<OfflineEvent> = received.try_iter().collect();
        assert!(
            events.contains(&OfflineEvent::Cancelled {
                uri: "tidal:track:1".into()
            }),
            "{stage}: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, OfflineEvent::Done { .. })),
            "{stage}: {events:?}"
        );
        assert!(!store.is_available("tidal:track:1"), "{stage}");
    }
}

#[test]
fn replacing_a_playing_track_interrupts_queued_audio() {
    let playing = LocalState {
        playback: Playback::Playing,
        ..LocalState::default()
    };
    assert!(command_interrupts_audio(&playing, &PlayerCommand::Next));
    assert!(command_interrupts_audio(
        &playing,
        &PlayerCommand::Load(LoadSpec::default())
    ));
    assert!(!command_interrupts_audio(&playing, &PlayerCommand::Seek(1)));
    assert!(!command_interrupts_audio(
        &LocalState::default(),
        &PlayerCommand::Next
    ));
}

/// Bit-perfect playback through the whole engine: ReplayGain (-6 dB here),
/// the equalizer and the volume are all set, and the device still gets the
/// file's own samples, byte for byte. A capture stands in for the device,
/// so this load can play.
#[test]
fn bit_perfect_playback_reaches_the_device_unaltered() {
    let (noise, data) = exclusive::tests::noise_wav(decode::RATE, 16, 30_000);
    let catalog = Arc::new(FakeCatalog {
        url: serve(noise, None).url,
        asked: Mutex::default(),
    });
    let captured = Arc::new(Mutex::new(Vec::new()));
    let opener =
        exclusive::tests::capture(|format| format.container_bits == 16, Arc::clone(&captured));
    let eq = crate::eq::shared();
    {
        let mut settings = lock(&eq);
        settings.on = true;
        settings.preamp_db = 6.0;
    }
    let config = EngineConfig {
        bit_perfect: true,
        autoplay: false,
        crossfade_ms: 5_000,
        initial_volume: u16::MAX / 4,
        eq,
        ..config()
    };
    let (sender, states) = mpsc::channel();
    let sender = Mutex::new(sender);
    let notify: Notify = Arc::new(move |event| {
        if let EngineEvent::State(state) = event {
            let _ = lock(&sender).send(state);
        }
    });
    let engine = Engine::with_exclusive(config, catalog, notify, opener).unwrap();
    let first = until(&states, |state| state.connected);
    assert!(first.output.bit_perfect_requested && first.output.volume_locked);
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: vec!["tidal:track:1".into()],
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    let playing = until(&states, |state| state.output.bit_perfect);
    assert!(playing.output.exclusive && playing.output.volume_locked);
    assert_eq!(
        playing
            .output
            .device_format
            .map(|format| format.sample_rate),
        Some(decode::RATE)
    );
    until(&states, |state| {
        state.playback == Playback::Stopped && state.track.is_some()
    });
    let bytes = lock(&captured).clone();
    assert_eq!(bytes.len(), data.len());
    assert!(bytes == data, "the device got altered samples");
    engine.shutdown();
}

/// Songs and music videos: songs from a WAV file, video 5 from a two
/// segment playlist, and video 9 protected.
struct Videos {
    song: String,
    playlist: String,
}

impl Catalog for Videos {
    fn context_tracks(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Err(anyhow!("no contexts"))
    }

    fn tracks(&self, uris: &[String]) -> Result<Vec<LocalTrack>> {
        Ok(uris
            .iter()
            .map(|uri| LocalTrack {
                uri: uri.clone(),
                title: uri.clone(),
                duration_ms: 8_000,
                ..LocalTrack::default()
            })
            .collect())
    }

    fn stream(&self, _: &str, quality: Quality) -> Result<StreamInfo> {
        Ok(StreamInfo {
            urls: vec![self.song.clone()],
            mime: "audio/wav".into(),
            codec: "pcm".into(),
            quality,
            ..StreamInfo::default()
        })
    }

    fn video_stream(&self, video_id: &str) -> Result<StreamInfo> {
        if video_id == "9" {
            return Err(anyhow!("TIDAL sent a protected stream"));
        }
        Ok(StreamInfo {
            urls: vec![self.playlist.clone()],
            ..StreamInfo::default()
        })
    }

    fn autoplay(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Ok(Vec::new())
    }
}

#[test]
fn songs_and_videos_play_from_one_queue() {
    use super::ts::fixture::playable_segment;
    let song = serve(wav(decode::RATE, 1.0), None);
    let first = serve(playable_segment(900_000, 100), None);
    let second = serve(playable_segment(1_260_000, 100), None);
    let playlist = serve(
        format!(
            "#EXTM3U\n#EXTINF:4,\n{}\n#EXTINF:4,\n{}\n#EXT-X-ENDLIST\n",
            first.url, second.url
        )
        .into_bytes(),
        None,
    );
    let hub = Arc::new(super::video::VideoHub::default());
    let config = EngineConfig {
        video: super::video::VideoOptions {
            quality: super::hls::VideoQuality::Auto,
            hub: Arc::clone(&hub),
        },
        ..config()
    };
    let catalog = Arc::new(Videos {
        song: song.url.clone(),
        playlist: playlist.url.clone(),
    });
    let (engine, states) = engine_with(config, catalog);
    until(&states, |state| state.connected);
    let uris = [
        "tidal:track:1",
        "tidal:video:5",
        "tidal:video:9",
        "tidal:track:2",
    ];
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: uris.iter().map(|uri| uri.to_string()).collect(),
            offset_uri: Some("tidal:video:5".into()),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    // The video's sound opens like a song's, at the video's own length.
    let video = until(&states, paused_on("tidal:video:5"));
    let format = video.format.expect("the video's format");
    assert_eq!(format.codec, "aac");
    assert_eq!(format.sample_rate, 44_100);
    assert_eq!(engine.queue(10).next_up.len(), 2);
    // Its picture went to the hub.
    let picture = hub
        .segment_at("tidal:video:5", 0)
        .expect("the first segment");
    assert_eq!(picture.first_frame_ms, 0);
    assert_eq!(picture.fps, 25.0);
    // The protected video is skipped with TIDAL-like words, on to the song.
    engine.command(PlayerCommand::Next).unwrap();
    let song = until(&states, paused_on("tidal:track:2"));
    let error = song.error.expect("the skipped video says why");
    assert!(error.contains(super::video::UNPLAYABLE), "{error}");
    // The video's stream is gone from the hub with it.
    assert!(hub.info("tidal:video:5").is_none());
}

/// A restored session whose context cannot be read again, or no longer
/// holds the song it stopped on, goes on with its saved queue: Next plays
/// the saved next song, never an autoplay station.
#[test]
fn an_unreadable_context_continues_with_the_saved_queue() {
    let (engine, states) = engine(FakeCatalog::new());
    let saved: Vec<String> = [20, 21, 22]
        .iter()
        .map(|n| format!("tidal:track:{n}"))
        .collect();
    for context in [
        "tidal:toptracks:7",
        "tidal:artist:7",
        "tidal:mix:abc",
        "tidal:playlist:gone",
        "tidal:search:cats",
        "tidal:collection",
        // Readable, but without the song the session stopped on.
        "tidal:album:1",
    ] {
        engine
            .command(PlayerCommand::Load(LoadSpec {
                context_uri: Some(context.into()),
                offset_uri: Some("tidal:track:20".into()),
                fallback: saved.clone(),
                play: false,
                ..LoadSpec::default()
            }))
            .unwrap();
        until(&states, paused_on("tidal:track:20"));
        engine.command(PlayerCommand::Next).unwrap();
        until(&states, paused_on("tidal:track:21"));
    }
}

/// A context that is still readable wins over the saved queue.
#[test]
fn a_readable_context_is_not_replaced_by_the_saved_queue() {
    let (engine, states) = engine(FakeCatalog::new());
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:album:1".into()),
            offset_uri: Some("tidal:track:2".into()),
            fallback: vec!["tidal:track:2".into(), "tidal:track:20".into()],
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    until(&states, paused_on("tidal:track:2"));
    assert_eq!(
        engine.queue(10).next_up,
        vec![track(3), track(4)],
        "the album goes on"
    );
}

/// A plugin's source for the playing song (AltPlay: the listener's own
/// media server) plays in place of TIDAL's stream, as the same song from
/// where it was; a source that fails falls back to TIDAL's stream and says
/// so; a source for another song is ignored; the next song is TIDAL's.
#[test]
fn a_plugin_source_replaces_the_playing_songs_audio_and_falls_back_when_it_fails() {
    let catalog = FakeCatalog::new();
    let (engine, states) = engine(Arc::clone(&catalog));
    load_album(&engine, Some("tidal:track:2"));
    let tidal = until(&states, paused_on("tidal:track:2"));
    assert_eq!(tidal.format.as_ref().unwrap().origin, AudioOrigin::Stream);
    let sequence = tidal.track_sequence;
    let from = |origin: AudioOrigin| {
        move |state: &LocalState| {
            paused_on("tidal:track:2")(state)
                && state.format.as_ref().is_some_and(|f| f.origin == origin)
        }
    };

    let own = serve(wav(22_050, 1.0), None);
    let replace = |uri: &str, url: Option<&str>| {
        engine
            .command(PlayerCommand::ReplaceAudio {
                uri: uri.into(),
                url: url.map(|u| SourceUrl(u.into())),
                own: false,
                carrier: None,
            })
            .unwrap();
    };
    // About a song that is not playing: nothing happens.
    replace("tidal:track:4", Some(&own.url));
    replace("tidal:track:2", Some(&own.url));
    let replaced = until(&states, from(AudioOrigin::Plugin));
    assert_eq!(replaced.format.as_ref().unwrap().sample_rate, 22_050);
    assert_eq!(
        replaced.track_sequence, sequence,
        "the same song, not a new one"
    );
    assert!(!replaced.replace_failed);

    // Back to TIDAL's stream.
    replace("tidal:track:2", None);
    let back = until(&states, from(AudioOrigin::Stream));
    assert_eq!(back.track_sequence, sequence);

    // A source that cannot be reached: TIDAL's stream again, and it says so.
    replace(
        "tidal:track:2",
        Some("http://127.0.0.1:9/gone?api_key=secret"),
    );
    let failed = until(&states, |state| state.replace_failed);
    assert_eq!(failed.track_sequence, sequence, "not skipped");
    let fell_back = until(&states, from(AudioOrigin::Stream));
    assert!(fell_back.replace_failed);
    assert!(fell_back.error.is_none(), "{:?}", fell_back.error);

    // The next song plays from TIDAL, with nothing carried over.
    replace("tidal:track:2", Some(&own.url));
    until(&states, from(AudioOrigin::Plugin));
    engine.command(PlayerCommand::Next).unwrap();
    let next = until(&states, paused_on("tidal:track:4"));
    assert_eq!(next.format.as_ref().unwrap().origin, AudioOrigin::Stream);
    assert!(!next.replace_failed);
}

/// A song of a plugin's own (AltPlay's Jellyfin library: a file that is not
/// on TIDAL) takes the place of the TIDAL song that was loaded: it plays at
/// once from its start, when it ends the engine stays on it, paused, and
/// counts the end instead of going on to the next TIDAL song, and letting it
/// go leaves the TIDAL song paused where it was.
#[test]
fn a_plugins_own_song_plays_from_its_start_and_holds_at_its_end() {
    let catalog = FakeCatalog::new();
    let (engine, states) = quiet_engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    load_album(&engine, Some("tidal:track:2"));
    let tidal = until(&states, paused_on("tidal:track:2"));
    let sequence = tidal.track_sequence;
    engine.command(PlayerCommand::Seek(500)).unwrap();

    let own = serve(wav(22_050, 1.0), None);
    engine
        .command(PlayerCommand::ReplaceAudio {
            uri: "tidal:track:2".into(),
            url: Some(SourceUrl(own.url.clone())),
            own: true,
            carrier: None,
        })
        .unwrap();
    let playing = until(&states, |state| {
        state.playback == Playback::Playing
            && state
                .format
                .as_ref()
                .is_some_and(|f| f.origin == AudioOrigin::Plugin)
    });
    assert!(
        playing.position_ms < 1_000,
        "from the start, not the TIDAL song's {} ms",
        playing.position_ms
    );
    assert_eq!(playing.track_sequence, sequence);
    assert!(
        playing
            .source_ms
            .is_some_and(|ms| (900..=1100).contains(&ms)),
        "the file's own length, {:?}",
        playing.source_ms
    );

    // It plays to its end and stays: the album does not go on.
    let held = until(&states, |state| state.replace_ended == 1);
    assert_eq!(held.playback, Playback::Paused);
    assert_eq!(
        held.track.as_ref().map(|t| t.uri.as_str()),
        Some("tidal:track:2")
    );

    // Let go: the TIDAL song, paused, where it was.
    engine
        .command(PlayerCommand::ReplaceAudio {
            uri: "tidal:track:2".into(),
            url: None,
            own: false,
            carrier: None,
        })
        .unwrap();
    let back = until(&states, |state| {
        state
            .format
            .as_ref()
            .is_some_and(|f| f.origin == AudioOrigin::Stream)
    });
    assert_eq!(back.playback, Playback::Paused);
    assert!(
        (400..=600).contains(&back.position_ms),
        "{} ms",
        back.position_ms
    );
}

/// A song of a plugin's own can start when nothing is loaded (the music
/// ended): it takes the place of the last song, which stays the song the
/// player reports, so a plugin that hears a new song (and ends its own)
/// is not told one.
#[test]
fn a_plugins_own_song_starts_with_nothing_loaded_without_a_new_song() {
    let catalog = FakeCatalog::new();
    let stops = EngineConfig {
        autoplay: false,
        ..config()
    };
    let (engine, states) = quiet_engine_with(stops, Arc::clone(&catalog) as Arc<dyn Catalog>);
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: vec!["tidal:track:1".into()],
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    let ended = until(&states, |state| {
        state.playback == Playback::Stopped && state.track.is_some()
    });
    let sequence = ended.track_sequence;

    let own = serve(wav(22_050, 1.0), None);
    let replace = |uri: &str, own_song: bool| {
        engine
            .command(PlayerCommand::ReplaceAudio {
                uri: uri.into(),
                url: Some(SourceUrl(own.url.clone())),
                own: own_song,
                carrier: None,
            })
            .unwrap();
    };
    // Not a song of its own: nothing happens.
    replace("tidal:track:1", false);
    replace("tidal:track:1", true);
    let playing = until(&states, |state| {
        state.playback == Playback::Playing
            && state
                .format
                .as_ref()
                .is_some_and(|f| f.origin == AudioOrigin::Plugin)
    });
    assert_eq!(playing.track_sequence, sequence, "no new song is reported");
    assert_eq!(
        playing.track.as_ref().map(|t| t.uri.as_str()),
        Some("tidal:track:1")
    );
    assert!(playing.position_ms < 1_000, "{} ms", playing.position_ms);
    let held = until(&states, |state| state.replace_ended == 1);
    assert_eq!(held.playback, Playback::Paused);
    assert_eq!(held.track_sequence, sequence);
    engine.shutdown();

    // A remembered song nobody has played since the start is in no queue
    // yet: a song of the plugin's own still plays for the song it names,
    // with no song reported.
    let (fresh, states) = quiet_engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    fresh
        .command(PlayerCommand::ReplaceAudio {
            uri: "tidal:track:7".into(),
            url: Some(SourceUrl(own.url.clone())),
            own: true,
            carrier: Some(Box::new(LocalTrack {
                uri: "tidal:track:7".into(),
                title: "Remembered".into(),
                ..LocalTrack::default()
            })),
        })
        .unwrap();
    let playing = until(&states, |state| {
        state.playback == Playback::Playing
            && state
                .format
                .as_ref()
                .is_some_and(|f| f.origin == AudioOrigin::Plugin)
    });
    assert_eq!(playing.track_sequence, 0, "not a new song");
    assert_eq!(
        playing.track.as_ref().map(|t| t.title.as_str()),
        Some("Remembered"),
        "the song the bar shows stays"
    );
    fresh.shutdown();
}
