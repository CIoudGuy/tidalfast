//! A context read page by page: playback starts on the first page, and the
//! later pages join the queue behind it.

use std::sync::Condvar;

use super::*;

const PAGES: u32 = 5;
const PER_PAGE: u32 = 3;
const ARTIST: &str = "tidal:artist:1";

/// An artist whose top tracks (10 to 24) come in five pages of three. A
/// page is only delivered once the test has allowed it, and each takes
/// `latency` to arrive.
struct PagedCatalog {
    inner: Arc<FakeCatalog>,
    allowed: Mutex<u32>,
    open: Condvar,
    latency: Duration,
    /// Whether the catalogue hands pages out as they arrive.
    progressive: bool,
}

impl PagedCatalog {
    fn new(allowed: u32, latency: Duration, progressive: bool) -> Arc<Self> {
        Arc::new(Self {
            inner: FakeCatalog::new(),
            allowed: Mutex::new(allowed),
            open: Condvar::new(),
            latency,
            progressive,
        })
    }

    fn allow(&self, pages: u32) {
        *lock(&self.allowed) = pages;
        self.open.notify_all();
    }

    fn page(page: u32) -> Vec<LocalTrack> {
        (0..PER_PAGE)
            .map(|n| track(10 + page * PER_PAGE + n))
            .collect()
    }

    fn wait_for(&self, page: u32) {
        let mut allowed = lock(&self.allowed);
        while *allowed <= page {
            allowed = self
                .open
                .wait(allowed)
                .unwrap_or_else(PoisonError::into_inner);
        }
        drop(allowed);
        thread::sleep(self.latency);
    }
}

impl Catalog for PagedCatalog {
    fn context_tracks(&self, _: &str) -> Result<Vec<LocalTrack>> {
        let mut all = Vec::new();
        for page in 0..PAGES {
            self.wait_for(page);
            all.extend(Self::page(page));
        }
        Ok(all)
    }

    fn context_pages(
        &self,
        context_uri: &str,
        deliver: &mut (dyn FnMut(Vec<LocalTrack>) -> bool + Send),
    ) -> Result<()> {
        if !self.progressive {
            deliver(self.context_tracks(context_uri)?);
            return Ok(());
        }
        for page in 0..PAGES {
            self.wait_for(page);
            if !deliver(Self::page(page)) {
                break;
            }
        }
        Ok(())
    }

    fn tracks(&self, uris: &[String]) -> Result<Vec<LocalTrack>> {
        self.inner.tracks(uris)
    }

    fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        self.inner.stream(track_id, quality)
    }

    fn autoplay(&self, seed: &str) -> Result<Vec<LocalTrack>> {
        self.inner.autoplay(seed)
    }
}

fn load_artist(engine: &Engine, shuffle: bool, offset: Option<&str>) {
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(ARTIST.into()),
            offset_uri: offset.map(str::to_string),
            shuffle: Some(shuffle),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
}

fn uris_of(engine: &Engine) -> Vec<String> {
    let view = engine.queue(1000);
    view.current
        .into_iter()
        .chain(view.next_up)
        .map(|track| track.uri)
        .collect()
}

fn wait_for_queue(engine: &Engine, len: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let uris = uris_of(engine);
        if uris.len() == len {
            return uris;
        }
        assert!(Instant::now() < deadline, "the queue stopped at {uris:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn shuffle_starts_on_the_first_page_and_later_pages_join_without_repeats() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, true, None);

    // Paused on a song of page one while four pages are still to come.
    let started = until(&states, |state| {
        state.playback == Playback::Paused && !state.loading && state.track.is_some()
    });
    let first = started.track.unwrap().uri;
    assert!(
        (10..13).any(|n| first == format!("tidal:track:{n}")),
        "{first}"
    );
    let before = uris_of(&engine);
    assert_eq!(before.len(), 3);

    catalog.allow(PAGES);
    let all = wait_for_queue(&engine, (PAGES * PER_PAGE) as usize);
    // The playing song keeps its place.
    assert_eq!(all[0], before[0]);
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        (PAGES * PER_PAGE) as usize,
        "no repeats: {all:?}"
    );
    // It stays on the artist's own songs.
    let artists_songs: Vec<String> = (10..25).map(|n| format!("tidal:track:{n}")).collect();
    assert!(sorted.iter().all(|uri| artists_songs.contains(uri)));
}

#[test]
fn play_starts_with_the_first_song_and_later_pages_follow_in_order() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, false, None);
    until(&states, paused_on("tidal:track:10"));

    catalog.allow(PAGES);
    let all = wait_for_queue(&engine, (PAGES * PER_PAGE) as usize);
    let expected: Vec<String> = (10..25).map(|n| format!("tidal:track:{n}")).collect();
    assert_eq!(all, expected);
}

#[test]
fn a_start_in_a_later_page_waits_for_that_page() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, false, Some("tidal:track:17"));
    thread::sleep(Duration::from_millis(200));
    assert!(engine.queue(10).current.is_none(), "nothing starts early");
    catalog.allow(3);
    until(&states, paused_on("tidal:track:17"));
    catalog.allow(PAGES);
    assert_eq!(wait_for_queue(&engine, 8).len(), 8);
}

/// Time from the load to the first song, with 300 ms per page: the whole
/// list first (before) and the first page first (after).
#[test]
fn first_song_time_with_slow_pages() {
    let mut took = Vec::new();
    for progressive in [false, true] {
        let catalog = PagedCatalog::new(PAGES, Duration::from_millis(300), progressive);
        let (engine, states) = engine_with(config(), catalog as Arc<dyn Catalog>);
        until(&states, |state| state.connected);
        let started = Instant::now();
        load_artist(&engine, true, None);
        until(&states, |state| {
            state.playback == Playback::Paused && !state.loading && state.track.is_some()
        });
        took.push(started.elapsed());
    }
    eprintln!(
        "first song: whole list first {:?}, first page first {:?}",
        took[0], took[1]
    );
    assert!(took[0] >= Duration::from_millis(1500));
    assert!(took[1] < Duration::from_millis(1000));
}
