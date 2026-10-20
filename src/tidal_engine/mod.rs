//! Local playback of TIDAL streams.
//!
//! This is the playback engine behind the same
//! surface: the backend sends [`PlayerCommand`]s and receives
//! [`EngineEvent::State`] snapshots. The engine keeps its own queue (what
//! what a remote controller would otherwise do), asks the backend's [`Catalog`] for
//! tracks and streams, downloads and decodes each track on threads of its
//! own, and plays through Tidalfast's output chain, so the equalizer
//! carries on unchanged. The chain runs at each
//! track's own sample rate and converts once, to the device's, if at all.
//!
//! Tracks played are kept in a bounded disk cache, and tracks can be kept
//! for offline listening ([`store`]); both are played without asking TIDAL.
//! Tracks can overlap by a crossfade ([`crossfade`]) instead of joining
//! gaplessly.
//!
//! One engine thread owns the queue, the decoders and the output. Nothing
//! it does blocks on the network: catalogue calls run on short-lived
//! workers that post their answers back, and each track resolves, downloads
//! and decodes on its own thread ([`decode`]). The only wait on the engine
//! thread is the output's backpressure while it writes sound.

mod adaptive;
pub mod buffer;
pub mod catalog;
pub mod crossfade;
pub mod dash;
pub mod decode;
mod dedupe;
pub mod exclusive;
pub mod hls;
mod output;
pub mod queue;
mod recent;
pub mod store;
pub mod ts;
mod types;
pub mod video;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};

pub use catalog::{Catalog, Quality, StreamInfo};
pub use output::volume_gain;
pub use queue::QueueView;
pub use store::{ContentCache, DISK_FULL, OfflineEvent, OfflineJob, OfflineStore, OfflineTrack};
pub(crate) use types::Heard;
pub use types::{
    AudioOrigin, BlockedSet, DeviceFormat, EngineConfig, EngineEvent, Interrupted, LoadSpec,
    LocalState, LocalTrack, MAX_CROSSFADE_MS, Notify, OutputStatus, Playback, PlayerCommand,
    RepeatMode, SourceUrl, StreamFormat,
};

use crate::sink::{AudioControl, AudioSink, OutputEvent, Packet};
use crossfade::Crossfade;
use decode::{Poll, Source, Status, TrackStream};
use queue::{Advance, Queue, Start};

/// How long before a track's end the next one starts loading, at least:
/// a crossfade starts it earlier still.
const PRELOAD: Duration = Duration::from_secs(15);
/// How long a read on a stream's connection may wait for bytes before the
/// connection counts as stalled and the download reconnects.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// Previous restarts the playing song once it is this far in.
const RESTART_AFTER_MS: u32 = 3_000;
/// How often the state is reported while playing.
const REPORT: Duration = Duration::from_secs(1);
/// How often a paused song whose sound is still loading is looked at.
const PAUSED_LOOK: Duration = Duration::from_millis(500);
/// Unplayable tracks skipped in a row before playback stops.
const MAX_FAILURES: u32 = 5;
/// How long a playing song may wait for its next sound (the network is
/// behind) before it shows as loading, with its position held, rather than
/// as playing on.
const STALL: Duration = Duration::from_secs(1);
/// Songs autoplay is asked to start from before it gives up: TIDAL has no
/// station for some songs (one taken down, say).
const AUTOPLAY_SEEDS: usize = 3;

/// What a replaced engine was playing, for the engine that replaces it.
#[derive(Clone, Debug)]
pub enum PlaybackResume {
    /// The whole queue, its order and the position, from a live engine.
    Queue(Arc<Snapshot>),
    /// One track at a position.
    Track(LoadSpec),
}

/// An engine's queue and position.
#[derive(Clone, Debug)]
pub struct Snapshot {
    queue: Queue,
    position_ms: u32,
    playing: bool,
}

enum Msg {
    Command(PlayerCommand),
    Context {
        id: u64,
        spec: LoadSpec,
        result: Result<Vec<LocalTrack>>,
    },
    /// Later pages of the context a load started on its first page.
    ContextMore {
        id: u64,
        tracks: Vec<LocalTrack>,
    },
    Autoplay {
        id: u64,
        result: Result<Vec<LocalTrack>>,
    },
    Tracks(Result<Vec<LocalTrack>>),
    Restore(Arc<Snapshot>),
    Shutdown,
}

pub struct Engine {
    sender: Sender<Msg>,
    offline: Option<Arc<OfflineStore>>,
    state: Arc<Mutex<LocalState>>,
    queue: Arc<Mutex<Queue>>,
    volume: output::Volume,
    audio: Arc<AudioControl>,
    notify: Notify,
    shut: AtomicBool,
    /// Times the engine's thread has woken, for tests.
    #[cfg(test)]
    wakes: Arc<AtomicU64>,
}

