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

/// Loads the artist, from the row `offset` names when given, with that
/// row's details as the app sends them.
fn load_artist(engine: &Engine, shuffle: bool, offset: Option<&str>) {
    let row = offset
        .and_then(crate::util::uri_id)
        .and_then(|id| id.parse().ok())
        .map(|n| Box::new(track(n)));
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(ARTIST.into()),
            offset_uri: offset.map(str::to_string),
            offset_track: row,
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

/// What the catalogue was asked for `what` ("tracks", "autoplay"...).
fn asked(catalog: &PagedCatalog, what: &str) -> usize {
    lock(&catalog.inner.asked).get(what).copied().unwrap_or(0)
}

/// A double-clicked row far down a long playlist plays at once: its song
/// starts while no page before it has come (a user waited 2 to 3 s in
/// silence for the six pages before row 553). The pages then put the
/// context around it: the songs after it follow in the playlist's order,
/// and the songs before it are behind it (Previous goes to the row above).
#[test]
fn a_row_in_a_later_page_plays_at_once_and_the_rest_follows_in_order() {
    let catalog = PagedCatalog::new(0, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, false, Some("tidal:track:17"));
    // Not one page is in.
    let started = until(&states, paused_on("tidal:track:17"));
    assert_eq!(started.track.unwrap().title, "Song 17");
    assert!(!started.shuffle);
    let view = engine.queue(10);
    assert_eq!(
        view.current.map(|track| track.uri).as_deref(),
        Some("tidal:track:17")
    );
    assert!(view.next_up.is_empty(), "nothing after it is known yet");
    assert_eq!(view.context_uri.as_deref(), Some(ARTIST), "Playing from");

    catalog.allow(PAGES);
    let all = wait_for_queue(&engine, 8);
    let expected: Vec<String> = (17..25).map(|n| format!("tidal:track:{n}")).collect();
    assert_eq!(all, expected, "the songs after it follow in order");
    engine.command(PlayerCommand::Previous).unwrap();
    until(&states, paused_on("tidal:track:16"));
}

/// The same with Shuffle on: the row's song plays at once, and once the
/// pages are in, every other song follows shuffled, each once.
#[test]
fn a_row_in_a_later_page_plays_at_once_and_the_rest_follows_shuffled() {
    let catalog = PagedCatalog::new(0, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, true, Some("tidal:track:17"));
    let started = until(&states, paused_on("tidal:track:17"));
    assert!(started.shuffle);
    assert!(engine.queue(10).next_up.is_empty());

    catalog.allow(PAGES);
    let all = wait_for_queue(&engine, (PAGES * PER_PAGE) as usize);
    assert_eq!(all[0], "tidal:track:17");
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "no repeats: {all:?}");
    let artists_songs: Vec<String> = (10..25).map(|n| format!("tidal:track:{n}")).collect();
    assert_eq!(sorted, artists_songs, "every song of the artist");
    let in_order: Vec<String> = artists_songs
        .into_iter()
        .filter(|uri| uri != "tidal:track:17")
        .collect();
    assert_ne!(all[1..], in_order[..], "the rest is shuffled");
}

/// The row's own details start it: the catalogue is not asked for the
/// song, and the player bar shows the row's title from the first report.
#[test]
fn the_rows_own_details_start_it_without_asking_the_catalogue() {
    let catalog = PagedCatalog::new(0, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(ARTIST.into()),
            offset_uri: Some("tidal:track:20".into()),
            offset_track: Some(Box::new(LocalTrack {
                title: "As the row shows it".into(),
                ..track(20)
            })),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    let first = until(&states, |state| state.track.is_some());
    assert_eq!(first.track.unwrap().title, "As the row shows it");
    until(&states, paused_on("tidal:track:20"));
    assert_eq!(asked(&catalog, "tracks"), 0);
    catalog.allow(PAGES);
    assert_eq!(wait_for_queue(&engine, 5).len(), 5);
}

/// A load that names its song without the row's details (a plugin's)
/// waits for the page that holds it, as before.
#[test]
fn a_named_song_without_the_rows_details_waits_for_its_page() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(ARTIST.into()),
            offset_uri: Some("tidal:track:17".into()),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(engine.queue(10).current.is_none(), "nothing starts early");
    catalog.allow(3);
    until(&states, paused_on("tidal:track:17"));
    catalog.allow(PAGES);
    assert_eq!(wait_for_queue(&engine, 8).len(), 8);
}

