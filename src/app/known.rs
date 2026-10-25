//! What the app knows of a song: one lookup over every list it holds, so
//! that a queued song shows the row it was queued from (its cover, title
//! and artists) wherever that row was, and a row that arrives with less is
//! completed rather than shown bare.

use super::*;
use crate::api::models::{Shelf, ShelfItem, TopHit, Video};

/// How much of a row `track` carries: a title, artists, a cover.
fn detail(track: &Track) -> u8 {
    u8::from(!track.name.is_empty())
        + u8::from(!track.artists.is_empty())
        + u8::from(
            track
                .album
                .as_ref()
                .is_some_and(|album| !album.images.is_empty()),
        )
}

/// A row with a title, artists and a cover lacks nothing.
const FULL: u8 = 3;

/// Whether the row `item` lacks its title, artists or cover.
pub(super) fn lacks_details(item: &PlayableItem) -> bool {
    detail(track_of(item)) < FULL
}

fn track_of(item: &PlayableItem) -> &Track {
    let PlayableItem::Track(track) = item;
    track
}

impl App {
    /// The fullest details the app holds for the song or music video
    /// `uri`: TIDAL's answer for it, the queue, and every page and list on
    /// hand (albums, playlists, mixes, an artist's songs, radios, Liked
    /// Songs, search, Home, history, Save for Later, Purchases, downloads).
    /// `None` when nothing held names it with more than its address.
    pub(super) fn known_item(&self, uri: &str) -> Option<Track> {
        self.fullest_item(uri)
    }

    /// The fullest row held for `uri`.
    fn fullest_item(&self, uri: &str) -> Option<Track> {
        let mut best = self.cached_item(uri);
        if let Some(track) = best.filter(|track| detail(track) == FULL) {
            return Some(track.clone());
        }
        for track in self.held_tracks().filter(|track| track.uri == uri) {
            if best.is_none_or(|best| detail(track) > detail(best)) {
                best = Some(track);
                if detail(track) == FULL {
                    return Some(track.clone());
                }
            }
        }
        // Videos and the engine's own song are kept in other shapes.
        let mut best = best.cloned();
        let others = self
            .held_video(uri)
            .map(|video| video.as_track())
            .into_iter()
            .chain(
                self.local
                    .track
                    .as_ref()
                    .filter(|track| track.uri == uri)
                    .map(crate::backend::track_from_local),
            );
        for track in others {
            if best
                .as_ref()
                .is_none_or(|best| detail(&track) > detail(best))
            {
                best = Some(track);
            }
        }
        best.filter(|track| detail(track) > 0)
    }

    /// `item`, or what the app knows of its song when that is fuller.
    fn completed_item(&self, item: PlayableItem) -> PlayableItem {
        let have = detail(track_of(&item));
        if have == FULL {
            return item;
        }
        match self.known_item(item.uri()) {
            Some(track) if detail(&track) > have => PlayableItem::Track(track),
            _ => item,
        }
    }

    /// Asks TIDAL for a queued song the app holds too little of, once at a
    /// time; the answer completes its rows ([`Self::complete_queue_rows`]).
    /// A song TIDAL turned down is asked for again when it is `queued`
    /// again, not on each read of the queue that lists it.
    pub(super) fn ask_row_details(&mut self, uri: &str, queued: bool) {
        let Some(id) = util::uri_id(uri).map(str::to_string) else {
            return;
        };
        let video = crate::tidal_engine::video::is_video_uri(uri);
        let key = if video {
            format!("video:{id}")
        } else if uri.starts_with("tidal:track:") {
            id.clone()
        } else {
            return;
        };
        if queued {
            self.details_refused.remove(&key);
        } else if self.details_refused.contains(&key) {
            return;
        }
        if self.track_cache.contains_key(&key) || !self.track_requests.insert(key) {
            return;
        }
        self.backend.api(if video {
            ApiRequest::Video { id }
        } else {
            ApiRequest::Track { id }
        });
    }

    /// `item` as a queue row: completed from what the app holds, and when
    /// that still lacks something, asked of TIDAL.
    pub(super) fn queue_row(&mut self, item: PlayableItem) -> PlayableItem {
        let item = self.completed_item(item);
        if lacks_details(&item) {
            let uri = item.uri().to_string();
            self.ask_row_details(&uri, true);
        }
        item
    }