/// Makes an output in place of the device's.
pub(crate) type SinkFactory = Arc<dyn Fn() -> Box<dyn AudioSink> + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Engine {
    /// Starts the engine. Nothing plays, and no device opens, until a load.
    pub fn new(config: EngineConfig, catalog: Arc<dyn Catalog>, notify: Notify) -> Result<Self> {
        Self::with_exclusive(config, catalog, notify, exclusive::platform_opener())
    }

    /// As [`Engine::new`], with the device the bit-perfect output opens.
    pub(crate) fn with_exclusive(
        config: EngineConfig,
        catalog: Arc<dyn Catalog>,
        notify: Notify,
        exclusive: exclusive::Opener,
    ) -> Result<Self> {
        Self::with_outputs(config, catalog, notify, exclusive, None)
    }

    /// As [`Engine::with_exclusive`], with the output made by `make_sink`
    /// in place of the shared device output: tests play through to the
    /// end of songs without making a sound.
    pub(crate) fn with_outputs(
        config: EngineConfig,
        catalog: Arc<dyn Catalog>,
        notify: Notify,
        exclusive: exclusive::Opener,
        make_sink: Option<SinkFactory>,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(LocalState {
            volume: config.initial_volume,
            connected: true,
            output: OutputStatus {
                bit_perfect_requested: config.bit_perfect,
                volume_locked: config.bit_perfect,
                ..OutputStatus::default()
            },
            ..LocalState::default()
        }));
        let mut fresh_queue = Queue::default();
        // Recent plays live beside the other state files; a test with no
        // state folder keeps them in memory.
        if let Some(dir) = config.volume_dir.parent() {
            fresh_queue.remember_in(dir.join("recent_plays.json"));
        }
        let queue = Arc::new(Mutex::new(fresh_queue));
        let volume = output::Volume::new(config.initial_volume);
        let audio = AudioControl::new(config.buffer_ms);
        let (sender, receiver) = mpsc::channel();
        let cache = config.audio_cache_dir.as_ref().map(|dir| {
            // Kept in its own subfolder of the audio cache.
            Arc::new(ContentCache::new(
                dir.join("tidal"),
                config.audio_cache_limit,
            ))
        });
        let offline = config
            .offline_dir
            .clone()
            .map(|dir| Arc::new(OfflineStore::open(dir)));
        let stores = (cache, offline.clone());
        let parts = (
            config,
            catalog,
            Arc::clone(&state),
            Arc::clone(&queue),
            Arc::clone(&notify),
            volume.clone(),
            Arc::clone(&audio),
            sender.clone(),
        );
        #[cfg(test)]
        let wakes = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let core_wakes = Arc::clone(&wakes);
        thread::Builder::new()
            .name("tidal-engine".into())
            .spawn(move || {
                // This thread writes the sound: it keeps ahead of other
                // work, whichever output it writes to.
                crate::sink::take_precedence();
                // Made here: the output and its converter stay on this thread.
                let (config, catalog, state, queue, notify, volume, audio, sender) = parts;
                let (cache, offline) = stores;
                Core {
                    source: None,
                    cache,
                    offline,
                    outgoing: None,
                    catalog,
                    state,
                    queue,
                    notify,
                    volume,
                    audio,
                    sender,
                    sink: None,
                    sink_running: false,
                    exclusive,
                    make_sink,
                    told_format: None,
                    current: None,
                    next: None,
                    playing: false,
                    announced: false,
                    sounded: false,
                    factor: 1.0,
                    prefer_album: false,
                    load_id: 0,
                    live_load: Arc::default(),
                    autoplay_id: None,
                    autoplay_back: 0,
                    autoplay_asked: false,
                    autoplay_waiting: false,
                    failures: 0,
                    last_report: Instant::now(),
                    waiting_since: None,
                    stalled: false,
                    replacement: None,
                    replacement_own: false,
                    carrier_ms: None,
                    replaced_at: None,
                    adaptive: config
                        .adaptive
                        .then(|| adaptive::Adaptive::new(config.quality)),
                    config,
                    #[cfg(test)]
                    wakes: core_wakes,
                }
                .run(receiver)
            })
            .map_err(|error| anyhow!("unable to start the playback engine: {error}"))?;
        notify(EngineEvent::State(lock(&state).clone()));
        Ok(Self {
            sender,
            offline,
            state,
            queue,
            volume,
            audio,
            notify,
            shut: AtomicBool::new(false),
            #[cfg(test)]
            wakes,
        })
    }

    /// Times the engine's thread has woken, by a message or a timeout.
    #[cfg(test)]
    pub(crate) fn wakes(&self) -> u64 {
        self.wakes.load(Ordering::SeqCst)
    }

    pub fn command(&self, command: PlayerCommand) -> Result<()> {
        match command {
            PlayerCommand::Volume(volume) => return self.note_volume(volume, false),
            PlayerCommand::VolumePreview(volume) => return self.note_volume(volume, true),
            _ => {}
        }
        let interrupts_audio = command_interrupts_audio(&lock(&self.state), &command);
        if interrupts_audio {
            self.audio.interrupt();
        }
        let sent = self
            .sender
            .send(Msg::Command(command))
            .map_err(|_| anyhow!("the playback engine has stopped"));
        if interrupts_audio && sent.is_err() {
            self.audio.stopped();
        }
        sent
    }

    /// Applies a level at once. The kept level is reported; a preview, the
    /// slider mid-drag, is not.
    fn note_volume(&self, volume: u16, preview: bool) -> Result<()> {
        self.volume.set(volume);
        let snapshot = {
            let mut state = lock(&self.state);
            state.volume = volume;
            state.clone()
        };
        if !preview {
            (self.notify)(EngineEvent::State(snapshot));
        }
        Ok(())
    }

    /// The tracks kept for offline listening, when the engine keeps any.
    /// [`OfflineStore::download`] fills it; the engine plays what it holds
    /// without asking TIDAL.
    pub fn offline(&self) -> Option<Arc<OfflineStore>> {
        self.offline.clone()
    }

    /// The queue as it stands, with up to `next_up` songs of the context.
    pub fn queue(&self, next_up: usize) -> QueueView {
        lock(&self.queue).view(next_up)
    }

    /// Playback state to resume after replacing this engine.
    pub fn interrupted(&self) -> Option<Interrupted> {
        lock(&self.state).interrupted()
    }

    /// What this engine is heard at, kept past the engine itself: the next
    /// engine starts there.
    // The backend switches to this engine in the integration phase; until
    // then only the tests call it.
    #[allow(dead_code)]
    pub(crate) fn heard(&self) -> Heard {
        Heard::new(Arc::clone(&self.state))
    }

    /// The queue and position to pick up in the engine that replaces this.
    pub fn resume_point(&self) -> Option<PlaybackResume> {
        let interrupted = self.interrupted()?;
        let queue = lock(&self.queue).clone();
        if queue
            .current()
            .is_some_and(|current| current.uri == interrupted.uri)
        {
            return Some(PlaybackResume::Queue(Arc::new(Snapshot {
                queue,
                position_ms: interrupted.position_ms,
                playing: interrupted.playing,
            })));
        }
        Some(PlaybackResume::Track(LoadSpec {
            uris: vec![interrupted.uri],
            position_ms: interrupted.position_ms,
            play: interrupted.playing,
            ..LoadSpec::default()
        }))
    }

    pub fn resume(&self, resume: PlaybackResume) -> Result<()> {
        match resume {
            PlaybackResume::Queue(snapshot) => self
                .sender
                .send(Msg::Restore(snapshot))
                .map_err(|_| anyhow!("the playback engine has stopped")),
            PlaybackResume::Track(spec) => self.command(PlayerCommand::Load(spec)),
        }
    }

    pub fn shutdown(&self) {
        if !self.shut.swap(true, Ordering::SeqCst) {
            let _ = self.sender.send(Msg::Shutdown);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// How long a plugin's source for the playing song may take to have sound.
const SOURCE_PATIENCE: Duration = Duration::from_secs(5);

fn command_interrupts_audio(state: &LocalState, command: &PlayerCommand) -> bool {
    state.playback == Playback::Playing
        && matches!(
            command,
            PlayerCommand::Next | PlayerCommand::Previous | PlayerCommand::Load(_)
        )
}

fn set<T: PartialEq>(target: &mut T, value: T) -> bool {
    if *target == value {
        false
    } else {
        *target = value;
        true
    }
}

/// The engine thread's state.
struct Core {
    config: EngineConfig,
    catalog: Arc<dyn Catalog>,
    /// Where tracks come from; made on this thread, since a blocking HTTP
    /// client must not be made or dropped inside the backend's runtime.
    source: Option<std::result::Result<Source, String>>,
    /// Played tracks kept on disk.
    cache: Option<Arc<ContentCache>>,
    /// Tracks kept for offline listening.
    offline: Option<Arc<OfflineStore>>,
    /// The previous track, fading out under the current one.
    outgoing: Option<Crossfade>,
    state: Arc<Mutex<LocalState>>,
    queue: Arc<Mutex<Queue>>,
    notify: Notify,
    volume: output::Volume,
    audio: Arc<AudioControl>,
    sender: Sender<Msg>,
    sink: Option<Box<dyn AudioSink>>,
    sink_running: bool,
    /// Opens the device for the bit-perfect output.
    exclusive: exclusive::Opener,
    /// Makes the output in place of the usual chain, for tests.
    make_sink: Option<SinkFactory>,
    /// The track format the sink was last told.
    told_format: Option<StreamFormat>,
    current: Option<TrackStream>,
    /// The track after this one, loading ahead of time.
    next: Option<TrackStream>,
    /// Whether the listener wants sound.
    playing: bool,
    /// The current track's start has been reported.
    announced: bool,
    /// A song of this load has had sound: a song that cannot be played at
    /// the end of the context then goes on into autoplay, but a load whose
    /// only song cannot be played does not start some other music.
    sounded: bool,
    factor: f64,
    prefer_album: bool,
    load_id: u64,
    /// `load_id` as workers read it, so a context still being read page by
    /// page stops once another load replaces it.
    live_load: Arc<AtomicU64>,
    autoplay_id: Option<u64>,
    /// How many songs back from the last one autoplay was seeded with.
    autoplay_back: usize,
    /// Autoplay was asked ahead of this song's end. Once is enough: an
    /// answer of nothing is not asked again on every packet.
    autoplay_asked: bool,
    /// The context ended and waits for autoplay's answer.
    autoplay_waiting: bool,
    failures: u32,
    last_report: Instant,
    /// When the playing song began waiting for its next sound.
    waiting_since: Option<Instant>,
    /// It has waited past [`STALL`] and shows as loading.
    stalled: bool,
    /// The playing track's URI and the plugin source it plays from in
    /// place of TIDAL's stream ([`PlayerCommand::ReplaceAudio`]).
    replacement: Option<(String, String)>,
    /// That source is a song of the plugin's own: it ends without a next
    /// song ([`PlayerCommand::ReplaceAudio`]).
    replacement_own: bool,
    /// Where the TIDAL song was when the plugin's own song took its place.
    carrier_ms: Option<u32>,
    /// When the playing song was opened from that source, until it has
    /// sound.
    replaced_at: Option<Instant>,
    /// The quality the next song streams at, with adaptive streaming on.
    adaptive: Option<adaptive::Adaptive>,
    #[cfg(test)]
    wakes: Arc<AtomicU64>,
}

impl Core {
    fn run(mut self, receiver: Receiver<Msg>) {
        loop {
            let first = match self.idle_time() {
                // Nothing to look at until a message comes.
                None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(timeout) if timeout.is_zero() => {
                    receiver.try_recv().map_err(|error| match error {
                        mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                        mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                    })
                }
                Some(timeout) => receiver.recv_timeout(timeout),
            };
            #[cfg(test)]
            self.wakes.fetch_add(1, Ordering::SeqCst);
            match first {
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(message) => {
                    if !self.handle(message) {
                        break;
                    }
                    let mut stop = false;
                    while let Ok(message) = receiver.try_recv() {
                        if matches!(message, Msg::Shutdown) || !self.handle(message) {
                            stop = true;
                            break;
                        }
                    }
                    if stop {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.step();
        }
        lock(&self.queue).flush_recent();
        self.current = None;
        self.next = None;
        let mut state = lock(&self.state);
        state.playback = Playback::Stopped;
        state.position_at = None;
        state.loading = false;
    }

    /// How long to wait for a message before looking at the audio again;
    /// `None` while nothing can change without one: no song, or a paused
    /// song whose sound is ready ([`Self::step`] does nothing then).
    fn idle_time(&mut self) -> Option<Duration> {
        let stream = self.current.as_mut()?;
        if !stream.ready() {
            if self.announced && !self.playing && self.replaced_at.is_none() {
                // Paused while its sound loads: nothing waits on it.
                return Some(PAUSED_LOOK);
            }
            // Decoding or downloading: look again soon.
            Some(Duration::from_millis(5))
        } else if self.playing || !self.announced {
            Some(Duration::ZERO)
        } else {
            None
        }
    }

    fn report(&mut self) {
        self.last_report = Instant::now();
        let delay = self.audio.output_delay().as_millis() as u32;
        let snapshot = {
            let mut state = lock(&self.state);
            state.output_delay_ms = delay;
            state.clone()
        };
        (self.notify)(EngineEvent::State(snapshot));
    }

    fn update(&mut self, change: impl FnOnce(&mut LocalState) -> bool) {
        let changed = change(&mut lock(&self.state));
        if changed {
            self.report();
        }
    }

    fn source(&mut self) -> std::result::Result<Source, String> {
        let config = &self.config;
        let catalog = &self.catalog;
        let (cache, offline) = (&self.cache, &self.offline);
        self.source
            .get_or_insert_with(|| {
                stream_client(&config.proxy).map(|client| Source::Remote {
                    catalog: Arc::clone(catalog),
                    quality: config.quality,
                    chosen: config.quality,
                    client,
                    cache: cache.clone(),
                    offline: offline.clone(),
                    video: config.video.clone(),
                })
            })
            .clone()
    }

    /// Opens a song to play next, at the quality adaptive streaming has
    /// for it.
    fn open(&mut self, track: LocalTrack, start_ms: u32) -> TrackStream {
        self.open_at(track, start_ms, None)
    }

    /// Opens a song; `keep` is the quality it already played at, for the
    /// same song opened again (a song changes quality only between songs).
    fn open_at(&mut self, track: LocalTrack, start_ms: u32, keep: Option<Quality>) -> TrackStream {
        if let Some((_, url)) = self
            .replacement
            .as_ref()
            .filter(|(uri, _)| *uri == track.uri)
        {
            let url = url.clone();
            return match stream_client(&self.config.proxy) {
                Ok(client) => TrackStream::open(Source::Replaced { client, url }, track, start_ms),
                Err(error) => TrackStream::failed(track, error),
            };
        }
        let wanted = keep.unwrap_or_else(|| self.next_quality());
        if wanted != self.config.quality && !video::is_video_uri(&track.uri) {
            log::info!(
                "{} is asked at {} (adaptive streaming; chosen {})",
                track.uri,
                wanted.api_name(),
                self.config.quality.api_name()
            );
        }
        match self.source() {
            Ok(mut source) => {
                if let Source::Remote { quality, .. } = &mut source {
                    *quality = wanted;
                }
                TrackStream::open(source, track, start_ms)
            }
            Err(error) => TrackStream::failed(track, error),
        }
    }

    /// The quality to ask the next song at: the chosen one, or with
    /// adaptive streaming on, what the playing song's download says the
    /// connection keeps up with.
    fn next_quality(&mut self) -> Quality {
        let Some(adaptive) = &mut self.adaptive else {
            return self.config.quality;
        };
        let Some(stream) = self
            .current
            .as_ref()
            .filter(|stream| stream.asked().is_some())
        else {
            return adaptive.quality();
        };
        // Nothing came off the network for a song played from a kept copy.
        let Some(reading) = stream.meter().take().filter(|reading| reading.bytes > 0) else {
            return adaptive.quality();
        };
        // The stream's own rate, once all of it is in, for the quality
        // TIDAL gave.
        let whole = reading
            .whole
            .zip(stream.duration_ms().filter(|ms| *ms > 0))
            .zip(stream.format().and_then(|format| format.quality))
            .map(|((bytes, ms), quality)| (quality, bytes * 8 * 1000 / u64::from(ms)));
        let sample = adaptive::Sample {
            bytes: reading.bytes,
            micros: reading.micros,
            finished: reading.finished,
            stalled: false,
            stream: whole,
        };
        let kbps = (reading.bytes * 8 * 1000)
            .checked_div(reading.micros)
            .unwrap_or_default();
        log::info!(
            "adaptive streaming: {} came at {kbps} kbit/s ({} bytes in {} ms{})",
            stream.track.uri,
            reading.bytes,
            reading.micros / 1000,
            if reading.finished { ", all of it" } else { "" }
        );
        if let Some(quality) = adaptive.observe(sample) {
            log::info!(
                "adaptive streaming: the next songs stream at {} (the connection gave {kbps} kbit/s)",
                quality.api_name()
            );
        }
        adaptive.quality()
    }

    /// Returns false to stop the engine.
    fn handle(&mut self, message: Msg) -> bool {
        match message {
            Msg::Command(command) => self.command(command),
            Msg::Context { id, spec, result } => {
                if id == self.load_id {
                    self.context_loaded(spec, result);
                }
            }
            Msg::ContextMore { id, tracks } => {
                if id == self.load_id && lock(&self.queue).extend_context(tracks) {
                    self.check_preload();
                    self.update(|_| true);
                }
            }
            Msg::Autoplay { id, result } => self.autoplay_answered(id, result),
            Msg::Tracks(result) => match result {
                Ok(tracks) => {
                    lock(&self.queue).fill(&tracks);
                    if let Some(current) = &self.current
                        && let Some(track) = tracks.iter().find(|t| t.uri == current.track.uri)
                    {
                        let track = track.clone();
                        self.update(|state| {
                            if state.track.as_ref().is_some_and(|t| t.title.is_empty()) {
                                state.track = Some(track);
                                true
                            } else {
                                false
                            }
                        });
                    }
                }
                Err(error) => log::warn!("queued track metadata: {error:#}"),
            },
            Msg::Restore(snapshot) => self.restore(&snapshot),
            Msg::Shutdown => return false,
        }
        true
    }

    fn command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::SetBlocked(blocked) => {
                lock(&self.queue).set_blocked(blocked);
                self.check_preload();
            }
            PlayerCommand::Toggle => self.toggle(),
            PlayerCommand::Next => self.next_track(),
            PlayerCommand::Previous => self.previous(),
            PlayerCommand::ClearQueue => {
                lock(&self.queue).clear_queued();
                self.check_preload();
            }
            PlayerCommand::AddToQueue(uri) => {
                lock(&self.queue).add(LocalTrack {
                    uri: uri.clone(),
                    ..LocalTrack::default()
                });
                self.check_preload();
                let catalog = Arc::clone(&self.catalog);
                let offline = self.offline.clone();
                let sender = self.sender.clone();
                spawn_worker(move || {
                    let uris = [uri];
                    let tracks = catalog
                        .tracks(&uris)
                        .or_else(|error| offline_tracks(offline.as_deref(), &uris).ok_or(error));
                    let _ = sender.send(Msg::Tracks(tracks));
                });
            }
            PlayerCommand::SkipContext { uri, occurrence } => {
                lock(&self.queue).skip_context(&uri, occurrence);
                self.check_preload();
            }
            PlayerCommand::Seek(position_ms) => self.seek(position_ms),
            PlayerCommand::Volume(volume) | PlayerCommand::VolumePreview(volume) => {
                self.volume.set(volume);
                self.update(|state| set(&mut state.volume, volume));
            }
            PlayerCommand::Shuffle(shuffle) => {
                lock(&self.queue).set_shuffle(shuffle);
                self.prefer_album &= !shuffle;
                self.check_preload();
                self.update(|state| set(&mut state.shuffle, shuffle));
            }
            PlayerCommand::Repeat(mode) => {
                lock(&self.queue).set_repeat(mode);
                self.check_preload();
                self.update(|state| set(&mut state.repeat, mode));
            }
            PlayerCommand::Load(spec) => self.load(spec),
            PlayerCommand::ReplaceAudio {
                uri,
                url,
                own,
                carrier,
            } => self.replace_audio(uri, url.map(|u| u.0), own, carrier.map(|track| *track)),
            PlayerCommand::Transfer => {}
        }
    }

    fn bump_load(&mut self) {
        self.load_id += 1;
        self.live_load.store(self.load_id, Ordering::SeqCst);
    }

    fn load(&mut self, spec: LoadSpec) {
        self.bump_load();
        let id = self.load_id;
        self.autoplay_id = None;
        self.autoplay_waiting = false;
        self.sounded = false;
        // The old track stops feeding the output while the new one resolves,
        // so it cannot end and move the queue on in the meantime.
        self.current = None;
        self.next = None;
        self.outgoing = None;
        if spec.context_uri.is_none() && spec.uris.is_empty() {
            self.audio.stopped();
            self.update(|state| set(&mut state.error, Some("Nothing to play".into())));
            return;
        }
        if let Some(repeat) = spec.repeat {
            lock(&self.queue).set_repeat(repeat);
        }
        let repeat = lock(&self.queue).repeat();
        log::info!(
            "loading {} ({} songs named, autoplay {}, play {})",
            spec.context_uri.as_deref().unwrap_or("a list of songs"),
            spec.uris.len(),
            spec.autoplay,
            spec.play
        );
        let play = spec.play;
        self.update(|state| {
            if state.playback == Playback::Stopped {
                state.playback = Playback::Loading;
            }
            state.held_paused = !play;
            state.loading = true;
            state.error = None;
            state.repeat = repeat;
            true
        });
        let catalog = Arc::clone(&self.catalog);
        let offline = self.offline.clone();
        let sender = self.sender.clone();
        let live = Arc::clone(&self.live_load);
        let queue = Arc::clone(&self.queue);
        spawn_worker(move || {
            if streams_in_pages(&spec) {
                load_in_pages(
                    catalog.as_ref(),
                    offline.as_deref(),
                    &queue,
                    &live,
                    id,
                    spec,
                    &sender,
                );
                return;
            }
            let result = resolve_or_fall_back(catalog.as_ref(), offline.as_deref(), &spec);
            let _ = sender.send(Msg::Context { id, spec, result });
        });
    }

    fn context_loaded(&mut self, spec: LoadSpec, result: Result<Vec<LocalTrack>>) {
        let tracks = match result {
            Ok(tracks) => tracks,
            Err(error) if spec.autoplay => {
                // The music that ended stays ended; there is nothing wrong
                // to tell the listener about.
                log::warn!("autoplay has nothing to follow with: {error:#}");
                self.stop();
                return;
            }
            Err(error) => {
                log::warn!("unable to load playback: {error:#}");
                self.load_failed(format!("{error:#}"));
                return;
            }
        };
        let start = if let Some(uri) = spec.offset_uri.clone() {
            Start::Uri(uri)
        } else if let Some(index) = spec.offset_index {
            Start::Index(index as usize)
        } else {
            Start::Beginning
        };
        let shuffle = spec.shuffle.unwrap_or(false);
        let context_uri = spec.context_uri.clone();
        let named = spec.offset_uri.is_some() || spec.offset_index.is_some();
        let first = {
            let mut queue = lock(&self.queue);
            // A load that finds nothing to play must leave the queue as it
            // was: the playing song keeps its own context.
            let before = (!spec.autoplay).then(|| queue.clone());
            // A station never brings back what played lately.
            let tracks = if spec.autoplay {
                queue.autoplay_fresh(tracks)
            } else {
                tracks
            };
            let first = queue.load(context_uri.clone(), tracks, start, shuffle);
            // A context of nothing but blocked songs has nothing to play
            // either, unless the listener named one.
            let first = first.filter(|first| named || !queue.blocked().hit(first));
            queue.follow_order(&spec.order);
            // A station plays on like autoplay's continuation.
            queue.set_autoplay(spec.autoplay);
            if let (None, Some(before)) = (&first, before) {
                *queue = before;
            }
            first
        };
        let Some(first) = first else {
            if spec.autoplay {
                log::info!("autoplay has nothing to follow with");
                self.stop();
            } else {
                self.load_failed("There is nothing to play here".into());
            }
            return;
        };
        self.prefer_album = !shuffle
            && context_uri
                .as_deref()
                .and_then(crate::util::uri_kind)
                .is_some_and(|kind| kind == "album");
        self.update(|state| set(&mut state.shuffle, shuffle));
        self.next = None;
        self.failures = 0;
        self.start_track(first, spec.position_ms, spec.play, false);
    }

    fn load_failed(&mut self, error: String) {
        self.audio.stopped();
        let has_track = self.current.is_some();
        if !has_track {
            // Nothing plays: the device stops asking for sound, as at any
            // other stop, rather than run on silent until the next Play.
            self.playing = false;
            self.stop_sink();
        }
        self.update(|state| {
            state.error = Some(error);
            state.loading = false;
            if !has_track {
                state.playback = Playback::Stopped;
                state.position_at = None;
            }
            true
        });
    }

    /// Makes `track` the playing one. `natural` is the end of the previous
    /// track, which keeps the output running for a gapless join.
    fn start_track(&mut self, track: LocalTrack, position_ms: u32, play: bool, natural: bool) {
        let preloaded = self
            .next
            .take()
            .filter(|next| position_ms == 0 && next.track.uri == track.uri);
        if !natural {
            self.outgoing = None;
        }
        if natural && !self.config.gapless && self.outgoing.is_none() {
            self.stop_sink();
        }
        log::info!(
            "next song {} at {position_ms} ms ({}{})",
            track.uri,
            if natural {
                "the last one ended"
            } else {
                "chosen"
            },
            if preloaded.is_some() {
                ", loaded ahead"
            } else {
                ""
            }
        );
        // A plugin's source is for the song it was given for (played again
        // with repeat, it stays, as TIDAL's player keeps its element).
        if self
            .replacement
            .as_ref()
            .is_some_and(|(uri, _)| *uri != track.uri)
        {
            self.replacement = None;
            self.replacement_own = false;
        }
        let preloaded = preloaded.filter(|_| self.replacement.is_none());
        let stream = match preloaded {
            Some(stream) => stream,
            None => self.open(track.clone(), position_ms),
        };
        self.replaced_at = self.replacement.is_some().then(Instant::now);
        self.current = Some(stream);
        self.announced = false;
        self.waiting_since = None;
        self.stalled = false;
        self.autoplay_asked = false;
        self.playing = play;
        let context_uri = lock(&self.queue).context_uri().map(str::to_string);
        self.update(|state| {
            state.context_uri = context_uri;
            state.replay_pending = state
                .track
                .as_ref()
                .is_some_and(|previous| previous.uri == track.uri);
            state.track = Some(track);
            state.track_sequence = state.track_sequence.wrapping_add(1);
            state.error = None;
            state.loading = true;
            state.format = None;
            state.position_ms = position_ms;
            state.position_at = None;
            state.held_paused = !play;
            state.replace_failed = false;
            if state.playback == Playback::Stopped {
                state.playback = Playback::Loading;
            }
            true
        });
    }

    /// Reports the current track as started, once it has sound ready.
    fn announce(&mut self) {
        self.announced = true;
        self.replaced_at = None;
        self.sounded = true;
        self.failures = 0;
        let Some(stream) = &self.current else {
            return;
        };
        let position_ms = stream.held_position().unwrap_or(stream.position_ms());
        let loudness = stream.loudness().unwrap_or_default();
        let duration = stream.duration_ms();
        let format = stream.format().cloned();
        log::info!(
            "{} has sound at {position_ms} ms of {}: {}",
            stream.track.uri,
            duration.map_or_else(|| "an unknown length".to_string(), |ms| format!("{ms} ms")),
            format.as_ref().map_or_else(
                || "unknown format".to_string(),
                |format| format!(
                    "{} {} Hz {:?} from {:?}",
                    format.codec, format.sample_rate, format.quality, format.origin
                )
            )
        );
        // Bit-perfect plays the samples as they are: no ReplayGain.
        self.factor = if self.config.normalisation && !self.config.bit_perfect {
            loudness.factor(self.prefer_album)
        } else {
            1.0
        };
        self.audio.handle_event(OutputEvent::TrackChanged);
        let playing = self.playing;
        let own_length = duration.filter(|_| self.replacement_own);
        self.update(|state| {
            state.loading = false;
            state.position_ms = position_ms;
            state.playback = if playing {
                Playback::Playing
            } else {
                Playback::Paused
            };
            state.position_at = playing.then(Instant::now);
            state.format = format;
            if let Some(track) = &mut state.track
                && track.duration_ms == 0
                && let Some(duration) = duration
            {
                track.duration_ms = duration;
            }
            if std::mem::take(&mut state.replay_pending) {
                state.seek_sequence = state.seek_sequence.wrapping_add(1);
            }
            state.source_ms = own_length;
            true
        });
    }

    /// Moves the audio along: starts a loaded track, writes one packet,
    /// handles the end, preloads, and reports the position.
    fn step(&mut self) {
        let Some(stream) = &mut self.current else {
            return;
        };
        if !stream.ready() {
            return self.waiting();
        }
        self.waiting_since = None;
        if self.stalled {
            self.stall_over();
        }
        let Some(stream) = &mut self.current else {
            return;
        };
        if !self.announced {
            match stream.status() {
                Status::Failed(error) => return self.failed(error),
                Status::Ended => return self.track_ended(),
                Status::Audio | Status::Pending => self.announce(),
            }
        }
        if !self.playing {
            return;
        }
        let Some(stream) = &mut self.current else {
            return;
        };
        match stream.poll() {
            Poll::Pcm { rate, samples, .. } => self.write(rate, samples),
            Poll::Pending => {}
            Poll::Ended => return self.track_ended(),
            Poll::Failed(error) => return self.failed(error),
        }
        self.preload();
        self.maybe_crossfade();
        if self.playing && self.last_report.elapsed() >= REPORT {
            let position_ms = self.current.as_ref().map_or(0, TrackStream::position_ms);
            {
                let mut state = lock(&self.state);
                state.position_ms = position_ms;
                state.position_at = Some(Instant::now());
            }
            self.report();
        }
    }

    /// The playing song has nothing to play yet: once that has lasted
    /// [`STALL`], it shows as loading, at the position it reached, so the
    /// interface does not count on past sound that is not playing and then
    /// jump back.
    fn waiting(&mut self) {
        // A plugin's source that does not answer soon (the listener's own
        // server is off) gives way to TIDAL's stream, rather than leave the
        // song silent while the download rides out an outage.
        if self
            .replaced_at
            .is_some_and(|at| at.elapsed() >= SOURCE_PATIENCE)
        {
            self.replaced_at = None;
            return self.failed("it did not answer".into());
        }
        if !self.announced || !self.playing || self.stalled {
            return;
        }
        let since = *self.waiting_since.get_or_insert_with(Instant::now);
        if since.elapsed() < STALL {
            return;
        }
        log::info!("the song waits for the network");
        self.stalled = true;
        // Ran dry while its download runs: the network is behind, and the
        // next song is asked lower. Nothing changes for this one.
        if let Some(adaptive) = &mut self.adaptive
            && self
                .current
                .as_ref()
                .is_some_and(|stream| stream.asked().is_some() && stream.meter().running())
            && let Some(quality) = adaptive.observe(adaptive::Sample::stall())
        {
            log::info!(
                "adaptive streaming: the next songs stream at {} (the song ran dry)",
                quality.api_name()
            );
        }
        let position_ms = self.current.as_ref().map_or(0, TrackStream::position_ms);
        self.update(|state| {
            state.loading = true;
            state.position_ms = position_ms;
            state.position_at = None;
            true
        });
    }

    /// Sound again after a stall: the song plays on from where it held.
    fn stall_over(&mut self) {
        self.stalled = false;
        let position_ms = self.current.as_ref().map_or(0, |stream| {
            stream.held_position().unwrap_or(stream.position_ms())
        });
        let playing = self.playing;
        self.update(|state| {
            state.loading = false;
            state.position_ms = position_ms;
            state.position_at = playing.then(Instant::now);
            true
        });
    }

    fn write(&mut self, rate: u32, mut samples: Vec<f64>) {
        if self.factor != 1.0 {
            for sample in &mut samples {
                *sample *= self.factor;
            }
        }
        if let Some(outgoing) = &mut self.outgoing {
            outgoing.mix(&mut samples);
            if outgoing.finished() {
                self.outgoing = None;
            }
        }
        if self.sink.is_none()
            && let Some(make) = &self.make_sink
        {
            self.sink = Some(make());
        }
        if self.sink.is_none() {
            self.sink = Some(output::build(
                &self.config,
                Arc::clone(&self.state),
                Arc::clone(&self.notify),
                &self.volume,
                Arc::clone(&self.audio),
                &self.exclusive,
            ));
        }
        let format = self
            .current
            .as_ref()
            .and_then(|stream| stream.format().cloned());
        let Some(sink) = &mut self.sink else {
            return;
        };
        if let Some(format) = format
            && self.told_format.as_ref() != Some(&format)
        {
            sink.track_format(&format);
            self.told_format = Some(format);
        }
        if !self.sink_running {
            sink.prefer_rate(rate);
            let _ = sink.start();
            self.sink_running = true;
        }
        if let Err(error) = sink.write(Packet::new(rate, samples)) {
            // The sink has told the interface why; playback pauses here.
            log::warn!("audio output failed; pausing: {error}");
            self.pause();
        }
    }

    fn stop_sink(&mut self) {
        if self.sink_running
            && let Some(sink) = &mut self.sink
        {
            let _ = sink.stop();
        }
        self.sink_running = false;
    }

    fn pause(&mut self) {
        self.playing = false;
        self.outgoing = None;
        self.stop_sink();
        let position_ms = self.current.as_ref().map_or(0, TrackStream::position_ms);
        let announced = self.announced;
        let stalled = std::mem::take(&mut self.stalled);
        self.waiting_since = None;
        self.update(|state| {
            let mut changed = set(&mut state.held_paused, true);
            if announced {
                changed |= set(&mut state.playback, Playback::Paused);
            }
            if stalled {
                changed |= set(&mut state.loading, false);
            }
            changed |= set(&mut state.position_ms, position_ms);
            changed |= set(&mut state.position_at, None);
            changed
        });
    }

    fn toggle(&mut self) {
        if self.current.is_none() && self.autoplay_waiting {
            // The songs ended and autoplay's answer is still on its way:
            // the music still reads as playing, so this is a pause. The
            // last song does not start again, and the answer, when it
            // comes, starts nothing.
            log::info!("paused while waiting for autoplay");
            return self.stop();
        }
        if self.current.is_none() {
            // Stopped at the end: Play starts the last song again.
            let track = lock(&self.queue).current().cloned();
            if let Some(track) = track {
                self.start_track(track, 0, true, false);
            }
            return;
        }
        if self.playing {
            self.pause();
            return;
        }
        self.playing = true;
        if self.announced {
            let position_ms = self.current.as_ref().map_or(0, TrackStream::position_ms);
            self.update(|state| {
                state.playback = Playback::Playing;
                state.held_paused = false;
                state.position_ms = position_ms;
                state.position_at = Some(Instant::now());
                true
            });
        } else {
            // Still loading: it plays once it has sound.
            self.update(|state| set(&mut state.held_paused, false));
        }
    }

    fn next_track(&mut self) {
        let advance = lock(&self.queue).advance(false);
        match advance {
            Advance::Track(track) => {
                let play = self.playing || self.current.is_none();
                self.start_track(track, 0, play, false);
            }
            Advance::NeedAutoplay => self.context_ended(),
        }
    }

    fn previous(&mut self) {
        let position = lock(&self.state).position_now();
        if position > RESTART_AFTER_MS || self.current.is_none() {
            return self.seek(0);
        }
        let previous = lock(&self.queue).previous();
        match previous {
            Some(track) => {
                let play = self.playing;
                self.next = None;
                self.start_track(track, 0, play, false);
            }
            None => self.seek(0),
        }
    }

    /// The playing track goes on from a plugin's source, or back to TIDAL's
    /// stream, from where it is: the same song, so nothing is reported as a
    /// new one.
    fn replace_audio(
        &mut self,
        uri: String,
        url: Option<String>,
        own: bool,
        carrier: Option<LocalTrack>,
    ) {
        log::debug!(
            "plugin source for {uri}: {} (own song {own}, {})",
            if url.is_some() { "set" } else { "cleared" },
            if self.current.is_some() {
                "a song is loaded"
            } else {
                "nothing is loaded"
            }
        );
        if self.current.is_none() {
            // Nothing is loaded (the music ended, or stopped): a song of the
            // plugin's own can still take the place of the last song, which
            // the player bar and the plugin still show. It is a carrier: no
            // new song is reported (a plugin that heard one would end its
            // own song), only the source and the playing state change.
            // A remembered song nobody has played since the start is not in
            // the queue yet: the plugin still names it, by its address.
            if !(own && url.is_some()) {
                return;
            }
            let track = lock(&self.queue)
                .current()
                .filter(|track| track.uri == uri)
                .cloned()
                .or(carrier.filter(|track| track.uri == uri))
                .unwrap_or_else(|| LocalTrack {
                    uri: uri.clone(),
                    ..LocalTrack::default()
                });
            // The song the player bar shows is the carrier, under the same
            // sequence number: it is not a new song.
            let shown = track.clone();
            self.update(|state| {
                let same = state.track.as_ref().is_some_and(|t| t.uri == shown.uri);
                if !same {
                    state.track = Some(shown);
                }
                state.error = None;
                !same
            });
            self.current = Some(TrackStream::failed(track, String::new()));
        }
        let Some(stream) = &self.current else {
            return;
        };
        if stream.track.uri != uri {
            // Said about a song that is no longer playing.
            return;
        }
        let wanted = url.map(|url| (uri, url));
        let own = own && wanted.is_some();
        if wanted == self.replacement && own == self.replacement_own {
            return;
        }
        let had = self.replacement.is_some();
        // A song of the plugin's own starts over, and plays.
        let restart = own && wanted != self.replacement;
        let was_own = self.replacement_own;
        if restart && !was_own {
            self.carrier_ms = self
                .current
                .as_ref()
                .map(|stream| stream.held_position().unwrap_or(stream.position_ms()));
        }
        self.replacement = wanted;
        self.replacement_own = own;
        if !had && self.replacement.is_none() {
            return;
        }
        log::info!(
            "{} plays from {}",
            stream.track.uri,
            if self.replacement.is_some() {
                "a plugin's source"
            } else {
                "TIDAL's stream again"
            }
        );
        self.update(|state| set(&mut state.replace_failed, false));
        let back = (was_own && !own).then(|| self.carrier_ms.take()).flatten();
        self.reopen(if restart { Some(0) } else { back });
        if restart && !self.playing {
            self.toggle();
        }
        if was_own && !own && self.playing {
            // The plugin let its own song go: the TIDAL song waits, where it was.
            self.pause();
        }
    }

    /// Opens the playing track again where it is, from wherever
    /// [`Self::open`] now finds it.
    fn reopen(&mut self, start_ms: Option<u32>) {
        let Some(stream) = self.current.take() else {
            return;
        };
        let position_ms =
            start_ms.unwrap_or_else(|| stream.held_position().unwrap_or(stream.position_ms()));
        let track = stream.track.clone();
        // At the quality it was playing at: adaptive streaming changes it
        // only between songs.
        let keep = stream.asked();
        drop(stream);
        self.current = Some(self.open_at(track, position_ms, keep));
        self.replaced_at = self.replacement.is_some().then(Instant::now);
        self.announced = false;
        self.waiting_since = None;
        self.outgoing = None;
        self.audio.handle_event(OutputEvent::Seeked);
    }

    fn seek(&mut self, position_ms: u32) {
        let Some(stream) = &mut self.current else {
            self.audio.stopped();
            return;
        };
        let limit = stream
            .duration_ms()
            .or(Some(stream.track.duration_ms).filter(|ms| *ms > 0))
            .unwrap_or(u32::MAX);
        let position_ms = position_ms.min(limit.saturating_sub(1));
        stream.seek(position_ms);
        self.waiting_since = None;
        self.outgoing = None;
        self.audio.handle_event(OutputEvent::Seeked);
        // A song still waiting for the network holds its new place too.
        let playing = self.playing && self.announced && !self.stalled;
        self.update(|state| {
            state.position_ms = position_ms;
            state.position_at = playing.then(Instant::now);
            state.seek_sequence = state.seek_sequence.wrapping_add(1);
            true
        });
    }

    fn track_ended(&mut self) {
        if let Some(stream) = &self.current {
            log::info!("{} ended at {} ms", stream.track.uri, stream.position_ms());
            if self.replacement_own
                && self
                    .replacement
                    .as_ref()
                    .is_some_and(|(uri, _)| *uri == stream.track.uri)
            {
                // A song of the plugin's own: its player decides what is next.
                self.pause();
                self.update(|state| {
                    state.replace_ended = state.replace_ended.wrapping_add(1);
                    true
                });
                return;
            }
        }
        let advance = lock(&self.queue).advance(true);
        match advance {
            Advance::Track(track) => self.start_track(track, 0, true, true),
            Advance::NeedAutoplay => self.context_ended(),
        }
    }

    /// The current song cannot be played: it is skipped, and playback
    /// carries on with the next one, past the end of the context into
    /// autoplay too. Only a run of failures in a row stops it.
    fn failed(&mut self, error: String) {
        if let Some(stream) = &self.current
            && self
                .replacement
                .as_ref()
                .is_some_and(|(uri, _)| *uri == stream.track.uri)
        {
            // The plugin's source failed (its server is down, the file is
            // gone): the song goes on from TIDAL's stream.
            log::warn!(
                "the plugin's source for {} cannot be played, playing TIDAL's stream: {error}",
                stream.track.uri
            );
            self.replacement = None;
            self.replacement_own = false;
            self.update(|state| set(&mut state.replace_failed, true));
            self.reopen(None);
            return;
        }
        let skipped = self.current.as_ref().map(|stream| stream.track.clone());
        let uri = skipped.as_ref().map_or("", |track| track.uri.as_str());
        log::warn!("{uri} cannot be played, skipping it: {error}");
        self.failures += 1;
        let went_on = if self.failures >= MAX_FAILURES {
            log::warn!("{MAX_FAILURES} songs in a row could not be played; stopping");
            self.stop();
            false
        } else {
            let advance = lock(&self.queue).advance(false);
            match advance {
                Advance::Track(track) => {
                    let play = self.playing;
                    self.start_track(track, 0, play, false);
                    true
                }
                Advance::NeedAutoplay if self.playing && self.sounded => {
                    self.context_ended();
                    self.autoplay_waiting
                }
                Advance::NeedAutoplay => {
                    self.stop();
                    false
                }
            }
        };
        let message = skipped_message(skipped.as_ref(), &error, went_on);
        // Said after the skip, which clears the previous track's error.
        self.update(|state| set(&mut state.error, Some(message)));
    }

    /// The context has no more songs: autoplay continues it, if allowed.
    fn context_ended(&mut self) {
        if !self.config.autoplay {
            log::info!("the end of the songs, and autoplay is off; stopping");
            return self.stop();
        }
        log::info!("the end of the songs; continuing with autoplay");
        self.autoplay_waiting = true;
        self.current = None;
        self.update(|state| set(&mut state.loading, true));
        if self.autoplay_id.is_none() {
            self.ask_autoplay(0);
        }
    }

    /// Asks for the station that follows the song `back` songs before the
    /// last one reached.
    fn ask_autoplay(&mut self, back: usize) {
        let Some(seed) = lock(&self.queue)
            .seed_back(back)
            // TIDAL's radio starts from songs, not videos.
            .filter(|track| !track.is_video())
            .and_then(|track| crate::util::uri_id(&track.uri).map(str::to_string))
        else {
            if self.autoplay_waiting {
                log::info!("autoplay has no song to start from");
                self.stop();
            }
            return;
        };
        self.autoplay_back = back;
        self.bump_load();
        let id = self.load_id;
        self.autoplay_id = Some(id);
        let catalog = Arc::clone(&self.catalog);
        let sender = self.sender.clone();
        spawn_worker(move || {
            let result = catalog.autoplay(&seed);
            let _ = sender.send(Msg::Autoplay { id, result });
        });
    }

    fn autoplay_answered(&mut self, id: u64, result: Result<Vec<LocalTrack>>) {
        if self.autoplay_id != Some(id) {
            return;
        }
        self.autoplay_id = None;
        let back = self.autoplay_back;
        let result = result.map(|tracks| {
            let mut queue = lock(&self.queue);
            // The songs are the radio of the song autoplay was seeded with.
            let station = queue
                .seed_back(back)
                .and_then(|seed| crate::util::station_uri(&seed.uri));
            queue.continue_with(tracks, station)
        });
        match result {
            Ok(true) => {
                log::info!("autoplay continues");
                self.prefer_album = false;
            }
            outcome => {
                match outcome {
                    Err(error) => log::warn!("autoplay: {error:#}"),
                    Ok(_) => log::info!("autoplay found nothing to follow that song with"),
                }
                // Another song of the context may have a station.
                let back = self.autoplay_back + 1;
                if back < AUTOPLAY_SEEDS && lock(&self.queue).seed_back(back).is_some() {
                    self.ask_autoplay(back);
                } else if self.autoplay_waiting {
                    self.autoplay_waiting = false;
                    self.stop();
                }
                return;
            }
        }
        if std::mem::take(&mut self.autoplay_waiting) {
            let advance = lock(&self.queue).advance(true);
            if let Advance::Track(track) = advance {
                self.start_track(track, 0, true, false);
            }
        }
    }

    /// Nothing more to play.
    fn stop(&mut self) {
        log::info!("playback stopped");
        self.current = None;
        self.next = None;
        self.outgoing = None;
        self.playing = false;
        self.autoplay_waiting = false;
        self.waiting_since = None;
        self.stalled = false;
        self.stop_sink();
        self.audio.stopped();
        self.audio.handle_event(OutputEvent::Stopped);
        self.update(|state| {
            let mut changed = set(&mut state.playback, Playback::Stopped);
            changed |= set(&mut state.loading, false);
            changed |= set(&mut state.format, None);
            changed |= set(&mut state.position_ms, 0);
            changed |= set(&mut state.position_at, None);
            changed
        });
    }

    /// Opens the next track once the current one is near its end.
    fn preload(&mut self) {
        if self.next.is_some() {
            return;
        }
        let Some(stream) = &self.current else {
            return;
        };
        let duration = stream
            .duration_ms()
            .or(Some(stream.track.duration_ms).filter(|ms| *ms > 0));
        let Some(duration) = duration else {
            return;
        };
        let remaining = duration.saturating_sub(stream.position_ms());
        let ahead = PRELOAD.max(Duration::from_millis(
            u64::from(self.crossfade_ms()) + 10_000,
        ));
        if u128::from(remaining) > ahead.as_millis() {
            return;
        }
        let peek = lock(&self.queue).peek();
        match peek {
            Advance::Track(track) => {
                log::info!("loading {} ahead, {remaining} ms before the end", track.uri);
                self.next = Some(self.open(track, 0));
            }
            Advance::NeedAutoplay => {
                if self.config.autoplay && self.autoplay_id.is_none() && !self.autoplay_asked {
                    self.autoplay_asked = true;
                    self.ask_autoplay(0);
                }
            }
        }
    }

    /// No crossfade while bit-perfect: mixing two tracks alters both.
    fn crossfade_ms(&self) -> u32 {
        if self.config.bit_perfect {
            return 0;
        }
        self.config.crossfade_ms.min(MAX_CROSSFADE_MS)
    }

    /// Starts the next track under the end of this one, when a crossfade
    /// is set, the next track is decoded and ready at the same rate, and
    /// this one is within the crossfade of its end.
    fn maybe_crossfade(&mut self) {
        let crossfade_ms = self.crossfade_ms();
        if crossfade_ms == 0 || self.outgoing.is_some() || !self.playing {
            return;
        }
        let (Some(current), Some(next)) = (&mut self.current, &mut self.next) else {
            return;
        };
        let Some(duration) = current
            .duration_ms()
            .or(Some(current.track.duration_ms).filter(|ms| *ms > 0))
        else {
            return;
        };
        let remaining = duration.saturating_sub(current.position_ms());
        if remaining > crossfade_ms || remaining < 50 || !next.ready() {
            return;
        }
        let (Some(from), Some(to)) = (current.format(), next.format()) else {
            return;
        };
        if from.sample_rate != to.sample_rate || from.sample_rate == 0 {
            // Different rates cannot be mixed: a gapless join instead.
            return;
        }
        let rate = from.sample_rate;
        let advance = lock(&self.queue).advance(true);
        let Advance::Track(track) = advance else {
            return;
        };
        let Some(old) = self.current.take() else {
            return;
        };
        let frames = (u64::from(remaining) * u64::from(rate) / 1000) as usize;
        log::info!("crossfading into {} over {remaining} ms", track.uri);
        // Set first, so a join without gapless keeps the output running.
        self.outgoing = Some(Crossfade::new(old, self.factor, rate, frames));
        // The incoming track is the preloaded one, so it starts at once.
        self.start_track(track, 0, true, true);
    }

    /// Drops a preloaded track the queue no longer plays next.
    fn check_preload(&mut self) {
        let Some(next) = &self.next else {
            return;
        };
        let still = matches!(lock(&self.queue).peek(), Advance::Track(track) if track.uri == next.track.uri);
        if !still {
            self.next = None;
        }
    }

    fn restore(&mut self, snapshot: &Snapshot) {
        let (track, shuffle, repeat) = {
            let mut queue = lock(&self.queue);
            let blocked = queue.blocked().clone();
            *queue = snapshot.queue.clone();
            queue.set_blocked(blocked);
            (queue.current().cloned(), queue.shuffle(), queue.repeat())
        };
        self.bump_load();
        self.update(|state| {
            let mut changed = set(&mut state.shuffle, shuffle);
            changed |= set(&mut state.repeat, repeat);
            changed
        });
        if let Some(track) = track {
            self.next = None;
            self.start_track(track, snapshot.position_ms, snapshot.playing, false);
        }
    }
}

/// What the listener is told about a song that was skipped: which one, in
/// a few words. The cause, in TIDAL's words, goes to the log.
fn skipped_message(track: Option<&LocalTrack>, error: &str, went_on: bool) -> String {
    let name = track
        .map(|track| track.title.trim())
        .filter(|title| !title.is_empty())
        .map_or_else(|| "A song".to_string(), |title| format!("“{title}”"));
    let what = if error.starts_with(video::UNPLAYABLE) {
        // TIDAL's own words for a video it will not show here.
        video::UNPLAYABLE.to_string()
    } else if error.contains("isn't available") {
        format!("{name} isn't available on TIDAL")
    } else {
        format!("{name} couldn't be played")
    };
    if went_on {
        format!("{what}, so it was skipped")
    } else {
        what
    }
}

/// The client streams are fetched with: a read that waits longer than
/// [`READ_TIMEOUT`] for bytes fails, and the download reconnects.
fn stream_client(
    proxy: &crate::settings::ProxyConfig,
) -> std::result::Result<reqwest::blocking::Client, String> {
    crate::http::blocking_builder(proxy).and_then(|builder| {
        builder
            .connect_timeout(Duration::from_secs(10))
            .timeout(READ_TIMEOUT)
            .build()
            .map_err(|error| error.to_string())
    })
}

/// The kept details of `uris` that are kept offline, in order; `None` if
/// none is.
fn offline_tracks(offline: Option<&OfflineStore>, uris: &[String]) -> Option<Vec<LocalTrack>> {
    let offline = offline?;
    let tracks: Vec<LocalTrack> = uris.iter().filter_map(|uri| offline.track(uri)).collect();
    (!tracks.is_empty()).then_some(tracks)
}

/// Every track kept offline, as a context to play.
pub const OFFLINE_CONTEXT: &str = "tidal:offline";

/// The tracks a load plays: its context's, its URIs', or the autoplay
/// station that follows its context. Without TIDAL, tracks kept offline
/// still play: their own details stand in for the catalogue's.
fn resolve(
    catalog: &dyn Catalog,
    offline: Option<&OfflineStore>,
    spec: &LoadSpec,
) -> Result<Vec<LocalTrack>> {
    if let Some(offline) = offline
        && spec.context_uri.as_deref() == Some(OFFLINE_CONTEXT)
    {
        return Ok(offline.list().into_iter().map(|kept| kept.track).collect());
    }
    let error = match resolve_online(catalog, spec) {
        Ok(tracks) => return Ok(tracks),
        Err(error) => error,
    };
    let kept = match &spec.context_uri {
        _ if spec.autoplay => None,
        Some(context) if crate::util::uri_kind(context) == Some("track") => {
            offline_tracks(offline, std::slice::from_ref(context))
        }
        Some(context) => offline
            .map(|offline| offline.context_tracks(context))
            .filter(|tracks| !tracks.is_empty()),
        None => offline_tracks(offline, &spec.uris),
    };
    match kept {
        Some(tracks) => {
            log::info!("TIDAL is out of reach ({error:#}); playing what is kept offline");
            Ok(tracks)
        }
        None => Err(error),
    }
}

/// [`resolve`], except that a context which cannot be read, or which does
/// not hold the song the load starts at, is replaced by the load's fallback
/// songs when it has any: a restored session continues its saved queue
/// rather than whatever else would play.
fn resolve_or_fall_back(
    catalog: &dyn Catalog,
    offline: Option<&OfflineStore>,
    spec: &LoadSpec,
) -> Result<Vec<LocalTrack>> {
    let result = resolve(catalog, offline, spec);
    if spec.fallback.is_empty() || spec.autoplay {
        return result;
    }
    let holds_start = match (&result, &spec.offset_uri) {
        (Ok(tracks), Some(start)) => tracks.iter().any(|track| track.uri == *start),
        (Ok(tracks), None) => !tracks.is_empty(),
        (Err(_), _) => false,
    };
    if holds_start {
        return result;
    }
    let kept = catalog
        .tracks(&spec.fallback)
        .ok()
        .filter(|tracks| !tracks.is_empty())
        .or_else(|| offline_tracks(offline, &spec.fallback));
    match kept {
        Some(tracks) => {
            log::info!("continuing the saved queue instead of the context");
            Ok(tracks)
        }
        None => result,
    }
}

fn resolve_online(catalog: &dyn Catalog, spec: &LoadSpec) -> Result<Vec<LocalTrack>> {
    if spec.autoplay {
        let context = spec
            .context_uri
            .as_deref()
            .ok_or_else(|| anyhow!("autoplay needs a context"))?;
        let seed = if crate::util::uri_kind(context) == Some("track") {
            crate::util::uri_id(context).map(str::to_string)
        } else {
            catalog
                .context_tracks(context)?
                .iter()
                .rev()
                // A playlist's video is no seed for a song radio.
                .find(|track| !crate::tidal_engine::video::is_video_uri(&track.uri))
                .and_then(|track| crate::util::uri_id(&track.uri).map(str::to_string))
        };
        let seed = seed.ok_or_else(|| anyhow!("nothing to base autoplay on"))?;
        return catalog.autoplay(&seed);
    }
    match &spec.context_uri {
        Some(context) if crate::util::uri_kind(context) == Some("track") => {
            catalog.tracks(std::slice::from_ref(context))
        }
        Some(context) => catalog.context_tracks(context),
        None => catalog.tracks(&spec.uris),
    }
}

/// How many songs of a context are kept loaded ahead of the playing one
/// while the rest is still being read.
const READ_AHEAD: usize = 200;

/// How many pages a shuffle reads at most to find a first song it has not
/// played lately.
const SHUFFLE_PAGES_TO_WAIT: usize = 4;

/// A load that can start on the first page of its context: a plain play
/// of a context. A saved session, which continues its own queue, and
/// autoplay, which reads the context's last song, need all of it.
fn streams_in_pages(spec: &LoadSpec) -> bool {
    !spec.autoplay
        && spec.fallback.is_empty()
        && spec.order.is_empty()
        && spec.context_uri.as_deref().is_some_and(|context| {
            context != OFFLINE_CONTEXT
                && !matches!(crate::util::uri_kind(context), Some("track" | "video"))
        })
}

/// Reads the context page by page. The load starts as soon as a page holds
/// what it starts at (any song, when it starts anywhere); later pages
/// follow as [`Msg::ContextMore`]. A context that cannot be read at all
/// goes the whole way round [`resolve_or_fall_back`], for what is kept
/// offline.
fn load_in_pages(
    catalog: &dyn Catalog,
    offline: Option<&OfflineStore>,
    queue: &Mutex<Queue>,
    live: &AtomicU64,
    id: u64,
    spec: LoadSpec,
    sender: &Sender<Msg>,
) {
    let Some(context) = spec.context_uri.clone() else {
        return;
    };
    let mut held: Vec<LocalTrack> = Vec::new();
    let mut started = false;
    // A shuffle from nowhere in particular waits (for a few pages at most)
    // for a song not played lately, so it does not open on the same few
    // top songs every time.
    let lately =
        (spec.shuffle == Some(true) && spec.offset_uri.is_none() && spec.offset_index.is_none())
            .then(|| lock(queue).recent_in(&context));
    let mut pages = 0;
    let outcome = catalog.context_pages(&context, &mut |page| {
        if live.load(Ordering::SeqCst) != id {
            return false;
        }
        if started {
            // A catalogue as long as an artist's is read as the queue nears
            // its end, not all at the start.
            while lock(queue).ahead() > READ_AHEAD {
                if live.load(Ordering::SeqCst) != id {
                    return false;
                }
                thread::sleep(Duration::from_millis(500));
            }
            if live.load(Ordering::SeqCst) != id {
                return false;
            }
            return sender.send(Msg::ContextMore { id, tracks: page }).is_ok();
        }
        held.extend(page);
        pages += 1;
        let ready = !held.is_empty()
            && lately.as_ref().is_none_or(|lately| {
                pages >= SHUFFLE_PAGES_TO_WAIT || held.iter().any(|track| !lately.hit(track))
            })
            && spec
                .offset_uri
                .as_ref()
                .is_none_or(|uri| held.iter().any(|track| track.uri == *uri))
            && spec
                .offset_index
                .is_none_or(|index| (index as usize) < held.len());
        if ready {
            started = true;
            let result = Ok(std::mem::take(&mut held));
            return sender
                .send(Msg::Context {
                    id,
                    spec: spec.clone(),
                    result,
                })
                .is_ok();
        }
        true
    });
    if started || live.load(Ordering::SeqCst) != id {
        if let (true, Err(error)) = (started, outcome) {
            log::warn!("the rest of {context} could not be read: {error:#}");
        }
        return;
    }
    // Every page is in without the start in it, or the first could not be
    // read: what the load did before pages, with the same fallbacks.
    let result = match outcome {
        Ok(()) if !held.is_empty() => Ok(held),
        _ => resolve_or_fall_back(catalog, offline, &spec),
    };
    let _ = sender.send(Msg::Context { id, spec, result });
}

fn spawn_worker(work: impl FnOnce() + Send + 'static) {
    if let Err(error) = thread::Builder::new()
        .name("tidal-catalog".into())
        .spawn(work)
    {
        log::error!("unable to start a catalogue request: {error}");
    }
}

#[cfg(test)]
mod tests;
