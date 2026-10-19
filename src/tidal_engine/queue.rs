//! The play order: a context (album, playlist, mix...) in its play order,
//! the songs the listener queued by hand in front of it, and what plays
//! now. This is the part of TIDAL's Spirc the local engine needs, kept
//! free of threads and audio so it can be tested on its own.
//!
//! The rules are `docs/_reference/queue.md`'s: queued songs play before
//! the context continues, starting a context keeps them, clearing removes
//! only them, and a song that starts leaves the queue.

use std::collections::{HashMap, HashSet, VecDeque};

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use super::dedupe::Dedupe;
use super::recent::{self, Avoid, Recent};
use super::types::BlockedSet;
use super::{LocalTrack, RepeatMode};

/// A shuffled repeat-all round does not begin with the songs the last one
/// ended on: up to this many.
const NO_REPEAT_WINDOW: usize = 50;

/// Where the playing song came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Context,
    Queued,
}

/// Where a load starts in its context.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Start {
    /// The first track, or anywhere when shuffled.
    #[default]
    Beginning,
    Uri(String),
    Index(usize),
}

/// What comes after the playing song.
#[derive(Clone, Debug, PartialEq)]
pub enum Advance {
    Track(LocalTrack),
    /// The context ran out: ask autoplay for more, if it is on.
    NeedAutoplay,
}

/// A snapshot of the queue for the interface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueView {
    pub context_uri: Option<String>,
    pub current: Option<LocalTrack>,
    /// Songs queued by hand, in play order (*Playing next*).
    pub queued: Vec<LocalTrack>,
    /// The context's songs after the playing one, in play order (*Next up*).
    pub next_up: Vec<LocalTrack>,
    /// The context is autoplay's continuation, not what was loaded.
    pub autoplay: bool,
}

#[derive(Clone, Debug)]
pub struct Queue {
    context_uri: Option<String>,
    /// The context in its own order.
    context: Vec<LocalTrack>,
    /// Indices into `context` in play order.
    order: Vec<usize>,
    /// Where in `order` the context has reached: the playing song when it
    /// came from the context, the last one played when a queued song plays,
    /// `None` before the context's first song.
    pos: Option<usize>,
    queued: VecDeque<LocalTrack>,
    current: Option<(LocalTrack, Origin)>,
    shuffle: bool,
    repeat: RepeatMode,
    autoplay: bool,
    rng: StdRng,
    /// What the shuffled order holds already, by id, ISRC and version.
    seen: Dedupe,
    /// Songs played lately, kept toward the end of a shuffled order.
    avoid: Avoid,
    /// Where the recently played songs start in `order`.
    recent_from: usize,
    /// The next round of a shuffled repeat-all, made as the last song of
    /// this one plays, so what preloads is what plays.
    next_round: Option<Vec<usize>>,
    recent: Recent,
    /// Songs and artists TIDAL's block lists name: never played from the
    /// queue or the context (a song named by hand at a load still is).
    blocked: BlockedSet,
}

impl Default for Queue {
    fn default() -> Self {
        Self::with_rng(StdRng::from_os_rng())
    }
}

impl Queue {
    /// A queue that shuffles from a fixed seed, for tests.
    pub fn seeded(seed: u64) -> Self {
        Self::with_rng(StdRng::seed_from_u64(seed))
    }

    fn with_rng(rng: StdRng) -> Self {
        Self {
            context_uri: None,
            context: Vec::new(),
            order: Vec::new(),
            pos: None,
            queued: VecDeque::new(),
            current: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            autoplay: false,
            rng,
            seen: Dedupe::default(),
            avoid: Avoid::default(),
            recent_from: 0,
            next_round: None,
            recent: Recent::default(),
            blocked: BlockedSet::default(),
        }
    }

    /// Sets what is left out of the play order. Nothing is removed: a song
    /// is passed over when its turn comes, so lifting a block plays it again.
    pub fn set_blocked(&mut self, blocked: BlockedSet) {
        self.blocked = blocked;
    }

    pub fn blocked(&self) -> &BlockedSet {
        &self.blocked
    }

    fn is_blocked(&self, at: usize) -> bool {
        self.blocked.hit(&self.context[self.order[at]])
    }

    /// Where the context goes next: the place in `order` of the first song
    /// after the playing one that is not blocked, and whether that is the
    /// next Repeat all round (then the place is in that round).
    fn next_position(&self) -> Option<(usize, bool)> {
        let start = self.pos.map_or(0, |pos| pos + 1);
        if let Some(at) = (start..self.order.len()).find(|at| !self.is_blocked(*at)) {
            return Some((at, false));
        }
        if self.repeat == RepeatMode::Context && !self.order.is_empty() {
            let round = match &self.next_round {
                Some(round) if self.shuffle => round,
                _ => &self.order,
            };
            if let Some(at) = round
                .iter()
                .position(|index| !self.blocked.hit(&self.context[*index]))
            {
                return Some((at, true));
            }
        }
        None
    }

    /// Remembers what plays in `path` (across restarts).
    pub fn remember_in(&mut self, path: std::path::PathBuf) {
        self.recent = Recent::at(path);
    }

    /// Writes the memory of recent plays now.
    pub fn flush_recent(&self) {
        self.recent.flush();
    }

    /// The songs played lately in `context`, for a load to look at.
    pub fn recent_in(&self, context: &str) -> Avoid {
        self.recent.in_context(context, recent::PER_CONTEXT)
    }

    /// Sets the playing song and remembers it as played.
    fn begin(&mut self, track: LocalTrack, origin: Origin) {
        let context = match origin {
            Origin::Context if !self.autoplay => self.context_uri.as_deref(),
            _ => None,
        };
        self.recent.note(context, &track);
        self.current = Some((track, origin));
    }

    pub fn context_uri(&self) -> Option<&str> {
        self.context_uri.as_deref()
    }

    pub fn current(&self) -> Option<&LocalTrack> {
        self.current.as_ref().map(|(track, _)| track)
    }