    /// TIDAL's answer for a song completes every row of it on show and
    /// every addition still pending, where they had less; nothing moves.
    pub(super) fn complete_queue_rows(&mut self, track: &Track) {
        let fuller =
            |row: &PlayableItem| row.uri() == track.uri && detail(track_of(row)) < detail(track);
        if let Loadable::Loaded(queue) = &mut self.queue {
            for row in queue.currently_playing.iter_mut().chain(&mut queue.queue) {
                if fuller(row) {
                    *row = PlayableItem::Track(track.clone());
                }
            }
        }
        for addition in &mut self.pending_queue_adds {
            if fuller(&addition.item) {
                addition.item = PlayableItem::Track(track.clone());
            }
        }
    }

    /// A queue read from the player or TIDAL may list a song with less than
    /// the row on show (the engine is still looking it up): that row stays.
    /// An answer only completes a row, never blanks or shortens it. The
    /// playing song and *Playing next* are also completed from what the app
    /// knows, and asked of TIDAL when nothing here describes them; *Next up*
    /// is the context's own list, with its own details.
    pub(super) fn complete_fetched_queue(&mut self, fetched: &mut Queue) {
        let queued = Self::end_of_queued_rows(&fetched.queue, &self.manual_queue);
        let mut ask = Vec::new();
        {
            let mut shown: HashMap<&str, &PlayableItem> = HashMap::new();
            let on_show = self
                .queue
                .get()
                .into_iter()
                .flat_map(|queue| queue.currently_playing.iter().chain(&queue.queue))
                .chain(
                    self.pending_queue_adds
                        .iter()
                        .map(|addition| &addition.item),
                );
            for item in on_show {
                if shown
                    .get(item.uri())
                    .is_none_or(|have| detail(track_of(item)) > detail(track_of(have)))
                {
                    shown.insert(item.uri(), item);
                }
            }
            let rows = fetched
                .currently_playing
                .iter_mut()
                .map(|row| (row, true))
                .chain(
                    fetched
                        .queue
                        .iter_mut()
                        .enumerate()
                        .map(|(index, row)| (row, index < queued)),
                );
            for (row, own) in rows {
                let have = detail(track_of(row));
                if have == FULL {
                    continue;
                }
                let mut fuller = shown.get(row.uri()).map(|item| (*item).clone());
                if own
                    && fuller
                        .as_ref()
                        .is_none_or(|item| detail(track_of(item)) < FULL)
                {
                    let known = self.known_item(row.uri()).map(PlayableItem::Track);
                    if known.as_ref().is_some_and(|known| {
                        fuller
                            .as_ref()
                            .is_none_or(|item| detail(track_of(known)) > detail(track_of(item)))
                    }) {
                        fuller = known;
                    }
                }
                if let Some(item) = fuller.filter(|item| detail(track_of(item)) > have) {
                    *row = item;
                }
                if own && detail(track_of(row)) < FULL {
                    ask.push(row.uri().to_string());
                }
            }
        }
        for uri in ask {
            self.ask_row_details(&uri, false);
        }
    }

