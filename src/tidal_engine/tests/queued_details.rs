//! Songs queued by their address alone: their details are looked up once,
//! kept when the app clears and re-adds Playing next to reorder it, and
//! asked once more when TIDAL gave none, so no slot stays bare.

use std::sync::atomic::AtomicBool;

use super::*;

/// The fake catalogue, whose lookups of single songs can be slowed down,
/// and leave every song out the first `fail` times, as the catalogue does
/// for a lookup TIDAL turns away (it is too busy, say).
struct Lookups {
    inner: Arc<FakeCatalog>,
    calls: AtomicUsize,
    slow: AtomicBool,
    fail: usize,
}

impl Lookups {
    fn new(fail: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: FakeCatalog::new(),
            calls: AtomicUsize::new(0),
            slow: AtomicBool::new(false),
            fail,
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Catalog for Lookups {
    fn context_tracks(&self, context_uri: &str) -> Result<Vec<LocalTrack>> {
        self.inner.context_tracks(context_uri)
    }

    fn tracks(&self, uris: &[String]) -> Result<Vec<LocalTrack>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.slow.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(1500));
        }
        if call < self.fail {
            return Ok(Vec::new());
        }
        self.inner.tracks(uris)
    }

    fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        self.inner.stream(track_id, quality)
    }

    fn autoplay(&self, seed_track_id: &str) -> Result<Vec<LocalTrack>> {
        self.inner.autoplay(seed_track_id)
    }
}

fn add(engine: &Engine, n: u32) {
    engine
        .command(PlayerCommand::AddToQueue(format!("tidal:track:{n}")))
        .unwrap();
}

/// Waits until every command sent before has been carried out: the
/// engine's thread takes commands in order, and reports a new repeat mode.
fn settle(engine: &Engine, states: &Receiver<LocalState>) {
    engine
        .command(PlayerCommand::Repeat(RepeatMode::Track))
        .unwrap();
    until(states, |state| state.repeat == RepeatMode::Track);
}

/// Waits until the queued songs are `want`, details and all.
fn queued_become(engine: &Engine, want: &[LocalTrack]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.queue(10).queued != want {
        assert!(
            Instant::now() < deadline,
            "the queued songs never became {want:?}: {:?}",
            engine.queue(10).queued
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// The app reorders Playing next by clearing it and adding its songs again
/// in the new order. Each song keeps the details it had, at once, and none
/// is looked up again: the slots used to be bare until a new lookup per
/// song answered (and stayed bare when one of those failed).
#[test]
fn songs_queued_again_after_a_clear_keep_their_details_without_a_lookup() {
    let catalog = Lookups::new(0);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    add(&engine, 8);
    add(&engine, 9);
    queued_become(&engine, &[track(8), track(9)]);
    assert_eq!(catalog.calls(), 2);

    // Any lookup from here on would take a while to answer.
    catalog.slow.store(true, Ordering::SeqCst);
    engine.command(PlayerCommand::ClearQueue).unwrap();
    add(&engine, 9);
    add(&engine, 8);
    settle(&engine, &states);
    assert_eq!(
        engine.queue(10).queued,
        [track(9), track(8)],
        "the songs added again have their details at once"
    );
    thread::sleep(Duration::from_millis(200));
    assert_eq!(catalog.calls(), 2, "nothing is looked up again");
}

/// A song queued again while its lookup is still on its way is not looked
/// up a second time: the one answer fills both.
#[test]
fn a_song_queued_again_while_it_is_looked_up_is_looked_up_once() {
    let catalog = Lookups::new(0);
    catalog.slow.store(true, Ordering::SeqCst);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    add(&engine, 9);
    engine.command(PlayerCommand::ClearQueue).unwrap();
    add(&engine, 9);
    queued_become(&engine, &[track(9)]);
    assert_eq!(catalog.calls(), 1);
}

/// TIDAL gave nothing for a queued song's lookup (the real catalogue leaves
/// out a song whose lookup failed): it is asked once more a moment later,
/// and its slot fills then, instead of staying bare until it plays.
#[test]
fn a_queued_song_tidal_gave_no_details_for_is_asked_once_more() {
    let catalog = Lookups::new(1);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    add(&engine, 9);
    queued_become(&engine, &[track(9)]);
    assert_eq!(catalog.calls(), 2);
}

/// Once more is all: a song TIDAL never describes is not asked over and
/// over.
#[test]
fn a_queued_song_is_asked_for_twice_at_most() {
    let catalog = Lookups::new(usize::MAX);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    add(&engine, 9);
    let deadline = Instant::now() + Duration::from_secs(5);
    while catalog.calls() < 2 {
        assert!(Instant::now() < deadline, "the song was not asked again");
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(600));
    assert_eq!(catalog.calls(), 2);
    assert!(engine.queue(10).queued[0].title.is_empty());
}