    pub fn origin(&self) -> Option<Origin> {
        self.current.as_ref().map(|(_, origin)| *origin)
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn is_autoplay(&self) -> bool {
        self.autoplay
    }

    /// Replaces the context and makes `start` the playing song. Songs
    /// queued by hand stay queued. `None` when the context is empty.
    pub fn load(
        &mut self,
        context_uri: Option<String>,
        tracks: Vec<LocalTrack>,
        start: Start,
        shuffle: bool,
    ) -> Option<LocalTrack> {
        self.context_uri = context_uri;
        self.context = tracks;
        self.autoplay = false;
        self.shuffle = shuffle;
        self.next_round = None;
        if self.context.is_empty() {
            self.order.clear();
            self.pos = None;
            self.current = None;
            return None;
        }
        let named = !matches!(start, Start::Beginning);
        let first = match start {
            Start::Beginning => None,
            Start::Uri(uri) => Some(
                self.context
                    .iter()
                    .position(|track| track.uri == uri)
                    .unwrap_or(0),
            ),
            Start::Index(index) => Some(index.min(self.context.len() - 1)),
        };
        self.order = self.play_order(first);
        let mut pos = if shuffle { 0 } else { first.unwrap_or(0) };
        // Play on a song nobody blocked, unless one was named.
        if !named && let Some(at) = (pos..self.order.len()).find(|at| !self.is_blocked(*at)) {
            pos = at;
        }
        self.pos = Some(pos);
        let track = self.context[self.order[pos]].clone();
        self.begin(track.clone(), Origin::Context);
        self.prepare_round();
        Some(track)
    }

    /// Adds later pages of the context a load started on its first page.
    /// Played songs and the one playing keep their place. Unshuffled, the
    /// new songs follow the old; shuffled, each new song takes a random
    /// place among the songs not yet played (an inside-out Fisher-Yates
    /// step), so the unplayed part stays a uniform shuffle once the last
    /// page is in, and the work is the size of the page. Songs played
    /// lately stay in their region at the end, and a recording the order
    /// holds already is left out. Only the first song, which had to start
    /// before the later pages existed, comes from the first page alone.
    /// `false` when there was nothing to add to.
    pub fn extend_context(&mut self, tracks: Vec<LocalTrack>) -> bool {
        if tracks.is_empty() || self.autoplay || self.context.is_empty() {
            return false;
        }
        let first = self.context.len();
        self.context.extend(tracks);
        self.next_round = None;
        if self.shuffle {
            let keep = self.pos.map_or(0, |pos| pos + 1).min(self.order.len());
            self.recent_from = self.recent_from.clamp(keep, self.order.len());
            for index in first..self.context.len() {
                let track = &self.context[index];
                if !self.seen.admit(track) {
                    continue;
                }
                let end = self.order.len();
                self.order.push(index);
                if self.avoid.hit(track) {
                    let place = self.rng.random_range(self.recent_from..=end);
                    self.order.swap(place, end);
                } else {
                    let boundary = self.recent_from;
                    // The first recent song moves to the end, making room.
                    self.order.swap(boundary, end);
                    let place = self.rng.random_range(keep..=boundary);
                    self.order.swap(place, boundary);
                    self.recent_from += 1;
                }
            }
        } else {
            self.order.extend(first..self.context.len());
            self.recent_from = self.order.len();
        }
        self.prepare_round();
        true
    }

    /// How many context songs are still to play after the current one.
    pub fn ahead(&self) -> usize {
        self.order
            .len()
            .saturating_sub(self.pos.map_or(0, |pos| pos + 1))
    }

    /// Makes a freshly shuffled order follow `hint`: the songs after the
    /// playing one, as a saved session had them. Songs the hint lacks stay
    /// behind it in their shuffled order; ones it names twice need two
    /// copies in the context.
    pub fn follow_order(&mut self, hint: &[String]) {
        if !self.shuffle || hint.is_empty() || self.pos != Some(0) || self.order.is_empty() {
            return;
        }
        // The songs the order holds, by uri, the first to play last in
        // each list so that popping takes the first.
        let mut by_uri: HashMap<&str, Vec<usize>> = HashMap::new();
        for index in self.order.iter().skip(1).rev() {
            by_uri
                .entry(self.context[*index].uri.as_str())
                .or_default()
                .push(*index);
        }
        let mut order = vec![self.order[0]];
        let mut used = vec![false; self.context.len()];
        used[self.order[0]] = true;
        for uri in hint {
            if let Some(index) = by_uri.get_mut(uri.as_str()).and_then(Vec::pop) {
                used[index] = true;
                order.push(index);
            }
        }
        order.extend(self.order.iter().copied().filter(|index| !used[*index]));
        self.order = order;
        self.recent_from = self.order.len();
        self.next_round = None;
        self.prepare_round();
    }

    /// The context's play order, with `first` (when given) leading a
    /// shuffled order. A shuffle holds each recording once and puts the
    /// songs played lately last, in a random order among themselves.
    fn play_order(&mut self, first: Option<usize>) -> Vec<usize> {
        self.seen = Dedupe::default();
        if !self.shuffle {
            self.avoid = Avoid::default();
            self.recent_from = self.context.len();
            return (0..self.context.len()).collect();
        }
        if let Some(first) = first {
            self.seen.admit(&self.context[first]);
        }
        let mut songs: Vec<usize> = (0..self.context.len())
            .filter(|index| Some(*index) != first)
            .filter(|index| self.seen.admit(&self.context[*index]))
            .collect();
        // Recent means the last half of the songs at most, so a short
        // context always has songs that are not.
        let window = recent::PER_CONTEXT.min(songs.len().div_ceil(2)).max(1);
        self.avoid = match self.context_uri.as_deref() {
            Some(context) => self.recent.in_context(context, window),
            None => Avoid::default(),
        };
        songs.shuffle(&mut self.rng);
        let (fresh, lately): (Vec<usize>, Vec<usize>) = songs
            .into_iter()
            .partition(|index| !self.avoid.hit(&self.context[*index]));
        let mut order = Vec::with_capacity(fresh.len() + lately.len() + 1);
        order.extend(first);
        order.extend(fresh);
        self.recent_from = order.len();
        order.extend(lately);
        order
    }

    /// Makes the next round of a shuffled repeat-all as soon as its last
    /// song is reached, and drops it when it no longer applies.
    fn prepare_round(&mut self) {
        let at_end = self.shuffle
            && self.repeat == RepeatMode::Context
            && self.order.len() > 1
            && self.pos == Some(self.order.len() - 1);
        if !at_end {
            self.next_round = None;
        } else if self.next_round.is_none() {
            self.next_round = Some(self.reshuffled());
        }
    }

    /// The whole order shuffled again so that its first places hold no
    /// song of the last [`NO_REPEAT_WINDOW`] (at most half) that played.
    fn reshuffled(&mut self) -> Vec<usize> {
        let count = self.order.len();
        let window = NO_REPEAT_WINDOW.min(count / 2);
        let lately: HashSet<usize> = self.order[count - window..].iter().copied().collect();
        let mut all = self.order.clone();
        all.shuffle(&mut self.rng);
        let mut head = Vec::with_capacity(count);
        let mut rest = Vec::with_capacity(count);
        for index in all {
            if head.len() < window && !lately.contains(&index) {
                head.push(index);
            } else {
                rest.push(index);
            }
        }
        head.extend(rest);
        head
    }

    /// Moves to what plays after the current song. `ended` is a natural
    /// end, where Repeat track plays the same song again; Next always moves
    /// on. Nothing changes when the context needs autoplay.
    pub fn advance(&mut self, ended: bool) -> Advance {
        if ended
            && self.repeat == RepeatMode::Track
            && let Some((track, _)) = &self.current
        {
            return Advance::Track(track.clone());
        }
        while let Some(track) = self.queued.pop_front() {
            if self.blocked.hit(&track) {
                continue;
            }
            self.begin(track.clone(), Origin::Queued);
            return Advance::Track(track);
        }
        let Some((next, wraps)) = self.next_position() else {
            return Advance::NeedAutoplay;
        };
        let next = if wraps {
            if self.shuffle {
                self.order = match self.next_round.take() {
                    Some(round) => round,
                    None => self.reshuffled(),
                };
                self.recent_from = self.order.len();
            }
            (0..self.order.len())
                .find(|at| !self.is_blocked(*at))
                .unwrap_or(0)
        } else {
            next
        };
        self.pos = Some(next);
        let track = self.context[self.order[next]].clone();
        self.begin(track.clone(), Origin::Context);
        self.prepare_round();
        Advance::Track(track)
    }

    /// What `advance(true)` would give, without moving.
    pub fn peek(&self) -> Advance {
        if self.repeat == RepeatMode::Track
            && let Some((track, _)) = &self.current
        {
            return Advance::Track(track.clone());
        }
        if let Some(track) = self.queued.iter().find(|track| !self.blocked.hit(track)) {
            return Advance::Track(track.clone());
        }
        match self.next_position() {
            Some((at, false)) => Advance::Track(self.context[self.order[at]].clone()),
            Some((at, true)) => {
                let round = match &self.next_round {
                    Some(round) if self.shuffle => round,
                    _ => &self.order,
                };
                Advance::Track(self.context[round[at]].clone())
            }
            None => Advance::NeedAutoplay,
        }
    }

    /// Moves back one song. `None` means there is nothing before it, and
    /// the playing song starts again instead.
    pub fn previous(&mut self) -> Option<LocalTrack> {
        let origin = self.origin()?;
        let pos = match (origin, self.pos) {
            // A queued song goes back to the context song before it.
            (Origin::Queued, Some(pos)) => pos,
            (Origin::Queued, None) => return None,
            (Origin::Context, Some(pos)) if pos > 0 => pos - 1,
            (Origin::Context, Some(_)) if self.repeat == RepeatMode::Context => {
                self.order.len().checked_sub(1)?
            }
            (Origin::Context, _) => return None,
        };
        self.pos = Some(pos);
        let track = self.context[self.order[pos]].clone();
        self.begin(track.clone(), Origin::Context);
        self.prepare_round();
        Some(track)
    }

    /// Turns shuffle on, with a new order every time, or off. The playing
    /// song stays where it is and leads the new order.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let anchor = self.pos.and_then(|pos| self.order.get(pos).copied());
        self.order = self.play_order(anchor);
        self.pos = match (shuffle, anchor) {
            (_, None) => None,
            (true, Some(_)) => Some(0),
            (false, Some(anchor)) => Some(anchor),
        };
        self.next_round = None;
        self.prepare_round();
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
        self.prepare_round();
    }