    /// Every song row the app holds, as borrowed tracks.
    fn held_tracks(&self) -> impl Iterator<Item = &Track> + '_ {
        fn paged<T>(list: &PagedList<T>) -> impl Iterator<Item = &T> {
            list.items.iter().chain(list.windows.values().flatten())
        }
        fn shelves(shelves: Option<&[Shelf]>) -> impl Iterator<Item = &Track> {
            shelves
                .into_iter()
                .flatten()
                .flat_map(|shelf| &shelf.items)
                .filter_map(|item| match item {
                    ShelfItem::Track(track) => Some(track),
                    _ => None,
                })
        }
        let queue = self
            .queue
            .get()
            .into_iter()
            .flat_map(|queue| queue.currently_playing.iter().chain(&queue.queue))
            .chain(
                self.pending_queue_adds
                    .iter()
                    .map(|addition| &addition.item),
            )
            .map(track_of);
        let playlists = self.playlist_pages.values().flat_map(|page| {
            paged(&page.items)
                .chain(&page.optimistic_tail)
                .filter_map(PlaylistItem::playable)
                .map(track_of)
                .chain(page.recommendations.iter().flat_map(|songs| &songs.songs))
        });
        let pages = self
            .album_pages
            .values()
            .flat_map(|page| paged(&page.tracks))
            .chain(self.mix_pages.values().flat_map(|page| paged(&page.tracks)))
            .chain(
                self.artist_top_pages
                    .values()
                    .flat_map(|page| paged(&page.tracks)),
            )
            .chain(self.artist_pages.values().flat_map(|page| {
                page.top_tracks
                    .get()
                    .into_iter()
                    .flat_map(|tracks| tracks.iter())
                    .chain(page.credits_all.get().into_iter().flatten())
                    .chain(
                        page.modules
                            .get()
                            .into_iter()
                            .flat_map(|modules| &modules.credits),
                    )
            }))
            .chain(
                self.radio_pages
                    .values()
                    .flat_map(|page| page.songs.get().into_iter().flatten()),
            );
        let search = self.search.results.get().into_iter().flat_map(|results| {
            let pages = [&results.tracks, &results.uploads]
                .into_iter()
                .flatten()
                .flat_map(|page| &page.items);
            let hits =
                results
                    .top_hit
                    .iter()
                    .chain(&results.top_hits)
                    .filter_map(|hit| match hit {
                        TopHit::Track(track) | TopHit::Upload(track) => Some(track),
                        _ => None,
                    });
            pages.chain(hits)
        });
        // The dropdown's direct hits, under the search field.
        let dropdown = self
            .search
            .suggestions
            .iter()
            .flat_map(|found| &found.direct_hits)
            .filter_map(|hit| match hit {
                TopHit::Track(track) | TopHit::Upload(track) => Some(track),
                _ => None,
            });
        let home = &self.home;
        let home = home
            .top_tracks
            .get()
            .into_iter()
            .flat_map(|tracks| tracks.iter())
            .chain(home.top_songs.get().into_iter().flatten())
            .chain(
                home.recommendations
                    .get()
                    .into_iter()
                    .flat_map(|tracks| tracks.iter()),
            )
            .chain(
                home.recently_played
                    .get()
                    .into_iter()
                    .flat_map(|plays| plays.iter())
                    .map(|play| &play.track),
            )
            .chain(shelves(
                home.shelves.get().map(|shelves| shelves.as_slice()),
            ))
            .chain(
                self.shelf_pages
                    .values()
                    .chain(self.home_tabs.values())
                    .flat_map(|page| shelves(page.get().map(Vec::as_slice))),
            );
        let liked = paged(&self.library.liked).map(|saved| &saved.track);
        // TIDAL's plays, and the list on show with the plays made here.
        let recents = self
            .recents
            .items
            .iter()
            .chain(self.recents_view.iter())
            .map(|play| &play.track);
        let lists = liked
            .chain(self.queue_history.iter())
            .chain(recents)
            .chain(
                self.save_later
                    .details
                    .values()
                    .filter_map(|saved| match saved {
                        crate::save_later::Saved::Track(track) => Some(track),
                        _ => None,
                    }),
            )
            .chain(
                self.purchases
                    .get()
                    .into_iter()
                    .flat_map(|bought| &bought.tracks),
            )
            .chain(self.downloads.items.iter().map(|(item, _)| track_of(item)));
        queue
            .chain(playlists)
            .chain(pages)
            .chain(search)
            .chain(dropdown)
            .chain(home)
            .chain(lists)
    }

    /// The music video `uri` from a page or list that holds it.
    fn held_video(&self, uri: &str) -> Option<&Video> {
        let id = uri.strip_prefix("tidal:video:")?;
        fn shelves(shelves: Option<&[Shelf]>) -> impl Iterator<Item = &Video> {
            shelves
                .into_iter()
                .flatten()
                .flat_map(|shelf| &shelf.items)
                .filter_map(|item| match item {
                    ShelfItem::Video(video) => Some(video),
                    _ => None,
                })
        }
        let search = self.search.results.get().into_iter().flat_map(|results| {
            results.videos.iter().flat_map(|page| &page.items).chain(
                results
                    .top_hit
                    .iter()
                    .chain(&results.top_hits)
                    .filter_map(|hit| match hit {
                        TopHit::Video(video) => Some(video),
                        _ => None,
                    }),
            )
        });
        let mut videos = self
            .artist_pages
            .values()
            .flat_map(|page| {
                page.videos
                    .items
                    .iter()
                    .chain(page.videos.windows.values().flatten())
            })
            .chain(search)
            .chain(shelves(
                self.home.shelves.get().map(|shelves| shelves.as_slice()),
            ))
            .chain(
                self.shelf_pages
                    .values()
                    .chain(self.home_tabs.values())
                    .flat_map(|page| shelves(page.get().map(Vec::as_slice))),
            )
            .chain(
                self.save_later
                    .details
                    .values()
                    .filter_map(|saved| match saved {
                        crate::save_later::Saved::Video(video) => Some(video),
                        _ => None,
                    }),
            );
        videos.find(|video| video.id == id)
    }
}