/// Next before the pages are in waits for them, then plays the song after
/// the row; it does not end the context into autoplay.
#[test]
fn next_before_the_pages_come_plays_the_song_after_the_row() {
    let catalog = PagedCatalog::new(0, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, false, Some("tidal:track:17"));
    until(&states, paused_on("tidal:track:17"));
    engine.command(PlayerCommand::Next).unwrap();
    until(&states, |state| state.loading);
    thread::sleep(Duration::from_millis(100));
    catalog.allow(PAGES);
    until(&states, paused_on("tidal:track:18"));
    assert_eq!(asked(&catalog, "autoplay"), 0);
    assert_eq!(wait_for_queue(&engine, 7)[0], "tidal:track:18");
}

/// Shuffle switched off while the context is still read up to the named
/// song is how the songs after it play: the named song first, then the
/// rest in order, and the engine says Shuffle is off. It started shuffled
/// while the player bar showed Shuffle off (a report: "shuffle off but it
/// played Escape Plan").
#[test]
fn shuffle_switched_off_while_a_later_page_loads_plays_in_order() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, true, Some("tidal:track:17"));
    thread::sleep(Duration::from_millis(100));
    engine.command(PlayerCommand::Shuffle(false)).unwrap();
    catalog.allow(PAGES);
    until(&states, |state| {
        paused_on("tidal:track:17")(state) && !state.shuffle
    });
    let all = wait_for_queue(&engine, 8);
    let expected: Vec<String> = (17..25).map(|n| format!("tidal:track:{n}")).collect();
    assert_eq!(all, expected, "the songs after it follow in order");
}

/// The other way round: Shuffle switched on while the page with the named
/// song loads plays that song first and shuffles the rest.
#[test]
fn shuffle_switched_on_while_a_later_page_loads_shuffles_the_rest() {
    let catalog = PagedCatalog::new(1, Duration::ZERO, true);
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_artist(&engine, false, Some("tidal:track:17"));
    thread::sleep(Duration::from_millis(100));
    engine.command(PlayerCommand::Shuffle(true)).unwrap();
    catalog.allow(PAGES);
    until(&states, |state| {
        paused_on("tidal:track:17")(state) && state.shuffle
    });
    // Every song of the artist, the named one first and only once.
    let all = wait_for_queue(&engine, (PAGES * PER_PAGE) as usize);
    assert_eq!(all[0], "tidal:track:17");
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "no repeats: {all:?}");
}

/// Waits up to `limit` for a state that passes `test`.
fn within(
    states: &Receiver<LocalState>,
    limit: Duration,
    test: impl Fn(&LocalState) -> bool,
) -> Option<LocalState> {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match states.recv_timeout(left) {
            Ok(state) if test(&state) => return Some(state),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

/// Play or Space while a load still waits for its song (a context whose
/// first page has not come) is for that load: it does not start the song
/// of the queue being replaced again. A user pressed Space during a
/// long load and heard the previous song restart.
#[test]
fn play_during_a_load_does_not_restart_the_previous_song() {
    let catalog = PagedCatalog::new(0, Duration::ZERO, true);
    let (engine, states) = quiet_engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:track:1".into()),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    let before = until(&states, paused_on("tidal:track:1"));
    // The artist from its start: nothing can play before its first page.
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(ARTIST.into()),
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    until(&states, |state| state.loading);
    engine.command(PlayerCommand::Toggle).unwrap();
    let restarted = within(&states, Duration::from_millis(500), |state| {
        state.track_sequence != before.track_sequence
    });
    assert!(
        restarted.is_none(),
        "the previous song started again: {:?}",
        restarted
            .and_then(|state| state.track)
            .map(|track| track.uri)
    );
    // Space turned the load's Play into a pause.
    catalog.allow(PAGES);
    let started = within(&states, Duration::from_secs(10), |state| {
        state
            .track
            .as_ref()
            .is_some_and(|track| track.uri == "tidal:track:10")
            && !state.loading
    })
    .expect("the artist's first song starts");
    assert_eq!(started.playback, Playback::Paused);
    // And Space again plays it.
    engine.command(PlayerCommand::Toggle).unwrap();
    within(&states, Duration::from_secs(10), |state| {
        state.playback == Playback::Playing
    })
    .expect("Space plays it");
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