    /// Queues a song after the ones queued already.
    pub fn add(&mut self, track: LocalTrack) {
        self.queued.push_back(track);
    }

    /// Takes one upcoming song of the context out of the play order, so it
    /// is not played in this pass of the context (a plugin removed it, as
    /// TIDAL's queue lets one). `occurrence` counts the rows with this uri
    /// in the order the queue view lists them, from 0; a Repeat all
    /// round's wrap rows are listed after the rest. Nothing happens, and
    /// `false` comes back, when no such row exists.
    pub fn skip_context(&mut self, uri: &str, occurrence: usize) -> bool {
        let after = self.pos.map_or(0, |pos| pos + 1);
        let mut seen = 0;
        let mut found = None;
        for at in after..self.order.len() {
            if self.context[self.order[at]].uri == uri {
                if seen == occurrence {
                    found = Some(at);
                    break;
                }
                seen += 1;
            }
        }
        if found.is_none()
            && self.repeat == RepeatMode::Context
            && !(self.shuffle && self.next_round.is_some())
        {
            for at in 0..after.min(self.order.len()) {
                if self.context[self.order[at]].uri == uri {
                    if seen == occurrence {
                        found = Some(at);
                        break;
                    }
                    seen += 1;
                }
            }
        }
        let Some(at) = found else {
            return false;
        };
        self.order.remove(at);
        if at < self.recent_from {
            self.recent_from -= 1;
        }
        if self.pos.is_some_and(|pos| at <= pos) {
            self.pos = self.pos.and_then(|pos| pos.checked_sub(1));
        }
        self.prepare_round();
        true
    }

    /// Empties *Playing next*, keeping the context.
    pub fn clear_queued(&mut self) {
        self.queued.clear();
    }

    /// Fills in metadata for queued songs added by URI alone.
    pub fn fill(&mut self, tracks: &[LocalTrack]) {
        for slot in &mut self.queued {
            if !slot.title.is_empty() {
                continue;
            }
            if let Some(track) = tracks.iter().find(|track| track.uri == slot.uri) {
                *slot = track.clone();
            }
        }
        if let Some((current, _)) = &mut self.current
            && current.title.is_empty()
            && let Some(track) = tracks.iter().find(|track| track.uri == current.uri)
        {
            *current = track.clone();
        }
        // A song queued by its address alone shows its artists only now.
        let blocked = std::mem::take(&mut self.blocked);
        self.queued.retain(|track| !blocked.hit(track));
        self.blocked = blocked;
    }

    /// Continues past the end of the context with autoplay's tracks. The
    /// next `advance` plays the first of them.
    /// What was played in the last [`recent::AUTOPLAY_WINDOW`] songs, or
    /// is listed twice, is left out. Nothing changes, and `false` comes
    /// back, when nothing is left.
    pub fn continue_with(&mut self, tracks: Vec<LocalTrack>) -> bool {
        let tracks = self.autoplay_fresh(tracks);
        if tracks.is_empty() {
            return false;
        }
        self.context = tracks;
        self.order = (0..self.context.len()).collect();
        self.recent_from = self.order.len();
        self.pos = None;
        self.autoplay = true;
        self.next_round = None;
        true
    }

    /// `tracks` without the songs played lately and without repeats.
    pub fn autoplay_fresh(&self, tracks: Vec<LocalTrack>) -> Vec<LocalTrack> {
        let lately = self.recent.overall(recent::AUTOPLAY_WINDOW);
        let mut seen = Dedupe::default();
        if let Some(track) = self.current() {
            seen.admit(track);
        }
        tracks
            .into_iter()
            .filter(|track| !self.blocked.hit(track) && !lately.hit(track) && seen.admit(track))
            .collect()
    }

    /// Marks the context as autoplay's own, a station rather than what the
    /// listener chose.
    pub fn set_autoplay(&mut self, autoplay: bool) {
        self.autoplay = autoplay;
    }

    /// The last context song reached, as the seed for autoplay.
    pub fn seed(&self) -> Option<&LocalTrack> {
        self.seed_back(0)
    }

