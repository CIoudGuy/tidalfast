//! A playlist that lists one song twice: a clicked row starts its own
//! entry, not the song's first one.

use std::sync::Condvar;

use super::*;

const PLAYLIST: &str = "tidal:playlist:e2";
/// The playlist's entries, read four to a page.
const ENTRIES: u32 = 13;
const PER_PAGE: u32 = 4;
/// The song listed at rows 3 and 9.
const TWICE: u32 = 50;
/// Row 1 does not play (TIDAL marks it unavailable): it is left out of
/// what the catalogue delivers, and its number stays unused.
const GAP: u32 = 1;

/// The song at `row`: song 10 + row, except the one listed twice.
fn song_at(row: u32) -> LocalTrack {
    match row {
        3 | 9 => track(TWICE),
        _ => track(10 + row),
    }
}

/// The songs of the rows after `row`, in the playlist's order.
fn rows_after(row: u32) -> Vec<String> {
    (row + 1..ENTRIES)
        .filter(|row| *row != GAP)
        .map(|row| song_at(row).uri)
        .collect()
}

/// The playlist, read page by page, each page only once the test has
/// allowed it.
struct Listed {
    inner: Arc<FakeCatalog>,
    allowed: Mutex<u32>,
    open: Condvar,
}

impl Listed {
    fn new(allowed: u32) -> Arc<Self> {
        Arc::new(Self {
            inner: FakeCatalog::new(),
            allowed: Mutex::new(allowed),
            open: Condvar::new(),
        })
    }

    fn pages() -> u32 {
        ENTRIES.div_ceil(PER_PAGE)
    }

    fn allow_all(&self) {
        *lock(&self.allowed) = Self::pages();
        self.open.notify_all();
    }

    fn wait_for(&self, page: u32) {
        let mut allowed = lock(&self.allowed);
        while *allowed <= page {
            allowed = self
                .open
                .wait(allowed)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The songs of `page`, each with its own entry in the playlist.
    fn page(page: u32) -> Vec<(u32, LocalTrack)> {
        (page * PER_PAGE..((page + 1) * PER_PAGE).min(ENTRIES))
            .filter(|row| *row != GAP)
            .map(|row| (row, song_at(row)))
            .collect()
    }
}

impl Catalog for Listed {
    fn context_tracks(&self, _: &str) -> Result<Vec<LocalTrack>> {
        let mut all = Vec::new();
        for page in 0..Self::pages() {
            self.wait_for(page);
            all.extend(Self::page(page).into_iter().map(|(_, track)| track));
        }
        Ok(all)
    }

    fn context_pages(
        &self,
        _: &str,
        deliver: &mut (dyn FnMut(Vec<LocalTrack>) -> bool + Send),
    ) -> Result<()> {
        for page in 0..Self::pages() {
            self.wait_for(page);
            if !deliver(Self::page(page).into_iter().map(|(_, t)| t).collect()) {
                break;
            }
        }
        Ok(())
    }

    fn context_rows(
        &self,
        _: &str,
        deliver: &mut (dyn FnMut(Vec<(u32, LocalTrack)>) -> bool + Send),
    ) -> Result<()> {
        for page in 0..Self::pages() {
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

/// Double-clicks `row` of the playlist, paused, with the row's own song
/// details (`early`, the app's instant start) or without them.
fn load_row(engine: &Engine, row: u32, early: bool) {
    engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some(PLAYLIST.into()),
            offset_uri: Some(song_at(row).uri),
            offset_row: Some(row),
            offset_track: early.then(|| Box::new(song_at(row))),
            shuffle: Some(false),
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

/// Waits for the queue (the playing song, then Next up) to be `expected`.
fn wait_for_queue(engine: &Engine, expected: &[String]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let uris = uris_of(engine);
        if uris == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the queue is {uris:?}, not {expected:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// Row 9 holds the song row 3 holds too. Double-clicked, row 9 plays,
/// and Next up continues from row 10, as in TIDAL; it went back to row 3
/// and followed on from row 4. Row 3 still starts its own entry. With the
/// row's details (the instant start) and without them alike, and the
/// unavailable row 1 does not shift the count.
#[test]
fn a_song_listed_twice_plays_from_the_row_clicked() {
    for early in [true, false] {
        // With its details, the row's song starts before any page is in.
        let catalog = Listed::new(0);
        let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
        until(&states, |state| state.connected);

        load_row(&engine, 9, early);
        if !early {
            catalog.allow_all();
        }
        until(&states, paused_on(&song_at(9).uri));
        catalog.allow_all();
        let mut expected = vec![song_at(9).uri];
        expected.extend(rows_after(9));
        wait_for_queue(&engine, &expected);
        engine.command(PlayerCommand::Next).unwrap();
        until(&states, paused_on(&song_at(10).uri));
        engine.command(PlayerCommand::Previous).unwrap();
        until(&states, paused_on(&song_at(9).uri));
        engine.command(PlayerCommand::Previous).unwrap();
        until(&states, paused_on(&song_at(8).uri));

        load_row(&engine, 3, early);
        until(&states, paused_on(&song_at(3).uri));
        let mut expected = vec![song_at(3).uri];
        expected.extend(rows_after(3));
        wait_for_queue(&engine, &expected);
        engine.shutdown();
    }
}

/// A sorted or filtered view plays its rows as a list of songs, from the
/// clicked row's place in that list: the second copy of a song starts
/// there, and the rows after it follow.
#[test]
fn a_sorted_views_second_copy_plays_from_its_own_place() {
    let catalog = Listed::new(Listed::pages());
    let (engine, states) = engine_with(config(), Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    // The view's order: the song listed twice is at places 3 and 9.
    let view: Vec<String> = [21, 22, 23, TWICE, 24, 25, 26, 27, 28, TWICE, 29, 30]
        .map(|n| track(n).uri)
        .to_vec();
    engine
        .command(PlayerCommand::Load(LoadSpec {
            uris: view.clone(),
            offset_index: Some(9),
            play: false,
            ..LoadSpec::default()
        }))
        .unwrap();
    until(&states, paused_on(&track(TWICE).uri));
    wait_for_queue(&engine, &view[9..]);
    engine.command(PlayerCommand::Previous).unwrap();
    until(&states, paused_on(&track(28).uri));
}