    /// The context song `back` songs before the last one reached: another
    /// seed for autoplay when TIDAL has no station for the first.
    pub fn seed_back(&self, back: usize) -> Option<&LocalTrack> {
        let reached = self
            .pos
            .and_then(|pos| pos.checked_sub(back))
            .and_then(|pos| self.order.get(pos))
            .map(|index| &self.context[*index]);
        match back {
            0 => reached.or_else(|| self.current()),
            _ => reached,
        }
    }

    pub fn view(&self, next_up_limit: usize) -> QueueView {
        let after = self.pos.map_or(0, |pos| pos + 1);
        let mut next_up: Vec<LocalTrack> = self
            .order
            .iter()
            .skip(after)
            .map(|index| &self.context[*index])
            .filter(|track| !self.blocked.hit(track))
            .take(next_up_limit)
            .cloned()
            .collect();
        if self.repeat == RepeatMode::Context {
            let room = next_up_limit.saturating_sub(next_up.len());
            // A shuffled repeat-all plays its next round, already made by
            // now, not the round ending: list what will play.
            let wrap = match &self.next_round {
                Some(round) if self.shuffle => round,
                _ => &self.order,
            };
            next_up.extend(
                wrap.iter()
                    .map(|index| &self.context[*index])
                    .filter(|track| !self.blocked.hit(track))
                    .take(room.min(after))
                    .cloned(),
            );
        }
        QueueView {
            context_uri: self.context_uri.clone(),
            current: self.current().cloned(),
            queued: self
                .queued
                .iter()
                .filter(|track| !self.blocked.hit(track))
                .cloned()
                .collect(),
            next_up,
            autoplay: self.autoplay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: usize) -> LocalTrack {
        LocalTrack {
            uri: format!("tidal:track:{n}"),
            title: format!("Song {n}"),
            duration_ms: 180_000,
            ..LocalTrack::default()
        }
    }

    fn album(count: usize) -> Vec<LocalTrack> {
        (1..=count).map(track).collect()
    }

    fn uri(advance: Advance) -> String {
        match advance {
            Advance::Track(track) => track.uri,
            Advance::NeedAutoplay => "autoplay".into(),
        }
    }

    fn loaded(count: usize) -> Queue {
        let mut queue = Queue::seeded(7);
        queue.load(
            Some("tidal:album:1".into()),
            album(count),
            Start::Beginning,
            false,
        );
        queue
    }

    fn by(n: usize, artists: &[&str]) -> LocalTrack {
        LocalTrack {
            artists: artists
                .iter()
                .map(|id| crate::api::models::ArtistRef {
                    id: Some((*id).into()),
                    name: format!("Artist {id}"),
                    uri: Some(format!("tidal:artist:{id}")),
                    picture: None,
                })
                .collect(),
            ..track(n)
        }
    }

    fn blocking(tracks: &[&str], artists: &[&str]) -> BlockedSet {
        BlockedSet {
            tracks: tracks.iter().map(|id| (*id).to_string()).collect(),
            artists: artists.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    /// TIDAL's apps filter what they play by the block lists: a blocked
    /// song and a song by a blocked artist (main or featured) are passed
    /// over, in the context, in the queue, and in what the view lists.
    #[test]
    fn blocked_songs_and_songs_by_blocked_artists_are_skipped() {
        let mut queue = Queue::seeded(7);
        let songs = vec![
            by(1, &["a"]),
            by(2, &["a"]),
            by(3, &["b"]),
            by(4, &["a", "x"]),
            by(5, &["a"]),
        ];
        queue.load(Some("tidal:album:1".into()), songs, Start::Beginning, false);
        queue.add(by(9, &["a"]));
        queue.add(by(10, &["x"]));
        queue.set_blocked(blocking(&["2"], &["x"]));
        let view = queue.view(10);
        assert_eq!(
            view.next_up
                .iter()
                .map(|t| t.uri.as_str())
                .collect::<Vec<_>>(),
            ["tidal:track:3", "tidal:track:5"],
            "the song and the featured artist's song are not listed"
        );
        assert_eq!(
            view.queued.len(),
            1,
            "the queued song by the blocked artist"
        );
        assert_eq!(uri(queue.peek()), "tidal:track:9");
        assert_eq!(uri(queue.advance(true)), "tidal:track:9");
        assert_eq!(uri(queue.advance(true)), "tidal:track:3");
        assert_eq!(uri(queue.advance(true)), "tidal:track:5");
        assert_eq!(queue.advance(true), Advance::NeedAutoplay);
    }

    /// Lifting a block makes the song playable again: nothing was removed.
    #[test]
    fn unblocking_makes_a_song_playable_again() {
        let mut queue = Queue::seeded(7);
        queue.load(
            Some("tidal:album:1".into()),
            vec![by(1, &["a"]), by(2, &["b"]), by(3, &["a"])],
            Start::Beginning,
            false,
        );
        queue.set_blocked(blocking(&[], &["b"]));
        assert_eq!(uri(queue.peek()), "tidal:track:3");
        queue.set_blocked(BlockedSet::default());
        assert_eq!(uri(queue.peek()), "tidal:track:2");
        assert_eq!(uri(queue.advance(true)), "tidal:track:2");
    }

    /// A load begins on a song that is not blocked, unless one was named.
    #[test]
    fn a_load_begins_on_an_unblocked_song_unless_one_is_named() {
        let mut queue = Queue::seeded(7);
        queue.set_blocked(blocking(&["1"], &[]));
        let songs = vec![by(1, &["a"]), by(2, &["a"])];
        let first = queue.load(None, songs.clone(), Start::Beginning, false);
        assert_eq!(first.unwrap().uri, "tidal:track:2");
        let named = queue.load(None, songs, Start::Uri("tidal:track:1".into()), false);
        assert_eq!(named.unwrap().uri, "tidal:track:1", "named by the listener");
    }

    /// Repeat all goes round without the blocked songs, and a context of
    /// nothing but blocked songs asks for autoplay instead of looping.
    #[test]
    fn repeat_all_skips_blocked_songs_and_does_not_loop_on_a_blocked_context() {
        let mut queue = Queue::seeded(7);
        queue.load(
            None,
            vec![by(1, &["a"]), by(2, &["b"]), by(3, &["a"])],
            Start::Beginning,
            false,
        );
        queue.set_repeat(RepeatMode::Context);
        queue.set_blocked(blocking(&[], &["b"]));
        assert_eq!(uri(queue.advance(true)), "tidal:track:3");
        assert_eq!(
            uri(queue.advance(true)),
            "tidal:track:1",
            "round two skips 2"
        );
        queue.set_blocked(blocking(&[], &["a", "b"]));
        assert_eq!(queue.advance(true), Advance::NeedAutoplay);
    }

    /// Autoplay's songs are filtered too.
    #[test]
    fn autoplay_leaves_out_blocked_songs() {
        let mut queue = Queue::seeded(7);
        queue.load(None, vec![by(1, &["a"])], Start::Beginning, false);
        queue.set_blocked(blocking(&["7"], &["b"]));
        let fresh = queue.autoplay_fresh(vec![by(7, &["a"]), by(8, &["b"]), by(9, &["a"])]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].uri, "tidal:track:9");
    }

    #[test]
    fn a_context_plays_in_order_and_then_asks_for_autoplay() {
        let mut queue = loaded(3);
        assert_eq!(queue.current().unwrap().uri, "tidal:track:1");
        assert_eq!(uri(queue.advance(true)), "tidal:track:2");
        assert_eq!(uri(queue.advance(false)), "tidal:track:3");
        assert_eq!(queue.peek(), Advance::NeedAutoplay);
        assert_eq!(queue.advance(true), Advance::NeedAutoplay);
        assert_eq!(queue.current().unwrap().uri, "tidal:track:3", "unchanged");
    }

    #[test]
    fn a_load_starts_at_the_offset_by_uri_or_index() {
        let mut queue = Queue::seeded(1);
        let first = queue.load(None, album(5), Start::Uri("tidal:track:4".into()), false);
        assert_eq!(first.unwrap().uri, "tidal:track:4");
        assert_eq!(uri(queue.advance(true)), "tidal:track:5");
        let first = queue.load(None, album(5), Start::Index(1), false);
        assert_eq!(first.unwrap().uri, "tidal:track:2");
        // An offset that is not in the context starts at the beginning.
        let first = queue.load(None, album(5), Start::Uri("tidal:track:99".into()), false);
        assert_eq!(first.unwrap().uri, "tidal:track:1");
        assert!(
            queue
                .load(None, Vec::new(), Start::Beginning, false)
                .is_none()
        );
    }

    #[test]
    fn queued_songs_play_before_the_context_continues() {
        let mut queue = loaded(3);
        queue.add(track(10));
        queue.add(track(11));
        queue.add(track(10));
        assert_eq!(queue.view(10).queued.len(), 3, "duplicates are kept");
        assert_eq!(uri(queue.advance(true)), "tidal:track:10");
        assert_eq!(queue.origin(), Some(Origin::Queued));
        assert_eq!(queue.view(10).queued.len(), 2, "a started song leaves");
        assert_eq!(uri(queue.advance(false)), "tidal:track:11");
        assert_eq!(uri(queue.advance(true)), "tidal:track:10");
        assert_eq!(uri(queue.advance(true)), "tidal:track:2");
    }

    #[test]
    fn starting_a_context_keeps_queued_songs_and_clear_removes_only_them() {
        let mut queue = loaded(3);
        queue.add(track(10));
        queue.load(
            Some("tidal:playlist:x".into()),
            album(2),
            Start::Beginning,
            false,
        );
        let view = queue.view(10);
        assert_eq!(view.queued, vec![track(10)]);
        assert_eq!(view.next_up, vec![track(2)]);
        queue.clear_queued();
        let view = queue.view(10);
        assert!(view.queued.is_empty());
        assert_eq!(view.next_up, vec![track(2)]);
    }

    #[test]
    fn previous_steps_back_through_the_context() {
        let mut queue = loaded(3);
        assert_eq!(queue.previous(), None, "the first song restarts");
        queue.advance(true);
        queue.advance(true);
        assert_eq!(queue.previous().unwrap().uri, "tidal:track:2");
        // From a queued song, back to the context song before it.
        queue.add(track(10));
        queue.advance(false);
        assert_eq!(queue.previous().unwrap().uri, "tidal:track:2");
        queue.set_repeat(RepeatMode::Context);
        queue.previous();
        assert_eq!(queue.previous().unwrap().uri, "tidal:track:3", "wraps");
    }

    #[test]
    fn repeat_track_replays_on_a_natural_end_but_next_moves_on() {
        let mut queue = loaded(3);
        queue.set_repeat(RepeatMode::Track);
        assert_eq!(uri(queue.peek()), "tidal:track:1");
        assert_eq!(uri(queue.advance(true)), "tidal:track:1");
        assert_eq!(uri(queue.advance(false)), "tidal:track:2");
    }

    #[test]
    fn repeat_context_wraps_to_the_start() {
        let mut queue = loaded(2);
        queue.set_repeat(RepeatMode::Context);
        queue.advance(true);
        assert_eq!(uri(queue.peek()), "tidal:track:1");
        assert_eq!(uri(queue.advance(true)), "tidal:track:1");
        // Next up shows the wrap as well.
        assert_eq!(queue.view(5).next_up, vec![track(2), track(1)]);
    }

    /// On the last song of a shuffled repeat-all, Next up lists the round
    /// that will play, the one `peek` and `advance` take, not the old one.
    #[test]
    fn next_up_at_the_end_of_a_shuffled_round_shows_the_next_round() {
        let mut queue = loaded(6);
        queue.set_repeat(RepeatMode::Context);
        queue.set_shuffle(true);
        for _ in 0..5 {
            queue.advance(true);
        }
        let shown: Vec<String> = queue
            .view(3)
            .next_up
            .into_iter()
            .map(|track| track.uri)
            .collect();
        assert_eq!(shown.len(), 3);
        assert_eq!(shown[0], uri(queue.peek()));
        let played: Vec<String> = (0..3).map(|_| uri(queue.advance(true))).collect();
        assert_eq!(shown, played);
    }

    #[test]
    fn shuffle_keeps_the_playing_song_first_and_plays_every_song_once() {
        let mut queue = loaded(20);
        queue.advance(true);
        queue.advance(true);
        queue.set_shuffle(true);
        assert_eq!(queue.current().unwrap().uri, "tidal:track:3");
        let mut heard = vec![queue.current().unwrap().uri.clone()];
        while let Advance::Track(track) = queue.advance(true) {
            heard.push(track.uri);
        }
        assert_eq!(heard.len(), 20, "every song once, the earlier ones too");
        heard.sort();
        heard.dedup();
        assert_eq!(heard.len(), 20);
    }

    #[test]
    fn each_shuffle_is_a_new_order_and_unshuffling_returns_to_the_album() {
        let mut queue = loaded(30);
        queue.set_shuffle(true);
        let first = queue.view(30).next_up;
        queue.set_shuffle(false);
        assert_eq!(queue.view(30).next_up, album(30)[1..].to_vec());
        queue.set_shuffle(true);
        let second = queue.view(30).next_up;
        assert_ne!(first, second, "toggling reshuffles");
        // Unshuffled from the middle, the album continues after the song.
        queue.advance(true);
        let playing = queue.current().unwrap().clone();
        queue.set_shuffle(false);
        let number: usize = playing.uri.rsplit(':').next().unwrap().parse().unwrap();
        assert_eq!(queue.view(30).next_up, album(30)[number..].to_vec());
    }

    #[test]
    fn a_shuffled_load_starts_with_the_chosen_song() {
        let mut queue = Queue::seeded(3);
        let first = queue.load(None, album(10), Start::Index(6), true);
        assert_eq!(first.unwrap().uri, "tidal:track:7");
        assert_eq!(queue.view(20).next_up.len(), 9);
        assert!(queue.shuffle());
    }

    #[test]
    fn autoplay_continues_after_the_context() {
        let mut queue = loaded(2);
        queue.advance(true);
        assert_eq!(queue.advance(true), Advance::NeedAutoplay);
        assert_eq!(queue.seed().unwrap().uri, "tidal:track:2");
        queue.continue_with(vec![track(50), track(51)]);
        assert!(queue.is_autoplay());
        assert_eq!(uri(queue.advance(true)), "tidal:track:50");
        assert_eq!(uri(queue.advance(true)), "tidal:track:51");
        // A queued song still comes first.
        queue.add(track(9));
        assert_eq!(uri(queue.peek()), "tidal:track:9");
    }

    #[test]
    fn queued_uris_get_their_metadata_later() {
        let mut queue = loaded(1);
        queue.add(LocalTrack {
            uri: "tidal:track:77".into(),
            ..LocalTrack::default()
        });
        queue.fill(&[track(77)]);
        assert_eq!(queue.view(1).queued, vec![track(77)]);
    }
    #[test]
    fn a_saved_shuffled_order_is_followed_and_the_rest_is_shuffled() {
        let mut queue = Queue::seeded(7);
        let first = queue
            .load(
                Some("tidal:album:a".into()),
                album(10),
                Start::Uri("tidal:track:4".into()),
                true,
            )
            .unwrap();
        assert_eq!(first.uri, "tidal:track:4");
        let hint: Vec<String> = [9, 2, 7]
            .iter()
            .map(|n| format!("tidal:track:{n}"))
            .collect();
        queue.follow_order(&hint);
        assert_eq!(uri(queue.advance(false)), "tidal:track:9");
        assert_eq!(uri(queue.advance(false)), "tidal:track:2");
        assert_eq!(uri(queue.advance(false)), "tidal:track:7");
        // Everything else still plays once, never the playing song again.
        let mut rest = Vec::new();
        while let Advance::Track(track) = queue.advance(false) {
            rest.push(track.uri);
        }
        rest.sort();
        assert_eq!(rest.len(), 6);
        assert!(!rest.contains(&"tidal:track:4".to_string()));
        // Songs gone from the context are ignored.
        let mut queue = Queue::seeded(7);
        queue.load(None, album(3), Start::Index(0), true);
        queue.follow_order(&["tidal:track:99".into(), "tidal:track:3".into()]);
        assert_eq!(uri(queue.advance(false)), "tidal:track:3");
    }

    #[test]
    fn later_pages_join_the_order_behind_the_played_songs() {
        let mut queue = Queue::seeded(3);
        queue.load(None, album(4), Start::Beginning, true);
        let Advance::Track(second) = queue.advance(true) else {
            panic!("no second song");
        };
        let played = queue.view(100);
        assert!(queue.extend_context((5..=10).map(track).collect()));
        let view = queue.view(100);
        assert_eq!(view.current, Some(second));
        assert!(played.next_up.len() < view.next_up.len());
        let mut all: Vec<String> = view.next_up.iter().map(|t| t.uri.clone()).collect();
        all.push(view.current.unwrap().uri);
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 9, "every song once: the first one was played");
        // Played songs stay behind: going back reaches the first song.
        assert!(queue.previous().is_some());
    }

    #[test]
    fn later_pages_of_an_unshuffled_context_follow_in_order() {
        let mut queue = loaded(3);
        assert!(queue.extend_context(vec![track(4), track(5)]));
        let uris: Vec<String> = queue
            .view(10)
            .next_up
            .iter()
            .map(|t| t.uri.clone())
            .collect();
        assert_eq!(
            uris,
            [
                "tidal:track:2",
                "tidal:track:3",
                "tidal:track:4",
                "tidal:track:5"
            ]
        );
        assert!(!queue.extend_context(Vec::new()));
    }

    /// Pearson's chi-square of `counts` against equal expectation.
    fn chi_square(counts: &[u32]) -> f64 {
        let total: u32 = counts.iter().sum();
        let expected = f64::from(total) / counts.len() as f64;
        counts
            .iter()
            .map(|count| (f64::from(*count) - expected).powi(2) / expected)
            .sum()
    }

    /// 20 songs, df 19: 60 is far past any fair draw (p about 1e-6), and a
    /// fixed first song or a lopsided shuffle is far past it too.
    #[test]
    fn every_song_is_equally_likely_to_start_a_shuffle() {
        let mut queue = Queue::seeded(0x5eed);
        let mut counts = [0u32; 20];
        for _ in 0..4000 {
            let first = queue
                .load(None, album(20), Start::Beginning, true)
                .expect("a first song");
            counts[first
                .uri
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap()
                - 1] += 1;
        }
        let chi = chi_square(&counts);
        assert!(chi < 60.0, "chi-square {chi}: {counts:?}");
    }

    /// Every position of a full shuffle is uniform, not only the first.
    #[test]
    fn every_song_is_equally_likely_at_any_place_in_a_shuffle() {
        let mut queue = Queue::seeded(11);
        let mut last = [0u32; 20];
        for _ in 0..4000 {
            queue.load(None, album(20), Start::Beginning, true);
            let view = queue.view(100);
            let n: usize = view
                .next_up
                .last()
                .unwrap()
                .uri
                .rsplit(':')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            last[n - 1] += 1;
        }
        let chi = chi_square(&last);
        assert!(chi < 60.0, "chi-square {chi}: {last:?}");
    }

    /// Pages that arrive later mix in without favouring old or new songs:
    /// with a song playing from the first page, what plays next is equally
    /// likely to be any of the 19 others, first page or not.
    #[test]
    fn later_pages_keep_the_rest_of_a_shuffle_uniform() {
        let mut queue = Queue::seeded(77);
        let mut counts = [0u32; 19];
        for _ in 0..3800 {
            queue.load(None, album(10), Start::Uri("tidal:track:1".into()), true);
            assert!(queue.extend_context((11..=20).map(track).collect()));
            let next = queue.view(1).next_up.remove(0);
            let n: usize = next.uri.rsplit(':').next().unwrap().parse().unwrap();
            counts[n - 2] += 1;
        }
        let chi = chi_square(&counts);
        assert!(chi < 50.0, "chi-square {chi}: {counts:?}");
        // And the new songs reach every place, not only the end.
        let mut queue = Queue::seeded(78);
        let mut places = [0u32; 19];
        for _ in 0..3800 {
            queue.load(None, album(10), Start::Uri("tidal:track:1".into()), true);
            queue.extend_context((11..=20).map(track).collect());
            let rest = queue.view(100).next_up;
            let place = rest.iter().position(|t| t.uri == "tidal:track:20").unwrap();
            places[place] += 1;
        }
        let chi = chi_square(&places);
        assert!(chi < 50.0, "chi-square {chi}: {places:?}");
    }

    #[test]
    fn each_default_queue_shuffles_differently() {
        let orders: Vec<Vec<String>> = (0..3)
            .map(|_| {
                let mut queue = Queue::default();
                queue.load(None, album(30), Start::Beginning, true);
                queue
                    .view(100)
                    .next_up
                    .iter()
                    .map(|t| t.uri.clone())
                    .collect()
            })
            .collect();
        assert!(orders[0] != orders[1] && orders[1] != orders[2]);
    }

    /// An artist of 25 top tracks and 120 more in their albums: Next plays
    /// all 145 before autoplay is asked for, shuffled or not.
    #[test]
    fn an_artists_whole_catalogue_plays_before_autoplay() {
        for shuffle in [false, true] {
            let mut queue = Queue::seeded(5);
            let artist = Some("tidal:artist:9".to_string());
            queue.load(artist, album(25), Start::Beginning, shuffle);
            assert!(queue.extend_context((26..=85).map(track).collect()));
            assert!(queue.extend_context((86..=145).map(track).collect()));
            let mut played = vec![queue.current().unwrap().uri.clone()];
            while let Advance::Track(next) = queue.advance(false) {
                played.push(next.uri);
            }
            assert_eq!(played.len(), 145, "shuffle {shuffle}");
            played.sort();
            played.dedup();
            assert_eq!(played.len(), 145, "no song twice, shuffle {shuffle}");
            assert_eq!(queue.ahead(), 0);
        }
    }

    /// A song with an ISRC of its own (`n`), or sharing `n`'s ISRC.
    fn recording(n: usize, id: usize) -> LocalTrack {
        LocalTrack {
            uri: format!("tidal:track:{id}"),
            title: format!("Song {n}"),
            isrc: Some(format!("ISRC{n}")),
            duration_ms: 180_000,
            ..LocalTrack::default()
        }
    }

    fn number(track: &LocalTrack) -> usize {
        track.uri.rsplit(':').next().unwrap().parse().unwrap()
    }

    fn play_all(queue: &mut Queue) -> Vec<LocalTrack> {
        let mut played = vec![queue.current().unwrap().clone()];
        while let Advance::Track(next) = queue.advance(false) {
            played.push(next);
        }
        played
    }

    #[test]
    fn a_shuffle_holds_each_recording_once_across_page_merges() {
        for seed in 0..20 {
            let mut queue = Queue::seeded(seed);
            // Recordings 1..=30; a second id (100 + n) shares each ISRC.
            let first: Vec<LocalTrack> = (1..=10)
                .map(|n| recording(n, n))
                .chain((1..=3).map(|n| recording(n, 100 + n)))
                .collect();
            queue.load(Some("tidal:artist:1".into()), first, Start::Beginning, true);
            let page = |range: std::ops::RangeInclusive<usize>| -> Vec<LocalTrack> {
                range
                    .flat_map(|n| [recording(n, n), recording(n, 200 + n)])
                    .collect()
            };
            queue.extend_context(page(8..=20));
            queue.extend_context(page(18..=30));
            let played = play_all(&mut queue);
            let mut ids: Vec<&str> = played.iter().map(|t| t.uri.as_str()).collect();
            let mut isrcs: Vec<&str> = played.iter().map(|t| t.isrc.as_deref().unwrap()).collect();
            ids.sort();
            ids.dedup();
            isrcs.sort();
            isrcs.dedup();
            assert_eq!(ids.len(), played.len(), "ids, seed {seed}");
            assert_eq!(isrcs.len(), played.len(), "isrcs, seed {seed}");
            assert_eq!(played.len(), 30, "every recording plays, seed {seed}");
        }
    }

    #[test]
    fn an_explicit_and_a_clean_version_without_isrcs_play_once() {
        let mut queue = Queue::seeded(3);
        let version = |id: usize, title: &str, ms: u32| LocalTrack {
            uri: format!("tidal:track:{id}"),
            title: title.into(),
            artists: vec![crate::api::models::ArtistRef {
                name: "Band".into(),
                ..Default::default()
            }],
            duration_ms: ms,
            ..LocalTrack::default()
        };
        let tracks = vec![
            version(1, "Tune (Explicit)", 200_000),
            version(2, "Tune", 201_000),
            version(3, "Tune (Live)", 200_000),
            version(4, "Other", 200_000),
        ];
        queue.load(None, tracks, Start::Beginning, true);
        assert_eq!(play_all(&mut queue).len(), 3);
    }

    fn play_n(queue: &mut Queue, count: usize) -> Vec<String> {
        let mut played = vec![queue.current().unwrap().uri.clone()];
        while played.len() < count {
            played.push(uri(queue.advance(true)));
        }
        played
    }

    #[test]
    fn a_repeat_all_round_does_not_open_with_the_songs_that_just_played() {
        for seed in 0..30 {
            let mut queue = Queue::seeded(seed);
            queue.set_repeat(RepeatMode::Context);
            queue.load(
                Some("tidal:album:1".into()),
                album(120),
                Start::Beginning,
                true,
            );
            let first_round = play_n(&mut queue, 120);
            // The preloaded song is the one that plays after the wrap.
            let peeked = uri(queue.peek());
            let wrapped = uri(queue.advance(true));
            assert_eq!(wrapped, peeked, "preload matches, seed {seed}");
            let last_50 = &first_round[70..];
            let mut round = vec![wrapped];
            for _ in 0..49 {
                round.push(uri(queue.advance(true)));
            }
            assert!(
                round.iter().all(|uri| !last_50.contains(uri)),
                "the next round opens with fresh songs, seed {seed}"
            );
        }
    }

    #[test]
    fn a_small_repeat_all_context_never_repeats_straight_away() {
        let mut queue = Queue::seeded(4);
        queue.set_repeat(RepeatMode::Context);
        queue.load(None, album(6), Start::Beginning, true);
        let mut previous = queue.current().unwrap().uri.clone();
        for _ in 0..60 {
            let next = uri(queue.advance(true));
            assert_ne!(next, previous);
            previous = next;
        }
    }

    #[test]
    fn a_new_shuffle_starts_on_a_song_not_played_lately() {
        let mut queue = Queue::seeded(21);
        let mut firsts = Vec::new();
        for _ in 0..50 {
            let first = queue
                .load(
                    Some("tidal:artist:9".into()),
                    album(100),
                    Start::Beginning,
                    true,
                )
                .unwrap();
            firsts.push(number(&first));
        }
        let mut distinct = firsts.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            50,
            "no first song twice in 50 presses: {firsts:?}"
        );
        assert!(distinct.iter().any(|n| *n <= 25) && distinct.iter().any(|n| *n > 75));
    }

    #[test]
    fn songs_played_lately_go_last_and_the_rest_start_uniformly() {
        let mut counts = [0u32; 10];
        for seed in 0..3000 {
            let mut queue = Queue::seeded(seed);
            // Songs 1..=10 played lately in a context of 20.
            for n in 1..=10 {
                queue.recent.note(Some("tidal:album:2"), &track(n));
            }
            let first = queue
                .load(
                    Some("tidal:album:2".into()),
                    album(20),
                    Start::Beginning,
                    true,
                )
                .unwrap();
            counts[number(&first) - 11] += 1;
            let order: Vec<usize> = queue.view(100).next_up.iter().map(number).collect();
            assert!(order[..9].iter().all(|n| *n > 10), "{order:?}");
            assert!(order[9..].iter().all(|n| *n <= 10), "{order:?}");
        }
        let chi = chi_square(&counts);
        assert!(chi < 30.0, "chi-square {chi}: {counts:?}");
    }

    #[test]
    fn later_pages_join_a_shuffle_with_recent_songs_last() {
        let mut queue = Queue::seeded(8);
        for n in [3, 14, 15] {
            queue.recent.note(Some("tidal:album:2"), &track(n));
        }
        queue.load(
            Some("tidal:album:2".into()),
            album(10),
            Start::Uri("tidal:track:1".into()),
            true,
        );
        queue.extend_context((11..=20).map(track).collect());
        let order: Vec<usize> = queue.view(100).next_up.iter().map(number).collect();
        assert_eq!(order.len(), 19);
        assert!(
            order[16..].iter().all(|n| [3, 14, 15].contains(n)),
            "{order:?}"
        );
    }

    #[test]
    fn the_memory_survives_a_restart() {
        let dir =
            std::env::temp_dir().join(format!("tidalfast-queue-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("recent_plays.json");
        let mut before = Queue::seeded(1);
        before.remember_in(path.clone());
        before.load(
            Some("tidal:album:3".into()),
            album(40),
            Start::Beginning,
            true,
        );
        let mut heard = vec![number(before.current().unwrap())];
        for _ in 0..9 {
            let Advance::Track(next) = before.advance(false) else {
                panic!()
            };
            heard.push(number(&next));
        }
        before.flush_recent();
        for seed in 0..40 {
            let mut after = Queue::seeded(seed);
            after.remember_in(path.clone());
            let first = after
                .load(
                    Some("tidal:album:3".into()),
                    album(40),
                    Start::Beginning,
                    true,
                )
                .unwrap();
            assert!(!heard.contains(&number(&first)), "seed {seed}: {heard:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoplay_leaves_out_what_played_lately() {
        let mut queue = loaded(5);
        let played = play_all(&mut queue);
        assert_eq!(played.len(), 5);
        let station: Vec<LocalTrack> = (3..=9).map(track).collect();
        assert!(queue.continue_with(station));
        let next: Vec<usize> = queue.view(20).next_up.iter().map(number).collect();
        assert_eq!(next, vec![6, 7, 8, 9]);
        // Nothing fresh: the music does not restart on old songs.
        let mut again = loaded(5);
        play_all(&mut again);
        assert!(!again.continue_with(album(5)));
        assert!(!again.is_autoplay());
    }

    /// A plugin took a song out of the playlist (No Remix): it is not played
    /// in this pass of the context, the rest plays on in order.
    #[test]
    fn a_skipped_context_song_is_never_played_in_this_pass() {
        let mut queue = loaded(5);
        assert!(queue.skip_context("tidal:track:3", 0));
        assert!(!queue.skip_context("tidal:track:3", 0), "already gone");
        assert!(
            !queue.skip_context("tidal:track:1", 0),
            "playing now, not upcoming"
        );
        let next_up: Vec<String> = queue.view(10).next_up.into_iter().map(|t| t.uri).collect();
        assert_eq!(next_up, ["tidal:track:2", "tidal:track:4", "tidal:track:5"]);
        let mut played = Vec::new();
        while let Advance::Track(track) = queue.advance(true) {
            played.push(track.uri);
        }
        assert_eq!(played, ["tidal:track:2", "tidal:track:4", "tidal:track:5"]);
    }

    /// Two rows of one song: the occurrence names which one goes.
    #[test]
    fn a_skipped_context_song_listed_twice_goes_by_occurrence() {
        let mut queue = Queue::seeded(1);
        let tracks = vec![track(1), track(2), track(3), track(2), track(4)];
        queue.load(
            Some("tidal:playlist:p".into()),
            tracks,
            Start::Beginning,
            false,
        );
        assert!(queue.skip_context("tidal:track:2", 1));
        let next_up: Vec<String> = queue.view(10).next_up.into_iter().map(|t| t.uri).collect();
        assert_eq!(next_up, ["tidal:track:2", "tidal:track:3", "tidal:track:4"]);
        assert!(
            !queue.skip_context("tidal:track:2", 1),
            "only one copy is left"
        );
    }

    /// With Repeat all the rows of the next round are listed after this
    /// one's; taking one out of the playing song's own place must not move
    /// the song after it.
    #[test]
    fn skipping_a_repeat_all_row_keeps_the_place() {
        let mut queue = loaded(4);
        queue.set_repeat(RepeatMode::Context);
        assert_eq!(uri(queue.advance(true)), "tidal:track:2");
        // Next up: 3, 4, then the wrap rows 1 and 2 (the playing one).
        assert!(queue.skip_context("tidal:track:2", 0));
        let next_up: Vec<String> = queue.view(10).next_up.into_iter().map(|t| t.uri).collect();
        assert_eq!(next_up, ["tidal:track:3", "tidal:track:4", "tidal:track:1"]);
        assert_eq!(uri(queue.advance(true)), "tidal:track:3");
        assert_eq!(uri(queue.advance(true)), "tidal:track:4");
        assert_eq!(uri(queue.advance(true)), "tidal:track:1");
    }

    /// 2,000 songs: a shuffle of the first 1,000 and 20 pages of 50.
    /// `cargo test --release -- --ignored shuffle_cost --nocapture`
    #[test]
    #[ignore = "a timing, not a rule"]
    fn shuffle_cost_of_a_2000_song_context() {
        let mut best = std::time::Duration::MAX;
        for round in 0..20 {
            let mut queue = Queue::seeded(round);
            let first = album(1000);
            let pages: Vec<Vec<LocalTrack>> = (0..20)
                .map(|page| (1001 + page * 50..1051 + page * 50).map(track).collect())
                .collect();
            let started = std::time::Instant::now();
            queue.load(Some("tidal:artist:9".into()), first, Start::Beginning, true);
            for page in pages {
                queue.extend_context(page);
            }
            best = best.min(started.elapsed());
            assert_eq!(queue.ahead(), 1999);
        }
        eprintln!("initial shuffle + 20 page merges (2000 songs): {best:?}");
    }
}
