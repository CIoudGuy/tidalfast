//! Sample data for screenshots and headless rendering tests.
//!
//! Nothing here talks to TIDAL: the backend is switched offline, the
//! session is marked connected, and every page is filled with plausible
//! content. Cover art comes from a public placeholder service so the artwork
//! pipeline (download, disk cache, accent colour) is exercised too.

use std::sync::Arc;
use std::time::Instant;

use jiff::{SignedDuration, Timestamp};

use crate::api::models::{
    Album, AlbumShelf, AlbumShelfKind, Artist, ArtistRef, Context, Copyright, Device, Followers,
    Image, Mix, Owner, Page as ApiPage, PlayHistory, PlayableItem, PlaybackState, Playlist,
    PlaylistItem, Queue, SavedAlbum, SavedTrack, SearchResults, Shelf, ShelfItem, Track,
    TrackCount, User,
};
use crate::app::{App, RemoteSnapshot};
use crate::backend::AuthStatus;
use crate::model::*;

fn image(seed: u32) -> Vec<Image> {
    // `TIDALFAST_DEMO_COVER=<TIDAL image id, as a/b/c/d/e>`: every cover is
    // that TIDAL cover, for plugins that only act on TIDAL's image server
    // (3d-cover).
    if let Ok(id) = std::env::var("TIDALFAST_DEMO_COVER") {
        return [640u32, 320, 80]
            .iter()
            .map(|&s| Image {
                url: format!("https://resources.tidal.com/images/{id}/{s}x{s}.jpg"),
                width: Some(s),
                height: Some(s),
            })
            .collect();
    }
    vec![
        Image {
            url: format!("https://picsum.photos/seed/tidalfast{seed}/640/640"),
            width: Some(640),
            height: Some(640),
        },
        Image {
            url: format!("https://picsum.photos/seed/tidalfast{seed}/300/300"),
            width: Some(300),
            height: Some(300),
        },
        Image {
            url: format!("https://picsum.photos/seed/tidalfast{seed}/64/64"),
            width: Some(64),
            height: Some(64),
        },
    ]
}

const ARTISTS: &[&str] = &[
    "Bonobo",
    "Khruangbin",
    "Nils Frahm",
    "Little Simz",
    "Floating Points",
    "Jon Hopkins",
    "Sault",
    "Four Tet",
];

const ALBUMS: &[(&str, usize, &str)] = &[
    ("Fragments", 0, "2022"),
    ("Mordechai", 1, "2020"),
    ("All Melody", 2, "2018"),
    ("Sometimes I Might Be Introvert", 3, "2021"),
    ("Promises", 4, "2021"),
    ("Immunity", 5, "2013"),
    ("Untitled (Black Is)", 6, "2020"),
    ("There Is Love in You", 7, "2010"),
];

const TRACKS: &[&str] = &[
    "Rosewood",
    "Otomo",
    "Shadows",
    "Tides",
    "Elysian",
    "Closer",
    "Counterpart",
    "Sapien",
    "From You",
    "Day by Day",
    "Age of Phase",
    "Polyghost",
    "Time Moves Slow",
    "August 10",
    "So Rare",
    "Fugue",
    "Encores",
    "Sunlight",
    "My Friend the Forest",
    "Kaleidoscope",
];

/// Invented Hebrew and Arabic titles for `--demo-show rtl`: whole lines,
/// lines mixed with English, numbers, brackets, and punctuation.
#[cfg(any(test, feature = "demo"))]
const RTL_TRACKS: &[(&str, &str, &str)] = &[
    ("שיר ישן (גרסה חיה)", "להקת הים", "גלים, 2024"),
    ("Song 12 שיר ישן, part 3", "Kasia & נועה", "Sessions: חלק ב"),
    ("غيوم في السماء (Live) 2024", "فرقة الغيوم", "السماء"),
    ("ليل طويل، الجزء الأول", "نور", "رحلة 7"),
    ("Tel Aviv Nights: לילות, חלק 2", "Sam & דנה", "Nights"),
    ("مدينة [Remix]", "فرقة الغيوم", "Remixes: مدينة"),
];

fn demo_added_at(index: usize, now: Timestamp) -> String {
    let age = match index {
        0 => SignedDuration::from_secs(30),
        1 => SignedDuration::from_mins(5),
        2 => SignedDuration::from_hours(3),
        3 => SignedDuration::from_hours(2 * 24),
        4 => SignedDuration::from_hours(2 * 7 * 24),
        _ => SignedDuration::from_hours((35 + index as i64) * 24),
    };
    (now - age).to_string()
}

const PLAYLISTS: &[&str] = &[
    "My Daily Discovery",
    "Late night focus",
    "Sunday morning",
    "Running 2026",
    "My New Arrivals",
    "Berlin nights",
    "Dinner party",
    "Deep work",
    "Road trip",
    "Kitchen jams",
];

fn artist_ref(index: usize) -> ArtistRef {
    ArtistRef {
        id: Some(format!("art{index}")),
        name: ARTISTS[index % ARTISTS.len()].to_string(),
        uri: Some(format!("tidal:artist:art{index}")),
        picture: None,
    }
}

fn artist(index: usize) -> Artist {
    Artist {
        id: format!("art{index}"),
        name: ARTISTS[index % ARTISTS.len()].to_string(),
        uri: format!("tidal:artist:art{index}"),
        images: image(100 + index as u32),
        genres: vec!["electronic".into(), "downtempo".into(), "ambient".into()],
        followers: Some(Followers {
            total: 1_284_930 + index as u64 * 10_431,
        }),
        popularity: Some(70),
        ..Artist::default()
    }
}

/// A demo artist (`art3`) as TIDAL's v1 artist JSON, for the off-screen
/// Luna demo: plugins asking who sings the next song (up-next) get the name
/// a signed-in session would.
pub fn artist_json(id: &str) -> Option<serde_json::Value> {
    let index: usize = id.strip_prefix("art")?.parse().ok()?;
    let a = artist(index);
    Some(serde_json::json!({ "id": a.id, "name": a.name, "picture": null }))
}

/// TIDAL's `search` answer in the off-screen Luna demo: the demo songs
/// whose titles the query names, each with a made-up music video of the
/// same title (MusicVideoButton looks a song's video up this way).
pub fn search_json(query: &str) -> serde_json::Value {
    let query = query.to_lowercase();
    let found: Vec<(usize, Track)> = (0..TRACKS.len())
        .map(|i| (i, track(i)))
        .filter(|(_, t)| query.contains(&t.name.to_lowercase()))
        .collect();
    let item = |id: String, t: &Track| {
        serde_json::json!({
            "id": id,
            "title": t.name,
            "version": null,
            "duration": t.duration_ms / 1000,
            "artists": t.artists.iter().map(|a| serde_json::json!({ "id": a.id, "name": a.name })).collect::<Vec<_>>(),
        })
    };
    let tracks: Vec<_> = found
        .iter()
        .map(|(_, t)| item(t.id.clone().unwrap_or_default(), t))
        .collect();
    let videos: Vec<_> = found
        .iter()
        .map(|(i, t)| item(format!("vid{i}"), t))
        .collect();
    serde_json::json!({
        "tracks": { "items": tracks, "totalNumberOfItems": tracks.len() },
        "videos": { "items": videos, "totalNumberOfItems": videos.len() },
    })
}

/// A demo music video as TIDAL's `videos/<id>` answers it (`vid3`), for
/// the off-screen Luna demo.
pub fn video_json(id: &str) -> Option<serde_json::Value> {
    let index: usize = id.strip_prefix("vid")?.parse().ok()?;
    let t = track(index);
    Some(serde_json::json!({
        "id": id,
        "title": t.name,
        "type": "Music Video",
        "duration": t.duration_ms / 1000,
        "artists": t.artists.iter().map(|a| serde_json::json!({ "id": a.id, "name": a.name, "type": "MAIN" })).collect::<Vec<_>>(),
        "streamReady": true,
        "allowStreaming": true,
    }))
}

/// A demo album as TIDAL's `albums/<id>` answers it, for the off-screen
/// Luna demo (SongReleaseDate reads `releaseDate` from it).
pub fn album_json(id: &str) -> Option<serde_json::Value> {
    let index: usize = id.strip_prefix("alb")?.parse().ok()?;
    let a = album(index);
    Some(serde_json::json!({
        "id": a.id,
        "title": a.name,
        "releaseDate": a.release_date,
        "streamStartDate": a.release_date.as_ref().map(|d| format!("{d}T00:00:00.000+0000")),
        "numberOfTracks": a.total_tracks,
        "copyright": a.copyrights.first().map(|c| c.text.clone()),
    }))
}

fn album(index: usize) -> Album {
    let (name, artist_index, year) = ALBUMS[index % ALBUMS.len()];
    Album {
        id: format!("alb{index}"),
        name: name.to_string(),
        uri: format!("tidal:album:alb{index}"),
        album_type: Some(if index % 4 == 3 {
            "single".into()
        } else {
            "album".into()
        }),
        total_tracks: Some(12),
        images: image(200 + index as u32),
        artists: vec![artist_ref(artist_index)],
        release_date: Some(format!("{year}-03-1{}", index % 9)),
        label: Some("Ninja Tune".into()),
        copyrights: vec![Copyright {
            text: format!("{year} Ninja Tune"),
            kind: "C".into(),
        }],
        quality_tags: if index.is_multiple_of(3) {
            vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()]
        } else {
            vec!["LOSSLESS".into()]
        },
        // An animated cover (a TIDAL `videoCover` id) on the first album,
        // for measuring what one costs.
        video_cover: (index == 0)
            .then(|| std::env::var("TIDALFAST_DEMO_VIDEO_COVER").ok())
            .flatten(),
        // Some are the explicit version, with TIDAL's E on their cards.
        explicit: index % 3 == 1,
        ..Album::default()
    }
}

fn video(index: usize) -> crate::api::models::Video {
    crate::api::models::Video {
        id: format!("vid{index}"),
        name: format!("Live from the studio, part {}", index + 1),
        artist: ARTISTS[index % ARTISTS.len()].into(),
        duration: 180 + index as u32 * 37,
        images: image(700 + index as u32),
        explicit: index.is_multiple_of(3),
        ..Default::default()
    }
}

fn mix(index: usize) -> Mix {
    Mix {
        id: format!("mix{index}"),
        name: format!("My Mix {}", index + 1),
        uri: format!("tidal:mix:mix{index}"),
        subtitle: Some(format!(
            "{}, {} and more",
            ARTISTS[index % ARTISTS.len()],
            ARTISTS[(index + 3) % ARTISTS.len()]
        )),
        mix_type: Some("DAILY_MIX".into()),
        images: image(500 + index as u32),
        title_color: Some(["#DE6859", "#33FFEE", "#FFD432", "#B987FF"][index % 4].into()),
    }
}

/// An artist biography in TIDAL's markup, with links to an artist and an album.
const DEMO_BIO: &str = "Apart from being one of the best-selling artists in music history, [wimpLink artistId=\"art0\"]Bonobo[/wimpLink] is one of the greatest voices of her generation. She is effortlessly fast, fluid and unpredictable, capable of pulling off long-form narratives, and her commercial peak came around the time of [wimpLink albumId=\"alb1\"]Mordechai[/wimpLink].<br/><br/>Born in a small coastal town, she spent her childhood between two cities and began writing songs with a school friend. Later work with [wimpLink artistId=\"art1\"]Khruangbin[/wimpLink] and [wimpLink artistId=\"art2\"]Nils Frahm[/wimpLink] took her to larger stages.<br/><br/>Her most recent records turned toward quieter, more personal material, and critics have praised their craft. A long run of sold-out shows followed, and the band kept touring through the next year.";

/// The artist the demo account uploads as.
pub const UPLOADER: &str = "upl0";

/// The demo account's uploads as TIDAL's catalogue lists them: an album
/// "Demo Sessions" with two songs (explicit, analysed), and an unlisted single
/// whose song TIDAL's own profile leaves out.
fn uploader_profile(app: &mut App) {
    let artist = Artist {
        id: UPLOADER.into(),
        name: "Carmine".into(),
        uri: format!("tidal:artist:{UPLOADER}"),
        images: image(1),
        ..Artist::default()
    };
    let by = ArtistRef {
        id: Some(UPLOADER.into()),
        name: artist.name.clone(),
        uri: Some(artist.uri.clone()),
        picture: None,
    };
    let release = |id: &str, name: &str, kind: &str, date: &str, tracks: u32| Album {
        id: id.into(),
        name: name.into(),
        uri: format!("tidal:album:{id}"),
        album_type: Some(kind.into()),
        total_tracks: Some(tracks),
        images: image(260 + id.len() as u32),
        artists: vec![by.clone()],
        release_date: Some(date.into()),
        // An upload's audio quality, with no tags: TIDAL's LOW.
        quality_tags: vec!["HIGH".into()],
        upload: true,
        ..Album::default()
    };
    let untitled = release("upl-alb0", "Demo Sessions", "album", "2026-03-14", 2);
    let single = release("upl-alb1", "Basement Tapes", "single", "2026-01-28", 1);
    let song = |id: &str, name: &str, album: &Album, number: u32, seconds: u32| Track {
        id: Some(id.into()),
        name: name.into(),
        uri: format!("tidal:track:{id}"),
        duration_ms: seconds * 1000,
        artists: vec![by.clone()],
        album: Some(Album {
            tracks: None,
            ..album.clone()
        }),
        track_number: Some(number),
        disc_number: Some(1),
        upload: true,
        ..Track::default()
    };
    let mut first = song("upl-trk0", "Night drive", &untitled, 2, 201);
    first.explicit = true;
    first.audio_analysis = Some(crate::api::models::AudioAnalysis {
        bpm: Some(128),
        key: Some("F".into()),
        key_scale: Some("MINOR".into()),
    });
    let second = song("upl-trk1", "Morning take", &untitled, 1, 165);
    let hidden = song("upl-trk2", "Basement freestyle", &single, 1, 186);
    let mut artist_page = ArtistPage {
        artist: Loadable::Loaded(artist),
        top_tracks: Loadable::Loaded(Arc::new(vec![hidden, first.clone(), second.clone()])),
        related: Loadable::Loaded(Vec::new()),
        bio: Loadable::Loaded(None),
        followers: Loadable::Loaded(Some(3)),
        modules: Loadable::Loaded(Default::default()),
        ..ArtistPage::default()
    };
    for (filter, releases) in [
        (DiscographyFilter::Albums, vec![untitled.clone()]),
        (DiscographyFilter::Singles, vec![single]),
        (DiscographyFilter::AppearsOn, Vec::new()),
    ] {
        let mut list = PagedList::default();
        list.absorb(0, page(releases));
        artist_page.albums.insert(filter.groups().to_string(), list);
    }
    artist_page.videos.absorb(0, page(Vec::new()));
    app.artist_pages.insert(UPLOADER.into(), artist_page);
    // Its album opens as an album page, as any other.
    let mut album_page = AlbumPage {
        album: Loadable::Loaded(untitled),
        shelves: Loadable::Loaded(Vec::new()),
        review: Loadable::Loaded(None),
        credits: Loadable::Loaded(std::sync::Arc::new(AlbumCredits::default())),
        ..AlbumPage::default()
    };
    album_page.tracks.absorb(0, page(vec![second, first]));
    app.album_pages.insert("upl-alb0".into(), album_page);
}

fn track(index: usize) -> Track {
    let album_index = index % ALBUMS.len();
    let mut album = album(album_index);
    album.tracks = None;
    // As TIDAL's v1 tracks: `audioQuality` LOSSLESS, HiRes in the tags.
    let quality_tags = album.quality_tags.clone();
    Track {
        id: Some(format!("trk{index}")),
        name: TRACKS[index % TRACKS.len()].to_string(),
        uri: format!("tidal:track:trk{index}"),
        duration_ms: 180_000 + (index as u32 * 37_000) % 240_000,
        explicit: index % 7 == 3,
        artists: vec![artist_ref(album_index)],
        album: Some(album),
        track_number: Some((index % 12) as u32 + 1),
        disc_number: Some(1),
        popularity: Some(60 + (index % 40) as u8),
        audio_analysis: Some(crate::api::models::AudioAnalysis {
            bpm: Some(70 + (index as u32 * 17) % 110),
            key: Some(["C", "Eb", "CSharp", "A", "Bb", "FSharp", "G"][index % 7].into()),
            key_scale: Some(if index % 3 == 1 { "MAJOR" } else { "MINOR" }.into()),
        }),
        audio_quality: Some("LOSSLESS".into()),
        quality_tags,
        ..Track::default()
    }
}

const DEMO_REVIEW: &str = "Recorded over two winters in a borrowed studio, [wimpLink albumId=\"alb0\"]Fragments[/wimpLink] finds [wimpLink artistId=\"art0\"]Bonobo[/wimpLink] trading the dance floor for something slower and more patient. Strings drift in and out of the mix, and the rhythm section stays loose enough to let a pad or a flute take the lead for a few bars before it drops back.<br/><br/>The guests are used sparingly: [wimpLink artistId=\"art1\"]Khruangbin[/wimpLink] lends a guitar line to the middle of the record, and [wimpLink artistId=\"art2\"]Nils Frahm[/wimpLink] plays piano on the closer. It is a quiet album that rewards headphones, and one of the most coherent of the producer's career.";

/// The shelves under the demo album's footer, one of each kind, with the
/// page behind each View all filled in (`pages/single-module-page/demo/...`).
fn demo_album_shelves(app: &mut App) -> Vec<AlbumShelf> {
    let albums = |range: std::ops::Range<usize>| -> Vec<ShelfItem> {
        range.map(|i| ShelfItem::Album(album(i))).collect()
    };
    let make = |kind, name: &str, kind_name: &str, items: Vec<ShelfItem>, artist: Option<&str>| {
        let path = format!("pages/single-module-page/demo/{name}");
        AlbumShelf {
            kind,
            artist_id: artist.map(str::to_string),
            shelf: Shelf {
                title: kind_name.into(),
                kind: if matches!(kind, AlbumShelfKind::RelatedArtists) {
                    "ARTIST_LIST".into()
                } else {
                    "ALBUM_LIST".into()
                },
                items,
                more: Some(path),
                ..Default::default()
            },
        }
    };
    let shelves = vec![
        make(
            AlbumShelfKind::OtherVersions,
            "versions",
            "Other versions",
            albums(5..8),
            None,
        ),
        make(
            AlbumShelfKind::MoreByArtist,
            "more-by",
            "More Albums by Bonobo",
            albums(1..9),
            Some("art0"),
        ),
        make(
            AlbumShelfKind::RelatedAlbums,
            "related-albums",
            "Related Albums",
            albums(2..12),
            None,
        ),
        make(
            AlbumShelfKind::RelatedArtists,
            "related-artists",
            "Related Artists",
            (1..11).map(|i| ShelfItem::Artist(artist(i))).collect(),
            None,
        ),
    ];
    // The pages behind View all hold the same cards in full.
    for entry in &shelves {
        if let Some(path) = &entry.shelf.more {
            app.shelf_pages
                .insert(path.clone(), Loadable::Loaded(vec![entry.shelf.clone()]));
        }
    }
    shelves
}

/// An album's credits, as `--demo-show album-credits` lists them.
fn demo_album_credits() -> AlbumCredits {
    let person = |name: &str, id: Option<&str>| ArtistRef {
        id: id.map(str::to_string),
        name: name.to_string(),
        ..Default::default()
    };
    let credit = |role: &str, people: Vec<ArtistRef>| Credit {
        role: role.to_string(),
        contributors: people,
    };
    AlbumCredits {
        album: vec![
            credit("Primary Artist", vec![person("Bonobo", Some("art0"))]),
            credit("Executive Producer", vec![person("Peter Roberts", None)]),
            credit("Record Label", vec![person("Ninja Tune", None)]),
        ],
        tracks: (0..6)
            .map(|index| {
                let mut roles = vec![
                    credit("Producer", vec![person("Bonobo", Some("art0"))]),
                    credit(
                        "Composer",
                        vec![
                            person("Simon Green", None),
                            person("Nils Frahm", Some("art2")),
                        ],
                    ),
                ];
                // Songs with more roles than others, so the cards of a row
                // differ in height as TIDAL's do.
                if index % 3 != 2 {
                    roles.push(credit(
                        "AdditionalProducer",
                        vec![person("Floating Points", Some("art4"))],
                    ));
                    roles.push(credit("Mixing Engineer", vec![person("Ben Baptie", None)]));
                }
                if index % 2 == 1 {
                    roles.push(credit(
                        "Featured Artist",
                        vec![
                            person("Andreya Triana", Some("art3")),
                            person("Szjerdene", None),
                            person("Grey Reverend", None),
                        ],
                    ));
                }
                AlbumTrackCredits {
                    volume: 1,
                    number: index + 1,
                    title: TRACKS[index as usize].to_string(),
                    roles,
                    track: Some(Track {
                        artists: vec![artist_ref(0)],
                        explicit: index % 2 == 1,
                        ..track(index as usize)
                    }),
                }
            })
            .collect(),
    }
}

fn playlist(index: usize) -> Playlist {
    let name = PLAYLISTS[index % PLAYLISTS.len()];
    let tidal_owned = matches!(name, "My Daily Discovery" | "My New Arrivals");
    Playlist {
        id: format!("pl{index}"),
        name: name.to_string(),
        uri: format!("tidal:playlist:pl{index}"),
        description: Some(if tidal_owned {
            "Your weekly mixtape of fresh music. Enjoy new music and deep cuts picked for you. Updates every Monday.".into()
        } else {
            String::new()
        }),
        images: image(300 + index as u32),
        owner: Owner {
            id: Some(if tidal_owned {
                "tidal".into()
            } else {
                "demo".into()
            }),
            display_name: Some(if tidal_owned {
                "TIDAL".into()
            } else {
                "Carmine".into()
            }),
            uri: None,
        },
        public: Some(index.is_multiple_of(2)),
        collaborative: false,
        snapshot_id: Some("snap".into()),
        tracks: Some(TrackCount {
            total: 30 + index as u32 * 7,
        }),
        followers: Some(Followers {
            total: if tidal_owned {
                1_845_680
            } else {
                12_345 + index as u64
            },
        }),
        ..Playlist::default()
    }
}

fn page<T>(items: Vec<T>) -> ApiPage<T> {
    let total = items.len() as u32;
    ApiPage {
        items,
        total,
        limit: total,
        offset: 0,
        next: None,
    }
}

/// Songs in each playlist the `session` benchmark opens: a big playlist.
pub const BENCH_PLAYLIST_SONGS: usize = 550;

/// The `session` benchmark's playlist `index`, `bench{index}`: 550 songs
/// whose covers come from a pool of 1,500, so playlists share some covers
/// as a real library does. Put back whenever the page cache has let it go,
/// which is what a refetch from TIDAL would do.
pub fn bench_playlist(app: &mut App, index: usize) -> Page {
    let id = format!("bench{index}");
    if !app.playlist_pages.contains_key(&id) {
        let mut playlist = playlist(index % PLAYLISTS.len());
        playlist.id.clone_from(&id);
        playlist.uri = format!("tidal:playlist:{id}");
        playlist.name = format!("Bench {index}");
        playlist.tracks = Some(TrackCount {
            total: BENCH_PLAYLIST_SONGS as u32,
        });
        let now = Timestamp::now();
        let items = (0..BENCH_PLAYLIST_SONGS)
            .map(|row| {
                let mut track = track(index * BENCH_PLAYLIST_SONGS + row);
                track.name = format!("{} {}", track.name, row + 1);
                if let Some(album) = &mut track.album {
                    album.images = image(5000 + ((index * 131 + row) % 1500) as u32);
                }
                PlaylistItem {
                    added_at: Some(demo_added_at(row, now)),
                    is_local: false,
                    added_by: None,
                    item: Some(PlayableItem::Track(track)),
                    track: None,
                }
            })
            .collect();
        let mut page_data = PlaylistPage {
            playlist: Loadable::Loaded(playlist),
            ..PlaylistPage::default()
        };
        page_data.items.absorb(0, page(items));
        app.playlist_pages.insert(id.clone(), page_data);
    }
    Page::Playlist(id)
}

/// Populates the offline app with cards for screenshots and UI tests.
/// My Activity with made-up counts: this month and three before it, the
/// leader of this month without a picture (TIDAL's microphone disc).
/// Home's Staff Picks and Uploads tabs, shaped like TIDAL's `editorial` and
/// `uploads` feeds: card shelves, the three-column song grid with its
/// subtitle and (i), the upload badges and the wide Featured cards.
fn home_feed_tabs(app: &mut App, tracks: &[Track]) {
    use crate::api::models::{CardMark, Promo, ShelfLayout};
    let songs = |from: usize| -> Vec<ShelfItem> {
        tracks
            .iter()
            .skip(from)
            .take(9)
            .cloned()
            .map(ShelfItem::Track)
            .collect()
    };
    let playlists = |title: &str| Shelf {
        title: title.into(),
        items: (0..PLAYLISTS.len())
            .map(|i| ShelfItem::Playlist(playlist(i)))
            .collect(),
        more: Some("pages/demo".into()),
        ..Default::default()
    };
    let albums = |title: &str, marks: Vec<CardMark>| Shelf {
        title: title.into(),
        items: (0..ALBUMS.len())
            .map(|i| ShelfItem::Album(album(i)))
            .collect(),
        more: Some("pages/demo".into()),
        marks,
        ..Default::default()
    };
    let upload = CardMark {
        upload: true,
        ..Default::default()
    };
    let spotlighted = CardMark {
        spotlighted: true,
        ..Default::default()
    };
    let purchase = CardMark {
        upload: true,
        purchase: true,
        ..Default::default()
    };
    let editorial = vec![
        playlists("Playlists we just updated"),
        Shelf {
            title: "Songs on repeat".into(),
            items: songs(0),
            layout: ShelfLayout::Compact,
            more: Some("pages/demo".into()),
            ..Default::default()
        },
        playlists("Playlists we love"),
        albums("Albums with no skips", Vec::new()),
    ];
    let promo = |kicker: &str, title: &str, text: &str, index: u32| Promo {
        kicker: kicker.into(),
        title: title.into(),
        text: text.into(),
        image: image(300 + index).first().map(|image| image.url.clone()),
        item: ShelfItem::Album(album(index as usize % ALBUMS.len())),
    };
    let uploads = vec![
        albums("Uploaded albums", vec![upload; ALBUMS.len()]),
        Shelf {
            title: "Featured".into(),
            promos: vec![
                promo(
                    "Listen",
                    "Upload Headliners: The Winners",
                    "Listen to the 10 tracks that came out on top.",
                    0,
                ),
                promo(
                    "Upload Headliners",
                    "Annisse",
                    "The pop/R&B vocalist wows with \u{201c}Constellations\u{201d}",
                    1,
                ),
                promo(
                    "Upload Headliners",
                    "Mo J. Simpson",
                    "The H-Town MC offers \u{201c}Deans List\u{201d}",
                    2,
                ),
                promo("Listen", "Another featured card", "Text under it.", 3),
            ],
            more: Some("pages/demo".into()),
            ..Default::default()
        },
        Shelf {
            title: "Uploads for you".into(),
            subtitle: Some(
                "Independent artists recommended based on what you love to listen to".into(),
            ),
            items: songs(9),
            layout: ShelfLayout::Compact,
            marks: vec![upload; 9],
            more: Some("pages/demo".into()),
            ..Default::default()
        },
        Shelf {
            title: "Spotlighted Uploads".into(),
            subtitle: Some(
                "Independent tracks selected by Tidal's editorial team. Listen now for free."
                    .into(),
            ),
            info: true,
            items: songs(18),
            layout: ShelfLayout::Compact,
            marks: vec![spotlighted; 9],
            more: Some("pages/demo".into()),
            ..Default::default()
        },
        albums("Uploads for purchase", vec![purchase; ALBUMS.len()]),
    ];
    app.home_tabs
        .insert("editorial".into(), Loadable::Loaded(editorial));
    app.home_tabs
        .insert("uploads".into(), Loadable::Loaded(uploads));
}

fn activity(app: &mut App) {
    use crate::activity::{MonthKey, MonthReport, RankedArtist};
    let today = jiff::civil::date(2026, 10, 14);
    app.activity_fixed_day = Some(today);
    let colors = [
        [0x80, 0x24, 0x24],
        [0x24, 0x50, 0x80],
        [0x6a, 0x5a, 0x20],
        [0x40, 0x70, 0x48],
    ];
    let month_report = |offset: usize, top: u64, days_left: Option<u32>| MonthReport {
        artists: (0..crate::activity::TOP_ARTISTS)
            .map(|rank| {
                let index = (rank + offset) % 32;
                let name = ARTISTS.get(index).map_or_else(
                    || format!("Sample Artist {}", index + 1),
                    |name| (*name).to_string(),
                );
                let picture = if rank == 0 && offset == 0 {
                    Vec::new()
                } else {
                    image(700 + index as u32)
                };
                RankedArtist {
                    id: format!("art{index}"),
                    name,
                    streams: (top as f64 * 0.82f64.powi(rank as i32)).round().max(1.0) as u64,
                    thumbnail: if picture.is_empty() {
                        image(800 + index as u32)
                    } else {
                        picture.clone()
                    },
                    picture,
                    color: Some(colors[(rank + offset) % colors.len()]),
                }
            })
            .collect(),
        days_left,
        disclaimer: Some((
            "We do our best to calculate your listening activity.".into(),
            "https://tidal.com".into(),
        )),
    };
    let current = MonthKey::of(today);
    let past: Vec<MonthKey> = [1, 2, 3]
        .iter()
        .scan(current, |month, _| {
            *month = month.previous();
            Some(*month)
        })
        .collect();
    let mut reports = vec![(current, month_report(0, 84, Some(17)))];
    for (index, month) in past.iter().enumerate() {
        reports.push((
            *month,
            month_report(index + 1, 258 - 30 * index as u64, None),
        ));
    }
    let local = crate::backend::StatsReport {
        totals: crate::play_stats::Totals {
            listened_ms: 14 * 3_600_000 + 25 * 60_000,
            streams: 241,
            listens: 263,
            skips: 22,
        },
        top_tracks_by_plays: (0..crate::activity::LOCAL_TOP_TRACKS)
            .map(|index| crate::play_stats::TopEntry {
                uri: format!("tidal:track:trk{index}"),
                name: TRACKS[index].into(),
                subtitle: ARTISTS[index % ARTISTS.len()].into(),
                plays: 40 - 2 * index as u64,
                listened_ms: 0,
            })
            .collect(),
        ..Default::default()
    };
    app.activity.fill(Some("demo"), today, past, reports, local);
}

pub fn populate(app: &mut App) {
    crate::uploads::demo::populate(&mut app.uploads);
    app.backend.set_offline(true);
    app.offline = true;
    app.auth = AuthStatus::Connected {
        username: "demo".into(),
    };
    app.local_device_id = Some("local-demo".into());
    app.local_ready = true;
    app.local_playback = crate::backend::LocalPlayback::Ready {
        device_id: "local-demo".into(),
    };
    app.user = Some(User {
        id: "demo".into(),
        display_name: Some("Carmine".into()),
        images: image(1),
        product: Some("premium".into()),
        country: Some("DE".into()),
        uri: Some("tidal:user:demo".into()),
    });

    let playlists: Vec<Playlist> = (0..PLAYLISTS.len()).map(playlist).collect();
    for playlist in &playlists {
        app.saved.insert(playlist.uri.clone(), true);
    }
    app.library.playlists = Loadable::Loaded(playlists.clone());

    let tracks: Vec<Track> = (0..40).map(track).collect();
    for (index, track) in tracks.iter().enumerate() {
        app.saved.insert(track.uri.clone(), index % 3 == 0);
    }

    // Playlist page.
    let mut playlist_page = PlaylistPage {
        playlist: Loadable::Loaded(playlists[1].clone()),
        ..PlaylistPage::default()
    };
    let demo_now = Timestamp::now();
    playlist_page.items.absorb(
        0,
        page(
            tracks
                .iter()
                .enumerate()
                .map(|(index, track)| PlaylistItem {
                    // The first rows deliberately cover each relative-date
                    // unit; the rest remain absolute dates.
                    added_at: Some(demo_added_at(index, demo_now)),
                    is_local: false,
                    // Use multiple contributors so Added By and the byline render.
                    added_by: Some(crate::api::models::UserRef {
                        id: Some(if index % 3 == 1 { "kasia" } else { "sam" }.into()),
                    }),
                    item: Some(PlayableItem::Track(track.clone())),
                    track: None,
                })
                .collect(),
        ),
    );
    // TIDAL's Recommended songs for the playlist: ten it does not hold.
    playlist_page.recommendations = Some(PlaylistRecommendations {
        songs: (200..210).map(track).collect(),
        offset: 0,
        total: 100,
    });
    playlist_page.contributors.insert("kasia".into());
    playlist_page.contributors.insert("sam".into());
    app.playlist_pages.insert("pl1".into(), playlist_page);
    app.user_names.insert("kasia".into(), Some("Kasia".into()));
    app.user_names.insert("sam".into(), Some("Sam".into()));
    // The playlist owner's picture, as the profile view would answer.
    app.user_images.insert(
        "demo".into(),
        Some(image(1).first().map(|image| image.url.clone()).unwrap()),
    );
    let mut discover_page = PlaylistPage {
        playlist: Loadable::Loaded(playlists[0].clone()),
        ..PlaylistPage::default()
    };
    discover_page.items.absorb(
        0,
        page(
            tracks
                .iter()
                .rev()
                .take(30)
                .map(|track| PlaylistItem {
                    added_at: Some("2026-08-24T05:00:00Z".into()),
                    is_local: false,
                    added_by: None,
                    item: Some(PlayableItem::Track(track.clone())),
                    track: None,
                })
                .collect(),
        ),
    );
    app.playlist_pages.insert("pl0".into(), discover_page);

    // Album page.
    let shelves = demo_album_shelves(app);
    let mut album_page = AlbumPage {
        album: Loadable::Loaded(album(0)),
        shelves: Loadable::Loaded(shelves),
        review: Loadable::Loaded(Some(crate::api::bio::ArtistBio {
            text: DEMO_REVIEW.to_string(),
            source: Some("TiVo".into()),
        })),
        credits: Loadable::Loaded(std::sync::Arc::new(demo_album_credits())),
        ..AlbumPage::default()
    };
    album_page
        .tracks
        .absorb(0, page(tracks.iter().take(12).cloned().collect()));
    app.album_pages.insert("alb0".into(), album_page);
    app.saved.insert("tidal:album:alb0".into(), true);

    // Mix page.
    let mut mix_page = crate::model::MixPage {
        mix: Loadable::Loaded(mix(0)),
        ..crate::model::MixPage::default()
    };
    mix_page
        .tracks
        .absorb(0, page(tracks.iter().take(12).cloned().collect()));
    app.mix_pages.insert("mix0".into(), mix_page);

    // Artist page.
    let mut artist_page = ArtistPage {
        artist: Loadable::Loaded(artist(0)),
        top_tracks: Loadable::Loaded(Arc::new(tracks.iter().take(10).cloned().collect())),
        related: Loadable::Loaded((1..8).map(artist).collect()),
        bio: Loadable::Loaded(Some(crate::api::bio::ArtistBio {
            text: DEMO_BIO.to_string(),
            source: Some("TiVo".into()),
        })),
        ..ArtistPage::default()
    };
    // TIDAL's shelves: albums, EPs & singles, and appearances.
    for (filter, range) in [
        (DiscographyFilter::Albums, 0..5),
        (DiscographyFilter::Singles, 5..8),
        (DiscographyFilter::AppearsOn, 0..0),
    ] {
        let mut albums = PagedList::default();
        albums.absorb(0, page(range.map(album).collect()));
        artist_page
            .albums
            .insert(filter.groups().to_string(), albums);
    }
    app.artist_pages.insert("art0".into(), artist_page);
    app.saved.insert("tidal:artist:art0".into(), true);

    // Radio pages, for a song and for a playlist.
    for (seed, first) in [("tidal:track:trk0", 1), ("tidal:playlist:pl1", 8)] {
        app.radio_pages.insert(
            seed.into(),
            RadioPage {
                songs: Loadable::Loaded(tracks.iter().skip(first).take(30).cloned().collect()),
                ..RadioPage::default()
            },
        );
    }

    // Library.
    app.library.liked.absorb(
        0,
        page(
            tracks
                .iter()
                .filter(|track| app.saved.get(&track.uri) == Some(&true))
                .map(|track| SavedTrack {
                    added_at: Some("2026-06-12T08:30:00Z".into()),
                    track: track.clone(),
                })
                .collect(),
        ),
    );
    app.library.albums.absorb(
        0,
        page(
            (0..8)
                .map(|index| SavedAlbum {
                    added_at: None,
                    album: album(index),
                })
                .collect(),
        ),
    );
    app.library.artists.items = (0..8).map(artist).collect();
    app.library.artists.loaded_once = true;
    app.library.artists.complete = true;
    app.library.mixes.absorb(0, page((0..8).map(mix).collect()));
    app.library
        .videos
        .absorb(0, page((0..9).map(video).collect()));

    // Home.
    app.home.requested = true;
    app.home.loaded_at = Some(Instant::now());
    app.home.recently_played = Loadable::Loaded(
        tracks
            .iter()
            .skip(5)
            .take(12)
            .map(|track| PlayHistory {
                track: track.clone(),
                played_at: Some("2026-08-26T21:12:00Z".into()),
                context: None,
            })
            .collect::<Vec<_>>()
            .into(),
    );
    // Recents tab (queue sidebar) – deduped, timestamped, paginated.
    let recents_now = Timestamp::now();
    app.recents.items = tracks
        .iter()
        .skip(2)
        .take(24)
        .enumerate()
        .map(|(index, track)| PlayHistory {
            track: track.clone(),
            played_at: Some(demo_added_at(index, recents_now)),
            context: None,
        })
        .collect();
    app.recents.loaded_once = true;
    app.recents.loading = false;
    app.recents.error = None;
    // Has more to load (before cursor of oldest item).
    let oldest = recents_now - SignedDuration::from_hours(48);
    // Cursor is unix millis; jiff Timestamp exposes seconds + nanos.
    let millis = oldest.as_second() * 1000 + i64::from(oldest.subsec_nanosecond() / 1_000_000);
    app.recents.after = Some(millis.to_string());
    app.recents.complete = false;
    app.home.top_artists = Loadable::Loaded(Arc::new((0..8).map(artist).collect()));
    app.home.top_tracks =
        Loadable::Loaded(Arc::new(tracks.iter().skip(10).take(10).cloned().collect()));
    app.home.top_songs = Loadable::Loaded(tracks.iter().skip(10).cloned().collect());
    app.home.top_songs_complete = true;
    app.home.recommendations =
        Loadable::Loaded(Arc::new(tracks.iter().skip(20).take(10).cloned().collect()));
    // TIDAL's Home shelves, shaped like the logged-in web player's.
    let essentials: Vec<ShelfItem> = (0..ARTISTS.len())
        .map(|index| {
            let mut essential = playlist(index);
            essential.id = format!("ess{index}");
            essential.uri = format!("tidal:playlist:ess{index}");
            essential.name = format!("{} Essentials", ARTISTS[index]);
            essential.images = image(400 + index as u32);
            essential.owner.display_name = Some("TIDAL".into());
            essential.owner.id = Some("tidal".into());
            ShelfItem::Playlist(essential)
        })
        .collect();
    app.home.shelves = Loadable::Loaded(Arc::new(vec![
        Shelf {
            title: "Your favorite artists".into(),
            items: (0..ARTISTS.len())
                .map(|i| ShelfItem::Artist(artist(i)))
                .collect(),
            more: None,
            module_id: "YOUR_FAVORITE_ARTISTS".into(),
            ..Default::default()
        },
        Shelf {
            title: "User playlists you’ll love".into(),
            items: (0..PLAYLISTS.len())
                .map(|i| ShelfItem::Playlist(playlist(i)))
                .collect(),
            more: None,
            ..Default::default()
        },
        Shelf {
            title: "Essentials to explore".into(),
            items: essentials,
            more: None,
            ..Default::default()
        },
        Shelf {
            title: "My mixes".into(),
            items: (0..8).map(|i| ShelfItem::Mix(mix(i))).collect(),
            more: None,
            ..Default::default()
        },
        Shelf {
            title: "Albums you’ll enjoy".into(),
            items: (0..ALBUMS.len())
                .map(|i| ShelfItem::Album(album(i)))
                .collect(),
            more: None,
            ..Default::default()
        },
    ]));

    home_feed_tabs(app, &tracks);

    // The signed-in user's profile: the account uploads as an artist, so
    // TIDAL shows it as that artist's page (`uploader_profile`).
    app.profiles.insert(
        "demo".into(),
        Loadable::Loaded(crate::api::models::Profile {
            id: "demo".into(),
            name: "Carmine".into(),
            images: image(1),
            followers: Some(12),
            following: Some(48),
            playlists: Vec::new(),
            artist_id: Some(UPLOADER.into()),
        }),
    );
    uploader_profile(app);

    activity(app);

    // Explore and a genre page, as TIDAL's page modules.
    let links = |titles: &[&str], prefix: &str| -> Vec<crate::api::models::PageLink> {
        titles
            .iter()
            .map(|title| crate::api::models::PageLink {
                title: (*title).into(),
                path: format!(
                    "pages/{prefix}_{}",
                    title.to_ascii_lowercase().replace([' ', '/', '&'], "_")
                ),
                icon: None,
            })
            .collect()
    };
    let cloud = |title: &str, links: Vec<crate::api::models::PageLink>| Shelf {
        title: title.into(),
        links,
        cloud: true,
        more: Some("pages/genre_page".into()),
        ..Default::default()
    };
    app.shelf_pages.insert(
        crate::ui::explore::PATH.into(),
        Loadable::Loaded(vec![
            cloud(
                "Genres",
                links(
                    &[
                        "Hip-Hop",
                        "Pop",
                        "R&B / Soul",
                        "Rock / Indie",
                        "Dance & Electronic",
                        "Latin",
                        "Country",
                        "Jazz",
                        "Reggae / Dancehall",
                        "Classical",
                        "Metal",
                        "Blues",
                        "Folk",
                    ],
                    "genre",
                ),
            ),
            cloud(
                "Moods & Activities",
                links(
                    &[
                        "For DJs",
                        "TIDAL Magazine",
                        "Record Labels",
                        "Artist Experiences",
                        "Autumn",
                        "Sleep",
                        "Wellness",
                        "Workout",
                        "Credits",
                        "Celebration",
                        "Party",
                        "Focus",
                    ],
                    "mood",
                ),
            ),
            cloud(
                "Decades",
                links(
                    &[
                        "1950s", "1960s", "1970s", "1980s", "1990s", "2000s", "2010s",
                    ],
                    "decade",
                ),
            ),
            Shelf {
                links: links(
                    &["New", "Top", "Videos", "HiRes", "Clean Content"],
                    "explore",
                ),
                ..Default::default()
            },
        ]),
    );
    app.page_titles
        .insert("pages/genre_hip-hop".into(), "Hip-Hop".into());
    app.shelf_pages.insert(
        "pages/genre_hip-hop".into(),
        Loadable::Loaded(vec![
            Shelf {
                title: "Playlists".into(),
                items: (0..PLAYLISTS.len())
                    .map(|i| ShelfItem::Playlist(playlist(i)))
                    .collect(),
                more: Some("pages/data/genre-playlists".into()),
                ..Default::default()
            },
            Shelf {
                title: "New Tracks".into(),
                items: tracks
                    .iter()
                    .take(8)
                    .cloned()
                    .map(ShelfItem::Track)
                    .collect(),
                more: Some("pages/data/genre-tracks".into()),
                ..Default::default()
            },
            Shelf {
                title: "New Albums".into(),
                items: (0..ALBUMS.len())
                    .map(|i| ShelfItem::Album(album(i)))
                    .collect(),
                more: Some("pages/data/genre-albums".into()),
                ..Default::default()
            },
        ]),
    );

    // Search.
    app.search.query = "Bonobo".into();
    app.search.committed = "Bonobo".into();
    app.search.results = Loadable::Loaded(Arc::new(SearchResults {
        tracks: Some(page(tracks.iter().take(10).cloned().collect())),
        artists: Some(page((0..6).map(artist).collect())),
        albums: Some(page((0..6).map(album).collect())),
        playlists: Some(page(playlists.iter().take(6).cloned().collect())),
        top_hit: None,
        // TIDAL's ranked "Top results", an upload among them, and a genre
        // the query names.
        top_hits: vec![
            crate::api::models::TopHit::Artist(artist(0)),
            crate::api::models::TopHit::Track(tracks[0].clone()),
            crate::api::models::TopHit::Album(album(0)),
            crate::api::models::TopHit::Upload(tracks[3].clone()),
            crate::api::models::TopHit::Playlist(playlists[0].clone()),
            crate::api::models::TopHit::Track(tracks[1].clone()),
            crate::api::models::TopHit::Album(album(1)),
            crate::api::models::TopHit::Artist(artist(1)),
        ],
        genres: vec![crate::api::models::SearchGenre {
            title: "Electronic".into(),
            api_path: "pages/genre_electronic".into(),
        }],
        uploads: Some(page(vec![tracks[3].clone()])),
        ..SearchResults::default()
    }));
    // Recent Searches: what was opened from a search, newest first.
    {
        use crate::recent_searches::RecentSearch;
        app.settings.recent_items = vec![
            RecentSearch::from_album(&album(4)),
            RecentSearch::from_artist(&artist(1)),
            RecentSearch::from_playlist(&playlists[0], "TIDAL".into()),
            RecentSearch::from_album(&album(2)),
            RecentSearch::from_artist(&artist(4)),
        ];
    }

    // Playback: a remote speaker is playing the second playlist.
    app.queue = Loadable::Loaded(Queue {
        currently_playing: Some(PlayableItem::Track(tracks[0].clone())),
        queue: tracks
            .iter()
            .skip(1)
            .take(12)
            .cloned()
            .map(PlayableItem::Track)
            .collect(),
    });
    app.devices = vec![
        Device {
            id: Some("local-demo".into()),
            name: "Tidalfast".into(),
            is_active: false,
            is_restricted: false,
            volume_percent: Some(70),
            supports_volume: Some(true),
            kind: "computer".into(),
        },
        Device {
            id: Some("remote1".into()),
            name: "Kitchen speaker".into(),
            is_active: true,
            is_restricted: false,
            volume_percent: Some(62),
            supports_volume: Some(true),
            kind: "speaker".into(),
        },
        Device {
            id: Some("remote2".into()),
            name: "Pixel 9".into(),
            is_active: false,
            is_restricted: false,
            volume_percent: Some(40),
            supports_volume: Some(true),
            kind: "smartphone".into(),
        },
    ];
    app.remote = Some(RemoteSnapshot {
        state: PlaybackState {
            device: Some(app.devices[1].clone()),
            repeat_state: "off".into(),
            shuffle_state: true,
            context: Some(Context {
                uri: playlists[1].uri.clone(),
                kind: "playlist".into(),
            }),
            timestamp: 0,
            progress_ms: Some(83_000),
            is_playing: true,
            item: Some(PlayableItem::Track(tracks[0].clone())),
            currently_playing_type: Some("track".into()),
        },
        received_at: Instant::now(),
    });
    for track in &tracks {
        if let Some(id) = &track.id {
            app.track_cache.insert(id.clone(), track.clone());
        }
    }
}

/// `TIDALFAST_DEMO_LYRICS_SCRIPT=greek|cyrillic|vietnamese|arabic|japanese|mixed`
/// swaps the sample words for that script, for font screenshots.
#[cfg(any(test, feature = "demo"))]
fn scripted_lyrics() -> Option<Vec<(u32, &'static str)>> {
    let words: &[&'static str] = match std::env::var("TIDALFAST_DEMO_LYRICS_SCRIPT").ok()?.as_str()
    {
        "greek" => &[
            "Ακόμα δε μπορώ να πιστέψω",
            "Τα κατάφερες, μου το έδειξες",
            "Πάλι μόνος μες στη νύχτα",
            "Μ' αγαπάς, μ' αγαπάς, μπορεί",
            "Και πού να πάω τώρα πια",
            "Όλα πέρασαν σαν όνειρο",
            "Σε περιμένω στο λιμάνι",
            "Η καρδιά μου σε ζητάει",
        ],
        "cyrillic" => &[
            "Я помню чудное мгновенье",
            "Передо мной явилась ты",
            "Как мимолётное виденье",
            "Как гений чистой красоты",
            "Жизнь продолжается, друзья мои",
            "Щедрый дождь над городом",
            "Ёлка, юла, съёмка, эхо",
            "Київ, Україна, ґанок",
        ],
        "vietnamese" => &[
            "Việt Nam quê hương tôi ở đây",
            "Những con đường đầy nắng ấm",
            "Em ơi đừng khóc nữa nhé",
            "Người ơi người ở đừng về",
            "Mưa rơi trên phố vắng",
            "Tình yêu như giấc mộng dài",
            "Ánh trăng soi sáng đêm nay",
            "Bước chân về nơi xa lắm",
        ],
        "arabic" => &[
            "يا حبيبي تعال إلى هنا",
            "قلبي يناديك في الليل",
            "ما أجمل الأيام معك",
            "أحبك أكثر من كل شيء",
            "الليل طويل والقمر بعيد",
            "عيناك سر الجمال",
            "تعال نعش الحلم معا",
            "وردة في بستان الحب",
        ],
        "japanese" => &[
            "君の名前を呼ぶたびに",
            "夜明けの空が少し滲んだ",
            "ありがとう、さよなら、また明日",
            "桜の花びらが舞い散る道",
            "遠い街の灯りを探して",
            "言葉にならない想いを抱いて",
            "Fall in love 恋におちて",
            "光の中へ歩き出そう",
        ],
        _ => &[
            "Ακόμα δε μπορώ (Greek)",
            "Я помню чудное мгновенье (Cyrillic)",
            "Việt Nam quê hương tôi (Vietnamese)",
            "يا حبيبي تعال إلى هنا (Arabic)",
            "君の名前を呼ぶたびに (Japanese)",
            "สวัสดีตอนเช้า (Thai)",
            "नमस्ते दुनिया (Devanagari)",
            "Plain Latin line for contrast",
        ],
    };
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, text)| (40_000 + 6_500 * i as u32, *text))
            .collect(),
    )
}

/// Words to go with the sample track, timed so that the one being sung
/// sits mid-panel at the demo's playback position.
#[cfg(any(test, feature = "demo"))]
pub(crate) fn sample_lyrics() -> crate::lyrics::Lyrics {
    let lines: Vec<(u32, &str)> = if let Some(lines) = scripted_lyrics() {
        lines
    } else {
        vec![
            (40_000, "Streetlights blinking down the river road"),
            (46_500, "Every window holding someone's evening"),
            (53_000, "I keep the radio low so you can sleep"),
            (59_500, "Counting mile markers like a rosary"),
            (66_000, "We left the city with the tank half full"),
            (72_500, "And a map that only shows the way back"),
            (79_000, "But the night is wide and the road is long"),
            (85_500, "And there's nowhere I would rather be"),
            (92_000, "Coffee going cold in the cup holder"),
            (98_500, "Your hand asleep on the gear stick"),
            (105_000, "Somewhere past the county line"),
            (111_500, "The stars come out to see us through"),
            (118_000, "Still the night is wide and the road is long"),
            (124_500, "And there's nowhere I would rather be"),
        ]
    };
    crate::lyrics::Lyrics {
        lines: lines
            .iter()
            .map(|(at_ms, text)| crate::lyrics::Line {
                at_ms: Some(*at_ms),
                text: (*text).to_string(),
            })
            .collect(),
        synced: true,
        instrumental: false,
    }
}

/// Applies `--demo-page` and `--demo-show`.
#[cfg(any(test, feature = "demo"))]
/// Credits as TIDAL lists them for a song, with linked and plain names.
fn sample_credits() -> crate::model::TrackCredits {
    let person = |name: &str, id: Option<&str>| ArtistRef {
        id: id.map(str::to_string),
        name: name.to_string(),
        ..Default::default()
    };
    let credit = |role: &str, people: Vec<ArtistRef>| crate::model::Credit {
        role: role.to_string(),
        contributors: people,
    };
    crate::model::TrackCredits {
        roles: vec![
            credit(
                "Producer",
                vec![person("Ada Reyes", None), person("Theo Lind", None)],
            ),
            credit(
                "Composer",
                vec![
                    person("M. Okafor", None),
                    person("J. Brandt", None),
                    person("Nova Ray", Some("art0")),
                ],
            ),
            credit(
                "Lyricist",
                vec![person("Nova Ray", Some("art0")), person("M. Okafor", None)],
            ),
            credit("Mixing Engineer", vec![person("Ada Reyes", None)]),
            credit("Recording Engineer", vec![person("Sam Ortiz", None)]),
        ],
        released: Some("2009".into()),
        label: Some("\u{2117} 2009 Demo Records".into()),
    }
}

/// Makes the displayed song this computer's playback, with that song's own
/// metadata, so the player bar draws the same as without it.
#[cfg(any(test, feature = "demo"))]
fn play_here(app: &mut App) {
    app.local_ready = true;
    let now = app.now_playing();
    app.local.track = Some(crate::player::LocalTrack {
        uri: now.as_ref().map(|now| now.uri.clone()).unwrap_or_default(),
        title: now
            .as_ref()
            .map(|now| now.title.clone())
            .unwrap_or_default(),
        artists: now
            .as_ref()
            .map(|now| now.artists.clone())
            .unwrap_or_default(),
        album: now
            .as_ref()
            .map(|now| now.album_name.clone())
            .unwrap_or_default(),
        art_url: now.as_ref().and_then(|now| now.art_url.clone()),
        art_small_url: now.as_ref().and_then(|now| now.art_small.clone()),
        duration_ms: now.as_ref().map(|now| now.duration_ms).unwrap_or_default(),
        album_uri: None,
        isrc: None,
    });
    app.local.volume = app.settings.volume;
    // Where the displayed song was, so the player bar shows the same time.
    app.local.position_ms = now.as_ref().map_or(0, |now| now.position_ms);
}

/// Select sample pages and UI states for deterministic, unauthenticated demos.
/// The By You fixtures cover owned rows and an intentionally unmatched search.
#[cfg(any(test, feature = "demo"))]
pub fn apply_flags(app: &mut App, page: Option<&str>, show: Option<&str>) {
    if let Some(page) = page.and_then(Page::decode) {
        app.open(page);
    }
    for surface in show.unwrap_or("").split(',').map(str::trim) {
        match surface {
            "library-list"
            | "library-list-narrow"
            | "library-list-wide"
            | "library-grid"
            | "library-grid-narrow"
            | "library-grid-wide" => {
                app.settings.sidebar_grid = surface.starts_with("library-grid");
                app.settings.art_expanded = false;
                app.settings.sidebar_width = if surface.ends_with("-narrow") {
                    230.0
                } else if surface.ends_with("-wide") {
                    600.0
                } else {
                    380.0
                };
            }
            "library-by-you" => app.settings.library_by_you = true,
            "library-by-you-empty" => {
                app.settings.library_by_you = true;
                app.library.filter = "No matching playlist".into();
            }
            "queue" => app.show_queue_panel = true,
            // Home's Staff Picks and Uploads tabs; the icon rail.
            "home-staff" => {
                app.open(Page::Home);
                app.home_tab = Some("editorial");
            }
            "home-uploads" => {
                app.open(Page::Home);
                app.home_tab = Some("uploads");
            }
            "rail" => app.settings.sidebar_visible = false,
            // The queue panel with a long History above the playing song.
            "queue-history" => {
                app.show_queue_panel = true;
                if let Loadable::Loaded(queue) = &app.queue {
                    app.queue_history = queue
                        .queue
                        .iter()
                        .map(|item| {
                            let PlayableItem::Track(track) = item;
                            track.clone()
                        })
                        .cycle()
                        .take(12)
                        .collect();
                }
            }
            // The search dropdown: typing "eminem", or the recent searches
            // of an empty field.
            "search" | "search-many" | "search-recent" => {
                app.search.dropdown = true;
                app.search.query.clear();
                if surface != "search-recent" {
                    app.search.query = "eminem".into();
                    let answer: serde_json::Value =
                        serde_json::from_str(include_str!("api/tidal/fixtures/suggestions.json"))
                            .expect("the suggestions fixture is JSON");
                    let mut found = crate::api::tidal::search::suggestions("eminem", &answer);
                    if surface == "search-many" {
                        // A dozen direct hits and the exact query in Recent.
                        let hits = found.direct_hits.clone();
                        found.direct_hits = hits.iter().cycle().take(12).cloned().collect();
                        app.settings.search_history.insert(0, "eminem".into());
                    }
                    app.search.suggestions = Some(found);
                    app.search.highlight = Some(1);
                }
            }
            "playing-next" => {
                app.show_queue_panel = true;
                if let Loadable::Loaded(queue) = &app.queue {
                    app.manual_queue = queue
                        .queue
                        .iter()
                        .take(6)
                        .map(|item| item.uri().to_string())
                        .collect();
                }
                // Local playback is the only target that can be reordered
                // or inserted into positionally; simulate it active so the
                // drag-to-reorder behaviour is reviewable here. Carry over
                // the displayed track's own metadata rather than a bare
                // default, so the player bar and top bar render the same
                // as without this override.
                play_here(app);
                app.local.playback = crate::player::Playback::Paused;
            }
            // The sign-in card and the card while the session connects.
            "signed-out" => {
                app.auth = AuthStatus::SignedOut;
                app.user = None;
            }
            "connecting" => {
                app.auth = AuthStatus::Connecting;
                app.user = None;
            }
            "recents" => {
                app.show_queue_panel = true;
                app.queue_tab = QueueTab::Recents;
            }
            // Deterministic panel states for translation and layout reviews.
            "queue-empty" | "queue-loading" | "queue-error" => {
                app.show_queue_panel = true;
                app.queue = match surface {
                    "queue-loading" => Loadable::Loading,
                    "queue-error" => Loadable::Failed("Connection interrupted".into()),
                    _ => Loadable::Loaded(Queue::default()),
                };
            }
            "recents-empty" | "recents-loading" | "recents-error" => {
                app.show_queue_panel = true;
                app.queue_tab = QueueTab::Recents;
                app.recents.items.clear();
                app.recents_view.clear();
                app.recents.complete = true;
                app.recents.loaded_once = true;
                app.recents.loading = surface == "recents-loading";
                app.recents.error =
                    (surface == "recents-error").then(|| "Connection interrupted".into());
            }
            "home-loading" => app.home.shelves = Loadable::Loading,
            // The own profile before anything was uploaded: TIDAL's empty
            // profile, without Play and Shuffle.
            "profile-empty" => {
                if let Some(page) = app.artist_pages.get_mut(UPLOADER) {
                    page.top_tracks = Loadable::Loaded(Arc::default());
                    for list in page.albums.values_mut() {
                        list.items.clear();
                        list.revision += 1;
                    }
                }
            }
            // An account without an artist: the plain profile.
            "profile-plain" => {
                if let Some(Loadable::Loaded(profile)) = app.profiles.get_mut("demo") {
                    profile.artist_id = None;
                    profile.playlists = (0..PLAYLISTS.len()).map(playlist).collect();
                }
                if matches!(app.page(), Page::Artist(id) if id == UPLOADER) {
                    app.open(Page::Profile("demo".into()));
                }
            }
            // Playback states for measuring what each one costs at rest.
            "paused" => {
                if let Some(remote) = app.remote.as_mut() {
                    remote.state.is_playing = false;
                }
            }
            "no-playback" => app.remote = None,
            // The page scrolled down, to check what moves with it.
            "scrolled" => app.demo_scroll = Some(300.0),
            // Far enough for a collection page's header and controls to
            // be gone, and its pinned bar to show.
            "scrolled-far" => app.demo_scroll = Some(700.0),
            // Scrolled to `TIDALFAST_DEMO_SCROLL` points (a playlist's
            // Recommended songs lie below its 40 rows).
            "scrolled-end" => {
                app.demo_scroll = std::env::var("TIDALFAST_DEMO_SCROLL")
                    .ok()
                    .and_then(|points| points.parse().ok())
                    .or(Some(2300.0));
            }
            // The demo album as a two-volume one.
            "volumes" => {
                if let Some(page) = app.album_pages.get_mut("alb0") {
                    for (index, track) in page.tracks.items.iter_mut().enumerate() {
                        track.disc_number = Some(if index >= 6 { 2 } else { 1 });
                    }
                    page.tracks.revision += 1;
                }
            }
            "collection-loading" => match app.page().clone() {
                Page::Playlist(id) => {
                    app.playlist_pages.get_mut(&id).unwrap().playlist = Loadable::Loading
                }
                Page::Album(id) => app.album_pages.get_mut(&id).unwrap().album = Loadable::Loading,
                Page::Artist(id) => {
                    app.artist_pages.get_mut(&id).unwrap().artist = Loadable::Loading
                }
                _ => {}
            },
            "lyrics-follow"
            | "lyrics-empty"
            | "lyrics-loading"
            | "lyrics-error"
            | "lyrics-instrumental"
            | "lyrics-no-playback" => {
                app.show_lyrics_panel = true;
                app.lyrics_uri = app.now_playing().map(|now| now.uri);
                app.lyrics_following = false;
                app.lyrics = match surface {
                    "lyrics-empty" => Loadable::Loaded(None),
                    "lyrics-loading" => Loadable::Loading,
                    "lyrics-error" => Loadable::Failed("Connection interrupted".into()),
                    _ => {
                        let mut lyrics = sample_lyrics();
                        lyrics.instrumental = surface == "lyrics-instrumental";
                        Loadable::Loaded(Some(lyrics))
                    }
                };
                if surface == "lyrics-no-playback" {
                    app.remote = None;
                }
            }
            // These paired states capture both outcomes of the collection
            // Shuffle click for the PR visual comparison.
            "shuffle-selected" => {
                app.apply(Action::SetShuffle(true), &egui::Context::default());
            }
            "shuffle-started" => {
                app.apply(
                    Action::ShufflePlay("tidal:playlist:pl0".into()),
                    &egui::Context::default(),
                );
            }
            "rtl" => {
                if let Some(page) = app.playlist_pages.get_mut("pl1") {
                    for (item, &(title, artist, album)) in
                        page.items.items.iter_mut().zip(RTL_TRACKS)
                    {
                        if let Some(PlayableItem::Track(track)) = &mut item.item {
                            track.name = title.into();
                            if let Some(first) = track.artists.first_mut() {
                                first.name = artist.into();
                            }
                            if let Some(album_ref) = &mut track.album {
                                album_ref.name = album.into();
                            }
                        }
                    }
                    page.items.revision += 1;
                }
            }
            "german" => {
                app.settings.language =
                    crate::settings::LanguageChoice::Locale(crate::i18n::Locale::German);
                app.locale = crate::i18n::Locale::German;
            }
            "update" | "update-dialog" => {
                // Three missed releases, newest first, as a signed manifest's
                // `history` lists them.
                let entry = |version: &str, date: &str, notes: &str| crate::updates::HistoryEntry {
                    version: version.into(),
                    date: date.into(),
                    notes: notes.into(),
                };
                let latest = "A short summary.\n\n**New**\n- **Queue shuffle.** Shuffle keeps the song playing.\n\n**Fixed**\n- **Lyrics.** They no longer jump.";
                crate::updates::demo_offer(
                    "0.6.9",
                    "0.7.1",
                    latest,
                    vec![
                        entry("0.7.1", "2026-10-16", latest),
                        entry(
                            "0.7.0",
                            "2026-10-10",
                            "A bigger release.\n\n**New**\n- **Plugin store.** New, Updated and All sections.\n- **Themes.** Nine community themes.",
                        ),
                        entry(
                            "0.6.10",
                            "2026-10-02",
                            "**Fixed**\n- **Downloads.** Resume after a dropped connection.",
                        ),
                        entry("0.6.9", "2026-09-25", "Already installed."),
                    ],
                );
                app.update = Some(crate::updates::Release {
                    version: "0.7.1".into(),
                    url: "https://cioudguy.github.io/tidalfast/download/".into(),
                });
                // `update-dialog` also opens the dialog, listing the missed releases.
                app.show_update = surface == "update-dialog";
            }
            // A plugin asking to run a program (the mpv plugin starting mpv).
            "program-prompt" => {
                app.plugins.demo_program = Some(crate::luna::ProgramRequest {
                    id: 1,
                    plugin: "@vmohammad/mpv".into(),
                    program: if cfg!(windows) {
                        r"C:\Program Files\mpv\mpv.exe".into()
                    } else {
                        "/usr/bin/mpv".into()
                    },
                    name: if cfg!(windows) { "mpv.exe" } else { "mpv" }.into(),
                    command: "mpv --no-video --idle=yes --input-ipc-server=mpv-ipc".into(),
                    shell: false,
                    remember: true,
                });
            }
            // A plugin asking for a permission at first use (the TIDAL
            // sign-in, system-wide shortcuts, mpv's pipe, saving audio).
            "permission-credentials"
            | "permission-shortcuts"
            | "permission-pipe"
            | "permission-audio" => {
                use crate::luna::Access;
                let (plugin, access, detail) = match surface {
                    "permission-credentials" => {
                        ("@jxnxsdev/discord-utils", Access::Credentials, vec![])
                    }
                    "permission-shortcuts" => (
                        "MediaKeysFixer",
                        Access::GlobalShortcuts,
                        vec!["MediaPlayPause".to_string(), "MediaNextTrack".to_string()],
                    ),
                    "permission-pipe" => (
                        "@vmohammad/mpv",
                        Access::Pipe {
                            path: if cfg!(windows) {
                                r"\\.\pipe\mpvserver-1".into()
                            } else {
                                "/tmp/node-mpv-1.sock".into()
                            },
                        },
                        vec![],
                    ),
                    _ => ("Song Downloader", Access::SaveAudio, vec![]),
                };
                app.plugins.demo_permission = Some(crate::luna::PermissionRequest {
                    id: 1,
                    plugin: plugin.into(),
                    access,
                    detail,
                });
            }
            "row-menu" => app.demo_open_menu = true,
            // This computer playing, on whatever page is open: for measuring
            // what a playing song costs outside the now-playing view.
            "playing-local" => {
                play_here(app);
                app.local.playback = crate::player::Playback::Playing;
            }
            // This computer playing CD-quality FLAC: the player bar's
            // quality pill.
            "quality-pill" | "quality-pill-max" => {
                play_here(app);
                app.local.playback = crate::player::Playback::Paused;
                let max = surface == "quality-pill-max";
                app.local.format = Some(crate::tidal_engine::StreamFormat {
                    codec: "flac".into(),
                    sample_rate: if max { 96_000 } else { 44_100 },
                    bit_depth: Some(if max { 24 } else { 16 }),
                    channels: 2,
                    quality: Some(if max {
                        crate::tidal_engine::Quality::HiResLossless
                    } else {
                        crate::tidal_engine::Quality::Lossless
                    }),
                    bitrate: Some(if max { 1_530_000 } else { 951_000 }),
                    ..Default::default()
                });
            }
            // Compact mode's window, cover and chrome, or its lyrics; shot at
            // e.g. --demo-size 320x320.
            "compact" | "compact-lyrics" => {
                play_here(app);
                app.demo_compact = true;
                if surface == "compact-lyrics" {
                    app.compact_lyrics = true;
                    app.lyrics_uri = app.now_playing().map(|now| now.uri);
                    app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
                }
            }
            "shortcuts" => app.dialog = Some(Dialog::Shortcuts),
            "whats-new" => app.dialog = Some(Dialog::WhatsNew { since: None }),
            "embed" => {
                app.dialog = Some(Dialog::Embed {
                    uri: "tidal:video:441699021".into(),
                    title: "Suzanne (Official Video)".into(),
                    subtitle: "Leonard Cohen".into(),
                    duration_ms: 236_000,
                })
            }
            "artist-bio" => app.dialog = Some(Dialog::ArtistBio { id: "art0".into() }),
            "blocked" => {
                app.blocked_view = Loadable::Loaded(crate::artist_blocks::BlockedContent {
                    artists: vec![crate::artist_blocks::BlockedArtist {
                        id: "7".into(),
                        name: "Example Artist".into(),
                    }],
                    tracks: vec![crate::artist_blocks::BlockedTrack {
                        id: "9".into(),
                        title: "Example Song".into(),
                        artist: "Example Artist".into(),
                        hidden: false,
                    }],
                });
                app.dialog = Some(Dialog::Blocked);
            }
            "album-credits" => app.open(Page::AlbumCredits("alb0".into())),
            "edit" => {
                app.dialog = Some(Dialog::EditPlaylist {
                    id: "pl1".into(),
                    name: "Long Way Home".into(),
                    description: "Songs for the road".into(),
                    public: Some(false),
                    cover: Default::default(),
                });
            }
            "create" => {
                app.dialog = Some(Dialog::CreatePlaylist {
                    name: "Autumn drives".into(),
                    public: false,
                    add_uris: vec!["tidal:track:trk1".into()],
                    description: String::new(),
                })
            }
            "duplicate" => {
                app.dialog = Some(Dialog::ConfirmPlaylistDuplicates {
                    position: None,
                    playlist_id: "pl1".into(),
                    playlist_name: "Long Way Home".into(),
                    items: vec![PlayableItem::Track(track(1))],
                    duplicate_uris: vec!["tidal:track:trk1".into()],
                })
            }
            // A TIDAL mix whose songs carry only the epoch TIDAL stamps on
            // dates it never recorded.
            "undated-mix" => {
                if let Some(page) = app.playlist_pages.get_mut("pl0") {
                    for item in &mut page.items.items {
                        item.added_at = Some("1970-01-01T00:00:00Z".into());
                    }
                }
            }
            "light" => {
                app.settings.theme = crate::settings::ThemeChoice::Light;
                app.actions.push(Action::SettingsChanged);
            }
            "dark" => {
                app.settings.theme = crate::settings::ThemeChoice::Dark;
                app.actions.push(Action::SettingsChanged);
            }
            "song-top-result" => {
                if let Loadable::Loaded(results) = &mut app.search.results {
                    let results = Arc::make_mut(results);
                    results.artists = None;
                }
            }
            "finite-playlist" => {
                if let Some(page) = app.playlist_pages.get_mut("pl1") {
                    let seed = page.items.items.clone();
                    page.items.items = seed.iter().cycle().take(50).cloned().collect();
                    page.items.total = Some(1000);
                    page.items.next_offset = Some(50);
                    page.items.revision += 1;
                    if let Some(playlist) = page.playlist.get_mut() {
                        playlist.tracks = Some(crate::api::models::TrackCount { total: 1000 });
                    }
                }
            }
            // 2,000 distinct songs over 250 covers, for `--demo-bench scroll`.
            "big-playlist" => {
                if let Some(page) = app.playlist_pages.get_mut("pl1") {
                    let now = Timestamp::now();
                    page.items.items = (0..2000)
                        .map(|index| {
                            let mut track = track(index);
                            track.name = format!("{} {}", track.name, index + 1);
                            if let Some(album) = &mut track.album {
                                album.images = image(1000 + (index % 250) as u32);
                            }
                            PlaylistItem {
                                added_at: Some(demo_added_at(index, now)),
                                is_local: false,
                                added_by: None,
                                item: Some(PlayableItem::Track(track)),
                                track: None,
                            }
                        })
                        .collect();
                    page.items.total = Some(2000);
                    page.items.next_offset = None;
                    page.items.revision += 1;
                    if let Some(playlist) = page.playlist.get_mut() {
                        playlist.tracks = Some(crate::api::models::TrackCount { total: 2000 });
                    }
                }
            }
            "focus" => app.settings.sidebar_visible = false,
            // A cold start: no device is playing anything, and all the app
            // has is the song the last session ended on.
            "resume" => {
                app.remote = None;
                app.resume_context = Some("tidal:playlist:pl1".into());
                app.resume_track = Some("tidal:track:trk0".into());
                app.resume_position_ms = 19_566;
            }
            // The same cold start, one press of Next in: the song moved on
            // and nothing started playing.
            "resume-next" => {
                app.remote = None;
                app.resume_context = Some("tidal:playlist:pl1".into());
                app.resume_track = Some("tidal:track:trk0".into());
                app.resume_position_ms = 19_566;
                app.actions.push(Action::Next);
            }
            // My Activity's Top Artists on the newest past month.
            "activity-past" => {
                let today = app.activity_today();
                app.activity.selected = app.activity.past_months(today).first().copied();
            }
            "eq" => {
                app.settings.eq_on = true;
                app.settings.eq_bands_db = crate::eq::PRESETS[13].bands_db;
            }
            // Parametric bands with a band pass and the crossfeed on.
            "eq-crossfeed" => {
                app.settings.eq_on = true;
                app.settings.eq_mode = crate::eq::EqMode::Parametric;
                app.settings.eq_bands = crate::eq::PARAMETRIC_PRESETS[3].bands();
                app.settings.eq_crossfeed = crate::eq::Crossfeed::Medium;
            }
            "art" => app.settings.art_expanded = true,
            "collection-sidebar" => app.settings.collection_in_sidebar = true,
            // Offline mode: the banner, and the pages that need the network.
            "offline-mode" => {
                app.settings.offline_mode = true;
                app.network_reachable = false;
            }
            // TIDAL's "Did you mean" under the pills of a misspelt search.
            "did-you-mean" => {
                app.search.query = "Bonobu".into();
                app.search.committed = "Bonobu".into();
                if let Loadable::Loaded(results) = &mut app.search.results {
                    let results = Arc::make_mut(results);
                    results.did_you_mean = Some("Bonobo".into());
                }
            }
            "windows-taskbar" => app.demo_windows_controls = true,
            // TIDAL's Windows title bar across the top of the window.
            name if name.starts_with("upload-") => {
                crate::uploads::demo::surface(app, name);
            }
            "titlebar" => crate::window::set_custom_titlebar(true),
            "pins" => {
                app.settings.pinned_contexts =
                    vec!["tidal:playlist:pl2".into(), "tidal:playlist:pl4".into()];
            }
            "sorted" => {
                app.table_sorts.insert(
                    Page::Playlist("pl1".into()),
                    crate::model::TableSort {
                        column: crate::model::SortColumn::Added,
                        ascending: false,
                    },
                );
            }
            "lyrics" | "lyrics-fullscreen" => {
                app.lyrics_uri = app.now_playing().map(|now| now.uri);
                app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
                app.lyrics_following = true;
                app.show_lyrics_panel = true;
                if surface == "lyrics-fullscreen" {
                    app.actions.push(Action::SetLyricsFullscreen(true));
                }
            }
            // TIDAL's now-playing view: the cover alone, or a tab beside it.
            "now-playing"
            | "now-playing-color"
            | "now-playing-similar"
            | "now-playing-credits"
            | "now-playing-lyrics"
            | "now-playing-queue"
            | "now-playing-local" => {
                // Playing here and staying so (the sample remote snapshot
                // goes stale after a few seconds): for measuring the view
                // while the background turns and an animated cover plays.
                if surface == "now-playing-local" {
                    play_here(app);
                    app.local.playback = crate::player::Playback::Playing;
                }
                let uri = app.now_playing().map(|now| now.uri);
                app.lyrics_uri.clone_from(&uri);
                app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
                app.lyrics_following = true;
                app.credits_uri = uri.clone();
                app.credits = Loadable::Loaded(sample_credits());
                if let Some(uri) = uri
                    && !app.radio_pages.contains_key(&uri)
                    && let Some(songs) = app
                        .radio_pages
                        .get("tidal:track:trk0")
                        .map(|page| page.songs.clone())
                {
                    app.radio_pages.insert(
                        uri,
                        RadioPage {
                            songs,
                            ..RadioPage::default()
                        },
                    );
                }
                app.now_playing_view = true;
                app.now_playing_tab = match surface {
                    "now-playing-similar" => Some(crate::model::NowPlayingTab::SimilarTracks),
                    "now-playing-credits" => Some(crate::model::NowPlayingTab::Credits),
                    "now-playing-lyrics" => Some(crate::model::NowPlayingTab::Lyrics),
                    _ => None,
                };
                if surface == "now-playing-queue" {
                    app.show_queue_panel = true;
                }
                // Settings → Player background → Color.
                if surface == "now-playing-color" {
                    app.settings.player_background = crate::settings::PlayerBackground::Color;
                }
            }
            // Full-screen lyrics drawn in the window as it is, for shots at
            // a chosen size, which a real full screen would override.
            "lyrics-fullscreen-view" | "lyrics-fullscreen-instrumental" => {
                app.lyrics_uri = app.now_playing().map(|now| now.uri);
                let mut lyrics = sample_lyrics();
                lyrics.instrumental = surface == "lyrics-fullscreen-instrumental";
                app.lyrics = Loadable::Loaded(Some(lyrics));
                app.lyrics_following = true;
                app.show_lyrics_panel = true;
                app.lyrics_fullscreen = Some(false);
            }
            // Emoji in titles, artists, playlist names and lyrics: joined
            // sequences, skin tones, flags and keycaps. Give it after
            // `lyrics` to put emoji in the words too.
            "emoji" => {
                let titles = [
                    ("Road Trip 🚗💨", "The Wanderers 🌵"),
                    ("Family 👨‍👩‍👧 Sunday", "Kasia 👋🏽"),
                    ("🇮🇹 Estate", "Nove 9️⃣"),
                    ("Heartbeat ❤️‍🔥", "Pulse"),
                    ("Rainbow 🏳️‍🌈 Parade", "Colours ✨"),
                    ("Night Drive 🌙", "Neon 🎧"),
                ];
                let rename = |track: &mut Track, (title, artist): (&str, &str)| {
                    track.name = title.to_string();
                    if let Some(first) = track.artists.first_mut() {
                        first.name = artist.to_string();
                    }
                };
                if let Some(page) = app.playlist_pages.get_mut("pl1") {
                    for (entry, names) in page.items.items.iter_mut().zip(titles) {
                        if let Some(PlayableItem::Track(track)) = &mut entry.item {
                            rename(track, names);
                        }
                    }
                    page.items.revision += 1;
                }
                if let Loadable::Loaded(queue) = &mut app.queue {
                    for (item, names) in queue.queue.iter_mut().zip(titles) {
                        let PlayableItem::Track(track) = item;
                        rename(track, names);
                    }
                }
                if let Some(remote) = &mut app.remote
                    && let Some(PlayableItem::Track(track)) = &mut remote.state.item
                {
                    rename(track, titles[0]);
                }
                if let Some(track) = app.track_cache.get_mut("trk0") {
                    rename(track, titles[0]);
                }
                if let Loadable::Loaded(playlists) = &mut app.library.playlists {
                    let names = ["☕ Morning", "Gym 💪🏿", "🎄 Christmas"];
                    for (playlist, name) in playlists.iter_mut().skip(3).zip(names) {
                        playlist.name = name.to_string();
                    }
                }
                // Text fields: the top bar's search and, with `create`, the
                // new playlist's name.
                app.search.query = "road trip 🚗".into();
                if let Some(Dialog::CreatePlaylist { name, .. }) = &mut app.dialog {
                    *name = "Autumn drives 🍂🚗".into();
                }
                if let Loadable::Loaded(Some(lyrics)) = &mut app.lyrics {
                    let marks = ["🌃", "🪟", "📻", "🛣️", "⛽", "🗺️", "🌌", "🥰"];
                    for (line, mark) in lyrics.lines.iter_mut().zip(marks.iter().cycle()) {
                        line.text = format!("{} {mark}", line.text);
                    }
                }
            }
            // Titles in scripts the interface font does not cover.
            "scripts" => {
                let titles = [
                    ("\u{591c}\u{306b}\u{99c6}\u{3051}\u{308b}", "YOASOBI"),
                    (
                        "\u{8d77}\u{98ce}\u{4e86}",
                        "\u{4e70}\u{8fa3}\u{6912}\u{4e5f}\u{7528}\u{5238}",
                    ),
                    (
                        "\u{bd04}\u{c5ec}\u{b984}\u{ac00}\u{c744}\u{aca8}\u{c6b8} (Still Life)",
                        "BIGBANG",
                    ),
                    (
                        "\u{6253}\u{4e0a}\u{82b1}\u{706b}",
                        "DAOKO, \u{7c73}\u{6d25}\u{7384}\u{5e2b}",
                    ),
                    (
                        "\u{5149}\u{5e74}\u{4e4b}\u{5916}",
                        "G.E.M. \u{9093}\u{7d2b}\u{68cb}",
                    ),
                    ("\u{bc24}\u{d3b8}\u{c9c0}", "IU"),
                    ("Lemon", "\u{7c73}\u{6d25}\u{7384}\u{5e2b}"),
                    (
                        "\u{7ea2}\u{8272}\u{9ad8}\u{8ddf}\u{978b}",
                        "\u{8521}\u{5065}\u{96c5}",
                    ),
                ];
                let rename = |track: &mut Track, (title, artist): (&str, &str)| {
                    track.name = title.to_string();
                    track.artists = vec![ArtistRef {
                        id: None,
                        name: artist.to_string(),
                        uri: None,
                        picture: None,
                    }];
                };
                if let Some(page) = app.playlist_pages.get_mut("pl1") {
                    for (entry, names) in page.items.items.iter_mut().zip(titles) {
                        if let Some(PlayableItem::Track(track)) = &mut entry.item {
                            rename(track, names);
                        }
                    }
                }
                if let Loadable::Loaded(queue) = &mut app.queue {
                    for (item, names) in queue.queue.iter_mut().zip(titles) {
                        let PlayableItem::Track(track) = item;
                        rename(track, names);
                    }
                }
                if let Some(remote) = &mut app.remote
                    && let Some(PlayableItem::Track(track)) = &mut remote.state.item
                {
                    rename(track, titles[0]);
                }
                if let Some(track) = app.track_cache.get_mut("trk0") {
                    rename(track, titles[0]);
                }
                if let Loadable::Loaded(playlists) = &mut app.library.playlists {
                    let names = [
                        "\u{901a}\u{52e4}\u{306e}BGM",
                        "\u{7761}\u{524d}\u{6b4c}\u{5355}",
                        "\u{cd9c}\u{adfc}\u{ae38} \u{d50c}\u{b808}\u{c774}\u{b9ac}\u{c2a4}\u{d2b8}",
                    ];
                    for (playlist, name) in playlists.iter_mut().skip(3).zip(names) {
                        playlist.name = name.to_string();
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppOptions;
    use crate::paths::AppDirs;
    use crate::settings::Settings;
    use std::sync::Arc;

    /// LuckyLP puts its shuffle button right after My Collection's
    /// "Albums" title: TIDAL's "..." beside the title moves right by the
    /// button's width, as in its flex row. Tidalfast drew the plugin's
    /// button over the "...".
    #[test]
    fn plugin_elements_after_a_collection_title_push_its_menu_button_along() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        // The window's own frame would switch the plugin overlay off with
        // no Luna running: draw the interface alone.
        let frame = |ctx: &egui::Context, app: &mut App| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
            output
                .platform_output
                .accesskit_update
                .expect("screen-reader tree")
        };
        let more_left = |overlay: Option<DomOverlay>| {
            let (ctx, mut app) = accessible_app("lucky-title");
            crate::ui::plugin_dom::test_overlay(overlay);
            apply_flags(&mut app, Some("albums"), None);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            let tree = frame(&ctx, &mut app);
            crate::ui::plugin_dom::test_overlay(None);
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some("More")
                        && node.bounds().is_some_and(|b| b.y0 < 200.0 && b.x0 > 300.0)
                })
                .and_then(|(_, node)| node.bounds())
                .expect("the title's menu button")
                .x0
        };
        let plain = more_left(None);
        let with_button = more_left(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: crate::luna::dom_anchors::keys::PAGE_TITLE.into(),
                position: Position::After,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    style: Style {
                        width: Some("53px".into()),
                        height: Some("27px".into()),
                        background_color: Some("#ff0000".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        assert!(
            (with_button - plain - 53.0).abs() < 1.5,
            "moved {} (from {plain})",
            with_button - plain
        );
    }

    /// LangRomanizer's Romanize Lyrics button goes after the now-playing
    /// header's Lyrics tab (the tabs' box's last child): it is drawn right of
    /// Lyrics, left of Full screen, as in TIDAL. The header had no tabs for
    /// plugins, so nothing was drawn there.
    #[test]
    fn a_plugin_button_after_the_lyrics_tab_sits_beside_it() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        let (ctx, mut app) = accessible_app("romanize-tab");
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: crate::luna::dom_anchors::keys::NP_CONTENT_BUTTONS.into(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    style: Style {
                        width: Some("40px".into()),
                        height: Some("20px".into()),
                        background_color: Some("#ff0000".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        apply_flags(&mut app, None, Some("now-playing-lyrics"));
        let frame = |ctx: &egui::Context, app: &mut App| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
            output
        };
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let output = frame(&ctx, &mut app);
        crate::ui::plugin_dom::test_overlay(None);
        let tree = output
            .platform_output
            .accesskit_update
            .expect("screen-reader tree");
        let bounds = |label: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(label) && node.bounds().is_some_and(|b| b.y0 < 120.0)
                })
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("no {label} in the header"))
        };
        let lyrics = bounds("Lyrics");
        let full = bounds("Full screen");
        let red = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == egui::Color32::from_rgb(255, 0, 0) => {
                    Some(r.rect)
                }
                _ => None,
            })
            .expect("the plugin's button");
        assert!(
            f64::from(red.left()) >= lyrics.x1 - 0.5 && f64::from(red.right()) <= full.x0 + 0.5,
            "{red:?} between {lyrics:?} and {full:?}"
        );
        assert!(f64::from(red.center().y) > lyrics.y0 && f64::from(red.center().y) < lyrics.y1);
    }

    /// AltPlay's page open over Home: it took Music's `_selected_` class,
    /// so the Music row no longer shows as selected, and its own link, a
    /// 40-point row, follows the Collection row with no gap. Tidalfast kept
    /// Music selected and left 8 points between them.
    #[test]
    fn a_plugin_row_after_collection_and_a_removed_selected_class() {
        use crate::luna::dom::{DomOverlay, HostOverride, Injected, Position, Style, VNode};
        let frame = |ctx: &egui::Context, app: &mut App| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
            output
        };
        let (ctx, mut app) = accessible_app("altplay-sidebar");
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            hosts: [(
                crate::luna::dom_anchors::keys::sidebar_item("sidebar-music"),
                HostOverride {
                    removed: vec!["_selected_29027d1".into()],
                    ..HostOverride::default()
                },
            )]
            .into_iter()
            .collect(),
            injected: vec![Injected {
                anchor: crate::luna::dom_anchors::keys::FIXED_NAV.into(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 1,
                    tag: "a".into(),
                    style: Style {
                        display: Some("flex".into()),
                        height: Some("40px".into()),
                        background_color: Some("#ff0000".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        apply_flags(&mut app, Some("home"), None);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let output = frame(&ctx, &mut app);
        crate::ui::plugin_dom::test_overlay(None);
        let tree = output
            .platform_output
            .accesskit_update
            .expect("screen-reader tree");
        let row = |label: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(label) && node.bounds().is_some_and(|b| b.x0 < 240.0)
                })
                .map(|(_, node)| node.clone())
                .unwrap_or_else(|| panic!("no {label} row"))
        };
        assert_eq!(
            row("Music").toggled(),
            Some(egui::accesskit::Toggled::False),
            "Music lost its selected class"
        );
        let collection = row("Collection").bounds().unwrap();
        let plugin = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == egui::Color32::from_rgb(255, 0, 0) => {
                    Some(r.rect)
                }
                _ => None,
            })
            .expect("the plugin's row");
        assert!(
            (f64::from(plugin.top()) - collection.y1).abs() < 1.0,
            "the plugin row starts at {} and Collection ends at {}",
            plugin.top(),
            collection.y1
        );
    }

    fn accessible_app(name: &str) -> (egui::Context, App) {
        let root = crate::paths::unique_test_dir(&format!("a11y-{name}"));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        // Finished frames: each one shows where its animations end.
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        populate(&mut app);
        // These frames never advance the clock or take pictures.
        app.reveal_theme_changes = false;
        (ctx, app)
    }

    /// Animations ask for frames only while they run: once the fades on a
    /// page under a still pointer have finished, none of them asks again.
    #[test]
    fn settled_animations_ask_for_no_frames() {
        for page in [Page::Home, Page::Album("alb0".into())] {
            let (ctx, mut app) = accessible_app("settled-motion");
            app.motion.force(false);
            crate::motion::apply(&ctx, false);
            app.open(page.clone());
            let mut time = 0.0;
            let mut motion_asked = false;
            for step in 0..40 {
                time += 0.1;
                let events = if step == 5 {
                    vec![egui::Event::PointerMoved(egui::pos2(500.0, 300.0))]
                } else {
                    vec![]
                };
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                motion_asked = ctx
                    .repaint_causes()
                    .iter()
                    .any(|cause| cause.file.ends_with("motion.rs"));
            }
            assert!(!motion_asked, "{page:?} keeps animating while idle");
            app.backend.shutdown();
        }
    }

    /// Runs `frames` passes a tenth of a second apart and returns the call
    /// sites in this crate's interface that asked for the last one.
    fn interface_causes(ctx: &egui::Context, app: &mut App, frames: usize) -> Vec<String> {
        let mut causes = Vec::new();
        for step in 0..frames {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    time: Some(step as f64 / 10.0),
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            causes = ctx
                .repaint_causes()
                .iter()
                .map(|cause| cause.file.replace('\\', "/"))
                .filter(|file| {
                    file.contains("src/ui/")
                        || file.ends_with("src/motion.rs")
                        || file.ends_with("src/theme.rs")
                })
                .collect();
        }
        causes
    }

    /// With nothing playing, a settled page asks the interface for no
    /// frames: no shimmer, clock, fade or relative date keeps it awake.
    #[test]
    fn an_idle_page_settles_without_frames() {
        for page in [Page::Home, Page::Playlist("pl1".into())] {
            let (ctx, mut app) = accessible_app("idle-settles");
            app.motion.force(false);
            crate::motion::apply(&ctx, false);
            app.remote = None;
            app.open(page.clone());
            let causes = interface_causes(&ctx, &mut app, 40);
            assert!(causes.is_empty(), "{page:?} keeps asking: {causes:?}");
            app.backend.shutdown();
        }
    }

    /// The skeleton shimmer runs while loading blocks are on screen, and
    /// stops when nobody can see the window.
    #[test]
    fn the_shimmer_stops_when_nobody_can_see_it() {
        let (ctx, mut app) = accessible_app("shimmer");
        app.motion.force(false);
        crate::motion::apply(&ctx, false);
        app.remote = None;
        app.home.shelves = Loadable::Loading;
        let shimmer = |causes: &[String]| causes.iter().any(|file| file.ends_with("skeleton.rs"));
        assert!(shimmer(&interface_causes(&ctx, &mut app, 5)));
        crate::theme::set_budget(&ctx, crate::power::Budget::Background);
        assert!(!shimmer(&interface_causes(&ctx, &mut app, 5)));
        app.backend.shutdown();
    }

    #[test]
    fn cover_editing_fits_large_and_narrow_windows_in_both_themes() {
        for (width, height) in [(1240.0, 800.0), (760.0, 520.0)] {
            for light in [false, true] {
                let (ctx, mut app) = accessible_app("cover-dialog");
                app.open(Page::Playlist("pl1".into()));
                app.dialog = Some(Dialog::EditPlaylist {
                    id: "pl1".into(),
                    name: "Test".into(),
                    description: String::new(),
                    public: Some(false),
                    cover: Default::default(),
                });
                if light {
                    app.settings.theme = crate::settings::ThemeChoice::Light;
                    app.actions.push(Action::SettingsChanged);
                }
                if let Some(Dialog::EditPlaylist { cover, .. }) = &mut app.dialog {
                    cover.error = Some("TIDAL refused this cover. Check that you own the playlist, then sign in again to grant image upload permission.".into());
                }
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ui| app.frame_ui(ui),
                    );
                    output.textures_delta.clear();
                }
                let rect = app.dialog_rect.unwrap();
                assert!(rect.left() >= 0.0 && rect.top() >= 0.0, "{rect:?}");
                assert!(rect.right() <= width && rect.bottom() <= height, "{rect:?}");
                app.backend.shutdown();
            }
        }
    }

    fn accessible_frame(
        ctx: &egui::Context,
        app: &mut App,
        events: Vec<egui::Event>,
    ) -> egui::accesskit::TreeUpdate {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.frame_ui(ui),
        );
        output.textures_delta.clear();
        output
            .platform_output
            .accesskit_update
            .expect("screen-reader tree")
    }

    fn accessible_node(
        tree: &egui::accesskit::TreeUpdate,
        label: &str,
        role: egui::accesskit::Role,
    ) -> egui::accesskit::NodeId {
        tree.nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label) && node.role() == role)
            .unwrap_or_else(|| panic!("missing {label:?} with role {role:?}"))
            .0
    }

    fn accessible_action(
        target: egui::accesskit::NodeId,
        action: egui::accesskit::Action,
        data: Option<egui::accesskit::ActionData>,
    ) -> egui::Event {
        egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: target,
            action,
            data,
        })
    }

    fn keyboard(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn accessible_navigation_and_pause_work_without_a_pointer() {
        use egui::accesskit::{Action, Role};
        let (ctx, mut app) = accessible_app("navigate");
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let collection = accessible_node(&tree, "Collection", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(collection, Action::Click, None)],
        );
        assert_eq!(app.page(), &Page::LikedSongs);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let pause = accessible_node(&tree, "Pause", Role::Button);
        assert!(app.believed_playing());
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(pause, Action::Focus, None)],
        );
        let tree = accessible_frame(
            &ctx,
            &mut app,
            vec![keyboard(egui::Key::Space, egui::Modifiers::NONE)],
        );
        assert!(
            !app.believed_playing(),
            "focused Space must pause once, without firing the global shortcut too"
        );
        assert_eq!(tree.focus, pause);
        app.backend.shutdown();
    }

    /// The saved By You choice still limits the playlist list to owned
    /// playlists (the sidebar has no button for it any more, so the setting
    /// is driven directly).
    #[test]
    fn by_you_setting_limits_the_sidebar_list_to_owned_playlists() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("by-you");
        app.settings.art_expanded = false;
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "My Daily Discovery", Role::Button);
        app.settings.library_by_you = true;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Late night focus", Role::Button);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("My Daily Discovery"))
        );
        app.settings.library_by_you = false;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "My Daily Discovery", Role::Button);
        app.backend.shutdown();
    }

    #[test]
    fn translated_sidebar_keeps_keyboard_navigation_and_accessible_names() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        use egui::accesskit::{Action as AccessibleAction, Role};
        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-sidebar-{locale:?}"));
            app.locale = locale;
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let music = accessible_node(&tree, &gettext(locale, "Music"), Role::Button);
            let explore = accessible_node(&tree, &gettext(locale, "Explore"), Role::Button);
            accessible_node(&tree, &gettext(locale, "Create a playlist"), Role::Button);
            accessible_node(&tree, &gettext(locale, "Sort playlists"), Role::Button);
            // The shortcut is named as the platform's keyboard writes it.
            let hide = if cfg!(target_os = "macos") {
                "Hide sidebar (Cmd+B)"
            } else {
                "Hide sidebar (Ctrl+B)"
            };
            accessible_node(&tree, &gettext(locale, hide), Role::Button);
            let collection = accessible_node(&tree, &gettext(locale, "Collection"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(music, AccessibleAction::Focus, None)],
            );
            let tree = accessible_frame(
                &ctx,
                &mut app,
                vec![keyboard(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            assert_eq!(tree.focus, explore, "Tab order must survive translation");
            // Focusing Collection opens its flyout, and Enter opens the page.
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(collection, AccessibleAction::Focus, None)],
            );
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            for label in ["Mixes & Radio", "Albums", "Tracks", "Videos", "Profiles"] {
                accessible_node(&tree, &gettext(locale, label), Role::Button);
            }
            accessible_frame(
                &ctx,
                &mut app,
                vec![keyboard(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(app.page(), &Page::LikedSongs);

            app.library.filter = "LATE NIGHT".to_string();
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            accessible_node(&tree, "Late night focus", Role::Button);
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("My Daily Discovery"))
            );
            app.backend.shutdown();
        }
    }

    /// TIDAL's right side, left to right: play queue, sound output,
    /// volume, compact mode, then the quality pill at the far right. No
    /// lyrics button; the volume slider waits for the pointer.
    #[test]
    fn player_bar_right_side_follows_tidals_order() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("player-bar-order");
        super::play_here(&mut app);
        app.local.playback = crate::player::Playback::Paused;
        app.local.format = Some(crate::tidal_engine::StreamFormat {
            codec: "flac".into(),
            sample_rate: 44_100,
            bit_depth: Some(16),
            channels: 2,
            quality: Some(crate::tidal_engine::Quality::Lossless),
            ..Default::default()
        });
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let left = |label: &str, role: Role| {
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(label)
                        && node.role() == role
                        && node.bounds().is_some_and(|bounds| bounds.y0 > 700.0)
                })
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("{label} is not in the player bar"))
                .x0
        };
        let order = [
            left("Queue", Role::Button),
            left("Sound output", Role::Button),
            left("Mute", Role::Button),
            left("Compact mode", Role::Button),
            left("16-bit 44.1kHz", Role::Button),
        ];
        assert!(
            order.windows(2).all(|pair| pair[0] < pair[1]),
            "out of TIDAL's order: {order:?}"
        );
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Lyrics")
                    && node.bounds().is_some_and(|bounds| bounds.y0 > 700.0)),
            "the player bar has no lyrics button"
        );
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Volume (%)")),
            "the volume slider shows only under the pointer"
        );
        app.backend.shutdown();
    }

    /// Compact mode opens the small window (embedded in tests) with its
    /// exit and lyrics buttons, and the exit button ends it.
    #[test]
    fn compact_mode_opens_and_its_cross_closes_it() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("compact-mode");
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let compact = accessible_node(&tree, "Compact mode", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(compact, AccessibleAction::Click, None)],
        );
        assert!(app.compact_mode);
        // The app's one window is swapped for the compact one: the shell
        // reopens it, and the reopened window starts without the intent.
        assert!(app.switch_intent);
        app.switch_intent = false;
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        // The lyrics toggle reports whether it is on.
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Lyrics"))
        );
        let exit = accessible_node(&tree, "Exit compact mode", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(exit, AccessibleAction::Click, None)],
        );
        assert!(!app.compact_mode);
        assert!(app.switch_intent, "the main window comes back");
        app.backend.shutdown();
    }

    #[test]
    fn translated_player_bar_keeps_control_identity_and_keyboard_actions() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        use egui::accesskit::{Action as AccessibleAction, Role};

        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-player-{locale:?}"));
            accessible_frame(&ctx, &mut app, vec![]);
            let english = accessible_frame(&ctx, &mut app, vec![]);
            let controls = [
                ("Shuffle", Role::CheckBox),
                ("Previous", Role::Button),
                ("Pause", Role::Button),
                ("Next", Role::Button),
                ("Repeat", Role::Button),
                ("Playback position (%)", Role::Slider),
                ("Mute", Role::Button),
                ("Queue", Role::Button),
                ("Sound output", Role::Button),
                ("Compact mode", Role::Button),
            ];
            let ids: Vec<_> = controls
                .iter()
                .map(|(source, role)| accessible_node(&english, source, *role))
                .collect();
            let pause = accessible_node(&english, "Pause", Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(pause, AccessibleAction::Focus, None)],
            );
            app.locale = locale;
            let translated = accessible_frame(&ctx, &mut app, vec![]);
            assert_eq!(translated.focus, pause, "translation must retain focus");
            for ((source, role), id) in controls.iter().zip(ids) {
                // The top bar's Back button can share a translation with
                // Previous (German Zurueck), so look only in the player bar.
                let name = gettext(locale, source);
                let found = translated
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.label() == Some(name.as_ref())
                            && node.role() == *role
                            && node.bounds().is_some_and(|bounds| bounds.y0 > 700.0)
                    })
                    .unwrap_or_else(|| panic!("missing {name:?} in the player bar"))
                    .0;
                assert_eq!(
                    found, id,
                    "{locale:?} translation must retain the {source} control"
                );
            }

            // A reported pause uses the same control with its new label.
            // Offline demo requests have no backend acknowledgment, so a
            // requested remote pause would keep the pending spinner visible.
            app.remote.as_mut().unwrap().state.is_playing = false;
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            assert_eq!(
                accessible_node(&tree, &gettext(locale, "Play"), Role::Button),
                pause
            );
            app.remote.as_mut().unwrap().state.is_playing = true;
            // TIDAL's volume slider shows above the speaker while it has
            // the pointer or the keyboard.
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let mute = accessible_node(&tree, &gettext(locale, "Mute"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(mute, AccessibleAction::Focus, None)],
            );
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let volume = accessible_node(&tree, &gettext(locale, "Volume (%)"), Role::Slider);
            let before = app.now_playing().unwrap().volume_percent;
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(volume, AccessibleAction::Focus, None)],
            );
            accessible_frame(
                &ctx,
                &mut app,
                vec![keyboard(egui::Key::ArrowRight, egui::Modifiers::NONE)],
            );
            assert_eq!(app.now_playing().unwrap().volume_percent, before + 5);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(pause, AccessibleAction::Focus, None)],
            );
            accessible_frame(
                &ctx,
                &mut app,
                vec![keyboard(egui::Key::Space, egui::Modifiers::NONE)],
            );
            assert!(!app.believed_playing());
            app.backend.shutdown();
        }
    }

    #[test]
    fn translated_player_bar_labels_follow_control_state_and_empty_playback() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        use egui::accesskit::{Action as AccessibleAction, Role};

        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-player-state-{locale:?}"));
            app.locale = locale;
            accessible_frame(&ctx, &mut app, vec![]);
            for (source, next) in [
                ("Repeat", "Repeat one"),
                ("Repeat one", "Repeat off"),
                ("Repeat off", "Repeat"),
                ("Mute", "Unmute"),
                ("Unmute", "Mute"),
                ("Remove from My Collection", "Add to My Collection"),
                ("Add to My Collection", "Remove from My Collection"),
            ] {
                let tree = accessible_frame(&ctx, &mut app, vec![]);
                let button = accessible_node(&tree, &gettext(locale, source), Role::Button);
                accessible_frame(
                    &ctx,
                    &mut app,
                    vec![accessible_action(button, AccessibleAction::Click, None)],
                );
                let tree = accessible_frame(&ctx, &mut app, vec![]);
                accessible_node(&tree, &gettext(locale, next), Role::Button);
            }

            app.remote = None;
            assert!(app.now_playing().is_none());
            let render = |app: &mut App, ui: &mut egui::Ui| crate::ui::player_bar::show(app, ui);
            view_frame(&ctx, &mut app, vec![], render);
            let painted = view_frame(&ctx, &mut app, vec![], render);
            for source in ["Nothing playing", "Pick a track, album, or playlist"] {
                assert!(
                    painted
                        .iter()
                        .any(|(text, _)| text == &gettext(locale, source)),
                    "missing translated empty playback label: {source}"
                );
            }
            app.backend.shutdown();
        }
    }

    #[test]
    fn translated_queue_controls_preserve_manual_and_context_rows() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        use egui::accesskit::{Action as AccessibleAction, Role};

        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-queue-{locale:?}"));
            app.locale = locale;
            app.show_queue_panel = true;
            app.local_ready = true;
            app.local.track = Some(crate::player::LocalTrack {
                uri: app.now_playing().unwrap().uri,
                ..Default::default()
            });
            app.local.playback = crate::player::Playback::Paused;
            let rows = app.queue.get().unwrap().queue.clone();
            app.manual_queue = rows[..2]
                .iter()
                .map(|item| item.uri().to_string())
                .collect();
            let saved_uris = app.queue_playlist_uris();
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let save = accessible_node(&tree, &gettext(locale, "Save as a playlist"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(save, AccessibleAction::Click, None)],
            );
            let Some(Dialog::CreatePlaylist {
                name,
                public,
                add_uris,
                ..
            }) = &app.dialog
            else {
                panic!("saving the translated queue must open the existing playlist dialog");
            };
            assert_eq!(name, &app.queue_playlist_name());
            assert!(!public);
            assert_eq!(add_uris, &saved_uris);
            app.dialog = None;
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let clear = accessible_node(&tree, &gettext(locale, "Clear queue"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(clear, AccessibleAction::Click, None)],
            );
            assert!(app.manual_queue.is_empty());
            assert_eq!(app.queue.get().unwrap().queue, rows[2..]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let close = accessible_node(&tree, &gettext(locale, "Close"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(close, AccessibleAction::Click, None)],
            );
            assert!(!app.show_queue_panel);
            app.backend.shutdown();
        }
    }

    #[test]
    fn translated_lyrics_controls_keep_sync_retry_and_fullscreen_actions() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        use egui::accesskit::{Action as AccessibleAction, Role};

        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-lyrics-{locale:?}"));
            app.locale = locale;
            app.show_lyrics_panel = true;
            app.lyrics_uri = app.now_playing().map(|now| now.uri);
            app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
            app.lyrics_following = false;
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let follow = accessible_node(&tree, &gettext(locale, "Sync Lyrics"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(follow, AccessibleAction::Click, None)],
            );
            assert!(app.lyrics_following);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let expand =
                accessible_node(&tree, &gettext(locale, "Full screen lyrics"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(expand, AccessibleAction::Click, None)],
            );
            assert!(app.lyrics_fullscreen.is_some());
            app.lyrics_following = false;
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let follow = accessible_node(&tree, &gettext(locale, "Sync Lyrics"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(follow, AccessibleAction::Click, None)],
            );
            assert!(app.lyrics_following);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let leave = accessible_node(
                &tree,
                &gettext(locale, "Leave full screen (Esc)"),
                Role::Button,
            );
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(leave, AccessibleAction::Click, None)],
            );
            assert!(app.lyrics_fullscreen.is_none());
            for fullscreen in [false, true] {
                app.lyrics_fullscreen = fullscreen.then_some(false);
                app.lyrics = Loadable::Failed("fixture failure".into());
                let tree = accessible_frame(&ctx, &mut app, vec![]);
                let retry = accessible_node(&tree, &gettext(locale, "Try again"), Role::Button);
                accessible_frame(
                    &ctx,
                    &mut app,
                    vec![accessible_action(retry, AccessibleAction::Click, None)],
                );
                // Offline retries finish without contacting a lyrics provider.
                assert!(matches!(app.lyrics, Loadable::Loaded(None)));
            }
            app.lyrics_fullscreen = None;
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let close = accessible_node(&tree, &gettext(locale, "Close"), Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(close, AccessibleAction::Click, None)],
            );
            assert!(!app.show_lyrics_panel);
            app.backend.shutdown();
        }
    }

    #[test]
    fn translated_panel_states_keep_original_lyrics_and_failure_details() {
        use crate::i18n::{Locale, gettext};
        use clap::ValueEnum;
        for &locale in Locale::value_variants() {
            let (ctx, mut app) = accessible_app(&format!("translated-panel-states-{locale:?}"));
            app.locale = locale;
            app.show_lyrics_panel = true;
            app.lyrics_uri = app.now_playing().map(|now| now.uri);
            for fullscreen in [false, true] {
                app.lyrics_fullscreen = fullscreen.then_some(false);
                let view = |app: &mut App, ui: &mut egui::Ui| app.frame_ui(ui);
                let mut instrumental = sample_lyrics();
                instrumental.instrumental = true;
                for (state, messages) in [
                    // Lyrics on their way show a skeleton, no words.
                    (Loadable::Loading, Vec::new()),
                    (
                        Loadable::Failed("fixture {count} 42".into()),
                        vec![
                            gettext(locale, "Couldn't fetch the lyrics: {error}")
                                .replace("{error}", "fixture {count} 42"),
                        ],
                    ),
                    (
                        Loadable::Loaded(None),
                        vec![
                            gettext(locale, "No lyrics").into_owned(),
                            gettext(locale, "No lyrics found for this track.").into_owned(),
                        ],
                    ),
                    (
                        Loadable::Loaded(Some(instrumental)),
                        vec![
                            gettext(locale, "Instrumental").into_owned(),
                            gettext(locale, "No timed lyrics for this track.").into_owned(),
                        ],
                    ),
                    (
                        Loadable::Loaded(Some(sample_lyrics())),
                        vec![sample_lyrics().lines[0].text.clone()],
                    ),
                ] {
                    app.lyrics = state;
                    app.lyrics_following = false;
                    view_frame(&ctx, &mut app, vec![], view);
                    let painted = view_frame(&ctx, &mut app, vec![], view);
                    for message in messages {
                        assert!(
                            painted.iter().any(|(text, _)| text == &message),
                            "{locale:?}: missing {message}"
                        );
                    }
                }
            }
            for (state, messages) in [
                // A skeleton, no words, while the queue loads.
                (Loadable::Loading, Vec::new()),
                (
                    Loadable::Failed("fixture {count} 42".into()),
                    vec![
                        "fixture {count} 42".into(),
                        gettext(locale, "Retry").into_owned(),
                    ],
                ),
                (
                    Loadable::Loaded(Queue::default()),
                    vec![
                        gettext(locale, "Nothing queued").into_owned(),
                        gettext(locale, "Queued tracks appear here.").into_owned(),
                    ],
                ),
            ] {
                app.queue = state;
                view_frame(&ctx, &mut app, vec![], crate::ui::queue::page);
                let painted = view_frame(&ctx, &mut app, vec![], crate::ui::queue::page);
                for message in messages {
                    assert!(
                        painted.iter().any(|(text, _)| text == &message),
                        "{locale:?}: missing {message}"
                    );
                }
            }
            app.queue_tab = QueueTab::Recents;
            app.recents.items.clear();
            app.recents_view.clear();
            app.recents.loaded_once = true;
            app.recents.complete = true;
            for (loading, error, sources) in [
                (
                    false,
                    None,
                    vec!["No recent plays", "Played tracks appear here."],
                ),
                (true, None, Vec::new()),
                (false, Some("fixture failure".into()), vec!["Retry"]),
            ] {
                app.recents.loading = loading;
                app.recents.error = error;
                view_frame(&ctx, &mut app, vec![], crate::ui::queue::page);
                let painted = view_frame(&ctx, &mut app, vec![], crate::ui::queue::page);
                for source in sources {
                    assert!(
                        painted
                            .iter()
                            .any(|(text, _)| text == &gettext(locale, source))
                    );
                }
                if app.recents.error.is_some() {
                    let retry = painted
                        .iter()
                        .find(|(text, _)| text == &gettext(locale, "Retry"))
                        .unwrap()
                        .1
                        .center();
                    app.actions.clear();
                    view_frame(
                        &ctx,
                        &mut app,
                        pointer_click(retry, egui::PointerButton::Primary),
                        crate::ui::queue::page,
                    );
                    assert!(
                        app.actions
                            .iter()
                            .any(|action| matches!(action, crate::model::Action::ReloadRecents))
                    );
                }
            }
            app.backend.shutdown();
        }
    }

    #[test]
    fn library_sort_menu_preserves_saved_order_and_keyboard_activation() {
        use crate::settings::{LibraryShelf, LibrarySort};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("library-sort");
        app.settings.sidebar_order = vec!["tidal:playlist:pl4".into(), "tidal:playlist:pl1".into()];
        let saved = app.settings.sidebar_order.clone();
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let sort = accessible_node(&tree, "Sort playlists", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(sort, AccessibleAction::Click, None)],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Recently added"))
        );
        let name = accessible_node(&tree, "Name", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(name, AccessibleAction::Focus, None)],
        );
        accessible_frame(
            &ctx,
            &mut app,
            vec![keyboard(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(
            app.settings.library_sort.get(&LibraryShelf::Playlists),
            Some(&LibrarySort::Name)
        );
        assert_eq!(app.settings.sidebar_order, saved);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let sort = accessible_node(&tree, "Sort playlists", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(sort, AccessibleAction::Click, None)],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let local = accessible_node(&tree, "Local custom order", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(local, AccessibleAction::Click, None)],
        );
        assert_eq!(
            app.settings.library_sort.get(&LibraryShelf::Playlists),
            Some(&LibrarySort::Local)
        );
        assert_eq!(app.settings.sidebar_order, saved);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(restored.library_sort, app.settings.library_sort);
        assert_eq!(restored.sidebar_order, saved);
        app.backend.shutdown();
    }

    #[test]
    fn the_avatar_menu_lists_tidals_entries_in_order() {
        use egui::accesskit::Role;
        let menu = |luna: bool| {
            let (ctx, mut app) = accessible_app("avatar-menu");
            app.settings.plugins.luna_enabled = luna;
            accessible_frame(&ctx, &mut app, vec![]);
            accessible_frame(
                &ctx,
                &mut app,
                pointer_click(egui::pos2(1252.0, 30.0), egui::PointerButton::Primary),
            );
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let mut rows: Vec<(f64, String)> = tree
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::Button)
                .filter_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    (bounds.x0 > 900.0 && bounds.y0 > 38.0 && bounds.height() > 40.0)
                        .then(|| (bounds.y0, node.label().unwrap_or_default().to_string()))
                })
                .collect();
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            app.backend.shutdown();
            let names: Vec<String> = rows.into_iter().map(|row| row.1).collect();
            // The page's own cards follow the menu's last row.
            let end = names
                .iter()
                .rposition(|name| name == "Luna Settings" || name == "Log out")
                .map_or(names.len(), |last| last + 1);
            names[..end].to_vec()
        };
        assert_eq!(
            menu(false),
            [
                "Profile",
                // Not in TIDAL's own menu: Tidalfast adds it.
                "My Activity",
                "Manage subscription",
                "Settings",
                "Help",
                "Legal",
                "Log out"
            ]
        );
        assert_eq!(menu(true).last().map(String::as_str), Some("Luna Settings"));
    }

    #[test]
    fn library_context_menu_labels_follow_the_locale() {
        use crate::i18n::{Locale, gettext};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("library-menu-locale");
        app.locale = Locale::German;
        let row = |tree: &egui::accesskit::TreeUpdate, label: &str| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button
                        && node.label() == Some(label)
                        && node.bounds().is_some_and(|bounds| bounds.x0 < 250.0)
                })
                .unwrap_or_else(|| panic!("missing sidebar row {label}"))
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let liked = row(&tree, "Late night focus").center();
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(liked, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, &gettext(Locale::German, "Play now"), Role::Button);
        let pin = accessible_node(&tree, &gettext(Locale::German, "Pin to top"), Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(pin, AccessibleAction::Click, None)],
        );

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let liked = row(&tree, "Late night focus").center();
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(liked, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, &gettext(Locale::German, "Unpin"), Role::Button);
        app.backend.shutdown();
    }

    /// The labels of the buttons a menu shows, top to bottom.
    fn parity_menu_labels(tree: &egui::accesskit::TreeUpdate, min_x: f64) -> Vec<String> {
        use egui::accesskit::Role;
        let mut rows: Vec<(f64, String)> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Button)
            .filter_map(|(_, node)| {
                let bounds = node.bounds()?;
                (bounds.x0 >= min_x && bounds.height() > 20.0 && bounds.height() < 60.0)
                    .then(|| (bounds.y0, node.label().unwrap_or_default().to_string()))
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows.into_iter().map(|row| row.1).collect()
    }

    #[test]
    fn an_own_playlists_menu_puts_save_for_later_before_its_owner_items_as_tidal_does() {
        let (ctx, mut app) = accessible_app("own-playlist-menu-order");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some("Late night focus")
                    && node.bounds().is_some_and(|bounds| bounds.x0 < 250.0)
            })
            .expect("the sidebar row")
            .1
            .bounds()
            .unwrap();
        let center = egui::pos2(
            ((row.x0 + row.x1) / 2.0) as f32,
            ((row.y0 + row.y1) / 2.0) as f32,
        );
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(center, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let labels = parity_menu_labels(&tree, 0.0);
        let at = |label: &str| {
            labels
                .iter()
                .position(|item| item == label)
                .unwrap_or_else(|| panic!("missing {label:?} in {labels:?}"))
        };
        // TIDAL: Add to playlist, Save for Later, Edit playlist, Make
        // public, Delete playlist, Share.
        assert!(at("Add to playlist") < at("Save for Later"));
        assert!(at("Save for Later") < at("Edit playlist"));
        assert!(at("Edit playlist") < at("Delete playlist"));
        assert!(at("Delete playlist") < at("Share"));
        app.backend.shutdown();
    }

    #[test]
    fn the_collection_filters_name_what_they_filter_as_tidal_does() {
        for (page, placeholder) in [
            (Page::Albums, "Filter albums"),
            (Page::Artists, "Filter profiles"),
            (Page::Playlists, "Filter playlists"),
            (Page::LikedSongs, "Filter tracks"),
        ] {
            let (ctx, mut app) = accessible_app("collection-filter-placeholder");
            app.open(page.clone());
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let found = tree.nodes.iter().any(|(_, node)| {
                node.placeholder() == Some(placeholder) || node.label() == Some(placeholder)
            });
            assert!(found, "{page:?} has no {placeholder:?} field");
            app.backend.shutdown();
        }
    }

    #[test]
    fn the_playback_settings_describe_themselves_in_tidals_words() {
        let (ctx, mut app) = accessible_app("settings-wording");
        app.open(Page::Settings);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let tab = accessible_node(&tree, "Playback", egui::accesskit::Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(tab, egui::accesskit::Action::Click, None)],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let texts: Vec<String> = tree
            .nodes
            .iter()
            .flat_map(|(_, node)| {
                [node.label(), node.value(), node.description()]
                    .into_iter()
                    .flatten()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        let has = |text: &str| texts.iter().any(|item| item.contains(text));
        assert!(
            has("Set the same volume level for all tracks."),
            "{texts:?}"
        );
        assert!(has("Keep playing similar content when your queue ends."));
        assert!(has(
            "Adjust the length of fading and overlap in between tracks."
        ));
        assert!(has("music labeled with the E badge will be playable"));
        app.backend.shutdown();
    }

    #[test]
    fn purchases_tabs_are_the_underlined_row_not_pills_as_in_tidal() {
        let (ctx, mut app) = accessible_app("purchases-tabs");
        app.open(Page::Purchases);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let tab = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some("Tracks")
                    && node.role() == egui::accesskit::Role::Button
                    && node.bounds().is_some_and(|bounds| bounds.x0 > 250.0)
            })
            .expect("the Tracks tab")
            .1
            .bounds()
            .unwrap();
        // Home's tab row is 36 high; a filter pill is 40.
        assert_eq!(tab.y1 - tab.y0, 36.0);
        app.backend.shutdown();
    }

    #[test]
    fn the_playlist_create_menu_offers_playlist_and_folder_as_tidal_does() {
        let (ctx, mut app) = accessible_app("sidebar-create-menu");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let create = accessible_node(&tree, "Create a playlist", egui::accesskit::Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                create,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let labels = parity_menu_labels(&tree, 0.0);
        assert!(labels.iter().any(|label| label == "Folder"), "{labels:?}");
        assert!(!labels.iter().any(|label| label == "New folder"));
        assert!(!labels.iter().any(|label| label == "New playlist"));
        app.backend.shutdown();
    }

    /// The button called `label` whose top edge lies between `top` and
    /// `bottom` and that starts right of the sidebar: a page's header
    /// controls, told apart from the table rows and the player's.
    fn header_button(
        tree: &egui::accesskit::TreeUpdate,
        label: &str,
        top: f64,
        bottom: f64,
    ) -> Option<egui::accesskit::NodeId> {
        tree.nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button
                    && node.label() == Some(label)
                    && node
                        .bounds()
                        .is_some_and(|b| b.x0 > 250.0 && b.y0 >= top && b.y0 < bottom)
            })
            .map(|(id, _)| *id)
    }

    #[test]
    fn the_tracks_page_header_offers_only_play_and_shuffle_as_tidal_does() {
        let (ctx, mut app) = accessible_app("tracks-header");
        app.open(Page::LikedSongs);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(header_button(&tree, "Play", 100.0, 230.0).is_some());
        assert!(header_button(&tree, "Shuffle", 100.0, 230.0).is_some());
        assert!(header_button(&tree, "More", 100.0, 230.0).is_none());
        assert!(header_button(&tree, "Share", 100.0, 230.0).is_none());
        app.backend.shutdown();
    }

    #[test]
    fn a_collection_pages_dots_open_tidals_sort_list_and_the_choice_is_kept() {
        use crate::settings::{LibraryShelf, PageSort};
        let (ctx, mut app) = accessible_app("collection-sort-menu");
        app.open(Page::Albums);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let more = header_button(&tree, "More", 60.0, 130.0).expect("the dots beside the title");
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                more,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let labels = parity_menu_labels(&tree, 250.0);
        let at = |label: &str| labels.iter().position(|item| item == label);
        let (added, name, artist, released) = (
            at("Date added"),
            at("A-Z"),
            at("Artist"),
            at("Release date"),
        );
        assert!(
            added < name && name < artist && artist < released && released.is_some(),
            "{labels:?}"
        );
        let choice = header_button(&tree, "A-Z", 60.0, 400.0).expect("A-Z");
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                choice,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(
            app.settings.page_sort.get(&LibraryShelf::Albums),
            Some(&PageSort::Name)
        );
        app.backend.shutdown();
    }

    #[test]
    fn the_playlists_dots_offer_create_playlist_create_folder_and_sorting() {
        let (ctx, mut app) = accessible_app("playlists-dots");
        app.open(Page::Playlists);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let more = header_button(&tree, "More", 60.0, 130.0).expect("the dots beside the title");
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                more,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let labels = parity_menu_labels(&tree, 250.0);
        let at = |label: &str| {
            labels
                .iter()
                .position(|item| item == label)
                .unwrap_or_else(|| panic!("missing {label:?} in {labels:?}"))
        };
        assert!(at("Create playlist") < at("Create folder"));
        assert!(at("Create folder") < at("Sorting"));
        app.backend.shutdown();
    }

    #[test]
    fn a_playlist_of_the_library_offers_move_to_folder_after_add_to_playlist() {
        let (ctx, mut app) = accessible_app("move-to-folder-menu");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some("Late night focus")
                    && node.bounds().is_some_and(|bounds| bounds.x0 < 250.0)
            })
            .expect("the sidebar row")
            .1
            .bounds()
            .unwrap();
        let center = egui::pos2(
            ((row.x0 + row.x1) / 2.0) as f32,
            ((row.y0 + row.y1) / 2.0) as f32,
        );
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(center, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let labels = parity_menu_labels(&tree, 0.0);
        let at = |label: &str| {
            labels
                .iter()
                .position(|item| item == label)
                .unwrap_or_else(|| panic!("missing {label:?} in {labels:?}"))
        };
        // TIDAL: Play next, Add to playlist, Move to folder, Save for Later.
        assert!(at("Play next") < at("Move to folder"));
        assert!(at("Move to folder") < at("Save for Later"));
        app.backend.shutdown();
    }

    #[test]
    fn an_empty_videos_collection_links_to_tidals_top_videos() {
        let (ctx, mut app) = accessible_app("videos-empty");
        app.library.videos.items.clear();
        app.library.videos.loaded_once = true;
        app.library.videos.loading = false;
        app.open(Page::Videos);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let button = accessible_node(
            &tree,
            "View TIDAL's top videos",
            egui::accesskit::Role::Button,
        );
        // TIDAL's empty Videos page has no "Videos" title of its own.
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Videos")
                    && node.bounds().is_some_and(|bounds| bounds.x0 > 250.0)),
        );
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                button,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(
            app.page(),
            &Page::Shelf(crate::ui::videos::TOP_VIDEOS_PATH.to_string())
        );
        app.backend.shutdown();
    }

    #[test]
    fn move_to_folder_create_folder_files_the_playlist_in_the_new_folder() {
        let (ctx, mut app) = accessible_app("create-folder-and-move");
        app.apply(crate::model::Action::CreateFolder("Chill".into()), &ctx);
        let pending = format!("pending-{}", app.library.pending_folder_serial);
        app.apply(
            crate::model::Action::MoveToFolder {
                playlist_id: "pl1".into(),
                folder_id: Some(pending.clone()),
            },
            &ctx,
        );
        let folder = app
            .library
            .folders
            .iter()
            .find(|folder| folder.id == pending)
            .expect("the new folder");
        assert_eq!(folder.playlist_ids, ["pl1"]);
        app.backend.shutdown();
    }

    #[test]
    fn the_playlist_and_folder_dialogs_are_tidals_cards() {
        // TIDAL's cards: 620 wide, and 486, 486, 236 and 176 high.
        let cards = vec![
            (
                Dialog::CreatePlaylist {
                    name: String::new(),
                    public: false,
                    add_uris: Vec::new(),
                    description: String::new(),
                },
                486.0,
            ),
            (
                Dialog::EditPlaylist {
                    cover: Default::default(),
                    id: "pl1".into(),
                    name: "Late night focus".into(),
                    description: String::new(),
                    public: Some(false),
                },
                486.0,
            ),
            (
                Dialog::FolderName {
                    id: None,
                    name: String::new(),
                    move_playlist: None,
                },
                236.0,
            ),
            (
                Dialog::ConfirmDeletePlaylist {
                    id: "pl1".into(),
                    name: "Late night focus".into(),
                    owned: true,
                },
                176.0,
            ),
        ];
        for (dialog, height) in cards {
            let label = format!("{dialog:?}");
            let (ctx, mut app) = accessible_app("dialog-card");
            app.dialog = Some(dialog);
            for _ in 0..3 {
                accessible_frame(&ctx, &mut app, vec![]);
            }
            let rect = app.dialog_rect.expect("the dialog was drawn");
            assert_eq!(rect.width(), 620.0, "{label}");
            assert_eq!(rect.height(), height, "{label}");
            app.backend.shutdown();
        }
    }

    #[test]
    fn the_profile_menu_rows_are_42_apart_and_hang_below_the_bar_as_tidals_do() {
        let (ctx, mut app) = accessible_app("profile-menu-rows");
        accessible_frame(&ctx, &mut app, vec![]);
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(egui::pos2(1250.0, 30.0), egui::PointerButton::Primary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let top = |label: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label) && node.bounds().is_some())
                .unwrap_or_else(|| panic!("missing {label:?}"))
                .1
                .bounds()
                .unwrap()
                .y0
        };
        assert_eq!(top("Settings") - top("Manage subscription"), 42.0);
        assert_eq!(top("Help") - top("Settings"), 42.0);
        // The menu starts at the bar's lower edge (56), its rows 4 inside.
        // On macOS the bar is taller by the traffic lights' row, and the
        // avatar the menu hangs from sits half of that lower.
        assert_eq!(
            top("Profile"),
            60.0 + f64::from(crate::theme::titlebar_inset(&ctx)) / 2.0
        );
        app.backend.shutdown();
    }

    #[test]
    fn an_own_playlist_header_adds_collaborators_through_tidals_card() {
        let (ctx, mut app) = accessible_app("collaborators-card");
        app.open(Page::Playlist("pl1".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let add = header_button(&tree, "Add collaborator", 100.0, 260.0)
            .expect("the round + after the owner's name");
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(add, egui::accesskit::Action::Click, None)],
        );
        for _ in 0..3 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        assert!(matches!(
            app.dialog,
            Some(Dialog::Collaborators { ref playlist_id }) if playlist_id == "pl1"
        ));
        let rect = app.dialog_rect.expect("the card was drawn");
        assert_eq!((rect.width(), rect.height()), (620.0, 242.0));
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        for label in ["Copy invitation link", "Close"] {
            accessible_node(&tree, label, egui::accesskit::Role::Button);
        }
        app.backend.shutdown();
    }

    #[test]
    fn deleting_a_playlist_asks_in_tidals_words_with_a_close_button() {
        let (ctx, mut app) = accessible_app("dialog-delete-words");
        app.dialog = Some(Dialog::ConfirmDeletePlaylist {
            id: "pl1".into(),
            name: "Late night focus".into(),
            owned: true,
        });
        for _ in 0..2 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        for label in ["Cancel", "Delete", "Close"] {
            accessible_node(&tree, label, egui::accesskit::Role::Button);
        }
        app.backend.shutdown();
    }

    #[test]
    fn the_playlist_menu_is_as_narrow_as_the_other_menus() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("playlist-menu-width");
        let button = |tree: &egui::accesskit::TreeUpdate, label: &str| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.role() == Role::Button && node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing button {label}"))
                .1
                .bounds()
                .unwrap();
            (
                bounds.x0 as f32,
                bounds.x1 as f32,
                bounds.y0 as f32,
                bounds.y1 as f32,
            )
        };

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let (x0, x1, y0, y1) = button(&tree, "Late night focus");
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(
                egui::pos2((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                egui::PointerButton::Secondary,
            ),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        // A menu item spans its menu, so its width is the menu's.
        let (x0, x1, _, _) = button(&tree, "Play now");
        // TIDAL's 240-point menu, less its 4-point inset on either side.
        assert_eq!(x1 - x0, crate::ui::menus::MENU_WIDTH);
        app.backend.shutdown();
    }

    #[test]
    fn library_grid_ignores_song_and_card_drops_behind_expanded_art() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("library-grid-art-drops");
        app.settings.sidebar_grid = true;
        app.settings.art_expanded = true;
        assert!(app.now_playing().unwrap().art_url.is_some());
        let frame = |ctx: &egui::Context, app: &mut App, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::sidebar::show(app, ui),
            );
            output.textures_delta.clear();
            output
                .platform_output
                .accesskit_update
                .expect("sidebar tree")
        };
        let tree = frame(&ctx, &mut app, vec![]);
        let art = ctx
            .read_response(egui::Id::new("sidebar-art"))
            .expect("expanded artwork")
            .rect;
        let playlists = app.library.playlists.get().expect("demo playlists");
        // Pick an editable card actually covered by the artwork, regardless
        // of platform font metrics and title-bar height.
        let pos = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                let label = node.label()?;
                if node.role() != Role::Button
                    || !playlists
                        .iter()
                        .any(|playlist| playlist.name == label && app.can_edit_playlist(playlist))
                {
                    return None;
                }
                let bounds = node.bounds()?;
                let card = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                let overlap = card.intersect(art);
                (overlap.width() > 16.0 && overlap.height() > 16.0).then(|| overlap.center())
            })
            .expect("an editable card behind the artwork");
        // A visible card, for the control drop that must still land.
        let visible = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                let label = node.label()?;
                if node.role() != Role::Button
                    || label == "Late night focus"
                    || !playlists.iter().any(|playlist| playlist.name == label)
                {
                    return None;
                }
                let bounds = node.bounds()?;
                let pos = egui::pos2(bounds.x0 as f32 + 24.0, bounds.y0 as f32 + 24.0);
                (!art.contains(pos) && bounds.x0 < 250.0 && bounds.y1 < f64::from(art.top()))
                    .then_some(pos)
            })
            .expect("a visible playlist card");
        assert!(!art.contains(visible));
        let release = |app: &mut App, pos| {
            frame(&ctx, app, vec![egui::Event::PointerMoved(pos)]);
            frame(
                &ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };
        app.actions.clear();
        let song = PlayableItem::Track(Track {
            uri: "tidal:track:art-drop".into(),
            name: "Art drop".into(),
            ..Default::default()
        });
        egui::DragAndDrop::set_payload(
            &ctx,
            DragTrack {
                title: "Art drop".into(),
                image: None,
                items: vec![song],
                from: None,
                source_playlist: None,
            },
        );
        release(&mut app, pos);
        assert!(!app.actions.iter().any(|action| matches!(
            action,
            Action::AddToPlaylist { .. } | Action::SetSavedMany { .. }
        )));
        egui::DragAndDrop::clear_payload(&ctx);

        let drag_card = || {
            egui::DragAndDrop::set_payload(
                &ctx,
                DragEntry {
                    uri: "tidal:playlist:pl1".into(),
                    title: "Late night focus".into(),
                    image: None,
                },
            );
        };
        let rearranged = |app: &App| {
            app.actions
                .iter()
                .any(|action| matches!(action, Action::ArrangeLibrary { .. }))
        };
        drag_card();
        release(&mut app, pos);
        assert!(!rearranged(&app), "a card dropped on the artwork moved");
        egui::DragAndDrop::clear_payload(&ctx);

        drag_card();
        release(&mut app, visible);
        assert!(rearranged(&app), "a card dropped on a visible card stayed");
        egui::DragAndDrop::clear_payload(&ctx);
        app.backend.shutdown();
    }

    #[test]
    fn library_grid_highlights_a_song_drop_target_during_drag() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("library-grid-drop-highlight");
        app.settings.sidebar_grid = true;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let liked = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == Role::Button && node.label() == Some("Late night focus"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .expect("Late night focus card");
        let pos = egui::pos2(liked.x0 as f32 + 24.0, liked.y0 as f32 + 24.0);
        egui::DragAndDrop::set_payload(
            &ctx,
            DragTrack {
                title: "Art drop".into(),
                image: None,
                items: vec![PlayableItem::Track(Track {
                    uri: "tidal:track:highlight".into(),
                    ..Default::default()
                })],
                from: None,
                source_playlist: None,
            },
        );
        let run = |app: &mut App, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            output
        };
        // Hold the button down away from the card, as a real drag does:
        // egui then stops reporting other widgets as hovered, so the
        // outline has to follow the pointer instead.
        let start = egui::pos2(1270.0, 795.0);
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run(&mut app, vec![egui::Event::PointerMoved(pos)]);
        let output = run(&mut app, vec![egui::Event::PointerMoved(pos)]);
        assert!(ctx.input(|input| input.pointer.primary_down()));
        assert!(
            output.shapes.iter().any(|clipped| matches!(&clipped.shape,
                egui::epaint::Shape::Rect(rect)
                    if rect.stroke.width == 2.0
                        && rect.stroke.color == app.palette.accent
                        && rect.rect.contains(pos)
            )),
            "no accent outline on the drop target"
        );
        egui::DragAndDrop::clear_payload(&ctx);
        app.backend.shutdown();
    }

    #[test]
    fn library_grid_cards_navigate_and_their_corner_buttons_play() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("library-grid-card-actions");
        app.settings.sidebar_grid = true;
        app.open(Page::Search);

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let card = accessible_node(&tree, "Sunday morning", Role::Button);
        let previous_context = app.playing_context_uri();
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(card, AccessibleAction::Click, None)],
        );
        assert_eq!(app.page(), &Page::Playlist("pl2".into()));
        assert_eq!(app.playing_context_uri(), previous_context);

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let play = accessible_node(&tree, "Play Sunday morning", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(play, AccessibleAction::Click, None)],
        );
        assert_eq!(
            app.playing_context_uri().as_deref(),
            Some("tidal:playlist:pl2")
        );
        assert_eq!(app.page(), &Page::Playlist("pl2".into()));
        app.backend.shutdown();
    }

    #[test]
    fn double_clicking_a_library_grid_card_only_navigates() {
        let (ctx, mut app) = accessible_app("library-grid-double-click");
        app.settings.sidebar_grid = true;
        app.open(Page::Search);
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let card = sidebar_text(&painted, "Sunday morning").center();
        app.actions.clear();

        let [first, second] = double_click(card);
        view_frame(&ctx, &mut app, first, view);
        view_frame(&ctx, &mut app, second, view);

        assert!(played_contexts(&app).is_empty());
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::Open(Page::Playlist(id)) if id == "pl2"))
        );
        app.backend.shutdown();
    }

    fn settings_filter(ctx: &egui::Context) -> String {
        ctx.data_mut(|data| {
            data.get_temp::<String>(egui::Id::new("settings-filter"))
                .unwrap_or_default()
        })
    }

    /// Whether the tree has a node with this label and role.
    fn has_node(
        tree: &egui::accesskit::TreeUpdate,
        label: &str,
        role: egui::accesskit::Role,
    ) -> bool {
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some(label) && node.role() == role)
    }

    #[test]
    fn tidal_settings_rows_flip_their_settings() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("tidal-settings-rows");
        app.open(Page::Settings);
        assert!(app.settings.allow_explicit && !app.settings.allow_ai);
        for (query, label, read) in [
            (
                "Allow explicit",
                "Allow explicit content",
                (|app: &App| app.settings.allow_explicit) as fn(&App) -> bool,
            ),
            ("Allow AI", "Allow AI content", |app| !app.settings.allow_ai),
            ("Audio metadata", "Audio metadata", |app| {
                !app.settings.audio_metadata
            }),
            ("Hide Uploads", "Hide Uploads tab", |app| {
                !app.settings.hide_uploads_tab
            }),
            ("Show collection", "Show collection in sidebar", |app| {
                !app.settings.collection_in_sidebar
            }),
        ] {
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("settings-filter"), query.to_string())
            });
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let row = accessible_node(&tree, label, Role::CheckBox);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(row, AccessibleAction::Click, None)],
            );
            // `read` is true while the switch still holds its default.
            assert!(!read(&app), "{label} did not flip");
        }
        app.backend.shutdown();
    }

    /// Whether the album row for `name` is greyed out (disabled).
    fn row_is_greyed(tree: &egui::accesskit::TreeUpdate, name: &str) -> bool {
        let prefix = format!("Play {name},");
        tree.nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|label| label.starts_with(&prefix)))
            .unwrap_or_else(|| panic!("no row for {name}"))
            .1
            .is_disabled()
    }

    /// The glyph alpha of every text shape reading `text`.
    fn text_alphas(ctx: &egui::Context, app: &mut App, text: &str) -> Vec<f32> {
        fn collect(shape: &egui::epaint::Shape, text: &str, out: &mut Vec<f32>) {
            match shape {
                egui::epaint::Shape::Text(shape) if shape.galley.job.text == text => {
                    let glyphs = shape
                        .galley
                        .rows
                        .iter()
                        .flat_map(|row| row.row.visuals.mesh.vertices.iter())
                        .map(|vertex| f32::from(vertex.color.a()) / 255.0)
                        .reduce(f32::max)
                        .unwrap_or(1.0);
                    out.push(glyphs.min(f32::from(shape.fallback_color.a()) / 255.0));
                }
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|s| collect(s, text, out));
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| app.frame_ui(ui),
        );
        output.textures_delta.clear();
        let mut found = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, text, &mut found);
        }
        assert!(!found.is_empty(), "{text:?} was not painted");
        found
    }

    /// The colour every text shape reading `text` is painted in: its
    /// glyphs' own colour, or the shape's fallback where they have none.
    fn text_colors(ctx: &egui::Context, app: &mut App, text: &str) -> Vec<egui::Color32> {
        fn collect(shape: &egui::epaint::Shape, text: &str, out: &mut Vec<egui::Color32>) {
            match shape {
                egui::epaint::Shape::Text(shape) if shape.galley.job.text == text => {
                    let glyph = shape
                        .galley
                        .rows
                        .iter()
                        .flat_map(|row| row.row.visuals.mesh.vertices.iter())
                        .map(|vertex| vertex.color)
                        .next()
                        .unwrap_or(egui::Color32::PLACEHOLDER);
                    out.push(if glyph == egui::Color32::PLACEHOLDER {
                        shape.override_text_color.unwrap_or(shape.fallback_color)
                    } else {
                        glyph
                    });
                }
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|s| collect(s, text, out));
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| app.frame_ui(ui),
        );
        output.textures_delta.clear();
        let mut found = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, text, &mut found);
        }
        assert!(!found.is_empty(), "{text:?} was not painted");
        found
    }

    /// Where each text shape of a 1280-wide frame is painted, the window
    /// tall enough to show a whole artist page.
    fn painted_texts(ctx: &egui::Context, app: &mut App) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(shape) => out.push((
                    shape.galley.job.text.clone(),
                    shape.galley.rect.translate(shape.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| app.frame_ui(ui),
        );
        output.textures_delta.clear();
        let mut found = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &mut found);
        }
        found
    }

    /// Whether TIDAL's E is painted just after the title `name`.
    fn e_after(texts: &[(String, egui::Rect)], name: &str) -> bool {
        let title = texts
            .iter()
            .find(|(text, _)| text == name)
            .unwrap_or_else(|| panic!("{name:?} was not painted"))
            .1;
        texts.iter().any(|(text, rect)| {
            text == "E"
                && (rect.center().y - title.center().y).abs() < 3.0
                && rect.center().x > title.right()
                && rect.center().x < title.right() + 24.0
        })
    }

    /// TIDAL draws its E after an explicit release's title on the artist
    /// page's shelves and on search's album cards; Tidalfast drew none, as
    /// its albums did not keep TIDAL's `explicit`.
    #[test]
    fn explicit_releases_carry_tidals_e_on_their_cards() {
        let (ctx, mut app) = accessible_app("album-card-explicit");
        // Titles no other card or row on the page shares.
        let (explicit, clean) = ("The Explicit Version", "The Clean Version");
        {
            let albums = &mut app.artist_pages.get_mut("art0").unwrap().albums;
            let items = &mut albums
                .get_mut(DiscographyFilter::Albums.groups())
                .unwrap()
                .items;
            items[0].name = clean.into();
            items[0].explicit = false;
            items[1].name = explicit.into();
            items[1].explicit = true;
        }
        app.open(Page::Artist("art0".into()));
        painted_texts(&ctx, &mut app);
        let texts = painted_texts(&ctx, &mut app);
        assert!(e_after(&texts, explicit), "no E after {explicit}");
        assert!(!e_after(&texts, clean), "an E after {clean}");
        app.open(Page::ArtistSection("art0".into(), ArtistSection::Albums));
        painted_texts(&ctx, &mut app);
        let texts = painted_texts(&ctx, &mut app);
        assert!(
            e_after(&texts, explicit),
            "no E after {explicit} in View all"
        );
        assert!(!e_after(&texts, clean), "an E after {clean} in View all");
        app.backend.shutdown();
    }

    /// TIDAL lists two releases of one title (J. Cole's two Come Up
    /// Mixtapes Vol. 1) as two cards, and live albums on a shelf of their
    /// own; Tidalfast kept one card per title.
    #[test]
    fn the_artist_page_shows_every_version_tidal_lists() {
        let (ctx, mut app) = accessible_app("artist-two-versions");
        let (twice, live) = {
            let albums = &mut app.artist_pages.get_mut("art0").unwrap().albums;
            let items = &mut albums
                .get_mut(DiscographyFilter::Albums.groups())
                .unwrap()
                .items;
            items[0].name = "Released Twice".into();
            items[1].name = "Released Twice".into();
            items[2].album_type = Some("live".into());
            items[2].name = "Live from the Studio".into();
            (items[0].name.clone(), items[2].name.clone())
        };
        app.open(Page::Artist("art0".into()));
        painted_texts(&ctx, &mut app);
        let texts = painted_texts(&ctx, &mut app);
        let cards = texts.iter().filter(|(text, _)| *text == twice).count();
        assert_eq!(cards, 2, "both versions of {twice}");
        let shelf = texts
            .iter()
            .find(|(text, _)| text == "Live albums")
            .expect("a Live albums shelf")
            .1;
        let card = texts
            .iter()
            .find(|(text, _)| *text == live)
            .expect("the live album's card")
            .1;
        assert!(
            card.top() > shelf.bottom(),
            "the live album is on its shelf"
        );
        app.open(Page::ArtistSection("art0".into(), ArtistSection::Albums));
        painted_texts(&ctx, &mut app);
        let texts = painted_texts(&ctx, &mut app);
        assert_eq!(texts.iter().filter(|(text, _)| *text == twice).count(), 2);
        assert!(!texts.iter().any(|(text, _)| *text == live));
        app.backend.shutdown();
    }

    #[test]
    fn home_tabs_at_rest_are_grey_as_in_tidal() {
        let (ctx, mut app) = accessible_app("home-tab-colours");
        app.open(Page::Home);
        text_colors(&ctx, &mut app, "Staff Picks");
        let secondary = app.palette.secondary;
        let text = app.palette.text;
        assert_eq!(text_colors(&ctx, &mut app, "For you"), vec![text]);
        assert_eq!(text_colors(&ctx, &mut app, "Staff Picks"), vec![secondary]);
        app.backend.shutdown();
    }

    #[test]
    fn view_all_at_rest_is_grey_as_in_tidal() {
        let (ctx, mut app) = accessible_app("view-all-colour");
        app.open(crate::ui::explore::page());
        text_colors(&ctx, &mut app, "View all");
        let secondary = app.palette.secondary;
        let colors = text_colors(&ctx, &mut app, "View all");
        assert!(colors.iter().all(|color| *color == secondary), "{colors:?}");
        app.backend.shutdown();
    }

    #[test]
    fn settings_tabs_at_rest_are_grey_as_in_tidal() {
        let (ctx, mut app) = accessible_app("settings-tab-colours");
        app.open(Page::Settings);
        text_colors(&ctx, &mut app, "Account");
        let secondary = app.palette.secondary;
        let text = app.palette.text;
        assert_eq!(text_colors(&ctx, &mut app, "General"), vec![text]);
        // The tab's colour fades (0.1 s); a settled frame is grey.
        assert!(
            text_colors(&ctx, &mut app, "Account")
                .iter()
                .all(|color| *color == secondary)
        );
        app.backend.shutdown();
    }

    #[test]
    fn a_disabled_row_is_painted_at_half_opacity_like_tidals() {
        let (ctx, mut app) = accessible_app("disabled-row-opacity");
        let (explicit, plain) = {
            let tracks = &mut app.album_pages.get_mut("alb0").unwrap().tracks.items;
            tracks[0].explicit = true;
            (tracks[0].name.clone(), tracks[2].name.clone())
        };
        app.open(Page::Album("alb0".into()));
        text_alphas(&ctx, &mut app, &explicit);
        assert!(
            text_alphas(&ctx, &mut app, &explicit)
                .iter()
                .all(|a| *a == 1.0)
        );
        app.settings.allow_explicit = false;
        let dimmed = text_alphas(&ctx, &mut app, &explicit);
        assert!(
            dimmed.iter().any(|alpha| (alpha - 0.5).abs() < 0.02),
            "{dimmed:?}"
        );
        assert!(
            text_alphas(&ctx, &mut app, &plain)
                .iter()
                .all(|a| *a == 1.0),
            "other rows keep full opacity"
        );
        app.backend.shutdown();
    }

    #[test]
    fn explicit_and_ai_off_grey_out_those_tracks_in_an_album() {
        let (ctx, mut app) = accessible_app("content-greyed");
        // Both start allowed; the test turns them off one at a time.
        app.settings.allow_ai = true;
        let (explicit, ai, plain) = {
            let tracks = &mut app.album_pages.get_mut("alb0").unwrap().tracks.items;
            tracks[0].explicit = true;
            tracks[1].ai = true;
            (
                tracks[0].name.clone(),
                tracks[1].name.clone(),
                tracks[2].name.clone(),
            )
        };
        app.open(Page::Album("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        for name in [&explicit, &ai, &plain] {
            assert!(!row_is_greyed(&tree, name), "{name} greyed by default");
        }
        app.settings.allow_explicit = false;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(row_is_greyed(&tree, &explicit));
        assert!(!row_is_greyed(&tree, &ai));
        assert!(!row_is_greyed(&tree, &plain));
        app.settings.allow_explicit = true;
        app.settings.allow_ai = false;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(!row_is_greyed(&tree, &explicit));
        assert!(row_is_greyed(&tree, &ai));
        assert!(!row_is_greyed(&tree, &plain));
        app.backend.shutdown();
    }

    #[test]
    fn a_track_tidal_labels_ai_shows_the_ai_badge_in_an_album() {
        fn album(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        let (ctx, mut app) = accessible_app("ai-badge");
        let badges = |text: &[(String, egui::Rect)]| text.iter().filter(|(t, _)| t == "AI").count();
        app.album_pages.get_mut("alb0").unwrap().tracks.items[1].ai = true;
        let flagged = view_frame(&ctx, &mut app, vec![], album);
        assert_eq!(badges(&flagged), 1, "{flagged:?}");
        app.backend.shutdown();
    }

    #[test]
    fn audio_metadata_adds_bpm_and_key_columns_to_an_album() {
        fn album(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        let (ctx, mut app) = accessible_app("audio-metadata-columns");
        let has = |text: &[(String, egui::Rect)], word: &str| text.iter().any(|(t, _)| t == word);
        let off = view_frame(&ctx, &mut app, vec![], album);
        assert!(!has(&off, "BPM") && !has(&off, "KEY"));
        app.settings.audio_metadata = true;
        let on = view_frame(&ctx, &mut app, vec![], album);
        assert!(has(&on, "BPM") && has(&on, "KEY"), "{on:?}");
        app.settings.audio_metadata = false;
        let off = view_frame(&ctx, &mut app, vec![], album);
        assert!(!has(&off, "BPM"));
        app.backend.shutdown();
    }

    #[test]
    fn hide_uploads_tab_removes_it_from_home_and_leaves_the_others() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("hide-uploads-tab");
        app.open(Page::Home);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(has_node(&tree, "Uploads", Role::Button));
        app.settings.hide_uploads_tab = true;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(!has_node(&tree, "Uploads", Role::Button));
        assert!(has_node(&tree, "Staff Picks", Role::Button));
        // A tab left open when it is hidden falls back to For you.
        app.settings.hide_uploads_tab = false;
        app.home_tab = Some("uploads");
        app.settings.hide_uploads_tab = true;
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(app.home_tab, None);
        app.backend.shutdown();
    }

    /// TIDAL's Hide Uploads tab also hides the sidebar's Upload row.
    #[test]
    fn hide_uploads_tab_hides_the_sidebar_upload_row_too() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("hide-upload-row");
        app.open(Page::Home);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(has_node(&tree, "Upload", Role::Button));
        app.settings.hide_uploads_tab = true;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(!has_node(&tree, "Upload", Role::Button));
        assert!(has_node(&tree, "Feed", Role::Button));
        app.backend.shutdown();
    }

    /// The Upload page: the releases with TIDAL's card lines, the one being
    /// made first with UPLOADING, and a card opens its release's dialog.
    #[test]
    fn upload_page_lists_releases_and_a_card_opens_its_dialog() {
        fn view(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::uploads::show(app, ui);
        }
        let (ctx, mut app) = accessible_app("upload-page");
        let text = view_frame(&ctx, &mut app, vec![], view);
        for label in [
            "Upload",
            "Morning Take",
            "UPLOADING...",
            "Night Drive Sketches",
            "· 4 TRACKS",
            "PUBLIC · 1 TRACK",
        ] {
            assert!(
                text.iter().any(|(text, _)| text == label),
                "no {label}: {text:?}"
            );
        }
        let card = text
            .iter()
            .find(|(text, _)| text == "Night Drive Sketches")
            .unwrap()
            .1
            .center()
            - egui::vec2(0.0, 60.0);
        view_frame(
            &ctx,
            &mut app,
            pointer_click(card, egui::PointerButton::Primary),
            view,
        );
        assert!(app.actions.iter().any(|action| matches!(
            action,
            Action::Upload(crate::uploads::UploadAction::OpenAlbum(
                crate::uploads::AlbumKey::Id(id)
            )) if id == "up1"
        )));
        app.backend.shutdown();
    }

    /// Right-click on a card of the Upload page offers TIDAL's menu: Edit
    /// album, Share (Copy link) and Delete album, in that order.
    #[test]
    fn upload_card_menu_offers_edit_share_and_delete() {
        fn view(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::uploads::show(app, ui);
        }
        fn open_menu(ctx: &egui::Context, app: &mut App) -> Vec<(String, egui::Rect)> {
            let text = view_frame(ctx, app, vec![], view);
            let card = text
                .iter()
                .find(|(text, _)| text == "Night Drive Sketches")
                .unwrap()
                .1
                .center()
                - egui::vec2(0.0, 60.0);
            view_frame(
                ctx,
                app,
                pointer_click(card, egui::PointerButton::Secondary),
                view,
            );
            view_frame(ctx, app, vec![], view)
        }
        fn find(text: &[(String, egui::Rect)], label: &str) -> egui::Rect {
            text.iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("no {label}: {text:?}"))
                .1
        }
        let (ctx, mut app) = accessible_app("upload-card-menu");
        let text = open_menu(&ctx, &mut app);
        let edit = find(&text, "Edit album");
        let share = find(&text, "Share");
        let delete = find(&text, "Delete album");
        assert!(edit.top() < share.top() && share.top() < delete.top());
        // A right-click is not a click: no dialog opened, nothing queued.
        assert!(app.actions.is_empty());
        app.backend.shutdown();

        // Copy link, in the Share submenu: the release's address on the
        // clipboard and the toast.
        let (ctx, mut app) = accessible_app("upload-card-menu-copy");
        let text = open_menu(&ctx, &mut app);
        let share = find(&text, "Share").center();
        view_frame(&ctx, &mut app, vec![egui::Event::PointerMoved(share)], view);
        let text = view_frame(&ctx, &mut app, vec![], view);
        let copy = find(&text, "Copy link").center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(copy, egui::PointerButton::Primary),
            view,
        );
        let action = app.actions.pop().expect("Copy link queued");
        assert!(matches!(
            &action,
            Action::Upload(crate::uploads::UploadAction::CopyAlbumLink(
                crate::uploads::AlbumKey::Id(id)
            )) if id == "up1"
        ));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.apply(action.clone(), ui.ctx());
        });
        output.textures_delta.clear();
        assert!(
            output
                .platform_output
                .commands
                .contains(&egui::OutputCommand::CopyText(
                    "https://tidal.com/upload/up1".into()
                ))
        );
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message == "Link copied")
        );
        app.backend.shutdown();

        // Delete album asks first, with the release's own confirmation.
        let (ctx, mut app) = accessible_app("upload-card-menu-delete");
        let text = open_menu(&ctx, &mut app);
        let delete = find(&text, "Delete album").center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(delete, egui::PointerButton::Primary),
            view,
        );
        let action = app.actions.pop().expect("Delete queued");
        assert!(matches!(
            &action,
            Action::Upload(crate::uploads::UploadAction::ConfirmDeleteAlbum(
                crate::uploads::AlbumKey::Id(id)
            )) if id == "up1"
        ));
        app.apply(action, &ctx);
        assert!(matches!(
            &app.dialog,
            Some(crate::model::Dialog::Upload(dialog))
                if matches!(**dialog, crate::uploads::UploadDialog::ConfirmDeleteAlbum { .. })
        ));
        // Nothing was deleted yet.
        assert!(app.backend.take_upload_requests().is_empty());
        app.backend.shutdown();

        // Edit album opens the release's dialog.
        let (ctx, mut app) = accessible_app("upload-card-menu-edit");
        let text = open_menu(&ctx, &mut app);
        let edit = find(&text, "Edit album").center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(edit, egui::PointerButton::Primary),
            view,
        );
        let action = app.actions.pop().expect("Edit queued");
        app.apply(action, &ctx);
        assert!(matches!(
            &app.dialog,
            Some(crate::model::Dialog::Upload(dialog))
                if matches!(**dialog, crate::uploads::UploadDialog::Edit(_))
        ));
        app.backend.shutdown();
    }

    /// TIDAL's release dialog: the cover with its two pills in a column at
    /// the left, the fields as boxes with their labels inside (the value
    /// under the label), the tracks under both, Delete and Done at the
    /// right of the foot.
    #[test]
    fn upload_details_dialog_is_two_columns_with_labels_inside_the_fields() {
        fn view(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::dialogs::show(app, ui.ctx());
        }
        let (ctx, mut app) = accessible_app("upload-details-layout");
        assert!(crate::uploads::demo::surface(&mut app, "upload-edit"));
        view_frame(&ctx, &mut app, vec![], view);
        let text = view_frame(&ctx, &mut app, vec![], view);
        let at = |label: &str| {
            text.iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("no {label}: {text:?}"))
                .1
        };
        let change = at("Change Image");
        let spectrogram = at("Create spectrogram");
        let title = at("Title");
        let value = at("Night Drive Sketches");
        let artist = at("Artist name");
        let kind = at("Album type");
        let visibility = at("Visibility");
        let tracks = at("Tracks");
        let done = at("Done");
        let delete = at("Delete");
        // The cover column is left of the fields; its pills stack.
        assert!(change.right() < title.left(), "{change:?} {title:?}");
        assert!(spectrogram.right() < title.left());
        assert!(spectrogram.top() > change.bottom());
        // Every field starts at the same x, one under the other.
        for label in [artist, kind] {
            assert!((label.left() - title.left()).abs() < 1.0);
        }
        assert!(artist.top() > title.top() && kind.top() > artist.top());
        // The label sits inside its box with the value right under it (the
        // old form put the label above a separate input).
        assert!((value.left() - title.left()).abs() < 1.0);
        assert!(value.top() > title.bottom() && value.top() - title.top() < 30.0);
        // The Visibility box carries its lock at the left: its label is
        // indented by the icon.
        assert!(visibility.left() > title.left() + 20.0);
        // Tracks come under the columns, and the foot is right-aligned.
        assert!(tracks.top() > visibility.bottom());
        assert!((done.center().y - delete.center().y).abs() < 1.0);
        assert!(delete.right() < done.left());
        assert!(done.right() > title.right() + 100.0);
        app.backend.shutdown();
    }

    #[test]
    fn collection_in_sidebar_lists_the_collection_pages_as_rows() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("collection-in-sidebar");
        app.open(Page::Home);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(has_node(&tree, "Collection", Role::Button));
        assert!(!has_node(&tree, "Mixes & Radio", Role::Button));
        app.settings.collection_in_sidebar = true;
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(!has_node(&tree, "Collection", Role::Button));
        for label in ["Mixes & Radio", "Albums", "Tracks", "Videos", "Profiles"] {
            assert!(has_node(&tree, label, Role::Button), "{label} row");
        }
        app.backend.shutdown();
    }

    /// My Activity's month tabs run past a narrow window: the wheel over the
    /// strip moves them sideways so the oldest month can be reached, and the
    /// wheel elsewhere does not.
    #[test]
    fn the_activity_month_tabs_scroll_sideways_with_the_wheel() {
        use egui::accesskit::Role;
        let size = egui::vec2(700.0, 800.0);
        let frame = |ctx: &egui::Context, app: &mut App, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            output
                .platform_output
                .accesskit_update
                .expect("screen-reader tree")
        };
        // The right edge of the oldest month's tab, and the strip's height.
        let oldest_right = |tree: &egui::accesskit::TreeUpdate| {
            tree.nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == Role::Button
                        && node.label().is_some_and(|l| l.contains('\u{2019}'))
                })
                .filter_map(|(_, node)| node.bounds())
                .map(|b| b.x1 as f32)
                .fold(0.0_f32, f32::max)
        };
        let strip_y = |tree: &egui::accesskit::TreeUpdate| {
            let b = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("This Month") && node.role() == Role::Button)
                .expect("This Month tab")
                .1
                .bounds()
                .unwrap();
            ((b.y0 + b.y1) / 2.0) as f32
        };
        let wheel = |y: f32, delta: egui::Vec2| {
            vec![
                egui::Event::PointerMoved(egui::pos2(300.0, y)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let (ctx, mut app) = accessible_app("activity-month-tabs");
        app.open(Page::TopArtists);
        frame(&ctx, &mut app, vec![]);
        let before = frame(&ctx, &mut app, vec![]);
        let y = strip_y(&before);
        let start = oldest_right(&before);
        assert!(
            start > size.x,
            "the oldest month starts out of reach ({start})"
        );
        // The ordinary wheel moves the strip until the oldest month is in
        // view (the wheel past the end would scroll the page instead).
        let mut tree = before;
        for _ in 0..40 {
            if oldest_right(&tree) <= size.x {
                break;
            }
            frame(&ctx, &mut app, wheel(y, egui::vec2(0.0, -60.0)));
            tree = frame(&ctx, &mut app, vec![]);
        }
        let end = oldest_right(&tree);
        assert!(end <= size.x, "the oldest month is in view ({end})");
        assert!(strip_y(&tree) == y, "the page itself did not move");
        // The wheel the other way moves it back.
        for _ in 0..2 {
            frame(&ctx, &mut app, wheel(y, egui::vec2(0.0, 100.0)));
            tree = frame(&ctx, &mut app, vec![]);
        }
        // The fingers lift and the scroll settles (eased, and on Linux
        // gliding): a gesture that went on at once would hold the axis it
        // started on, and what was left of the vertical one would still
        // be moving the tabs.
        for _ in 0..120 {
            tree = frame(&ctx, &mut app, vec![]);
        }
        let back = oldest_right(&tree);
        assert!(back > end, "the wheel moves the tabs back");
        // A sideways swipe to the left moves them on towards the oldest
        // month too.
        let y = strip_y(&tree);
        for _ in 0..8 {
            frame(&ctx, &mut app, wheel(y, egui::vec2(-400.0, 0.0)));
            tree = frame(&ctx, &mut app, vec![]);
        }
        assert!(
            oldest_right(&tree) < back,
            "a sideways swipe moves the tabs ({} vs {back})",
            oldest_right(&tree)
        );
        app.backend.shutdown();
    }

    /// TIDAL desktop draws the playing playlist in the sidebar (its name and
    /// the speaker over its cover) in its green-teal #16d8a7, measured from
    /// its plain captures, and the seek bar, the queue's playing song and
    /// links in the cyan accent #33ffee. Tidalfast drew the sidebar's in the
    /// cyan too. Light has no TIDAL reference: it keeps its one deep teal.
    #[test]
    fn the_playing_playlist_in_the_sidebar_is_tidals_teal_and_the_queue_stays_cyan() {
        for light in [false, true] {
            let (ctx, mut app) = accessible_app("sidebar-playing-teal");
            if light {
                app.settings.theme = crate::settings::ThemeChoice::Light;
                app.actions.push(Action::SettingsChanged);
            }
            app.show_queue_panel = true;
            for _ in 0..3 {
                accessible_frame(&ctx, &mut app, vec![]);
            }
            let colors = text_colors(&ctx, &mut app, "Late night focus");
            let accent = app.palette.accent;
            assert_eq!(
                accent,
                if light {
                    egui::Color32::from_rgb(0x00, 0x78, 0x6e)
                } else {
                    egui::Color32::from_rgb(0x33, 0xff, 0xee)
                },
                "the accent is TIDAL's #33ffee in Dark (measured on its seek bar)"
            );
            if light {
                assert!(!colors.contains(&crate::theme::TEAL), "{colors:?}");
            } else {
                assert!(
                    colors.contains(&crate::theme::TEAL),
                    "the sidebar's playing playlist is the teal: {colors:?}"
                );
            }
            assert!(
                colors.contains(&accent),
                "the queue's 'Playing from' link keeps the accent: {colors:?}"
            );
            app.backend.shutdown();
        }
    }

    /// TIDAL keeps Search's filter pills in one row that scrolls sideways
    /// (the wheel, a drag) when the page is too narrow for them, cut at the
    /// page's padding; it did wrap them to a second row in Tidalfast.
    #[test]
    fn the_search_pills_are_one_row_that_scrolls_when_they_do_not_fit() {
        use egui::accesskit::Role;
        const LABELS: [&str; 6] = [
            "Top results",
            "Profiles",
            "Tracks",
            "Uploads",
            "Albums",
            "Playlists",
        ];
        let frame = |ctx: &egui::Context, app: &mut App, size: egui::Vec2, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            output
                .platform_output
                .accesskit_update
                .expect("screen-reader tree")
        };
        // Each pill's box, from the top-most button of its name (the pills
        // sit above the results).
        let pills = |tree: &egui::accesskit::TreeUpdate| {
            LABELS.map(|label| {
                tree.nodes
                    .iter()
                    .filter(|(_, node)| node.role() == Role::Button && node.label() == Some(label))
                    .filter_map(|(_, node)| node.bounds())
                    .min_by(|a, b| a.y0.total_cmp(&b.y0))
                    .map(|b| {
                        egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        )
                    })
                    .unwrap_or_else(|| panic!("no {label:?} pill"))
            })
        };
        // At normal width they fit in one row, as they did.
        let wide = egui::vec2(1280.0, 800.0);
        let (ctx, mut app) = accessible_app("search-pills-wide");
        app.open(Page::Search);
        frame(&ctx, &mut app, wide, vec![]);
        let tree = frame(&ctx, &mut app, wide, vec![]);
        let row = pills(&tree);
        assert!(row.iter().all(|p| p.top() == row[0].top()), "{row:?}");
        assert!(row[5].right() <= wide.x, "all of them fit: {row:?}");
        assert!((row[0].height() - 40.0).abs() < 0.5, "{row:?}");
        // Pressing one still chooses it.
        let centre = row[2].center();
        frame(
            &ctx,
            &mut app,
            wide,
            pointer_click(centre, egui::PointerButton::Primary),
        );
        frame(&ctx, &mut app, wide, vec![]);
        assert_eq!(app.search.filter, crate::model::SearchFilter::Songs);
        app.backend.shutdown();

        // Narrow, they stay one row that runs past the page's edge.
        let size = egui::vec2(700.0, 800.0);
        let (ctx, mut app) = accessible_app("search-pills-narrow");
        app.open(Page::Search);
        frame(&ctx, &mut app, size, vec![]);
        let tree = frame(&ctx, &mut app, size, vec![]);
        let start = pills(&tree);
        assert!(
            start.iter().all(|p| p.top() == start[0].top()),
            "one row, not wrapped: {start:?}"
        );
        assert!(
            start[5].right() > size.x,
            "the last pill is past the edge: {start:?}"
        );
        let y = start[0].center().y;
        // The wheel over the row moves it until the last pill is in view,
        // and the page itself does not move.
        let wheel = |delta: egui::Vec2| {
            vec![
                egui::Event::PointerMoved(egui::pos2(300.0, y)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let mut tree = tree;
        for _ in 0..40 {
            if pills(&tree)[5].right() <= size.x {
                break;
            }
            // Gently, so the row's end is not overshot: what is left of
            // a wheel past it goes on to the page.
            frame(&ctx, &mut app, size, wheel(egui::vec2(0.0, -20.0)));
            tree = frame(&ctx, &mut app, size, vec![]);
        }
        for _ in 0..60 {
            tree = frame(&ctx, &mut app, size, vec![]);
        }
        let end = pills(&tree);
        assert!(
            end[5].right() <= size.x,
            "the last pill is in view: {end:?}"
        );
        assert_eq!(end[0].top(), start[0].top(), "the page did not move");
        assert!(end[0].left() < start[0].left(), "the row moved");
        // A drag from a pill carries the row back, and does not press the
        // pill it started on.
        let filter = app.search.filter;
        let from = end[2].center();
        let to = egui::pos2(from.x + 160.0, from.y);
        frame(
            &ctx,
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for step in 1..=8 {
            let at = egui::pos2(from.x + 20.0 * step as f32, from.y);
            frame(&ctx, &mut app, size, vec![egui::Event::PointerMoved(at)]);
        }
        frame(
            &ctx,
            &mut app,
            size,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let tree = frame(&ctx, &mut app, size, vec![]);
        let back = pills(&tree);
        assert!(
            back[0].left() > end[0].left() + 100.0,
            "the drag moved the row: {} vs {}",
            back[0].left(),
            end[0].left()
        );
        assert_eq!(app.search.filter, filter, "the drag chose no pill");
        app.backend.shutdown();
    }

    /// With TIDAL's "Show collection in sidebar" the navigation is taller
    /// than a short window, or the room above the cover: it then scrolls
    /// with the playlists, so Downloaded can be reached. In a tall window
    /// it stays fixed above them.
    #[test]
    fn the_navigation_scrolls_with_the_playlists_when_it_does_not_fit() {
        use egui::accesskit::Role;
        let frame = |ctx: &egui::Context, app: &mut App, size: egui::Vec2, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::sidebar::show(app, ui),
            );
            output.textures_delta.clear();
            output
                .platform_output
                .accesskit_update
                .expect("sidebar tree")
        };
        let bounds = |tree: &egui::accesskit::TreeUpdate, label: &str| {
            let b = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label) && node.role() == Role::Button)
                .unwrap_or_else(|| panic!("missing {label}"))
                .1
                .bounds()
                .unwrap();
            (b.y0 as f32, b.y1 as f32)
        };
        let wheel = |amount: f32| {
            vec![
                egui::Event::PointerMoved(egui::pos2(100.0, 300.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -amount),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        // (window, whether the cover is shown, whether the navigation fits)
        for (size, cover, fits) in [
            (egui::vec2(1000.0, 560.0), false, false),
            (egui::vec2(1400.0, 900.0), true, false),
            (egui::vec2(1000.0, 1400.0), false, true),
        ] {
            // A fresh sidebar for each window: the scroll offset is kept.
            let (ctx, mut app) = accessible_app("sidebar-navigation-overflow");
            app.settings.collection_in_sidebar = true;
            app.settings.art_expanded = cover;
            frame(&ctx, &mut app, size, vec![]);
            let before = frame(&ctx, &mut app, size, vec![]);
            let (music, _) = bounds(&before, "Music");
            let mut tree = before;
            for _ in 0..4 {
                frame(&ctx, &mut app, size, wheel(300.0));
                tree = frame(&ctx, &mut app, size, vec![]);
            }
            let (_, downloaded) = bounds(&tree, "Downloaded");
            let floor = if cover {
                ctx.read_response(egui::Id::new("sidebar-art"))
                    .expect("cover")
                    .rect
                    .top()
            } else {
                size.y
            };
            assert!(
                downloaded <= floor,
                "{size:?} cover {cover}: Downloaded ends at {downloaded}, past {floor}"
            );
            let (moved, _) = bounds(&tree, "Music");
            assert_eq!(
                (moved - music).abs() < 0.5,
                fits,
                "{size:?} cover {cover}: Music at {music} then {moved}"
            );
            app.backend.shutdown();
        }
    }

    #[test]
    fn update_window_keeps_downloads_running_and_waits_for_restart() {
        use crate::updates::{DownloadState, Installation, Kind, Prepared};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("update-window");
        app.update = Some(crate::updates::Release {
            version: "9.9.9".into(),
            url: "https://example.invalid/release".into(),
        });
        let installation = Installation {
            executable: std::path::PathBuf::from("/test/tidalfast"),
            kind: Kind::Portable,
        };
        app.update_support = Some(Ok(installation.clone()));
        assert!(!app.show_update);
        app.actions.push(Action::ShowUpdate);
        accessible_frame(&ctx, &mut app, vec![]);
        // The window's first frame only measures it, and egui keeps
        // widgets it has not shown out of the screen-reader tree.
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let download = accessible_node(&tree, "Update now", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(download, AccessibleAction::Click, None)],
        );
        assert!(matches!(
            app.update_download,
            DownloadState::Downloading { .. }
        ));
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let close = accessible_node(&tree, "Later", Role::Link);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(close, AccessibleAction::Click, None)],
        );
        assert!(!app.show_update);
        assert!(matches!(
            app.update_download,
            DownloadState::Downloading { .. }
        ));
        app.update_download =
            DownloadState::Ready(Box::new(Prepared::sample(installation, "9.9.9")));
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(!app.show_update);
        assert!(matches!(app.update_download, DownloadState::Ready(_)));
        app.actions.push(Action::ShowUpdate);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let restart = accessible_node(&tree, "Restart to update", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(restart, AccessibleAction::Click, None)],
        );
        assert!(matches!(app.update_download, DownloadState::Installing));
        app.backend.shutdown();
    }
    #[test]
    fn the_cross_beside_recent_searches_clears_the_list_without_searching() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("search-history");
        app.open(Page::Search);
        // Recent searches stand in for results only while nothing is searched
        app.search.query.clear();
        app.search.committed.clear();
        assert!(!app.settings.recent_items.is_empty());
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let clear = accessible_node(&tree, "Clear recent searches", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(clear, AccessibleAction::Click, None)],
        );
        assert!(app.settings.recent_items.is_empty());
        assert!(
            app.search.committed.is_empty(),
            "the cross must clear the list without running a search"
        );
        app.backend.shutdown();
    }

    /// Taking a recent item opens it and moves it to the top, once.
    #[test]
    fn taking_a_recent_search_opens_the_item_and_moves_it_up() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("search-history-open");
        app.open(Page::Search);
        app.search.query.clear();
        app.search.committed.clear();
        let before = app.settings.recent_items.clone();
        let second = before[1].clone();
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = accessible_node(&tree, &second.title, Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(row, AccessibleAction::Click, None)],
        );
        assert_eq!(app.settings.recent_items.len(), before.len());
        assert_eq!(app.settings.recent_items[0].id, second.id);
        assert_eq!(app.settings.recent_items[1].id, before[0].id);
        assert!(matches!(app.page(), Page::Artist(id) if *id == second.id));
        app.backend.shutdown();
    }

    /// The first node whose label begins with `prefix`.
    fn accessible_node_starting(
        tree: &egui::accesskit::TreeUpdate,
        prefix: &str,
        role: egui::accesskit::Role,
    ) -> Option<egui::accesskit::NodeId> {
        tree.nodes
            .iter()
            .find(|(_, node)| {
                node.role() == role && node.label().is_some_and(|l| l.starts_with(prefix))
            })
            .map(|(id, _)| *id)
    }

    /// Every control of the search page that opens or plays something puts
    /// it in Recent Searches, as TIDAL's list does (live, 2026-10-17: the
    /// Tracks list, the video cards and more recorded nothing).
    #[test]
    fn every_search_page_control_records_what_it_opens() {
        use crate::model::SearchFilter::*;
        use crate::recent_searches::RecentKind;
        use egui::accesskit::{Action as AccessibleAction, Role};
        // (filter, label start, what is recorded)
        let cases: [(crate::model::SearchFilter, &str, RecentKind, &str); 10] = [
            (All, "Bonobo", RecentKind::Artist, "art0"),
            (All, "Fragments", RecentKind::Album, "alb0"),
            (All, "My Daily Discovery", RecentKind::Playlist, "pl0"),
            (All, "Rosewood", RecentKind::Track, "tidal:track:trk0"),
            (Artists, "Khruangbin", RecentKind::Artist, "art1"),
            (Albums, "Mordechai", RecentKind::Album, "alb1"),
            (Playlists, "Late night focus", RecentKind::Playlist, "pl1"),
            (Songs, "Play Otomo", RecentKind::Track, "tidal:track:trk1"),
            (Uploads, "Play Tides", RecentKind::Track, "tidal:track:trk3"),
            (
                Videos,
                "Live from the studio, part 1",
                RecentKind::Video,
                "vid0",
            ),
        ];
        for (filter, label, kind, id) in cases {
            let (ctx, mut app) = accessible_app("search-records");
            if let Some(results) = app.search.results.get_mut().map(Arc::make_mut) {
                results.videos = Some(page(vec![video(0)]));
            }
            app.settings.recent_items.clear();
            app.open(Page::Search);
            app.search.filter = filter;
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let target = accessible_node_starting(&tree, label, Role::Button)
                .unwrap_or_else(|| panic!("{filter:?}: no {label:?}"));
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(target, AccessibleAction::Click, None)],
            );
            accessible_frame(&ctx, &mut app, vec![]);
            let recorded: Vec<_> = app
                .settings
                .recent_items
                .iter()
                .map(|entry| (entry.kind, entry.id.as_str()))
                .collect();
            assert_eq!(recorded, [(kind, id)], "{filter:?}: {label}");
            app.backend.shutdown();
        }
    }

    /// Play next from a search result's menu records the song, as playing it
    /// does (reported: songs added with Play next never showed in Recent
    /// Searches).
    #[test]
    fn play_next_from_a_search_result_is_a_recent_search() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("search-play-next");
        app.settings.recent_items.clear();
        app.open(Page::Search);
        app.search.filter = crate::model::SearchFilter::Songs;
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = accessible_node_starting(&tree, "Play Otomo", Role::Button).unwrap();
        let bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == row)
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let center = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(center, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let play_next = accessible_node(&tree, "Play next", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(play_next, AccessibleAction::Click, None)],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        let recorded: Vec<_> = app
            .settings
            .recent_items
            .iter()
            .map(|entry| (entry.kind, entry.id.as_str()))
            .collect();
        assert_eq!(
            recorded,
            [(
                crate::recent_searches::RecentKind::Track,
                "tidal:track:trk1"
            )]
        );
        app.backend.shutdown();
    }

    /// A direct hit of the dropdown, taken from another page, is recorded.
    #[test]
    fn a_dropdown_direct_hit_is_a_recent_search() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("search-dropdown-hit");
        app.settings.recent_items.clear();
        app.open(Page::Home);
        app.search.query = "Nils".into();
        app.search.dropdown = true;
        app.search.suggestions = Some(crate::api::models::SearchSuggestions {
            query: "Nils".into(),
            direct_hits: vec![crate::api::models::TopHit::Artist(Artist {
                name: "Zed Dropdown".into(),
                ..artist(2)
            })],
            ..Default::default()
        });
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let hit = accessible_node(&tree, "Zed Dropdown", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(hit, AccessibleAction::Click, None)],
        );
        assert!(matches!(app.page(), Page::Artist(id) if id == "art2"));
        assert_eq!(app.settings.recent_items.len(), 1);
        assert_eq!(app.settings.recent_items[0].id, "art2");
        app.backend.shutdown();
    }

    /// In the search dropdown, each artist named under a track or album hit
    /// opens that artist; the rest of the row still takes the hit.
    #[test]
    fn dropdown_hit_artists_are_links_and_the_rest_of_the_row_takes_the_hit() {
        use egui::accesskit::Role;
        let artists = vec![
            ArtistRef {
                id: Some("mustard".into()),
                name: "Mustard".into(),
                uri: Some("tidal:artist:mustard".into()),
                picture: None,
            },
            ArtistRef {
                id: Some("yg".into()),
                name: "YG".into(),
                uri: Some("tidal:artist:yg".into()),
                picture: None,
            },
        ];
        let setup = |name: &str| {
            let (ctx, mut app) = accessible_app(name);
            app.settings.recent_items.clear();
            app.open(Page::Home);
            app.search.query = "Dangerous".into();
            app.search.dropdown = true;
            app.search.suggestions = Some(crate::api::models::SearchSuggestions {
                query: "Dangerous".into(),
                direct_hits: vec![crate::api::models::TopHit::Track(Track {
                    name: "Dangerous World".into(),
                    artists: artists.clone(),
                    ..track(1)
                })],
                ..Default::default()
            });
            accessible_frame(&ctx, &mut app, vec![]);
            (ctx, app)
        };
        let center = |tree: &egui::accesskit::TreeUpdate, label: &str, role| {
            let id = accessible_node(tree, label, role);
            let bounds = tree
                .nodes
                .iter()
                .find(|(node, _)| *node == id)
                .and_then(|(_, node)| node.bounds())
                .expect("bounds");
            egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            )
        };
        for (name, id) in [("Mustard", "mustard"), ("YG", "yg")] {
            let (ctx, mut app) = setup(&format!("dropdown-artist-{id}"));
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let pos = center(&tree, name, Role::Link);
            accessible_frame(
                &ctx,
                &mut app,
                pointer_click(pos, egui::PointerButton::Primary),
            );
            assert!(
                matches!(app.page(), Page::Artist(open) if open == id),
                "{name} opens its artist page"
            );
            app.backend.shutdown();
        }
        // The title area of the same row still takes the hit (plays it).
        let (ctx, mut app) = setup("dropdown-artist-row");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = center(&tree, "Dangerous World", Role::Button);
        let click = egui::pos2(row.x - 150.0, row.y - 12.0);
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(click, egui::PointerButton::Primary),
        );
        assert!(
            matches!(app.page(), Page::Home),
            "the row's click does not open an artist"
        );
        assert!(
            app.search.suggestions.is_some() && !app.search.dropdown,
            "the row took the hit and closed the dropdown"
        );
        app.backend.shutdown();
    }

    /// Settings with one opened playlist and seven typed queries of older
    /// versions: the empty field's dropdown lists the playlist, with its
    /// title, and no query.
    #[test]
    fn the_empty_dropdown_lists_the_opened_item_and_no_typed_queries() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("search-older-shape");
        app.settings.search_history =
            ["jazz", "piano", "pian", "synthwave", "syn", "lo fi", "test"]
                .map(String::from)
                .into();
        app.settings.recent_items = serde_json::from_str(
            r#"[{"kind":"playlist","id":"abc","title":"Weekend Classics","names":"TIDAL"}]"#,
        )
        .unwrap();
        app.open(Page::Home);
        app.search.query.clear();
        app.search.dropdown = true;
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Weekend Classics", Role::Button);
        for query in app.settings.search_history.clone() {
            assert!(
                accessible_node_starting(&tree, &query, Role::Button).is_none(),
                "{query} is listed"
            );
        }
        app.backend.shutdown();
    }

    /// #596: clearing the global search from another page empties the field
    /// without leaving that page.
    #[test]
    fn clearing_the_global_search_stays_on_the_current_page() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("global-search-clear");
        app.open(Page::Home);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(!app.search.query.is_empty());
        let clear = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some("Clear")
                    && node.role() == Role::Button
                    && node.bounds().is_some_and(|bounds| bounds.y1 < 80.0)
            })
            .expect("Clear beside the global search")
            .0;
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(clear, AccessibleAction::Click, None)],
        );
        for _ in 0..3 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        assert!(app.search.query.is_empty());
        assert!(
            matches!(app.page(), Page::Home),
            "clearing must not open the search page"
        );
        assert!(
            ctx.memory(|memory| memory.has_focus(egui::Id::new("global-search"))),
            "the field stays ready for the next query"
        );
        // Typing a new query shows the dropdown over the same page, as
        // TIDAL's; Enter opens the results.
        accessible_frame(&ctx, &mut app, vec![egui::Event::Text("Rework".into())]);
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(app.search.query, "Rework");
        assert!(matches!(app.page(), Page::Home));
        assert!(app.search.dropdown);
        app.backend.shutdown();
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Where the text shapes reading `text` start, in a 1280 x 800 frame.
    fn text_positions(ctx: &egui::Context, app: &mut App, text: &str) -> Vec<egui::Pos2> {
        fn collect(shape: &egui::epaint::Shape, text: &str, out: &mut Vec<egui::Pos2>) {
            match shape {
                egui::epaint::Shape::Text(shape) if shape.galley.job.text == text => {
                    out.push(shape.pos);
                }
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|s| collect(s, text, out));
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| app.frame_ui(ui),
        );
        output.textures_delta.clear();
        let mut found = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, text, &mut found);
        }
        found
    }

    #[test]
    fn the_player_bar_leaves_its_cover_to_the_now_playing_view_as_tidal_does() {
        let (ctx, mut app) = accessible_app("bar-cover-now-playing");
        let title = app.now_playing().expect("a song plays in the demo").title;
        let bar_title_left = |ctx: &egui::Context, app: &mut App| {
            text_positions(ctx, app, &title);
            text_positions(ctx, app, &title)
                .into_iter()
                .filter(|pos| pos.y > 800.0 - 88.0)
                .map(|pos| pos.x)
                .reduce(f32::min)
                .expect("the bar shows the title")
        };
        // Beside the 52-point cover.
        assert!(bar_title_left(&ctx, &mut app) > 70.0);
        app.now_playing_view = true;
        // No cover: the words start near the window's edge.
        assert!(bar_title_left(&ctx, &mut app) < 50.0);
        app.backend.shutdown();
    }

    /// The dropdown lies exactly under the pill (the field's width and left
    /// edge), keeps showing the last suggestions while the text changes, and
    /// is walked with the arrow keys.
    #[test]
    fn the_search_dropdown_matches_the_field_and_takes_the_keyboard() {
        let (ctx, mut app) = accessible_app("search-dropdown");
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("search"));
        app.search.highlight = None;
        app.search.focus_requested = true;
        for _ in 0..4 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        let id = egui::Id::new("global-search");
        let pill = crate::ui::widgets::search_pill(&ctx, id).expect("the pill was drawn");
        let area = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("search-dropdown")))
            .expect("the dropdown is open");
        assert_eq!(area.left(), pill.left());
        assert_eq!(area.width(), pill.width());
        // TIDAL desktop's dropdown starts 17 under the field.
        assert_eq!(area.top(), pill.bottom() + 17.0);
        // Down lights the first row, Up leaves again for the typed text.
        accessible_frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(app.search.highlight, Some(0));
        accessible_frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(app.search.highlight, Some(1));
        accessible_frame(&ctx, &mut app, vec![key(egui::Key::ArrowUp)]);
        accessible_frame(&ctx, &mut app, vec![key(egui::Key::ArrowUp)]);
        assert_eq!(app.search.highlight, None);
        // Backspacing keeps the rows of the longer text until new ones come.
        app.search.query = "emine".into();
        assert!(crate::ui::search::dropdown_len(&app) > 4);
        // Escape closes it.
        accessible_frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(!app.search.dropdown);
        app.backend.shutdown();
    }

    /// A queue row with more artists than fit keeps the whole names that
    /// fit and ends in one ellipsis, inside the panel: no run of dots and
    /// nothing past the panel's edge.
    #[test]
    fn a_queue_row_with_many_artists_stays_inside_the_panel() {
        let (ctx, mut app) = accessible_app("queue-many-artists");
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("queue"));
        if let Loadable::Loaded(queue) = &mut app.queue
            && let Some(PlayableItem::Track(track)) = queue.queue.first_mut()
        {
            track.artists = (0..8)
                .map(|index| crate::api::models::ArtistRef {
                    id: Some(format!("zz{index}")),
                    name: format!("Zzlong Artistname {index}"),
                    uri: None,
                    picture: None,
                })
                .collect();
        }
        let size = egui::vec2(820.0, 700.0);
        let mut shapes = Vec::new();
        for step in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(step as f64 / 10.0),
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            shapes = output.shapes;
        }
        let texts: Vec<_> = shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        let first = texts
            .iter()
            .find(|text| text.galley.text().starts_with("Zzlong Artistname 0"))
            .expect("the row's first artist was drawn");
        let row_y = first.pos.y;
        let mut ellipses = 0;
        for text in texts.iter().filter(|text| (text.pos.y - row_y).abs() < 3.0) {
            let right = text.pos.x + text.galley.rect.width();
            assert!(
                right <= size.x,
                "{:?} runs to {right}, past the window",
                text.galley.text()
            );
            ellipses += text
                .galley
                .rows
                .iter()
                .flat_map(|row| row.glyphs.iter())
                .filter(|glyph| glyph.chr == '\u{2026}')
                .count();
        }
        assert_eq!(ellipses, 1, "one ellipsis ends the artists");
        app.backend.shutdown();
    }

    /// A long suggestion ends inside the dropdown with an ellipsis instead
    /// of running past its right edge.
    #[test]
    fn a_long_search_suggestion_stays_inside_the_dropdown() {
        let (ctx, mut app) = accessible_app("search-long-suggestion");
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("search"));
        let long =
            "let me love you baby stevie ray vaughan & double trouble live at the el mocambo 1983";
        if let Some(found) = app.search.suggestions.as_mut() {
            found.suggestions.insert(
                0,
                crate::api::models::SearchSuggestion {
                    query: long.into(),
                    highlights: vec![(0, 20)],
                },
            );
        }
        app.search.focus_requested = true;
        let mut shapes = Vec::new();
        for step in 0..4 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(820.0, 700.0),
                    )),
                    time: Some(step as f64 / 10.0),
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            let mut output = output;
            output.textures_delta.clear();
            shapes = output.shapes;
        }
        let area = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("search-dropdown")))
            .expect("the dropdown is open");
        let mut found = false;
        for clipped in &shapes {
            if let egui::epaint::Shape::Text(text) = &clipped.shape
                && text.galley.text().starts_with("let me love you baby")
            {
                found = true;
                let right = text.pos.x + text.galley.rect.width();
                assert!(
                    right <= area.right(),
                    "the suggestion runs to {right}, past the dropdown's {}",
                    area.right()
                );
                assert_eq!(text.galley.rows.len(), 1, "one line");
                let last = text.galley.rows[0].glyphs.last().map(|glyph| glyph.chr);
                assert_eq!(last, Some('\u{2026}'), "it ends in an ellipsis");
            }
        }
        assert!(found, "the long suggestion was drawn");
        app.backend.shutdown();
    }

    /// Frames at `size`, returning the dropdown's rectangle.
    fn dropdown_rect_at(size: egui::Vec2, name: &str) -> egui::Rect {
        let (ctx, mut app) = accessible_app(name);
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("search-many"));
        app.search.focus_requested = true;
        for step in 0..6 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(step as f64 / 10.0),
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
        }
        app.backend.shutdown();
        ctx.memory(|memory| memory.area_rect(egui::Id::new("search-dropdown")))
            .expect("the dropdown is open")
    }

    /// A context menu opened from a direct hit lies above the dropdown (the
    /// menu is the top layer, so its items are what a click reaches), and
    /// the dropdown stays open at the size it had.
    #[test]
    fn a_context_menu_from_a_search_hit_is_above_the_dropdown_which_keeps_its_size() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("search-hit-menu");
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("search-many"));
        app.search.focus_requested = true;
        for _ in 0..5 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        let dropdown = egui::Id::new("search-dropdown");
        let before = ctx
            .memory(|memory| memory.area_rect(dropdown))
            .expect("the dropdown is open");
        let hit = app
            .search
            .suggestions
            .as_ref()
            .and_then(|found| {
                found.direct_hits.iter().find_map(|hit| match hit {
                    crate::api::models::TopHit::Track(track) => Some(track.name.clone()),
                    _ => None,
                })
            })
            .expect("a track among the hits");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row = accessible_node(&tree, &hit, Role::Button);
        let bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == row)
            .and_then(|(_, node)| node.bounds())
            .expect("the hit row's bounds");
        let at = egui::pos2(
            (bounds.x0 + 60.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        accessible_frame(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        accessible_frame(&ctx, &mut app, vec![]);
        accessible_frame(&ctx, &mut app, vec![button(true)]);
        accessible_frame(&ctx, &mut app, vec![button(false)]);
        for _ in 0..4 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        assert!(app.search.dropdown, "the dropdown stays open");
        assert!(egui::Popup::is_any_open(&ctx), "the menu is open");
        // Where the menu lies over the dropdown, the menu is what is hit.
        let (menu, rect) = ctx.memory(|memory| {
            memory
                .layer_ids()
                .filter(|layer| layer.order == egui::Order::Foreground && layer.id != dropdown)
                .find_map(|layer| memory.area_rect(layer.id).map(|rect| (layer, rect)))
                .expect("the menu's layer")
        });
        assert!(rect.intersects(before), "the menu opens over the dropdown");
        assert_eq!(
            ctx.layer_id_at(rect.center()),
            Some(menu),
            "the dropdown is drawn over the menu"
        );
        assert_eq!(
            ctx.memory(|memory| memory.area_rect(dropdown)),
            Some(before),
            "the dropdown keeps its size"
        );
        app.backend.shutdown();
    }

    /// Real-shaped data (a recent search, suggestions and a dozen hits)
    /// fills the window down to just above the player bar, whatever the
    /// window, with a scrollbar only past that.
    /// Rows that arrive after the first frame make the popup grow on the
    /// next one, rather than keeping the size it had with fewer rows.
    #[test]
    fn the_search_dropdown_grows_when_its_rows_arrive_late() {
        let (ctx, mut app) = accessible_app("search-late-rows");
        app.open(Page::Home);
        apply_flags(&mut app, None, Some("search-recent"));
        app.search.focus_requested = true;
        let size = egui::vec2(1920.0, 1080.0);
        let frame = |app: &mut App, step: u32| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(f64::from(step) / 10.0),
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            ctx.memory(|memory| memory.area_rect(egui::Id::new("search-dropdown")))
        };
        let mut before = None;
        for step in 0..4 {
            before = frame(&mut app, step);
        }
        let before = before.expect("the recent searches show").height();
        apply_flags(&mut app, None, Some("search-many"));
        let after = frame(&mut app, 4).expect("still open").height();
        assert!(after > before + 400.0, "{before} -> {after}");
        app.backend.shutdown();
    }

    #[test]
    fn the_search_dropdown_is_as_tall_as_its_rows_down_to_the_window_bottom() {
        let tall = dropdown_rect_at(egui::vec2(1920.0, 1080.0), "search-tall");
        assert!(tall.height() > 800.0, "{}", tall.height());
        let limit = 1080.0 - 12.0;
        assert!((tall.bottom() - limit).abs() < 2.0, "{}", tall.bottom());
        let short = dropdown_rect_at(egui::vec2(1280.0, 600.0), "search-short");
        let limit = 600.0 - 12.0;
        assert!((short.bottom() - limit).abs() < 2.0, "{}", short.bottom());
    }

    #[test]
    fn settings_search_filters_rows_clearing_and_empty_state() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("settings-search");
        app.open(Page::Settings);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let field = accessible_node(&tree, "Search settings", Role::TextInput);
        // Typing narrows the page to matching rows.
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(field, AccessibleAction::Focus, None)],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![egui::Event::Text("volume".into())]);
        assert_eq!(settings_filter(&ctx), "volume");
        accessible_node(&tree, "Normalize volume", Role::CheckBox);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Animated covers")),
            "non-matching rows stay hidden while filtering"
        );
        // Clearing the field brings every row back.
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        // The populated demo also has a global search, with its own Clear
        // button. Accessibility tree order does not identify which field a
        // button belongs to. Click the one beside the Settings input.
        let field = accessible_node(&tree, "Search settings", Role::TextInput);
        let field_bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == field)
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let field_y = (field_bounds.y0 + field_bounds.y1) / 2.0;
        let clear_buttons: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Clear") && node.role() == Role::Button)
            .collect();
        assert_eq!(clear_buttons.len(), 2, "both searches have a Clear button");
        let clear = clear_buttons
            .iter()
            .find(|(_, node)| {
                node.bounds()
                    .is_some_and(|bounds| bounds.y0 <= field_y && field_y <= bounds.y1)
            })
            .expect("Clear beside Search settings")
            .0;
        let global_search = app.search.query.clone();
        let tree = accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(clear, AccessibleAction::Click, None)],
        );
        assert!(settings_filter(&ctx).is_empty());
        assert_eq!(
            app.search.query, global_search,
            "global search stays intact"
        );
        accessible_node(&tree, "Animated covers", Role::CheckBox);
        // Gibberish matches nothing: every section's controls disappear
        // and the empty state takes the page. (Plain labels expose no
        // accesskit name, so the empty state itself is covered through
        // the absence of each section's controls.)
        let field = accessible_node(&tree, "Search settings", Role::TextInput);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(field, AccessibleAction::Focus, None)],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![egui::Event::Text("zzznope".into())]);
        assert_eq!(settings_filter(&ctx), "zzznope");
        for (label, role) in [
            ("Sign out", Role::Button),
            ("Normalize volume", Role::CheckBox),
            ("Dark", Role::Button),
            ("Switch to it", Role::Button),
            ("Equalizer", Role::CheckBox),
            ("Clear artwork", Role::Button),
            ("Check for updates", Role::Button),
        ] {
            assert!(
                tree.nodes
                    .iter()
                    .all(|(_, node)| !(node.label() == Some(label) && node.role() == role)),
                "{label} is hidden when nothing matches"
            );
        }
        app.backend.shutdown();
    }

    fn settings_text(ctx: &egui::Context, app: &mut App, query: &str) -> Vec<String> {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-filter"), query.to_owned());
        });
        view_frame(ctx, app, vec![], crate::ui::settings::show);
        view_frame(ctx, app, vec![], crate::ui::settings::show)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn settings_search_finds_complete_descriptions_and_current_status() {
        let (ctx, mut app) = accessible_app("settings-search-text");
        for (query, row) in [
            ("moving album covers", "Animated covers"),
            ("Ctrl+0", "Interface zoom"),
            ("affiliated", "About"),
            (
                "Downloads in the background",
                "Download updates automatically",
            ),
        ] {
            let query = if cfg!(target_os = "macos") && query == "Ctrl+0" {
                "Cmd+0"
            } else {
                query
            };
            let text = settings_text(&ctx, &mut app, query);
            assert!(text.iter().any(|text| text == row), "{query}: {text:?}");
        }
        // Knobs that TIDAL's apps do not have are gone.
        for query in ["Rodio", "ALSA", "Device name", "Very high"] {
            let text = settings_text(&ctx, &mut app, query);
            assert!(
                text.iter().any(|text| text.starts_with("No settings for")),
                "{query}: {text:?}"
            );
        }
        app.local_playback =
            crate::backend::LocalPlayback::Failed("Test connection failure".into());
        let text = settings_text(&ctx, &mut app, "connection failure");
        assert!(text.iter().any(|text| text == "Status: Unavailable"));
        app.backend.shutdown();
    }

    #[test]
    fn settings_search_never_shows_a_section_for_an_unavailable_row() {
        let (ctx, mut app) = accessible_app("settings-search-availability");
        let offered = cfg!(any(windows, target_os = "linux"));
        let text = settings_text(&ctx, &mut app, "Use Tidalfast's title bar");
        assert_eq!(text.iter().any(|text| text == "Appearance"), offered);
        if !offered {
            assert!(text.iter().any(|text| text.starts_with("No settings for")));
        }
        app.demo_windows_controls = true;
        let text = settings_text(&ctx, &mut app, "Use Tidalfast's title bar");
        assert!(text.iter().any(|text| text == "Appearance"));
        assert!(text.iter().any(|text| text == "Use Tidalfast's title bar"));
        app.backend.shutdown();
    }

    #[test]
    fn settings_search_keeps_apply_available_after_a_playback_edit() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("settings-search-apply");
        app.open(Page::Settings);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-filter"), "normalize".to_string())
        });
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let toggle = accessible_node(&tree, "Normalize volume", Role::CheckBox);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(toggle, AccessibleAction::Click, None)],
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Apply and restart playback", Role::Button);
        app.backend.shutdown();
    }

    #[test]
    fn accessible_sliders_accept_keyboard_and_screen_reader_values() {
        use crate::ui::widgets::{SliderEvent, thin_slider};
        use egui::accesskit::{Action, ActionData, Role};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::install(&ctx);
        let palette = crate::theme::Palette::dark();
        let mut value = 0.5;
        let mut render = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let SliderEvent::Committed(next) = thin_slider(
                        ui,
                        &palette,
                        egui::Id::new("test-volume"),
                        "Volume (%)",
                        value,
                        200.0,
                        Some(0.05),
                    ) {
                        value = next;
                    }
                },
            );
            output.textures_delta.clear();
            (value, output.platform_output.accesskit_update.unwrap())
        };
        let (_, tree) = render(vec![]);
        let slider = accessible_node(&tree, "Volume (%)", Role::Slider);
        render(vec![accessible_action(slider, Action::Focus, None)]);
        let (value, _) = render(vec![keyboard(egui::Key::ArrowRight, egui::Modifiers::NONE)]);
        assert!((value - 0.55).abs() < 0.001);
        let (value, tree) = render(vec![accessible_action(
            slider,
            Action::SetValue,
            Some(ActionData::NumericValue(35.0)),
        )]);
        assert!((value - 0.35).abs() < 0.001);
        let node = &tree.nodes.iter().find(|(id, _)| *id == slider).unwrap().1;
        assert!((node.numeric_value().unwrap() - 35.0).abs() < 0.001);
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        let (value, _) = render(vec![accessible_action(
            slider,
            Action::SetValue,
            Some(ActionData::NumericValue(200.0)),
        )]);
        assert_eq!(value, 1.0);
        let (value, _) = render(vec![accessible_action(
            slider,
            Action::SetValue,
            Some(ActionData::NumericValue(f64::NAN)),
        )]);
        assert_eq!(value, 1.0);
    }

    #[test]
    fn accessible_switch_has_a_name_state_and_keyboard_activation() {
        use egui::accesskit::{Action, Role, Toggled};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::install(&ctx);
        let palette = crate::theme::Palette::dark();
        let mut on = false;
        let mut render = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::ui::widgets::switch(ui, &palette, "Autoplay", &mut on);
                },
            );
            output.textures_delta.clear();
            (on, output.platform_output.accesskit_update.unwrap())
        };
        let (_, tree) = render(vec![]);
        let switch = accessible_node(&tree, "Autoplay", Role::CheckBox);
        let node = &tree.nodes.iter().find(|(id, _)| *id == switch).unwrap().1;
        assert_eq!(node.toggled(), Some(Toggled::False));
        render(vec![accessible_action(switch, Action::Focus, None)]);
        let (on, tree) = render(vec![keyboard(egui::Key::Space, egui::Modifiers::NONE)]);
        assert!(on);
        let node = &tree.nodes.iter().find(|(id, _)| *id == switch).unwrap().1;
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn accessible_song_focus_survives_visible_row_changes_and_keeps_duplicates_distinct() {
        use crate::ui::widgets::{TrackRow, track_row};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("rows");
        let item = app.queue.get().unwrap().queue[0].clone();
        let context = crate::model::RowContext::Uris(Arc::from(vec![item.uri().to_string(); 2]));
        let label = format!("Play {}, {}", item.name(), item.subtitle());
        let mut render = |first, events| {
            app.actions.clear();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    for index in first..2 {
                        track_row(
                            ui,
                            &mut app,
                            TrackRow {
                                index,
                                number: Some(index + 1),
                                item: &item,
                                context: &context,
                                show_cover: false,
                                numbered: true,
                                show_album: false,
                                added_at: None,
                                show_added: false,
                                added_by: None,
                                show_added_by: false,
                                compact: false,
                                shift: 0.0,
                                picked: false,
                                picked_songs: &[],
                            },
                        );
                    }
                },
            );
            output.textures_delta.clear();
            (
                output.platform_output.accesskit_update.unwrap(),
                app.actions.clone(),
            )
        };
        let (tree, _) = render(0, vec![]);
        let mut positioned_rows: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label.as_str()) && node.role() == Role::Button)
            .map(|(id, node)| (*id, node.bounds().unwrap().y0))
            .collect();
        positioned_rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        let rows: Vec<_> = positioned_rows.into_iter().map(|(id, _)| id).collect();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0], rows[1]);
        render(
            0,
            vec![accessible_action(rows[1], AccessibleAction::Focus, None)],
        );
        let (tree, _) = render(1, vec![]);
        assert_eq!(
            accessible_node(&tree, &label, Role::Button),
            rows[1],
            "scrolling must not give a song another row's identity"
        );
        assert_eq!(tree.focus, rows[1]);
        let (_, actions) = render(1, vec![keyboard(egui::Key::Enter, egui::Modifiers::NONE)]);
        assert!(matches!(
            actions.as_slice(),
            [crate::model::Action::PlayFromRow { index: 1, .. }]
        ));
        let (tree, _) = render(1, vec![]);
        let more = accessible_node(&tree, "More", Role::Button);
        render(
            1,
            vec![accessible_action(more, AccessibleAction::Focus, None)],
        );
        let (tree, _) = render(1, vec![]);
        assert_eq!(
            tree.focus, more,
            "More must remain reachable after focus leaves the song row"
        );
        render(1, vec![keyboard(egui::Key::Enter, egui::Modifiers::NONE)]);
        // The menu's first frame only measures it, and egui keeps widgets it
        // has not shown out of the screen-reader tree.
        let (tree, _) = render(1, vec![]);
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Play next")),
            "the keyboard opens the song menu"
        );
        app.backend.shutdown();
    }

    /// A plugin that hides song rows (`display: none` on a
    /// `tracklist-row`, as a filter plugin could) leaves each hidden row's
    /// place empty: the rows below it stay where they were drawn, and a
    /// double-click on one plays that row's own song, never the song of
    /// the row that was there before or of a row that moved into its
    /// place. A double-click on the hidden row's empty place plays nothing.
    #[test]
    fn a_double_click_beside_rows_a_plugin_hid_plays_the_row_under_the_pointer() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::ui::widgets::{TrackRow, track_row};
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("hidden-rows");
        let items: Vec<PlayableItem> = (0..8)
            .map(|n| {
                PlayableItem::Track(Track {
                    id: Some(n.to_string()),
                    uri: format!("tidal:track:{n}"),
                    name: format!("Song {n}"),
                    ..Default::default()
                })
            })
            .collect();
        let context = crate::model::RowContext::Context {
            uri: "tidal:playlist:e2".into(),
            editable_playlist: None,
        };
        let list = crate::ui::plugin_dom::list_id(&context);
        // Each double-click a second after the last, as separate ones are.
        let mut time = 0.0;
        let mut render = |app: &mut App, events: Vec<egui::Event>| {
            app.actions.clear();
            time += if events.is_empty() { 0.05 } else { 1.0 };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 800.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    for (index, item) in items.iter().enumerate() {
                        track_row(
                            ui,
                            app,
                            TrackRow {
                                index,
                                number: Some(index + 1),
                                item,
                                context: &context,
                                show_cover: false,
                                numbered: true,
                                show_album: false,
                                added_at: None,
                                show_added: false,
                                added_by: None,
                                show_added_by: false,
                                compact: false,
                                shift: 0.0,
                                picked: false,
                                picked_songs: &[],
                            },
                        );
                    }
                },
            );
            output.textures_delta.clear();
            (
                output.platform_output.accesskit_update.unwrap(),
                app.actions.clone(),
            )
        };
        let centre = |tree: &egui::accesskit::TreeUpdate, n: usize| {
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button
                        && node
                            .label()
                            .is_some_and(|label| label.starts_with(&format!("Play Song {n},")))
                })
                .and_then(|(_, node)| node.bounds())
                .map(|b| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
        };
        let double_click = |at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            vec![
                egui::Event::PointerMoved(at),
                press(true),
                press(false),
                press(true),
                press(false),
            ]
        };
        let (plain, _) = render(&mut app, vec![]);
        let places: Vec<egui::Pos2> = (0..8)
            .map(|n| centre(&plain, n).expect("every row is drawn"))
            .collect();

        // Rows 2, 3 and 4 hidden by the plugin, as four rows of remixes
        // might be.
        let hidden = |index: usize| {
            (
                crate::luna::dom_anchors::keys::row(&list, index),
                HostOverride {
                    style: Style {
                        display: Some("none".into()),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            )
        };
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            hosts: [hidden(2), hidden(3), hidden(4)].into_iter().collect(),
            ..DomOverlay::default()
        }));
        let (tree, _) = render(&mut app, vec![]);
        for n in [0, 1, 5, 6, 7] {
            assert_eq!(centre(&tree, n), Some(places[n]), "row {n} keeps its place");
        }
        for n in [1, 5, 7] {
            let (_, actions) = render(&mut app, double_click(places[n]));
            render(&mut app, vec![]);
            let named: Vec<(String, u32)> = actions
                .iter()
                .filter_map(|action| match action {
                    crate::model::Action::PlayFromRow { uri, index, .. } => {
                        Some((uri.clone(), *index))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                named,
                [(format!("tidal:track:{n}"), n as u32)],
                "the row under the pointer"
            );
        }
        // The empty place of a hidden row plays nothing.
        let (_, actions) = render(&mut app, double_click(places[3]));
        crate::ui::plugin_dom::test_overlay(None);
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, crate::model::Action::PlayFromRow { .. })),
            "{actions:?}"
        );
        app.backend.shutdown();
    }

    #[test]
    fn accessible_tab_reaches_songs_beyond_the_visible_list() {
        use crate::ui::widgets::{TrackRow, track_row, virtual_rows};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("scroll");
        let item = app.queue.get().unwrap().queue[0].clone();
        let context = crate::model::RowContext::Uris(Arc::from(vec![item.uri().to_string(); 20]));
        let label = format!("Play {}, {}", item.name(), item.subtitle());
        let mut reached_last = false;
        let mut render = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 200.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::ScrollArea::vertical().animated(false).show(ui, |ui| {
                        virtual_rows(ui, 20, crate::theme::ROW_HEIGHT, |ui, index| {
                            if index == 19 && ui.cursor().top() < ui.clip_rect().bottom() {
                                reached_last = true;
                            }
                            track_row(
                                ui,
                                &mut app,
                                TrackRow {
                                    index,
                                    number: Some(index + 1),
                                    item: &item,
                                    context: &context,
                                    show_cover: false,
                                    numbered: true,
                                    show_album: false,
                                    added_at: None,
                                    show_added: false,
                                    added_by: None,
                                    show_added_by: false,
                                    compact: false,
                                    shift: 0.0,
                                    picked: false,
                                    picked_songs: &[],
                                },
                            );
                        });
                    });
                },
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        };
        let tree = render(vec![]);
        let first = accessible_node(&tree, &label, Role::Button);
        render(vec![accessible_action(
            first,
            AccessibleAction::Focus,
            None,
        )]);
        for _ in 0..120 {
            render(vec![keyboard(egui::Key::Tab, egui::Modifiers::NONE)]);
        }
        assert!(
            reached_last,
            "Tab must scroll through the virtual list instead of trapping focus in its first visible rows"
        );
        app.backend.shutdown();
    }

    #[test]
    fn accessible_tab_reaches_cards_beyond_the_visible_grid() {
        use crate::ui::widgets::{CardCover, card, card_row_height, virtual_wrapped_cards};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("card-grid");
        let mut reached_last = false;
        let mut render = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 280.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::ScrollArea::vertical().animated(false).show(ui, |ui| {
                        let height = card_row_height(ui);
                        virtual_wrapped_cards(ui, 24, height, |ui, index| {
                            if index == 23 && ui.cursor().top() < ui.clip_rect().bottom() {
                                reached_last = true;
                            }
                            card(
                                ui,
                                &mut app,
                                None,
                                &format!("Album {index}"),
                                "Artist",
                                CardCover::default(),
                            );
                        });
                    });
                },
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        };
        let tree = render(vec![]);
        let first = accessible_node(&tree, "Album 0, Artist", Role::Button);
        render(vec![accessible_action(
            first,
            AccessibleAction::Focus,
            None,
        )]);
        for _ in 0..80 {
            render(vec![keyboard(egui::Key::Tab, egui::Modifiers::NONE)]);
        }
        assert!(
            reached_last,
            "Tab must scroll through the virtual card grid instead of trapping focus in its first visible row"
        );
        app.backend.shutdown();
    }

    #[test]
    fn accessible_playing_and_queued_copies_target_their_own_context() {
        use crate::model::RowContext;
        use crate::ui::widgets::{TrackRow, track_row};
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("queued-copy");
        let item = app.queue.get().unwrap().currently_playing.clone().unwrap();
        let contexts = [
            RowContext::Uris(Arc::from([item.uri().to_string()])),
            RowContext::Queue,
        ];
        let label = format!("Play {}, {}", item.name(), item.subtitle());
        let mut render = |events| {
            app.actions.clear();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    for context in &contexts {
                        track_row(
                            ui,
                            &mut app,
                            TrackRow {
                                index: 0,
                                number: Some(1),
                                item: &item,
                                context,
                                show_cover: false,
                                numbered: true,
                                show_album: false,
                                added_at: None,
                                show_added: false,
                                added_by: None,
                                show_added_by: false,
                                compact: false,
                                shift: 0.0,
                                picked: false,
                                picked_songs: &[],
                            },
                        );
                    }
                },
            );
            output.textures_delta.clear();
            (
                output.platform_output.accesskit_update.unwrap(),
                app.actions.clone(),
            )
        };
        let (tree, _) = render(vec![]);
        let mut rows: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label.as_str()) && node.role() == Role::Button)
            .map(|(id, node)| (*id, node.bounds().unwrap().y0))
            .collect();
        rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(
            rows.len(),
            2,
            "the playing song and queued copy need separate controls"
        );
        let (_, actions) = render(vec![accessible_action(
            rows[1].0,
            AccessibleAction::Click,
            None,
        )]);
        assert!(matches!(
            actions.as_slice(),
            [crate::model::Action::PlayFromRow {
                context: RowContext::Queue,
                index: 0,
                ..
            }]
        ));
        let (_, actions) = render(vec![accessible_action(
            rows[0].0,
            AccessibleAction::Click,
            None,
        )]);
        assert!(matches!(
            actions.as_slice(),
            [crate::model::Action::PlayFromRow {
                context: RowContext::Uris(_),
                index: 0,
                ..
            }]
        ));
        app.backend.shutdown();
    }

    /// Linux offers middle-click autoscroll as a switch that starts off and
    /// is saved; Windows always autoscrolls and macOS never does, so neither
    /// shows the row.
    #[test]
    fn the_linux_autoscroll_switch_starts_off_and_is_saved() {
        use egui::accesskit::{Role, Toggled};
        let (ctx, mut app) = accessible_app("autoscroll-setting");
        let text = settings_text(&ctx, &mut app, "Middle-click autoscroll");
        assert_eq!(
            // The search field itself echoes the query, so look for the
            // row's description rather than its title.
            text.iter()
                .any(|text| text.starts_with("Middle-click a list")),
            cfg!(target_os = "linux")
        );
        if !cfg!(target_os = "linux") {
            app.backend.shutdown();
            return;
        }
        app.open(Page::Settings);
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let control = accessible_node(&tree, "Middle-click autoscroll", Role::CheckBox);
        let toggled = |tree: &egui::accesskit::TreeUpdate| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == control)
                .and_then(|(_, node)| node.toggled())
        };
        assert_eq!(toggled(&tree), Some(Toggled::False));
        assert!(!app.settings.middle_click_autoscroll);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                control,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        assert!(app.settings.middle_click_autoscroll);
        let path = app.dirs.config.join("autoscroll-choice.json");
        app.settings.save(&path);
        app.settings = Settings::load(&path);
        assert!(app.settings.middle_click_autoscroll);
        app.backend.shutdown();
    }

    fn labels(tree: &egui::accesskit::TreeUpdate) -> Vec<String> {
        tree.nodes
            .iter()
            .filter_map(|(_, node)| node.label().or(node.value()).map(str::to_string))
            .collect()
    }

    /// A video card plays its video, and the ones after it, like songs:
    /// the player bar names it at once.
    #[test]
    fn a_video_card_plays_the_video() {
        use egui::accesskit::{Action as Access, Role};
        let (ctx, mut app) = accessible_app("video-card");
        app.library.videos.items = (0..3).map(super::video).collect();
        app.actions.push(Action::Open(Page::Videos));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let first = super::video(1);
        let card = accessible_node(
            &tree,
            &format!("{}, {}", first.name, first.artist),
            Role::Button,
        );
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(card, Access::Click, None)],
        );
        let now = app.now_playing().expect("the video shows as playing");
        assert_eq!(now.uri, "tidal:video:vid1");
        assert_eq!(now.title, first.name);
        assert_eq!(now.album_id, None);
        app.backend.shutdown();
    }

    /// The video's now-playing view: Similar videos and Credits instead of
    /// a song's tabs, picture-in-picture beside full screen, and each tab's
    /// panel. Picture-in-picture closes once a song plays.
    #[test]
    fn a_playing_video_has_its_own_now_playing_view() {
        let (ctx, mut app) = accessible_app("video-now-playing");
        let video = super::video(0);
        app.local.track = Some(crate::player::LocalTrack {
            uri: video.uri(),
            title: video.name.clone(),
            artists: video.as_track().artists,
            duration_ms: video.duration * 1000,
            ..Default::default()
        });
        app.local.playback = crate::player::Playback::Paused;
        app.video_info_uri = Some(video.uri());
        app.video_info = Loadable::Loaded(crate::model::VideoInfo {
            video: crate::api::models::Video {
                release_date: Some("2026-10-02T00:00:00.000+0000".into()),
                ..video.clone()
            },
            similar: (1..4).map(super::video).collect(),
        });
        app.now_playing_view = true;
        app.now_playing_tab = Some(crate::model::NowPlayingTab::SimilarTracks);
        let mut shown = labels(&accessible_frame(&ctx, &mut app, vec![]));
        shown.extend(labels(&accessible_frame(&ctx, &mut app, vec![])));
        for label in [
            "Similar videos",
            "Credits",
            "Picture-in-picture",
            "Full screen",
        ] {
            assert!(
                shown.iter().any(|shown| shown == label),
                "{label} in {shown:?}"
            );
        }
        assert!(
            !shown
                .iter()
                .any(|shown| shown == "Similar tracks" || shown == "Lyrics")
        );
        let similar = super::video(2);
        assert!(
            shown
                .iter()
                .any(|shown| *shown == format!("{}, {}", similar.name, similar.artist))
        );
        app.now_playing_tab = Some(crate::model::NowPlayingTab::Credits);
        // A tree update carries what changed: read both frames.
        let mut text = labels(&accessible_frame(&ctx, &mut app, vec![])).join("\n");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        text += &labels(&tree).join("\n");
        assert!(
            text.contains("Released") && text.contains("2026-10-02"),
            "{text}"
        );
        // Picture-in-picture opens from its button and closes the view.
        let pip = accessible_node(&tree, "Picture-in-picture", egui::accesskit::Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(pip, egui::accesskit::Action::Click, None)],
        );
        assert!(crate::ui::now_playing_video_pip_open(&ctx));
        assert!(!app.now_playing_view);
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(crate::ui::now_playing_video_pip_open(&ctx));
        // A song next: no picture to show.
        app.local.track = Some(crate::player::LocalTrack {
            uri: "tidal:track:trk1".into(),
            ..Default::default()
        });
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(!crate::ui::now_playing_video_pip_open(&ctx));
        app.backend.shutdown();
    }

    fn frame(ctx: &egui::Context, app: &mut App) {
        frame_events(ctx, app, Vec::new());
    }

    /// Lunallena lays the page under a sidebar that floats over it (`#main`
    /// pulled left the sidebar's width and kept as wide as the window): the
    /// sidebar's rows still take a pointer click, which the page under them
    /// does not. Its layer is skipped by the hit test when it is not
    /// interactable, so that the sidebar was not clickable at all.
    #[test]
    fn the_navigation_of_a_sidebar_floating_over_the_page_takes_clicks() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("theme-sidebar-click");
        let host = |f: &dyn Fn(&mut Style)| {
            let mut s = Style::default();
            f(&mut s);
            HostOverride {
                style: s,
                ..HostOverride::default()
            }
        };
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        // The sidebar under the pointer, as its `:hover` rule leaves it.
        o.hosts
            .insert(keys::SIDEBAR.into(), host(&|s| s.left = Some("0px".into())));
        o.hosts.insert(
            keys::MAIN.into(),
            host(&|s| {
                s.left = Some("-240px".into());
                s.min_width = Some("100vw".into());
            }),
        );
        plugin_dom::test_overlay(Some(o));
        let size = egui::vec2(1280.0, 800.0);
        let draw = |ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| crate::ui::show(app, ui));
            out.textures_delta.clear();
            out.platform_output.accesskit_update
        };
        let mut tree = None;
        for _ in 0..4 {
            tree = draw(&ctx, &mut app, vec![]);
        }
        let tree = tree.expect("screen-reader tree");
        assert!(
            ctx.memory(|m| m.area_rect(egui::Id::new("theme-sidebar-over-page")))
                .is_some(),
            "the sidebar is drawn over the page"
        );
        let explore = accessible_node(&tree, "Explore", Role::Button);
        let (_, node) = tree.nodes.iter().find(|(id, _)| *id == explore).unwrap();
        let b = node.bounds().expect("bounds");
        let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        draw(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        draw(
            &ctx,
            &mut app,
            pointer_click(at, egui::PointerButton::Primary),
        );
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::Open(page) if *page == crate::ui::explore::page())),
            "the click at {at:?} reached the Explore row: {:?}",
            app.actions.len()
        );
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
    }

    /// With Animated covers off, an album with a `videoCover` starts no
    /// clip (no fetch, thread, decoder or texture); none starts with less
    /// motion asked for either; and turning the setting off stops every
    /// clip at once.
    #[test]
    fn animated_covers_start_only_when_on_and_stop_when_turned_off() {
        use crate::video_cover::{clips_started, live_clips};
        let (ctx, mut app) = accessible_app("animated-covers-off");
        if let Some(page) = app.album_pages.get_mut("alb0")
            && let Loadable::Loaded(album) = &mut page.album
        {
            album.video_cover = Some("687b8308-7758-43f1-95eb-42915f65dd9c".into());
        }
        app.open(Page::Album("alb0".into()));
        let before = clips_started();

        app.settings.animated_covers = false;
        app.motion.force(false);
        crate::motion::apply(&ctx, false);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(
            clips_started(),
            before,
            "a clip started with the setting off"
        );

        app.settings.animated_covers = true;
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(
            clips_started(),
            before,
            "a clip started with reduced motion"
        );

        app.motion.force(false);
        crate::motion::apply(&ctx, false);
        frame(&ctx, &mut app);
        assert!(
            clips_started() > before,
            "the album header's clip did not start"
        );

        app.settings.animated_covers = false;
        frame(&ctx, &mut app);
        assert_eq!(live_clips(), 0, "a clip outlived the setting");
        let after = clips_started();
        frame(&ctx, &mut app);
        assert_eq!(clips_started(), after);
        app.backend.shutdown();
    }

    #[test]
    fn playlist_filter_preserves_edit_permissions_and_keyboard_selection() {
        use egui::accesskit::Role;
        for count in [1, 2] {
            let (ctx, mut app) = accessible_app(&format!("playlist-filter-{count}"));
            app.backend.set_offline(true);
            let owner = app.user_id().unwrap().to_string();
            let make = |id: &str, name: &str, owned: bool, collaborative| Playlist {
                id: id.into(),
                uri: format!("tidal:playlist:{id}"),
                name: name.into(),
                owner: crate::api::models::Owner {
                    id: Some(if owned {
                        owner.clone()
                    } else {
                        "another-user".into()
                    }),
                    ..Default::default()
                },
                collaborative,
                ..Default::default()
            };
            app.library.playlists = Loadable::Loaded(vec![
                make("readonly", "Night locked", false, false),
                make("owned", "Night drive", true, false),
                make("shared", "Night together", false, true),
                make("day", "Daylight", true, false),
            ]);
            let items: Vec<_> = (0..count).map(|i| PlayableItem::Track(track(i))).collect();
            let mut query = String::new();
            let draw = |app: &mut App, query: &mut String, focus: bool, events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(760.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let field = crate::ui::widgets::playlist_picker(ui, app, &items, query);
                        if focus {
                            field.request_focus();
                        }
                    },
                );
                output.textures_delta.clear();
                output
            };
            draw(&mut app, &mut query, true, vec![]);
            let output = draw(
                &mut app,
                &mut query,
                false,
                vec![egui::Event::Text("  NiGhT  ".into())],
            );
            assert_eq!(query, "  NiGhT  ");
            let tree = output.platform_output.accesskit_update.unwrap();
            for name in ["Night locked", "Daylight"] {
                assert!(
                    !tree
                        .nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some(name)),
                    "{name} must not be offered"
                );
            }
            let owned = accessible_node(&tree, "Night drive", Role::Button);
            accessible_node(&tree, "Night together", Role::Button);
            let mut reached = false;
            for _ in 0..6 {
                let output = draw(
                    &mut app,
                    &mut query,
                    false,
                    vec![keyboard(egui::Key::Tab, egui::Modifiers::NONE)],
                );
                if output.platform_output.accesskit_update.unwrap().focus == owned {
                    reached = true;
                    break;
                }
            }
            assert!(
                reached,
                "Tab must reach the filtered playlist from the search field"
            );
            app.actions.clear();
            draw(
                &mut app,
                &mut query,
                false,
                vec![keyboard(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert!(
                matches!(app.actions.as_slice(), [crate::model::Action::AddToPlaylist { playlist_id, items: selected, .. }] if playlist_id == "owned" && selected == &items)
            );
            query = "no such playlist".into();
            let output = draw(&mut app, &mut query, false, vec![]);
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text == "No matching playlists")));
            app.backend.shutdown();
        }
    }

    /// Typing in the playlist filter chooses the first match, so Enter adds
    /// to it at once; Up and Down move the choice first.
    #[test]
    fn typing_in_the_playlist_filter_chooses_the_first_match() {
        let (ctx, mut app) = accessible_app("playlist-filter-choice");
        app.backend.set_offline(true);
        let owner = app.user_id().unwrap().to_string();
        let make = |id: &str, name: &str| Playlist {
            id: id.into(),
            uri: format!("tidal:playlist:{id}"),
            name: name.into(),
            owner: crate::api::models::Owner {
                id: Some(owner.clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        app.library.playlists = Loadable::Loaded(vec![
            make("drive", "Night drive"),
            make("day", "Daylight"),
            make("walk", "Night walk"),
            make("owls", "Night owls"),
        ]);
        let items = vec![PlayableItem::Track(track(0))];
        let draw = |app: &mut App, query: &mut String, focus: bool, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let field = crate::ui::widgets::playlist_picker(ui, app, &items, query);
                    if focus {
                        field.request_focus();
                    }
                },
            );
            output.textures_delta.clear();
        };
        let added = |app: &App| match app.actions.as_slice() {
            [crate::model::Action::AddToPlaylist { playlist_id, .. }] => Some(playlist_id.clone()),
            [] => None,
            other => panic!("expected one addition, got {other:?}"),
        };
        let key = |key| vec![keyboard(key, egui::Modifiers::NONE)];

        // #given the filter has focus
        let mut query = String::new();
        draw(&mut app, &mut query, true, vec![]);

        // #when a filter is typed and Enter pressed
        draw(
            &mut app,
            &mut query,
            false,
            vec![egui::Event::Text("night".into())],
        );
        draw(&mut app, &mut query, false, key(egui::Key::Enter));

        // #then the first match is added
        assert_eq!(added(&app).as_deref(), Some("drive"));

        // #when Down moves the choice twice and Up once, past both ends
        app.actions.clear();
        let mut query = String::new();
        draw(&mut app, &mut query, true, vec![]);
        draw(
            &mut app,
            &mut query,
            false,
            vec![egui::Event::Text("night".into())],
        );
        draw(&mut app, &mut query, false, key(egui::Key::ArrowUp));
        for _ in 0..3 {
            draw(&mut app, &mut query, false, key(egui::Key::ArrowDown));
        }
        draw(&mut app, &mut query, false, key(egui::Key::ArrowUp));
        assert_eq!(query, "night", "the arrows do not edit the filter");
        draw(&mut app, &mut query, false, key(egui::Key::Enter));

        // #then the choice it reached is added
        assert_eq!(added(&app).as_deref(), Some("walk"));

        // #when the filter changes after a choice was moved
        app.actions.clear();
        let mut query = String::new();
        draw(&mut app, &mut query, true, vec![]);
        draw(
            &mut app,
            &mut query,
            false,
            vec![egui::Event::Text("night".into())],
        );
        draw(&mut app, &mut query, false, key(egui::Key::ArrowDown));
        draw(
            &mut app,
            &mut query,
            false,
            vec![egui::Event::Text(" o".into())],
        );
        draw(&mut app, &mut query, false, key(egui::Key::Enter));

        // #then the choice starts again at the first match
        assert_eq!(added(&app).as_deref(), Some("owls"));

        // #when Enter is pressed with nothing typed
        app.actions.clear();
        let mut query = String::new();
        draw(&mut app, &mut query, true, vec![]);
        draw(&mut app, &mut query, false, key(egui::Key::Enter));

        // #then nothing is chosen for the listener
        assert_eq!(added(&app), None);
        app.backend.shutdown();
    }

    #[test]
    fn playlist_submenu_keeps_typing_and_resets_after_the_parent_closes() {
        use egui::accesskit::{Action, Role};
        let (ctx, mut app) = accessible_app("playlist-submenu");
        app.backend.set_offline(true);
        let songs = vec![PlayableItem::Track(track(0))];
        let draw = |app: &mut App, show_parent: bool, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if show_parent {
                        crate::ui::widgets::picked_menu(ui, app, &songs, None);
                    }
                },
            );
            output.textures_delta.clear();
            output
        };
        let tree = draw(&mut app, true, vec![])
            .platform_output
            .accesskit_update
            .unwrap();
        let add = accessible_node(&tree, "Add to playlist", Role::Button);
        let open = || vec![accessible_action(add, Action::Click, None)];
        // TIDAL's submenu offers its recent playlists; "Show all playlists"
        // swaps in the filter over every one.
        let show_all = |app: &mut App| {
            let tree = draw(app, true, vec![])
                .platform_output
                .accesskit_update
                .unwrap();
            let all = accessible_node(&tree, "Show all playlists", Role::Button);
            draw(app, true, vec![accessible_action(all, Action::Click, None)]);
            draw(app, true, vec![]);
        };
        draw(&mut app, true, open());
        show_all(&mut app);
        let tree = draw(&mut app, true, vec![egui::Event::Text("night".into())])
            .platform_output
            .accesskit_update
            .unwrap();
        assert!(egui::Popup::is_any_open(&ctx));
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.value() == Some("night"))
        );
        accessible_node(&tree, "Late night focus", Role::Button);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Sunday morning"))
        );
        draw(
            &mut app,
            true,
            vec![keyboard(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!egui::Popup::is_any_open(&ctx));
        // A closed outer context menu no longer draws its submenu at all.
        draw(&mut app, false, vec![]);
        draw(&mut app, true, vec![]);
        draw(&mut app, true, open());
        show_all(&mut app);
        let tree = draw(&mut app, true, vec![])
            .platform_output
            .accesskit_update
            .unwrap();
        accessible_node(&tree, "Sunday morning", Role::Button);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("night"))
        );
        app.backend.shutdown();
    }

    #[test]
    fn playlist_submenu_stays_open_for_a_click_in_its_filter_and_grows_back_when_cleared() {
        use egui::accesskit::{Action, Role};
        let (ctx, mut app) = accessible_app("playlist-submenu-click");
        app.backend.set_offline(true);
        let songs = vec![PlayableItem::Track(track(0))];
        let draw = |app: &mut App, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::widgets::picked_menu(ui, app, &songs, None),
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        };
        let tree = draw(&mut app, vec![]);
        let add = accessible_node(&tree, "Add to playlist", Role::Button);
        draw(&mut app, vec![accessible_action(add, Action::Click, None)]);
        let tree = draw(&mut app, vec![]);
        let all = accessible_node(&tree, "Show all playlists", Role::Button);
        draw(&mut app, vec![accessible_action(all, Action::Click, None)]);
        draw(&mut app, vec![]);
        let tree = draw(&mut app, vec![]);
        let field = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Filter playlists"))
            .and_then(|(_, node)| node.bounds())
            .expect("the playlist filter");
        let center = egui::pos2(
            ((field.x0 + field.x1) / 2.0) as f32,
            ((field.y0 + field.y1) / 2.0) as f32,
        );
        let height = || {
            let layer = ctx.layer_id_at(center).expect("the submenu's layer");
            ctx.memory(|memory| memory.area_rect(layer.id))
                .expect("the submenu's area")
                .height()
        };
        let full = height();

        // #when the filter field is clicked
        draw(
            &mut app,
            pointer_click(center, egui::PointerButton::Primary),
        );
        let tree = draw(&mut app, vec![]);

        // #then the submenu stays open with every playlist
        assert!(egui::Popup::is_any_open(&ctx));
        accessible_node(&tree, "Sunday morning", Role::Button);

        // #when a filter narrows the list and is then cleared
        draw(&mut app, vec![egui::Event::Text("night".into())]);
        draw(&mut app, vec![]);
        assert!(height() < full, "the filter shrinks the submenu");
        draw(
            &mut app,
            vec![keyboard(egui::Key::Backspace, egui::Modifiers::NONE); 5],
        );
        draw(&mut app, vec![]);
        draw(&mut app, vec![]);

        // #then the submenu grows back to its full height
        assert_eq!(height(), full);
        app.backend.shutdown();
    }

    #[test]
    fn an_empty_search_half_is_not_reported_as_no_results_while_the_other_waits_or_fails() {
        let (ctx, mut app) = accessible_app("partial-search-status");
        app.search.committed = "new query".into();
        app.search.results = Loadable::Loaded(Arc::default());
        for pending in [true, false] {
            app.search.catalogue_pending = pending;
            app.search.error = (!pending).then(|| "Search: network error".into());
            search_frame(&ctx, &mut app, vec![]);
            let text = search_frame(&ctx, &mut app, vec![]);
            assert!(!text.iter().any(|(label, _)| label.contains("No results")));
            if !pending {
                assert!(
                    text.iter()
                        .any(|(label, _)| label.contains("network error"))
                );
            }
        }
        app.backend.shutdown();
    }

    /// TIDAL's "Did you mean" shows under the pills for a misspelt search,
    /// its suggestion is a link that runs that search, and nothing shows
    /// when TIDAL has none or only changes the case.
    #[test]
    fn a_misspelt_search_offers_tidals_correction_that_runs_when_clicked() {
        let (ctx, mut app) = accessible_app("did-you-mean");
        app.search.committed = "eminme".into();
        app.search.results = Loadable::Loaded(Arc::new(SearchResults {
            did_you_mean: Some("eminem".into()),
            ..Default::default()
        }));
        search_frame(&ctx, &mut app, vec![]);
        let text = search_frame(&ctx, &mut app, vec![]);
        assert!(text.iter().any(|(label, _)| label == "Did you mean "));
        let link = text
            .iter()
            .find(|(label, _)| label == "eminem")
            .expect("the suggestion is shown")
            .1
            .center();
        app.actions.clear();
        search_frame(
            &ctx,
            &mut app,
            pointer_click(link, egui::PointerButton::Primary),
        );
        assert!(matches!(app.actions.as_slice(), [Action::Search(query)] if query == "eminem"));
        for none in [None, Some("EMINME".to_string()), Some("  ".to_string())] {
            app.search.results = Loadable::Loaded(Arc::new(SearchResults {
                did_you_mean: none,
                ..Default::default()
            }));
            search_frame(&ctx, &mut app, vec![]);
            let text = search_frame(&ctx, &mut app, vec![]);
            assert!(!text.iter().any(|(label, _)| label.contains("Did you mean")));
        }
        app.backend.shutdown();
    }

    /// A song kept on this computer plays without the network: the
    /// Downloaded page is not greyed out and disabled when there is none
    /// (offline mode, or a lost connection), as every other table is.
    #[test]
    fn a_downloaded_song_can_be_played_without_the_network() {
        let (ctx, mut app) = accessible_app("downloaded-offline-play");
        let track = Track {
            id: Some("77".into()),
            name: "Kept Song".into(),
            uri: "tidal:track:77".into(),
            duration_ms: 200_000,
            ..Track::default()
        };
        app.downloads
            .items
            .push((crate::api::models::PlayableItem::Track(track), 4_000_000));
        app.downloads.kept.insert("tidal:track:77".into());
        app.downloads.revision += 1;
        app.settings.offline_mode = true;
        app.network_reachable = false;
        let view = crate::ui::collection::downloaded;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let title = painted
            .iter()
            .find(|(label, _)| label == "Kept Song")
            .expect("the kept song is listed")
            .1
            .center();
        app.actions.clear();
        let [first, second] = double_click(title);
        view_frame(&ctx, &mut app, first, view);
        view_frame(&ctx, &mut app, second, view);
        assert!(
            app.actions.iter().any(|action| matches!(
                action,
                Action::PlayFromRow { uri, .. } | Action::PlayContext { uri, .. }
                    if uri.contains("77") || uri == "tidal:offline"
            ) || matches!(action, Action::PlayUris { .. })),
            "double clicking a kept song plays it: {:?}",
            app.actions
        );
        app.backend.shutdown();
    }

    /// In offline mode nothing that needs TIDAL waits as a placeholder: the
    /// queue and a page never read say so and offer Go online, and search
    /// says it needs a connection instead of showing the refusal raw.
    #[test]
    fn offline_mode_leaves_no_placeholder_waiting_and_no_raw_error() {
        let (ctx, mut app) = accessible_app("offline-blank");
        app.settings.offline_mode = true;
        app.network_reachable = false;
        app.queue = Loadable::NotLoaded;
        let queue = crate::ui::queue::page;
        queue_frame(&ctx, &mut app, queue);
        let text = queue_frame(&ctx, &mut app, queue);
        assert!(text.iter().any(|(label, _)| label == "Offline mode is on"));
        let button = text
            .iter()
            .find(|(label, _)| label == "Go online")
            .expect("the way out")
            .1
            .center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(button, egui::PointerButton::Primary),
            queue,
        );
        assert!(matches!(
            app.actions.as_slice(),
            [Action::SetOfflineMode(false)]
        ));

        app.search.committed = "eminem".into();
        search_frame(&ctx, &mut app, vec![]);
        let text = search_frame(&ctx, &mut app, vec![]);
        assert!(
            text.iter()
                .any(|(label, _)| label.starts_with("Search needs a connection")),
            "{text:?}"
        );
        assert!(
            !text
                .iter()
                .any(|(label, _)| label.contains("network error"))
        );
        assert!(text.iter().any(|(label, _)| label == "Go online"));
        app.backend.shutdown();
    }

    fn queue_frame(
        ctx: &egui::Context,
        app: &mut App,
        view: fn(&mut App, &mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        view_frame(ctx, app, vec![], view)
    }

    /// Offline mode shows a banner with a way out, and only while it is on.
    #[test]
    fn offline_mode_shows_a_banner_with_go_online() {
        let (ctx, mut app) = accessible_app("offline-banner");
        let view = crate::ui::downloads::offline_banner;
        view_frame(&ctx, &mut app, vec![], view);
        assert!(view_frame(&ctx, &mut app, vec![], view).is_empty());
        app.settings.offline_mode = true;
        view_frame(&ctx, &mut app, vec![], view);
        let text = view_frame(&ctx, &mut app, vec![], view);
        assert!(
            text.iter()
                .any(|(label, _)| label.starts_with("Offline mode is on"))
        );
        let button = text
            .iter()
            .find(|(label, _)| label == "Go online")
            .expect("the way out")
            .1
            .center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(button, egui::PointerButton::Primary),
            view,
        );
        assert!(matches!(
            app.actions.as_slice(),
            [Action::SetOfflineMode(false)]
        ));
        app.backend.shutdown();
    }

    fn search_frame(
        ctx: &egui::Context,
        app: &mut App,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        view_frame(ctx, app, events, crate::ui::search::show)
    }

    #[test]
    fn playing_artist_links_work_before_web_metadata_and_after_focus_returns() {
        use crate::player::{LocalState, LocalTrack, Playback};

        let (ctx, mut app) = accessible_app("playing-artist-links");
        let artists = vec![
            ArtistRef {
                id: Some("first".into()),
                name: "Tyler, the Creator".into(),
                uri: Some("tidal:artist:first".into()),
                picture: None,
            },
            ArtistRef {
                id: Some("guest".into()),
                name: "Guest".into(),
                uri: Some("tidal:artist:guest".into()),
                picture: None,
            },
        ];
        app.local = LocalState {
            playback: Playback::Playing,
            track: Some(LocalTrack {
                uri: "tidal:track:uncached".into(),
                title: "Song".into(),
                artists: artists.clone(),
                duration_ms: 200_000,
                ..LocalTrack::default()
            }),
            ..LocalState::default()
        };
        app.track_cache.clear();
        let view = crate::ui::player_bar::show;
        for cached in [false, true] {
            if cached {
                // A partial response must not turn working links back into text.
                app.track_cache.insert("uncached".into(), Track::default());
            }
            view_frame(&ctx, &mut app, vec![], view);
            let text = view_frame(&ctx, &mut app, vec![], view);
            for artist in &artists {
                let pos = text
                    .iter()
                    .find(|(text, _)| text == &artist.name)
                    .unwrap()
                    .1
                    .center();
                app.actions.clear();
                view_frame(
                    &ctx,
                    &mut app,
                    pointer_click(pos, egui::PointerButton::Primary),
                    view,
                );
                assert!(
                    matches!(app.actions.as_slice(), [Action::Open(Page::Artist(id))] if Some(id) == artist.id.as_ref())
                );
            }
            view_frame(
                &ctx,
                &mut app,
                vec![egui::Event::WindowFocused(false), egui::Event::PointerGone],
                view,
            );
            view_frame(&ctx, &mut app, vec![egui::Event::WindowFocused(true)], view);
        }
        app.backend.shutdown();
    }

    fn view_frame(
        ctx: &egui::Context,
        app: &mut App,
        events: Vec<egui::Event>,
        view: fn(&mut App, &mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        view_frame_sized(ctx, app, events, egui::vec2(1280.0, 2200.0), view)
    }

    fn view_frame_sized(
        ctx: &egui::Context,
        app: &mut App,
        events: Vec<egui::Event>,
        size: egui::Vec2,
        view: fn(&mut App, &mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| view(app, ui),
        );
        output.textures_delta.clear();
        fn walk(shape: &egui::epaint::Shape, text: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(shape) => text.push((
                    shape.galley.job.text.clone(),
                    shape.galley.rect.translate(shape.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|shape| walk(shape, text));
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for shape in &output.shapes {
            walk(&shape.shape, &mut text);
        }
        text
    }

    /// With plugins on, a pass builds the anchor tree only when what it
    /// shows changed. The player bar noted the song's position to the
    /// millisecond, so every pass (each frame of CatJam's cat or the
    /// now-playing background) built a tree of a few hundred anchors only to
    /// find it the same as the one sent, which shows whole seconds.
    #[cfg(feature = "demo")]
    #[test]
    fn passes_within_a_second_build_no_anchor_tree() {
        let root = crate::paths::unique_test_dir("anchor-builds");
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        populate(&mut app);
        app.reveal_theme_changes = false;
        apply_flags(&mut app, Some("home"), Some("playing-local"));
        // Paused, so the position is what the test sets.
        app.local.playback = crate::player::Playback::Paused;
        crate::ui::plugin_dom::test_overlay(Some(crate::luna::dom::DomOverlay {
            version: 1,
            ..Default::default()
        }));
        let pass = |app: &mut App, position_ms: u32| {
            app.local.position_ms = position_ms;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1240.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.refresh_frame_now();
                    crate::ui::show(app, ui);
                    crate::ui::plugin_dom::end_frame(ui.ctx());
                    app.clear_frame_now();
                },
            );
            output.textures_delta.clear();
            crate::ui::plugin_dom::test_tree_builds()
        };
        for _ in 0..6 {
            pass(&mut app, 61_000);
        }
        let settled = pass(&mut app, 61_200);
        assert_eq!(pass(&mut app, 61_700), settled, "the same second");
        assert!(pass(&mut app, 62_100) > settled, "the next second shows");
        crate::ui::plugin_dom::test_overlay(None);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// TIDAL 2.43.2's now-playing view has no `[data-test=artist-info]`:
    /// audio-visualizer's now-playing slots, which mount on it, never
    /// appear in TidaLuna. One made up here put its oscilloscope under the
    /// cover, where TIDAL shows nothing.
    #[cfg(feature = "demo")]
    #[test]
    fn the_now_playing_view_has_no_artist_info_block() {
        let root = crate::paths::unique_test_dir("np-artist-info");
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        apply_flags(&mut app, Some("home"), Some("playing-local"));
        app.local.playback = crate::player::Playback::Paused;
        app.now_playing_view = true;
        crate::ui::plugin_dom::test_overlay(Some(crate::luna::dom::DomOverlay {
            version: 1,
            ..Default::default()
        }));
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1240.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.refresh_frame_now();
                    crate::ui::show(&mut app, ui);
                    crate::ui::plugin_dom::end_frame(ui.ctx());
                    app.clear_frame_now();
                },
            );
            output.textures_delta.clear();
        }
        let tree = crate::ui::plugin_dom::test_sent_tree().expect("a tree was sent");
        let test_of = |n: &crate::luna::dom::HostNode| {
            n.attrs
                .iter()
                .find(|(k, _)| k == "data-test")
                .map(|(_, v)| v.clone())
        };
        assert!(
            tree.nodes
                .iter()
                .any(|n| test_of(n).as_deref() == Some("new-now-playing")),
            "the now-playing view is described"
        );
        assert!(
            !tree
                .nodes
                .iter()
                .any(|n| test_of(n).as_deref() == Some("artist-info")),
            "no artist-info block"
        );
        crate::ui::plugin_dom::test_overlay(None);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// One pass of the app as `crate::ui::show` draws it (with plugins'
    /// overlay as the test set it), and its screen-reader tree.
    fn shown_frame(ctx: &egui::Context, app: &mut App) -> Option<egui::accesskit::TreeUpdate> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| crate::ui::show(app, ui));
        out.textures_delta.clear();
        out.platform_output.accesskit_update
    }

    /// radiant-lyrics hides TIDAL's full screen button in the now-playing
    /// header (`[data-test=new-now-playing-expand]{display:none}`), and puts
    /// its own buttons there: the header is Similar tracks, Credits, Lyrics,
    /// its buttons, Close. The button was not an anchor, so it stayed; and a
    /// hidden one must stay described, or it comes back the frame after.
    #[test]
    fn a_plugin_can_hide_the_now_playing_full_screen_button() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let (ctx, mut app) = accessible_app("np-expand-hidden");
        apply_flags(&mut app, Some("home"), Some("playing-local"));
        app.local.playback = crate::player::Playback::Paused;
        app.now_playing_view = true;
        let shown = |ctx: &egui::Context, app: &mut App, overlay: DomOverlay| {
            plugin_dom::test_overlay(Some(overlay));
            let mut tree = None;
            for _ in 0..3 {
                tree = shown_frame(ctx, app);
                plugin_dom::end_frame(ctx);
            }
            let found = labels(&tree.expect("screen-reader tree"))
                .iter()
                .any(|l| l == "Full screen");
            // Described either way, so the rule that hides it keeps
            // matching it.
            let described = plugin_dom::test_sent_tree()
                .is_some_and(|t| t.nodes.iter().any(|n| n.key == keys::NP_EXPAND));
            plugin_dom::test_overlay(None);
            assert!(described, "the button is described");
            found
        };
        let plain = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        assert!(shown(&ctx, &mut app, plain.clone()), "drawn");
        let mut hiding = plain;
        hiding.hosts.insert(
            keys::NP_EXPAND.into(),
            HostOverride {
                style: Style {
                    display: Some("none".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        assert!(!shown(&ctx, &mut app, hiding), "hidden by the plugin");
        app.backend.shutdown();
    }

    /// A rule that hides TIDAL's lyric lines (`display: none` on the lines,
    /// or on the lyrics box round them) must leave them in the document, as
    /// in a browser. They were dropped from the tree while hidden, so the
    /// rule stopped matching, the lines came back the frame after and were
    /// dropped again: lyrics plugins that draw their own lines over hidden
    /// ones (radiant-lyrics) flickered between theirs and TIDAL's, and their
    /// own lines jumped before and after where TIDAL's had been.
    #[test]
    fn lyric_lines_a_plugin_hides_stay_in_the_document() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let (ctx, mut app) = accessible_app("np-lyrics-hidden");
        apply_flags(&mut app, Some("home"), Some("now-playing-lyrics"));
        let lines = app_lyrics_len(&app);
        assert!(lines > 2, "the demo song has lyrics");
        let described = |ctx: &egui::Context, app: &mut App, overlay: DomOverlay| {
            plugin_dom::test_overlay(Some(overlay));
            for _ in 0..3 {
                shown_frame(ctx, app);
                plugin_dom::end_frame(ctx);
            }
            let tree = plugin_dom::test_sent_tree().expect("a tree was sent");
            plugin_dom::test_overlay(None);
            (0..lines)
                .filter(|i| tree.nodes.iter().any(|n| n.key == keys::lyric(*i)))
                .count()
        };
        let plain = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        assert_eq!(described(&ctx, &mut app, plain.clone()), lines, "shown");
        let hide = |key: &str| {
            (
                key.to_string(),
                HostOverride {
                    style: Style {
                        display: Some("none".into()),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            )
        };
        let mut lines_hidden = plain.clone();
        lines_hidden
            .hosts
            .extend((0..lines).map(|i| hide(&keys::lyric(i))));
        assert_eq!(
            described(&ctx, &mut app, lines_hidden),
            lines,
            "each line hidden"
        );
        let mut box_hidden = plain;
        box_hidden.hosts.extend([hide(keys::LYRICS)]);
        assert_eq!(
            described(&ctx, &mut app, box_hidden),
            lines,
            "the lyrics box hidden"
        );
        app.backend.shutdown();
    }

    /// The lyrics box (`[data-test=now-playing-lyrics]`) is a scroll
    /// container: it reports its scroll position and height, and a plugin's
    /// `scrollTo` on it scrolls the lyrics. Plugins that draw their own lines
    /// and place the sung one themselves (radiant-lyrics) scroll it; it
    /// reported no scroll, so their `scrollTo` went nowhere and the sung line
    /// ran off the bottom of the view.
    #[test]
    fn a_plugin_scrolls_the_lyrics_box() {
        use crate::luna::dom::DomOverlay;
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let (ctx, mut app) = accessible_app("np-lyrics-scroll");
        apply_flags(&mut app, Some("home"), Some("now-playing-lyrics"));
        // Lines a plugin draws stand in for TIDAL's: nothing follows the song
        // but the plugin.
        app.lyrics_following = false;
        let overlay = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        let scroll = |ctx: &egui::Context, app: &mut App| {
            plugin_dom::test_overlay_keep(overlay.clone());
            // Scrolling alone sends the boxes at most every 100 ms.
            for _ in 0..3 {
                shown_frame(ctx, app);
                plugin_dom::end_frame(ctx);
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
            shown_frame(ctx, app);
            plugin_dom::end_frame(ctx);
            let tree = plugin_dom::test_sent_tree().expect("a tree was sent");
            let lyrics = tree
                .nodes
                .iter()
                .find(|n| n.key == keys::LYRICS)
                .expect("the lyrics box");
            (lyrics.scroll, lyrics.rect)
        };
        plugin_dom::test_overlay(Some(overlay.clone()));
        let (before, rect) = scroll(&ctx, &mut app);
        let [top, height] = before.expect("the lyrics box reports its scroll");
        let view = rect.expect("the lyrics box is on screen")[3];
        assert!(height > view, "{height} is more than the view's {view}");
        let wanted = (top + 150.0).min(height - view);
        plugin_dom::test_scroll_top(keys::LYRICS, wanted);
        let (after, _) = scroll(&ctx, &mut app);
        plugin_dom::test_overlay(None);
        let [moved, _] = after.expect("still reported");
        assert!(
            (moved - wanted).abs() < 1.0,
            "scrolled to {moved}, asked for {wanted} (from {top})"
        );
        app.backend.shutdown();
    }

    /// A plugin element inside another one whose box a script read
    /// (`DomOverlay::measure`) is reported where it was drawn: here the third
    /// of three lines a plugin lists in the lyrics, under the first two.
    #[test]
    fn a_nested_plugin_element_is_reported_where_it_is_drawn() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let (ctx, mut app) = accessible_app("np-lyrics-measure");
        apply_flags(&mut app, Some("home"), Some("now-playing-lyrics"));
        let line = |id: u64| VNode {
            id,
            tag: "div".into(),
            style: Style {
                display: Some("block".into()),
                height: Some("30px".into()),
                ..Style::default()
            },
            children: vec![VNode {
                id: id + 100,
                tag: "#text".into(),
                text: Some(format!("line {id}")),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::LYRICS_LINES.into(),
                position: Position::Prepend,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    style: Style {
                        display: Some("block".into()),
                        ..Style::default()
                    },
                    children: vec![line(2), line(3), line(4)],
                    ..VNode::default()
                }],
            }],
            measure: vec![2, 4],
            ..DomOverlay::default()
        };
        plugin_dom::test_overlay(Some(overlay));
        for _ in 0..3 {
            shown_frame(&ctx, &mut app);
            plugin_dom::end_frame(&ctx);
        }
        let tree = plugin_dom::test_sent_tree().expect("a tree was sent");
        plugin_dom::test_overlay(None);
        let at = |id: u64| {
            tree.node_rects
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{id} reported: {:?}", tree.node_rects))
        };
        assert!(
            tree.node_rects.iter().all(|(n, _)| [2, 4].contains(n)),
            "only what was asked for: {:?}",
            tree.node_rects
        );
        let (first, third) = (at(2), at(4));
        assert!(
            third[1] - first[1] >= 59.0 && third[1] - first[1] < 70.0,
            "two 30-point lines apart: {first:?} {third:?}"
        );
        assert!((third[3] - 30.0).abs() < 1.0, "its own height: {third:?}");
        app.backend.shutdown();
    }

    fn app_lyrics_len(app: &App) -> usize {
        match &app.lyrics {
            Loadable::Loaded(Some(l)) => l.lines.len(),
            _ => 0,
        }
    }

    /// radiant-lyrics gives the Lyrics tab `padding-right: 38px` and puts its
    /// sparkle (`div.rl-lyrics-menu-trigger`, `position: absolute; right: 0;
    /// width: 38px`) in that room: TIDAL's pill grows to hold it after the
    /// word. The pill kept its width and the sparkle was never drawn.
    /// (Shown here on Credits, which the demo song always has.)
    #[test]
    fn what_a_plugin_appends_to_a_now_playing_tab_is_inside_its_pill() {
        use crate::luna::dom::{DomOverlay, HostOverride, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let (ctx, mut app) = accessible_app("np-tab-append");
        apply_flags(&mut app, Some("home"), Some("playing-local"));
        app.local.playback = crate::player::Playback::Paused;
        app.now_playing_view = true;
        let lyrics_width = |ctx: &egui::Context, app: &mut App, overlay: DomOverlay| {
            plugin_dom::test_overlay(Some(overlay));
            let mut tree = None;
            for _ in 0..4 {
                tree = shown_frame(ctx, app);
            }
            plugin_dom::test_overlay(None);
            let tree = tree.expect("screen-reader tree");
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some("Credits"))
                .unwrap_or_else(|| panic!("the Credits tab: {:?}", labels(&tree)));
            let b = node.bounds().expect("bounds");
            (b.x1 - b.x0) as f32
        };
        let plain = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        let bare = lyrics_width(&ctx, &mut app, plain.clone());
        let tab = keys::now_playing_tab("credits");
        let mut sparkle = plain;
        sparkle.hosts.insert(
            tab.clone(),
            HostOverride {
                style: Style {
                    position: Some("relative".into()),
                    padding_right: Some("38px".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        sparkle.injected.push(Injected {
            anchor: tab.clone(),
            position: Position::Append,
            nodes: vec![VNode {
                id: 1,
                tag: "div".into(),
                style: Style {
                    position: Some("absolute".into()),
                    right: Some("0".into()),
                    top: Some("0".into()),
                    bottom: Some("0".into()),
                    width: Some("38px".into()),
                    background_color: Some("#ff6b9d".into()),
                    ..Style::default()
                },
                ..VNode::default()
            }],
        });
        plugin_dom::test_overlay(Some(sparkle.clone()));
        let mut pink = 0;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| crate::ui::show(&mut app, ui));
            out.textures_delta.clear();
            let pink_fill = egui::Color32::from_rgb(0xff, 0x6b, 0x9d);
            pink = out
                .shapes
                .iter()
                .filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == pink_fill))
                .count();
        }
        plugin_dom::test_overlay(None);
        assert!(pink > 0, "the sparkle's box is drawn");
        let holding = lyrics_width(&ctx, &mut app, sparkle);
        assert!(
            (holding - bare - 27.0).abs() < 1.5,
            "38 points of padding in place of 11: {holding} vs {bare}"
        );
        app.backend.shutdown();
    }

    /// The shapes of the now-playing view after a few frames under
    /// `overlay`, with the Credits tab open.
    fn open_tab_shapes(
        name: &str,
        overlay: crate::luna::dom::DomOverlay,
    ) -> Vec<egui::epaint::ClippedShape> {
        let (ctx, mut app) = accessible_app(name);
        apply_flags(&mut app, Some("home"), Some("playing-local"));
        app.local.playback = crate::player::Playback::Paused;
        app.now_playing_view = true;
        app.now_playing_tab = Some(crate::model::NowPlayingTab::Credits);
        crate::ui::plugin_dom::test_overlay(Some(overlay));
        let mut shapes = Vec::new();
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| crate::ui::show(&mut app, ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        crate::ui::plugin_dom::test_overlay(None);
        app.backend.shutdown();
        shapes
    }

    /// The colour a text shape is drawn in: its glyphs' own, or the
    /// shape's fallback where they have none.
    fn shape_text_color(t: &egui::epaint::TextShape) -> egui::Color32 {
        let glyph = t
            .galley
            .rows
            .iter()
            .flat_map(|row| row.row.visuals.mesh.vertices.iter())
            .map(|vertex| vertex.color)
            .next()
            .unwrap_or(egui::Color32::PLACEHOLDER);
        if glyph == egui::Color32::PLACEHOLDER {
            t.override_text_color.unwrap_or(t.fallback_color)
        } else {
            glyph
        }
    }

    /// radiant-lyrics' sparkle in the open Lyrics tab is `#1e1e1e` on the
    /// tab's white pill. It was read against the dark window, made white
    /// to stay readable there, and vanished into the white pill: it is
    /// read against the pill now and stays dark. (A glyph stands in for the
    /// sparkle; the open tab is Credits, which the demo song always has.)
    #[test]
    fn what_a_plugin_draws_in_the_open_tab_is_read_against_its_white_pill() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        let tab = keys::now_playing_tab("credits");
        let glyph = VNode {
            id: 2,
            tag: "span".into(),
            children: vec![VNode {
                id: 3,
                tag: "#text".into(),
                text: Some("Q".into()),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: tab,
                position: Position::Append,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    style: Style {
                        position: Some("absolute".into()),
                        right: Some("0".into()),
                        top: Some("0".into()),
                        bottom: Some("0".into()),
                        width: Some("38px".into()),
                        color: Some("#1e1e1e".into()),
                        ..Style::default()
                    },
                    children: vec![glyph],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        };
        let shapes = open_tab_shapes("np-tab-ink", overlay);
        let colors: Vec<egui::Color32> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Q" => Some(shape_text_color(t)),
                _ => None,
            })
            .collect();
        assert!(!colors.is_empty(), "the glyph is drawn");
        assert!(
            colors
                .iter()
                .all(|c| c.r() < 80 && c.g() < 80 && c.b() < 80),
            "dark on the white pill: {colors:?}"
        );
    }

    /// While radiant-lyrics' options menu hangs under the Lyrics tab, the
    /// tab is `min-width: 150px` with `border-radius: 12px 12px 0 0` (and
    /// its 38-point padding for the sparkle): TIDAL draws it square at the
    /// bottom and centres the word in what the padding leaves. The pill
    /// stayed round and kept its word at the left.
    #[test]
    fn a_widened_open_tab_takes_the_plugins_corners_and_centres_its_word() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        let mut overlay = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        overlay.hosts.insert(
            keys::now_playing_tab("credits"),
            HostOverride {
                style: Style {
                    padding_right: Some("38px".into()),
                    min_width: Some("150px".into()),
                    border_radius: Some("12px 12px 0 0".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        let shapes = open_tab_shapes("np-tab-menu", overlay);
        let pill = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r)
                    if r.fill.a() > 0
                        && r.fill.r() == r.fill.g()
                        && (r.rect.width() - 150.0).abs() < 0.5
                        && (r.rect.height() - 30.0).abs() < 0.5 =>
                {
                    Some(r.clone())
                }
                _ => None,
            })
            .expect("the 150-point pill");
        assert_eq!(
            pill.corner_radius,
            egui::CornerRadius {
                nw: 12,
                ne: 12,
                se: 0,
                sw: 0
            }
        );
        let word = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == "Credits" && pill.rect.contains(t.pos) =>
                {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                _ => None,
            })
            .expect("the word in the pill");
        let middle = (pill.rect.left() + 11.0 + pill.rect.right() - 38.0) / 2.0;
        assert!(
            (word.center().x - middle).abs() < 1.0,
            "centred in what the padding leaves: {word:?} in {:?}",
            pill.rect
        );
    }

    /// Allocations this thread makes drawing `view` once, after it settled.
    #[cfg(feature = "demo")]
    fn view_allocations(page: &str, view: fn(&mut App, &mut egui::Ui)) -> u64 {
        let root = crate::paths::unique_test_dir(&format!("allocations-{page}"));
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        populate(&mut app);
        app.reveal_theme_changes = false;
        apply_flags(&mut app, Some(page), None);
        let pass = |app: &mut App| {
            let before = std::cell::Cell::new(0);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    // As a window's pass: the playing song read once.
                    app.refresh_frame_now();
                    before.set(crate::bench::thread_allocations());
                    view(app, ui);
                    before.set(crate::bench::thread_allocations() - before.get());
                    app.clear_frame_now();
                },
            );
            output.textures_delta.clear();
            before.get()
        };
        for _ in 0..4 {
            pass(&mut app);
        }
        let made = pass(&mut app);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
        eprintln!("{page}: {made} allocations a pass");
        made
    }

    /// Home draws its shelves, recent plays, top artists and songs where
    /// they are: a pass copied every one of them (about 3,000 allocations
    /// in all, 3,055 here), which cost Home three to five times other pages.
    #[cfg(feature = "demo")]
    #[test]
    fn a_home_pass_does_not_copy_its_lists() {
        let made = view_allocations("home", crate::ui::home::show);
        assert!(made < 1_000, "{made} allocations a Home pass");
    }

    /// A song row asks whether it is the playing one, and a card whether
    /// it holds the playing context, without copying what plays (a page of
    /// rows made a copy of the playing song for each); search draws its
    /// results and an artist its top tracks without copying them.
    /// Before: 1,027, 1,275 and 906 allocations a pass.
    #[cfg(feature = "demo")]
    #[test]
    fn song_pages_do_not_copy_their_rows_or_what_plays() {
        fn top_songs(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::top_songs(app, ui);
        }
        fn search(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::search::show(app, ui);
        }
        fn artist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::artist::show(app, ui, "art0");
        }
        let passes = [
            ("top-songs", top_songs as fn(&mut App, &mut egui::Ui), 900),
            ("search", search, 900),
            ("artist:art0", artist, 700),
        ]
        .map(|(page, view, most)| (page, view_allocations(page, view), most));
        for (page, made, most) in passes {
            assert!(made < most, "{made} allocations a pass of {page}");
        }
    }

    /// TIDAL's account `username` is the e-mail address. It never reaches the
    /// interface: the views that name the signed-in user (top bar, Home,
    /// the Library, the header of their own playlist) show the profile name,
    /// or nothing until it is known.
    #[cfg(feature = "demo")]
    #[test]
    fn no_view_outside_the_account_settings_shows_the_session_email() {
        const EMAIL: &str = "listener@example.com";
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        fn playlists(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::library::show(app, ui, Page::Playlists);
        }
        let (ctx, mut app) = accessible_app("session-email");
        apply_flags(&mut app, Some("playlist:pl1"), None);
        // The account came back with its e-mail as the only name.
        app.user.as_mut().unwrap().display_name = Some(EMAIL.into());
        if let Some(lists) = app.library.playlists.get_mut() {
            for list in lists
                .iter_mut()
                .filter(|list| list.owner.id.as_deref() == Some("demo"))
            {
                list.owner.display_name = None;
            }
        }
        if let Some(page) = app.playlist_pages.get_mut("pl1")
            && let Some(list) = page.playlist.get_mut()
        {
            list.owner.display_name = Some(EMAIL.into());
        }
        type View = fn(&mut App, &mut egui::Ui);
        let views: [(&str, View); 4] = [
            ("playlist", playlist),
            ("playlists", playlists),
            ("home", crate::ui::home::show),
            ("top bar", |app, ui| {
                crate::ui::topbar::show(app, ui);
            }),
        ];
        for pass in 0..2 {
            for (name, view) in views {
                let painted = view_frame(&ctx, &mut app, vec![], view);
                for (text, _) in &painted {
                    assert!(
                        !text.contains(EMAIL) && !text.contains('@'),
                        "{name} shows the e-mail on pass {pass}: {text}"
                    );
                }
            }
            if pass == 0 {
                // The profile lookup then names the user; the header shows it.
                app.set_user_name("demo".into(), Some("Carmine C.".into()));
            }
        }
        let header = view_frame(&ctx, &mut app, vec![], playlist);
        assert!(header.iter().any(|(text, _)| text.contains("Carmine C.")));
        app.backend.shutdown();
    }

    /// `--demo-show rtl` fills the playlist with right-to-left titles, which
    /// the page hands to layout in the order they were typed.
    #[cfg(feature = "demo")]
    #[test]
    fn the_rtl_demo_shows_right_to_left_titles() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        let (ctx, mut app) = accessible_app("rtl-titles");
        apply_flags(&mut app, Some("playlist:pl1"), Some("rtl"));
        view_frame(&ctx, &mut app, vec![], playlist);
        let painted = view_frame(&ctx, &mut app, vec![], playlist);
        for (title, artist, _) in RTL_TRACKS {
            assert!(
                painted.iter().any(|(text, _)| text == title),
                "{title} is not drawn"
            );
            assert!(
                painted.iter().any(|(text, _)| text.contains(artist)),
                "{artist} is not drawn"
            );
        }
        app.backend.shutdown();
    }

    /// Emoji titles keep their own text: the colour pictures are painted
    /// over hidden glyphs, so layout, copying and screen readers see the
    /// title exactly as TIDAL sent it.
    #[cfg(feature = "demo")]
    #[test]
    fn emoji_titles_are_drawn_whole() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        let (ctx, mut app) = accessible_app("emoji-titles");
        apply_flags(&mut app, Some("playlist:pl1"), Some("emoji"));
        view_frame(&ctx, &mut app, vec![], playlist);
        let painted = view_frame(&ctx, &mut app, vec![], playlist);
        for title in ["Road Trip 🚗💨", "Family 👨‍👩‍👧 Sunday", "🇮🇹 Estate"] {
            assert!(
                painted.iter().any(|(text, _)| text == title),
                "{title} is not drawn"
            );
        }
        app.backend.shutdown();
    }

    /// Beside an open side panel the search filter pills stay one row, as
    /// TIDAL's, and run past the page's edge, cut there: the row scrolls
    /// (the wheel, a drag) so the ones past it can be reached (they used to
    /// wrap onto another row; see
    /// `the_search_pills_are_one_row_that_scrolls_when_they_do_not_fit`).
    #[cfg(feature = "demo")]
    #[test]
    fn search_filter_pills_are_one_row_cut_at_a_narrow_page() {
        fn page(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::search::show(app, ui);
        }
        let (ctx, mut app) = accessible_app("search-pills-narrow");
        apply_flags(&mut app, None, Some("search"));
        let size = egui::vec2(300.0, 1200.0);
        view_frame_sized(&ctx, &mut app, vec![], size, page);
        let painted = view_frame_sized(&ctx, &mut app, vec![], size, page);
        app.backend.shutdown();
        let find = |label: &str| {
            painted
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("a {label} pill"))
                .1
        };
        let (first, last) = (find("Top results"), find("Playlists"));
        assert_eq!(last.top(), first.top(), "one row, not wrapped");
        assert!(
            last.left() > size.x,
            "the Playlists pill starts at {}, past the {} point page",
            last.left(),
            size.x
        );
    }

    /// The My Collection playlists page says something when the list is
    /// empty, failed, or filtered to nothing: it used to stay blank.
    #[cfg(feature = "demo")]
    #[test]
    fn the_playlists_page_is_never_blank() {
        use crate::model::Loadable;
        fn page(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::library::show(app, ui, crate::model::Page::Playlists);
        }
        let says = |state: Loadable<Vec<crate::api::models::Playlist>>, word: &str| {
            let (ctx, mut app) = accessible_app("playlists-states");
            app.library.playlists = state;
            view_frame(&ctx, &mut app, vec![], page);
            let painted = view_frame(&ctx, &mut app, vec![], page);
            app.backend.shutdown();
            painted.iter().any(|(text, _)| text.contains(word))
        };
        assert!(says(Loadable::Loaded(Vec::new()), "Create a playlist"));
        assert!(says(Loadable::Failed("offline".into()), "Retry"));
    }

    /// TIDAL's own mixes carry only the epoch it stamps on dates it never
    /// recorded: their table has no Date added column, and the album names
    /// sit under their heading rather than a column's width to its right.
    #[cfg(feature = "demo")]
    #[test]
    fn an_undated_mix_has_no_date_added_column() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl0");
        }
        let left_of = |painted: &[(String, egui::Rect)], label: &str| {
            painted
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, rect)| rect.left())
        };
        let render = |name: &str, show: Option<&str>| {
            let (ctx, mut app) = accessible_app(name);
            apply_flags(&mut app, Some("playlist:pl0"), show);
            view_frame(&ctx, &mut app, vec![], playlist);
            let painted = view_frame(&ctx, &mut app, vec![], playlist);
            let first_album = match &app.playlist_pages["pl0"].items.items[0].item {
                Some(PlayableItem::Track(track)) => track.album.clone().expect("an album").name,
                _ => panic!("the demo playlist starts with a song"),
            };
            app.backend.shutdown();
            (painted, first_album)
        };

        // #given the dated demo playlist
        let (painted, _) = render("dated-mix", None);
        assert!(
            left_of(&painted, "DATE ADDED").is_some(),
            "a dated playlist has the column"
        );

        // #when every song carries the epoch
        let (painted, first_album) = render("undated-mix", Some("undated-mix"));

        // #then there is no column, and the album names line up with their heading
        assert_eq!(left_of(&painted, "DATE ADDED"), None);
        let heading = left_of(&painted, "ALBUM").expect("the album heading");
        let album = left_of(&painted, &first_album).expect("the first song's album");
        assert!(
            (heading - album).abs() < 1.0,
            "album heading at {heading}, album names at {album}"
        );
    }

    /// Over the now-playing view the player bar's words and icons are drawn
    /// for the darkened cover under them, in both themes, as TIDAL draws
    /// them: the Light palette's near-black on that dark glass was close to
    /// invisible. Each word and icon in the bar, laid over the scrim above
    /// a light cover and above a dark one, keeps WCAG AA contrast (4.5:1
    /// for words, 3:1 for icons).
    #[cfg(feature = "demo")]
    #[test]
    fn the_player_bar_stays_readable_over_the_now_playing_background_in_both_themes() {
        use egui::epaint::Shape;
        use egui::{Color32, Pos2};
        fn flat(shape: &Shape, out: &mut Vec<Shape>) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| flat(shape, out)),
                other => out.push(other.clone()),
            }
        }
        let (width, height) = (1280.0, 800.0);
        for theme in ["light", "dark"] {
            let (ctx, mut app) = accessible_app(&format!("np-bar-{theme}"));
            apply_flags(&mut app, None, Some(&format!("{theme},now-playing")));
            let mut shapes = Vec::new();
            for frame in 0..6 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        time: Some(f64::from(frame) * 0.05),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                shapes.clear();
                for clipped in &output.shapes {
                    flat(&clipped.shape, &mut shapes);
                }
            }
            assert_eq!(
                app.palette.dark,
                theme == "dark",
                "the {theme} theme applied"
            );
            let bar_top = height - 88.0;
            // The solid discs (the play button) carry their own icon colour.
            let discs: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Circle(circle)
                        if circle.fill.a() == 255 && circle.center.y > bar_top =>
                    {
                        Some(egui::Rect::from_center_size(
                            circle.center,
                            egui::Vec2::splat(circle.radius * 2.0),
                        ))
                    }
                    _ => None,
                })
                .collect();
            let mut words = Vec::new();
            let mut icons = Vec::new();
            for shape in &shapes {
                match shape {
                    Shape::Text(text)
                        if text.pos.y > bar_top && !text.galley.job.text.is_empty() =>
                    {
                        let color = text.override_text_color.unwrap_or_else(|| {
                            text.galley
                                .job
                                .sections
                                .iter()
                                .map(|section| section.format.color)
                                .find(|color| *color != Color32::PLACEHOLDER)
                                .unwrap_or(text.fallback_color)
                        });
                        let at = text.pos + text.galley.rect.center().to_vec2();
                        words.push((text.galley.job.text.clone(), color, at));
                    }
                    Shape::Rect(rect)
                        if rect.brush.is_some()
                            && rect.rect.min.y > bar_top
                            && rect.rect.width() <= 24.0
                            // Not the placeholder on the cover tile.
                            && rect.rect.center().x > 90.0
                            && !discs.iter().any(|disc| disc.contains_rect(rect.rect)) =>
                    {
                        icons.push((rect.rect.center(), rect.fill));
                    }
                    _ => {}
                }
            }
            assert!(words.len() >= 3, "{theme}: words in the bar: {words:?}");
            assert!(icons.len() >= 6, "{theme}: icons in the bar: {icons:?}");
            // A light cover, blurred, and a dark one: what shows under the
            // scrim at a point of the bar.
            let covers = [
                ("light cover", Color32::from_rgb(235, 230, 220)),
                ("dark cover", Color32::from_rgb(20, 18, 24)),
            ];
            let under = |cover: Color32, at: Pos2| {
                let alpha = crate::ui::player_background::scrim_alpha(at.x / width, at.y / height);
                crate::motion::blend(cover, Color32::BLACK, alpha)
            };
            let seen = |color: Color32, ground: Color32| {
                crate::motion::blend(ground, color.to_opaque(), f32::from(color.a()) / 255.0)
            };
            for (name, cover) in covers {
                // Dark's own times are TIDAL's 35 % white, a design choice
                // that stays as it is (3:1, over its dark covers only).
                // Light's words are drawn to AA over both.
                let words_need = match (theme, name) {
                    ("light", _) => 4.5,
                    (_, "dark cover") => 3.0,
                    _ => 0.0,
                };
                for (text, color, at) in &words {
                    let ground = under(cover, *at);
                    let ratio = crate::ui::plugin_dom::contrast(seen(*color, ground), ground);
                    assert!(
                        ratio >= words_need,
                        "{theme}, {name}: {text:?} {color:?} on {ground:?} is {ratio:.2}:1"
                    );
                }
                for (at, color) in &icons {
                    let ground = under(cover, *at);
                    let ratio = crate::ui::plugin_dom::contrast(seen(*color, ground), ground);
                    assert!(
                        ratio >= 3.0,
                        "{theme}, {name}: icon at {at:?} {color:?} on {ground:?} is {ratio:.2}:1"
                    );
                }
            }
            app.backend.shutdown();
        }
    }

    /// Opening the now-playing view never shows a raw, undimmed background:
    /// in every frame of the slide, each wash (the dark base the blurred
    /// cover sits on) is followed in its own layer by the scrim over the
    /// same area, and the scrim is already dark from the first frame.
    #[cfg(feature = "demo")]
    #[test]
    fn the_now_playing_slide_never_shows_an_undimmed_background() {
        use egui::epaint::Shape;
        let (ctx, mut app) = accessible_app("np-flash");
        app.motion.force(false);
        crate::motion::apply(&ctx, false);
        fn flat(shape: &Shape, out: &mut Vec<Shape>) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| flat(shape, out)),
                other => out.push(other.clone()),
            }
        }
        let base = egui::Color32::from_rgb(0x12, 0x12, 0x14);
        let mut washes = 0;
        for frame in 0..16 {
            if frame == 2 {
                app.now_playing_view = true;
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    time: Some(f64::from(frame) * 0.05),
                    ..Default::default()
                },
                |ui| crate::ui::show(&mut app, ui),
            );
            output.textures_delta.clear();
            let mut shapes = Vec::new();
            for clipped in &output.shapes {
                flat(&clipped.shape, &mut shapes);
            }
            for (index, shape) in shapes.iter().enumerate() {
                let Shape::Rect(rect) = shape else { continue };
                if rect.fill != base || rect.rect.width() < 1000.0 {
                    continue;
                }
                washes += 1;
                let scrim = shapes[index + 1..].iter().find_map(|later| match later {
                    Shape::Mesh(mesh) if mesh.vertices.len() > 100 => Some(mesh),
                    _ => None,
                });
                let scrim =
                    scrim.unwrap_or_else(|| panic!("frame {frame}: a wash without its scrim"));
                let bounds = scrim.calc_bounds();
                assert!(
                    (bounds.min - rect.rect.min).abs().max_elem() < 1.0
                        && (bounds.max - rect.rect.max).abs().max_elem() < 1.0,
                    "frame {frame}: the scrim {bounds:?} does not cover the wash {:?}",
                    rect.rect
                );
                let darkest = scrim.vertices.iter().map(|v| v.color.a()).min().unwrap();
                assert!(darkest >= 80, "frame {frame}: scrim alpha {darkest}");
            }
        }
        assert!(washes > 4, "the view was drawn in {washes} frames");
        app.backend.shutdown();
    }

    /// The background turns by the clock, so while an animated cover's frames
    /// wake the window it asks for no ~30 Hz timer of its own: the two timers
    /// both running kept the view at 72 passes a second for a 24 frame clip.
    #[cfg(feature = "demo")]
    #[test]
    fn the_turning_background_shares_passes_with_an_animated_cover() {
        use std::time::Duration;
        let (ctx, mut app) = accessible_app("np-shared-passes");
        app.motion.force(false);
        crate::motion::apply(&ctx, false);
        apply_flags(&mut app, None, Some("now-playing-local"));
        let mut asked = |driven: bool| {
            let mut delay = Duration::ZERO;
            for frame in 0..40 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        time: Some(f64::from(frame) * 0.05),
                        ..Default::default()
                    },
                    |ui| {
                        if driven {
                            crate::video_cover::drives_passes(ui.ctx());
                        }
                        crate::ui::show(&mut app, ui);
                    },
                );
                output.textures_delta.clear();
                delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            }
            delay
        };
        let alone = asked(false);
        assert!(
            alone <= Duration::from_millis(34),
            "turning alone: {alone:?}"
        );
        let shared = asked(true);
        assert!(
            shared > Duration::from_millis(100),
            "with a cover: {shared:?}"
        );
        app.backend.shutdown();
    }

    /// The now-playing view slides as one piece: mid-slide, in both themes
    /// and both directions, every wash, scrim and cover mesh lies inside the
    /// panel's animated rect (its top edge moves with the slide), and the
    /// page is still drawn above that edge.
    #[cfg(feature = "demo")]
    #[test]
    fn the_now_playing_wash_slides_with_the_panel() {
        use egui::epaint::Shape;
        fn flat(shape: &Shape, out: &mut Vec<(Shape, egui::Rect)>, clip: egui::Rect) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| flat(shape, out, clip)),
                other => out.push((other.clone(), clip)),
            }
        }
        let base = egui::Color32::from_rgb(0x12, 0x12, 0x14);
        for light in [false, true] {
            for closing in [false, true] {
                let (ctx, mut app) = accessible_app("np-slide-clip");
                app.settings.theme = if light {
                    crate::settings::ThemeChoice::Light
                } else {
                    crate::settings::ThemeChoice::Dark
                };
                app.motion.force(false);
                crate::motion::apply(&ctx, false);
                app.now_playing_view = closing;
                let mut checked = 0;
                for frame in 0..24 {
                    if frame == 6 {
                        app.now_playing_view = !closing;
                    }
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1280.0, 800.0),
                            )),
                            time: Some(f64::from(frame) * 0.04),
                            ..Default::default()
                        },
                        |ui| crate::ui::show(&mut app, ui),
                    );
                    output.textures_delta.clear();
                    let mut shapes = Vec::new();
                    for clipped in &output.shapes {
                        flat(&clipped.shape, &mut shapes, clipped.clip_rect);
                    }
                    // The panel's top edge is where its wash begins.
                    let Some(top) = shapes.iter().find_map(|(shape, _)| match shape {
                        Shape::Rect(rect) if rect.fill == base && rect.rect.width() > 1000.0 => {
                            Some(rect.rect.top())
                        }
                        _ => None,
                    }) else {
                        continue;
                    };
                    if !(40.0..=700.0).contains(&top) {
                        continue;
                    }
                    checked += 1;
                    for (shape, clip) in &shapes {
                        let bounds = match shape {
                            Shape::Mesh(mesh) if mesh.vertices.len() > 3 => mesh.calc_bounds(),
                            Shape::Rect(rect) if rect.fill == base => rect.rect,
                            _ => continue,
                        };
                        if bounds.width() < 600.0 {
                            continue;
                        }
                        // The clip is what bounds the turning cover, whose
                        // quad is far larger than the panel.
                        assert!(
                            clip.top() >= top - 1.0 && bounds.top() >= top - 1.0,
                            "light {light} closing {closing} frame {frame}: a wash shows from {} above the panel at {top}",
                            clip.top()
                        );
                    }
                    assert!(
                        shapes.iter().any(|(shape, clip)| {
                            matches!(shape, Shape::Text(text)
                                if text.pos.y < top - 8.0 && clip.top() < top - 8.0)
                        }),
                        "light {light} closing {closing} frame {frame}: the page above {top} is not drawn"
                    );
                }
                assert!(
                    checked > 2,
                    "light {light} closing {closing}: {checked} mid-slide frames"
                );
                app.backend.shutdown();
            }
        }
    }

    /// A plugin asking to run a program is shown over any page, nothing is
    /// answered until a button is used, and each button gives its own answer;
    /// Always is not offered for a command that needs a shell.
    #[test]
    fn the_program_prompt_asks_and_each_button_answers() {
        use crate::luna::{ProgramChoice, ProgramRequest};
        use egui::accesskit::{Action, Role};
        let request = |shell: bool| ProgramRequest {
            id: 7,
            plugin: "Mpv".into(),
            program: "/usr/bin/mpv".into(),
            name: "mpv".into(),
            command: "mpv --idle".into(),
            shell,
            remember: !shell,
        };
        let labels = [
            ("Allow once", ProgramChoice::Once),
            ("Always allow", ProgramChoice::Always),
            ("Deny", ProgramChoice::Deny),
        ];
        for (label, expected) in labels {
            let (ctx, mut app) = accessible_app("program-prompt");
            app.plugins.demo_program = Some(request(false));
            accessible_frame(&ctx, &mut app, vec![]);
            let tree = accessible_frame(&ctx, &mut app, vec![]);
            let title = "Mpv wants to run mpv on your computer";
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.value() == Some(title) || n.label() == Some(title)),
                "the question names the plugin and the program"
            );
            assert!(
                app.plugins.demo_program.is_some() && app.plugins.demo_answer.is_none(),
                "nothing is answered by showing it"
            );
            let button = accessible_node(&tree, label, Role::Button);
            accessible_frame(
                &ctx,
                &mut app,
                vec![accessible_action(button, Action::Click, None)],
            );
            assert_eq!(app.plugins.demo_answer, Some(expected), "{label}");
            assert!(app.plugins.demo_program.is_none(), "{label} closes it");
            app.backend.shutdown();
        }
        // A command that needs a shell cannot be allowed for good.
        let (ctx, mut app) = accessible_app("program-prompt-shell");
        app.plugins.demo_program = Some(request(true));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            tree.nodes
                .iter()
                .all(|(_, n)| n.label() != Some("Always allow")),
            "no Always for a shell command"
        );
        accessible_node(&tree, "Allow once", Role::Button);
        // Esc refuses.
        accessible_frame(
            &ctx,
            &mut app,
            vec![keyboard(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert_eq!(app.plugins.demo_answer, Some(ProgramChoice::Deny));
        app.backend.shutdown();
    }

    /// A plugin asking for a permission at first use: the question says what
    /// the plugin gets, nothing is answered by showing it, Deny is the
    /// filled button and has the keyboard focus (Enter denies), each button
    /// gives its own answer, and Esc denies.
    #[test]
    fn the_permission_prompt_says_what_it_gives_and_denies_by_default() {
        use crate::luna::{Access, PermissionRequest, ProgramChoice};
        use egui::accesskit::{Action, Role};
        let request = |access: Access| PermissionRequest {
            id: 9,
            plugin: "Plug".into(),
            access,
            detail: vec!["MediaPlayPause".into()],
        };
        let cases = [
            (
                Access::Credentials,
                "Plug wants your TIDAL sign-in",
                "act as you on TIDAL and send your sign-in anywhere",
            ),
            (
                Access::GlobalShortcuts,
                "Plug wants keyboard shortcuts across your whole computer",
                "MediaPlayPause",
            ),
            (
                Access::Pipe {
                    path: "/tmp/mpv.sock".into(),
                },
                "Plug wants to talk to another program on your computer",
                "/tmp/mpv.sock",
            ),
            (
                Access::SaveAudio,
                "Plug wants to save TIDAL songs as files",
                "This may break TIDAL's terms of use",
            ),
        ];
        let text_of = |tree: &egui::accesskit::TreeUpdate| -> String {
            tree.nodes
                .iter()
                .filter_map(|(_, n)| n.value().or(n.label()).map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for (access, title, says) in cases {
            for (label, expected) in [
                ("Allow once", ProgramChoice::Once),
                ("Always allow", ProgramChoice::Always),
                ("Deny", ProgramChoice::Deny),
            ] {
                let (ctx, mut app) = accessible_app("permission-prompt");
                app.plugins.demo_permission = Some(request(access.clone()));
                accessible_frame(&ctx, &mut app, vec![]);
                let tree = accessible_frame(&ctx, &mut app, vec![]);
                let text = text_of(&tree);
                assert!(text.contains(title), "{title} in {text}");
                assert!(text.contains(says), "{says} in {text}");
                assert!(
                    app.plugins.demo_permission_answer.is_none(),
                    "nothing is answered by showing it"
                );
                let deny = accessible_node(&tree, "Deny", Role::Button);
                assert_eq!(tree.focus, deny, "Deny has the keyboard focus");
                let button = accessible_node(&tree, label, Role::Button);
                accessible_frame(
                    &ctx,
                    &mut app,
                    vec![accessible_action(button, Action::Click, None)],
                );
                assert_eq!(
                    app.plugins.demo_permission_answer,
                    Some(expected),
                    "{label}"
                );
                assert!(app.plugins.demo_permission.is_none(), "{label} closes it");
                app.backend.shutdown();
            }
        }
        // Enter on the focused button, and Esc, deny.
        for key in [egui::Key::Enter, egui::Key::Escape] {
            let (ctx, mut app) = accessible_app("permission-prompt-keys");
            app.plugins.demo_permission = Some(request(Access::Credentials));
            accessible_frame(&ctx, &mut app, vec![]);
            accessible_frame(&ctx, &mut app, vec![]);
            accessible_frame(&ctx, &mut app, vec![keyboard(key, egui::Modifiers::NONE)]);
            accessible_frame(&ctx, &mut app, vec![]);
            assert_eq!(
                app.plugins.demo_permission_answer,
                Some(ProgramChoice::Deny),
                "{key:?}"
            );
            app.backend.shutdown();
        }
    }

    /// A plugin cannot draw over the permission prompt: a box it shows once
    /// the prompt is up (`position: fixed`, a positive `z-index`, clicks
    /// passing through) would otherwise be drawn above it, with whatever
    /// words it likes over the prompt's buttons, while the clicks still went
    /// to the buttons under it.
    #[test]
    fn a_plugin_box_is_never_drawn_over_the_permission_prompt() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        use crate::luna::{Access, PermissionRequest};
        let (ctx, mut app) = accessible_app("permission-cover");
        // As `ui::show` draws it, with Luna's overlay as the test sets it.
        let frame = |ctx: &egui::Context, app: &mut App| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
        };
        app.plugins.demo_permission = Some(PermissionRequest {
            id: 9,
            plugin: "Plug".into(),
            access: Access::Credentials,
            detail: vec![],
        });
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 77,
                    tag: "div".into(),
                    style: Style {
                        position: Some("fixed".into()),
                        z_index: Some("10".into()),
                        left: Some("0px".into()),
                        top: Some("0px".into()),
                        width: Some("1280px".into()),
                        height: Some("800px".into()),
                        background_color: Some("#ff0000".into()),
                        pointer_events: Some("none".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        let mut orders = Vec::new();
        for _ in 0..3 {
            frame(&ctx, &mut app);
            orders.push(ctx.memory(|m| m.layer_ids().collect::<Vec<_>>()));
        }
        crate::ui::plugin_dom::test_overlay(None);
        app.backend.shutdown();
        for order in orders {
            let at = |id: egui::Id| order.iter().position(|l| l.id == id);
            let prompt = at(egui::Id::new(("luna-permission-prompt", 9u64)));
            let cover = at(egui::Id::new(("luna-float", 77u64)));
            assert!(prompt.is_some() && cover.is_some(), "{order:?}");
            assert!(
                prompt > cover,
                "the plugin's box is over the prompt: {order:?}"
            );
        }
    }

    /// An image wider or taller than the GPU's largest texture (a plugin's
    /// or a theme's, here a 3000 by 3 PNG as a box's background against the
    /// harness's 2048) reaches the painter scaled to fit. The glow painter
    /// asserts on a larger texture, and release builds abort on a panic, so
    /// one such image used to close the app at every start.
    #[test]
    fn an_image_larger_than_the_gpu_takes_is_scaled_to_fit() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        use base64::Engine as _;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3000, 3, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let (ctx, mut app) = accessible_app("huge-texture");
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 78,
                    tag: "div".into(),
                    style: Style {
                        position: Some("fixed".into()),
                        z_index: Some("10".into()),
                        left: Some("0px".into()),
                        top: Some("0px".into()),
                        width: Some("600px".into()),
                        height: Some("40px".into()),
                        background_image: Some(format!("url(data:image/png;base64,{data})")),
                        background_size: Some("cover".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        let most = ctx.input(|i| i.max_texture_side);
        let mut sizes = Vec::new();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < end
            && !sizes.iter().any(|s: &[usize; 2]| s[0] >= 1000 && s[1] <= 3)
        {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::show(&mut app, ui),
            );
            sizes.extend(
                output
                    .textures_delta
                    .set
                    .values()
                    .flat_map(|d| d.iter().map(|d| d.image.size())),
            );
            output.textures_delta.clear();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        crate::ui::plugin_dom::test_overlay(None);
        app.backend.shutdown();
        assert!(
            sizes.iter().any(|s| s[0] >= 1000 && s[1] <= 3),
            "the image never arrived: {sizes:?}"
        );
        for s in &sizes {
            assert!(
                s[0] <= most && s[1] <= most,
                "{s:?} is over {most}: {sizes:?}"
            );
        }
    }

    /// The equalizer card in a demo window: the crossfeed row is there with
    /// its strengths, the switch is as the state left it, and a click on a
    /// strength asks for it.
    #[test]
    fn the_equalizer_card_offers_the_crossfeed() {
        fn card(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::equalizer::contents(app, ui);
        }
        let (ctx, mut app) = accessible_app("eq-crossfeed-card");
        apply_flags(&mut app, None, Some("eq-crossfeed"));
        view_frame(&ctx, &mut app, vec![], card);
        let painted = view_frame(&ctx, &mut app, vec![], card);
        let find = |label: &str| {
            painted
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, r)| *r)
        };
        assert!(find("Headphone crossfeed").is_some());
        for label in ["Off", "Light", "Medium"] {
            assert!(find(label).is_some(), "{label}");
        }
        let strong = find("Strong").expect("the strongest crossfeed");
        view_frame(
            &ctx,
            &mut app,
            pointer_click(strong.center(), egui::PointerButton::Primary),
            card,
        );
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::SetEqCrossfeed(crate::eq::Crossfeed::Strong))),
            "the click asks for the strong crossfeed"
        );
        app.backend.shutdown();
    }

    /// A click on the player bar itself, away from its controls, slides the
    /// now-playing view up and a second click closes it, as TIDAL's bar
    /// does; a click on a control stays that control's.
    #[cfg(feature = "demo")]
    #[test]
    fn clicking_the_player_bar_opens_and_closes_the_now_playing_view() {
        let (ctx, mut app) = accessible_app("player-bar-view");
        let draw = |app: &mut App, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::player_bar::show(app, ui),
            );
            output.textures_delta.clear();
        };
        let toggles = |app: &App| {
            app.actions
                .iter()
                .filter(|action| matches!(action, Action::ToggleNowPlaying))
                .count()
        };
        draw(&mut app, vec![]);
        assert!(!app.now_playing_view);
        // The margin at the bar's left edge, beside the cover.
        let empty = egui::pos2(6.0, 796.0);
        draw(&mut app, pointer_click(empty, egui::PointerButton::Primary));
        assert_eq!(toggles(&app), 1);
        for action in std::mem::take(&mut app.actions) {
            app.apply(action, &ctx);
        }
        assert!(app.now_playing_view, "the view slides up");

        draw(&mut app, pointer_click(empty, egui::PointerButton::Primary));
        assert_eq!(toggles(&app), 1);
        for action in std::mem::take(&mut app.actions) {
            app.apply(action, &ctx);
        }
        assert!(!app.now_playing_view, "a second click closes it");

        // The play button, in the middle of the bar, is still the play button.
        let play = egui::pos2(640.0, 800.0 - crate::theme::PLAYER_BAR_HEIGHT / 2.0 - 10.0);
        draw(&mut app, pointer_click(play, egui::PointerButton::Primary));
        assert_eq!(toggles(&app), 0, "{:?}", app.actions);
        app.backend.shutdown();
    }

    /// The player bar names the playing source under the artists, as
    /// TIDAL's does, and a click on it opens that source. With no source
    /// the line is not drawn.
    #[cfg(feature = "demo")]
    #[test]
    fn the_player_bar_names_its_source_and_a_click_opens_it() {
        let (ctx, mut app) = accessible_app("player-bar-source");
        let draw = |app: &mut App, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::player_bar::show(app, ui),
            );
            output.textures_delta.clear();
            let mut placed = Vec::new();
            fn walk(shape: &egui::epaint::Shape, placed: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::epaint::Shape::Text(text) => {
                        placed.push((text.galley.job.text.clone(), text.visual_bounding_rect()))
                    }
                    egui::epaint::Shape::Vec(shapes) => {
                        shapes.iter().for_each(|shape| walk(shape, placed))
                    }
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut placed);
            }
            placed
        };
        // The demo plays the second playlist.
        draw(&mut app, vec![]);
        let placed = draw(&mut app, vec![]);
        let (_, name) = placed
            .iter()
            .find(|(text, _)| text == "Late night focus")
            .expect("the source line is drawn");
        let name = *name;
        let above = placed
            .iter()
            .filter(|(_, rect)| rect.center().x < 500.0 && rect.bottom() <= name.top() + 1.0)
            .count();
        assert!(above >= 2, "the line sits under the title and artists");
        draw(
            &mut app,
            pointer_click(name.center(), egui::PointerButton::Primary),
        );
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::Open(Page::Playlist(_)))),
            "{:?}",
            app.actions
        );
        assert!(
            !app.actions
                .iter()
                .any(|action| matches!(action, Action::ToggleNowPlaying)),
            "the link keeps its own click"
        );

        // Nothing reported, nothing named.
        app.actions.clear();
        if let Some(remote) = app.remote.as_mut() {
            remote.state.context = None;
        }
        draw(&mut app, vec![]);
        let placed = draw(&mut app, vec![]);
        assert!(!placed.iter().any(|(text, _)| text == "Late night focus"));
        app.backend.shutdown();
    }

    #[test]
    fn loading_collections_keep_their_hero_layout() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        fn album(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        fn artist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::artist::show(app, ui, "art0");
        }

        let (ctx, mut app) = accessible_app("loading-collections");
        app.playlist_pages.remove("pl1");
        app.album_pages.remove("alb0");
        app.artist_pages.remove("art0");
        app.library.playlists = Loadable::Loaded(Vec::new());
        app.library.albums.items.clear();
        app.library.artists.items.clear();
        app.library.mixes.items.clear();
        Arc::make_mut(app.search.results.get_mut().unwrap()).albums = None;
        for (title, detail, view) in [
            (
                "Late night focus",
                "Carmine",
                playlist as fn(&mut App, &mut egui::Ui),
            ),
            ("Fragments", "Bonobo", album),
            ("Bonobo", " fans", artist),
        ] {
            let painted = view_frame(&ctx, &mut app, vec![], view);
            let title = painted.iter().find(|(text, _)| text == title).unwrap().1;
            assert!(painted.iter().any(|(text, _)| text.contains(detail)));
            // The songs on their way are a skeleton under the hero, not text.
            assert!(title.bottom() > 0.0);
            assert!(!painted.iter().any(|(text, _)| text == "Loading…"));
        }
        app.backend.shutdown();
    }

    #[test]
    fn the_artist_header_shows_the_follower_count_when_it_arrives() {
        use crate::i18n::Locale;
        fn artist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::artist::show(app, ui, "art0");
        }
        // TIDAL desktop: "19.1K fans" under an artist's name.
        assert_eq!(
            crate::ui::artist::followers_label(Locale::English, 1_951_000),
            "1.9M fans"
        );
        assert!(crate::ui::artist::followers_label(Locale::German, 19_118).contains("19,1K"));
        assert_eq!(
            crate::ui::artist::followers_label(Locale::English, 1),
            "1 fan"
        );
        let (ctx, mut app) = accessible_app("artist-followers");
        let page = app.artist_pages.get_mut("art0").unwrap();
        if let Loadable::Loaded(artist) = &mut page.artist {
            artist.followers = None;
        }
        page.followers = Loadable::Loaded(None);
        let painted = view_frame(&ctx, &mut app, vec![], artist);
        assert!(!painted.iter().any(|(text, _)| text.ends_with(" fans")));
        app.artist_pages.get_mut("art0").unwrap().followers = Loadable::Loaded(Some(1_951_000));
        let painted = view_frame(&ctx, &mut app, vec![], artist);
        assert!(painted.iter().any(|(text, _)| text == "1.9M fans"));
        app.backend.shutdown();
    }

    #[test]
    fn failed_collections_keep_known_metadata_above_retry() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        fn album(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        fn artist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::artist::show(app, ui, "art0");
        }

        let (ctx, mut app) = accessible_app("failed-collections");
        app.playlist_pages.get_mut("pl1").unwrap().playlist =
            Loadable::Failed("Connection interrupted".into());
        app.album_pages.get_mut("alb0").unwrap().album =
            Loadable::Failed("Connection interrupted".into());
        app.artist_pages.get_mut("art0").unwrap().artist =
            Loadable::Failed("Connection interrupted".into());

        for (title, detail, view) in [
            (
                "Late night focus",
                "Carmine",
                playlist as fn(&mut App, &mut egui::Ui),
            ),
            ("Fragments", "Bonobo", album),
            ("Bonobo", " fans", artist),
        ] {
            let painted = view_frame(&ctx, &mut app, vec![], view);
            let title = painted.iter().find(|(text, _)| text == title).unwrap().1;
            assert!(painted.iter().any(|(text, _)| text.contains(detail)));
            assert!(
                painted
                    .iter()
                    .any(|(text, _)| text == "Connection interrupted")
            );
            let retry = painted.iter().find(|(text, _)| text == "Retry").unwrap().1;
            assert!(retry.top() > title.bottom());
        }
        app.backend.shutdown();
    }

    fn pointer_click(pos: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    /// Painted labels of a freshly drawn menu, for asserting which entries
    /// a context offers.
    fn menu_text(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::epaint::Shape, text: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(shape) => text.push((
                    shape.galley.job.text.clone(),
                    shape.galley.rect.translate(shape.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|shape| walk(shape, text));
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for shape in &output.shapes {
            walk(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn choosing_a_language_redraws_the_interface_at_once_and_is_saved() {
        use crate::i18n::{Locale, gettext};
        use crate::settings::LanguageChoice;
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("language-picker");
        app.open(Page::Settings);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-filter"), "Language".to_string())
        });
        assert_eq!(app.settings.language, LanguageChoice::System);
        for _ in 0..3 {
            view_frame(&ctx, &mut app, vec![], App::frame_ui);
        }
        let painted = view_frame(&ctx, &mut app, vec![], App::frame_ui);
        let picker = sidebar_text(&painted, "System").center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(picker, egui::PointerButton::Primary),
            App::frame_ui,
        );
        let painted = view_frame(&ctx, &mut app, vec![], App::frame_ui);
        let menu_y = |name: &str| {
            painted
                .iter()
                .filter(|(text, rect)| text == name && rect.center().y > picker.y)
                .map(|(_, rect)| rect.center().y)
                .next()
        };
        // System first, then each language under its own name. The menu
        // scrolls, so only the entries above its fold are painted.
        let mut previous = menu_y("System").expect("System heads the menu");
        let shown = crate::i18n::LOCALES
            .iter()
            .map_while(|locale| menu_y(locale.native_name()))
            .inspect(|&y| {
                assert!(previous < y, "languages are listed in order");
                previous = y;
            })
            .count();
        assert!(shown >= 8, "only {shown} languages fit before scrolling");
        view_frame(
            &ctx,
            &mut app,
            pointer_click(
                sidebar_text(&painted, "Español").center(),
                egui::PointerButton::Primary,
            ),
            App::frame_ui,
        );
        assert_eq!(
            app.settings.language,
            LanguageChoice::Locale(Locale::Spanish)
        );
        assert_eq!(app.locale, Locale::Spanish);
        // The English search text no longer matches the Spanish row.
        ctx.data_mut(|data| data.remove::<String>(egui::Id::new("settings-filter")));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, &gettext(Locale::Spanish, "Music"), Role::Button);
        let id = accessible_node(&tree, &gettext(Locale::Spanish, "Language"), Role::ComboBox);
        let node = &tree
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .unwrap()
            .1;
        assert_eq!(node.value(), Some("Español"));

        app.apply(Action::SetLanguage(LanguageChoice::System), &ctx);
        assert_eq!(app.settings.language, LanguageChoice::System);
        assert_eq!(app.locale, Locale::English, "tests read an English system");
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Music", Role::Button);
        app.backend.shutdown();
    }

    #[test]
    fn theme_picker_offers_dark_light_and_follow_system_and_applies_the_click() {
        let (ctx, mut app) = accessible_app("theme-picker");
        app.open(Page::Settings);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-filter"), "Appearance".to_string())
        });
        for _ in 0..3 {
            view_frame(&ctx, &mut app, vec![], App::frame_ui);
        }
        let painted = view_frame(&ctx, &mut app, vec![], App::frame_ui);
        // New profiles start on TIDAL Dark.
        let picker = sidebar_text(&painted, "Dark").center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(picker, egui::PointerButton::Primary),
            App::frame_ui,
        );
        let painted = view_frame(&ctx, &mut app, vec![], App::frame_ui);
        let menu_y = |name: &str| {
            painted
                .iter()
                .filter(|(text, rect)| text == name && rect.center().y > picker.y + 1.0)
                .map(|(_, rect)| rect.center().y)
                .next()
                .expect("theme menu entry")
        };
        assert!(menu_y("Light") < menu_y("Follow system"));
        let light = painted
            .iter()
            .find(|(text, rect)| text == "Light" && rect.center().y > picker.y + 1.0)
            .map(|(_, rect)| rect.center())
            .unwrap();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(light, egui::PointerButton::Primary),
            App::frame_ui,
        );
        assert_eq!(app.settings.theme, crate::settings::ThemeChoice::Light);
        assert_eq!(app.palette, crate::theme::Palette::light());
        assert_eq!(ctx.theme(), egui::Theme::Light);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let id = accessible_node(&tree, "Theme", egui::accesskit::Role::ComboBox);
        let node = &tree
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .unwrap()
            .1;
        assert_eq!(node.value(), Some("Light"), "read out as shown");
        app.backend.shutdown();
    }

    /// The painted rect of a sidebar label, for pointer tests against the
    /// sidebar's private rows.
    fn sidebar_text(painted: &[(String, egui::Rect)], label: &str) -> egui::Rect {
        painted
            .iter()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("missing sidebar text {label:?}"))
    }

    fn double_click(pos: egui::Pos2) -> [Vec<egui::Event>; 2] {
        [
            pointer_click(pos, egui::PointerButton::Primary),
            pointer_click(pos, egui::PointerButton::Primary),
        ]
    }

    fn played_contexts(app: &App) -> Vec<String> {
        app.actions
            .iter()
            .filter_map(|action| match action {
                Action::PlayContext { uri, .. } => Some(uri.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn search_top_result_opens_the_menu_for_its_item() {
        for kind in ["track", "artist", "album", "playlist"] {
            let (ctx, mut app) = accessible_app(&format!("top-result-{kind}"));
            let (results, title, uri) = match kind {
                "track" => {
                    let item = track(0);
                    (
                        SearchResults {
                            tracks: Some(page(vec![item.clone()])),
                            ..Default::default()
                        },
                        item.name,
                        item.uri,
                    )
                }
                "artist" => {
                    let item = artist(0);
                    (
                        SearchResults {
                            artists: Some(page(vec![item.clone()])),
                            ..Default::default()
                        },
                        item.name,
                        item.uri,
                    )
                }
                "album" => {
                    let item = album(0);
                    (
                        SearchResults {
                            albums: Some(page(vec![item.clone()])),
                            ..Default::default()
                        },
                        item.name,
                        item.uri,
                    )
                }
                _ => {
                    let item = playlist(0);
                    (
                        SearchResults {
                            playlists: Some(page(vec![item.clone()])),
                            ..Default::default()
                        },
                        item.name,
                        item.uri,
                    )
                }
            };
            app.search.results = Loadable::Loaded(Arc::new(results));
            app.saved.clear();
            search_frame(&ctx, &mut app, vec![]);
            let text = search_frame(&ctx, &mut app, vec![]);
            let pos = text
                .iter()
                .find(|(text, _)| text == &title)
                .expect("top result title")
                .1
                .center();
            app.actions.clear();
            search_frame(
                &ctx,
                &mut app,
                pointer_click(pos, egui::PointerButton::Secondary),
            );
            let text = search_frame(&ctx, &mut app, vec![]);
            assert!(
                app.actions.is_empty(),
                "right-clicking {kind} must not navigate or play"
            );
            let expected = match kind {
                "track" => &[
                    "Play next",
                    "Add to My Collection",
                    "Add to playlist",
                    "Go to track radio",
                    "Go to artist",
                    "Go to album",
                    "Share",
                ][..],
                "artist" => &["Follow", "Share"][..],
                "album" => &["Play now", "Play next", "Add to My Collection", "Share"][..],
                _ => &["Play now", "Shuffle", "Share"][..],
            };
            for label in expected {
                assert!(
                    text.iter().any(|(text, _)| text == label),
                    "{kind} menu is missing {label}"
                );
            }
            // Copy link sits in TIDAL's Share submenu.
            let share = text
                .iter()
                .find(|(text, _)| text == "Share")
                .unwrap_or_else(|| {
                    panic!("right-clicking the {kind} top result must open its menu")
                })
                .1
                .center();
            search_frame(&ctx, &mut app, vec![egui::Event::PointerMoved(share)]);
            search_frame(&ctx, &mut app, vec![]);
            let text = search_frame(&ctx, &mut app, vec![]);
            let copy = text
                .iter()
                .find(|(text, _)| text == "Copy link")
                .unwrap_or_else(|| panic!("the {kind} Share submenu has Copy link"));
            search_frame(
                &ctx,
                &mut app,
                pointer_click(copy.1.center(), egui::PointerButton::Primary),
            );
            assert!(
                matches!(app.actions.as_slice(), [crate::model::Action::CopyLink(link)] if link == &uri)
            );
            app.backend.shutdown();
        }
    }

    #[test]
    fn song_top_result_artist_name_opens_the_available_profile_or_the_album() {
        for artist_id in [Some("ween"), None] {
            let (ctx, mut app) = accessible_app("top-result-song-artist");
            let mut item = track(0);
            let album_id = item.album.as_ref().unwrap().id.clone();
            item.artists = vec![ArtistRef {
                id: artist_id.map(str::to_string),
                name: "Ween".into(),
                uri: artist_id.map(|id| format!("tidal:artist:{id}")),
                picture: None,
            }];
            app.search.results = Loadable::Loaded(Arc::new(SearchResults {
                tracks: Some(page(vec![item])),
                ..Default::default()
            }));
            search_frame(&ctx, &mut app, vec![]);
            let text = search_frame(&ctx, &mut app, vec![]);
            // TIDAL's top results list the song with its artists after
            // "Track ·".
            let artist = text
                .iter()
                .find(|(text, _)| text == "Ween")
                .expect("top result artist name");
            app.actions.clear();
            search_frame(
                &ctx,
                &mut app,
                pointer_click(
                    egui::pos2(artist.1.right() - 4.0, artist.1.center().y),
                    egui::PointerButton::Primary,
                ),
            );
            if artist_id.is_some() {
                assert!(
                    matches!(app.actions.as_slice(), [Action::Open(Page::Artist(id))] if id == "ween")
                );
            } else {
                assert!(
                    matches!(app.actions.as_slice(), [Action::Open(Page::Album(id))] if id == &album_id)
                );
            }
            app.backend.shutdown();
        }
    }

    fn check_card_menu(
        app: &mut App,
        ctx: &egui::Context,
        view: fn(&mut App, &mut egui::Ui),
        section: &str,
        title: &str,
        uri: &str,
        labels: &[&str],
    ) {
        view_frame(ctx, app, vec![], view);
        let text = view_frame(ctx, app, vec![], view);
        let below = text
            .iter()
            .find(|(text, _)| text == section)
            .unwrap()
            .1
            .bottom();
        let pos = text
            .iter()
            .find(|(text, rect)| text == title && rect.top() >= below)
            .unwrap_or_else(|| panic!("{title} in {section}"))
            .1
            .center();
        let earlier_card = text
            .iter()
            .find(|(_, rect)| (rect.center().y - pos.y).abs() < 1.0 && rect.center().x < pos.x)
            .map(|(_, rect)| rect.center());
        app.actions.clear();
        view_frame(
            ctx,
            app,
            pointer_click(pos, egui::PointerButton::Secondary),
            view,
        );
        // A hovered Play button must not change which item owns the open menu.
        if let Some(pos) = earlier_card {
            view_frame(ctx, app, vec![egui::Event::PointerMoved(pos)], view);
        }
        let text = view_frame(ctx, app, vec![], view);
        assert!(
            app.actions
                .iter()
                .all(|action| matches!(action, Action::PrefetchPage(_))),
            "right-click must not navigate or play"
        );
        for label in labels {
            assert!(
                text.iter().any(|(text, _)| text == label),
                "{section}: missing {label}"
            );
        }
        // Copy link sits in TIDAL's Share submenu.
        let share = text
            .iter()
            .rfind(|(text, _)| text == "Share")
            .unwrap_or_else(|| panic!("{section}: no menu for {title}"))
            .1
            .center();
        view_frame(ctx, app, vec![egui::Event::PointerMoved(share)], view);
        view_frame(ctx, app, vec![], view);
        let text = view_frame(ctx, app, vec![], view);
        let copy = text
            .iter()
            .find(|(text, _)| text == "Copy link")
            .unwrap_or_else(|| panic!("{section}: no Share submenu for {title}"));
        view_frame(
            ctx,
            app,
            pointer_click(copy.1.center(), egui::PointerButton::Primary),
            view,
        );
        app.actions
            .retain(|action| !matches!(action, Action::PrefetchPage(_)));
        assert!(matches!(app.actions.as_slice(), [Action::CopyLink(link)] if link == uri));
    }

    #[test]
    fn library_and_discography_cards_open_their_own_menus() {
        for (page, section, title, uri, labels) in [
            (
                Page::Albums,
                "Albums",
                album(1).name,
                album(1).uri,
                vec!["Play now", "Play next"],
            ),
            (
                Page::Artists,
                "Profiles",
                artist(1).name,
                artist(1).uri,
                vec!["Follow"],
            ),
            (
                Page::Artist("art0".into()),
                "Albums",
                album(1).name,
                album(1).uri,
                vec!["Play now", "Play next"],
            ),
            (
                Page::Artist("art0".into()),
                "Fans also like",
                artist(2).name,
                artist(2).uri,
                vec!["Follow"],
            ),
        ] {
            let (ctx, mut app) = accessible_app(&format!("library-card-{section}"));
            app.open(page);
            fn view(app: &mut App, ui: &mut egui::Ui) {
                if let Page::Artist(id) = app.page().clone() {
                    crate::ui::artist::show(app, ui, &id);
                } else {
                    crate::ui::library::show(app, ui, app.page().clone());
                }
            }
            check_card_menu(&mut app, &ctx, view, section, &title, &uri, &labels);
            app.backend.shutdown();
        }
    }

    #[test]
    fn search_shelves_and_filtered_grids_open_item_menus() {
        for (filter, title, uri, labels) in [
            (
                SearchFilter::Artists,
                artist(1).name,
                artist(1).uri,
                vec!["Follow"],
            ),
            (
                SearchFilter::Albums,
                album(0).name,
                album(0).uri,
                vec!["Play next", "Add to My Collection"],
            ),
            (
                SearchFilter::Playlists,
                playlist(1).name,
                playlist(1).uri,
                vec!["Edit playlist", "Delete playlist"],
            ),
        ] {
            // In the top results an artist's name is also the link on
            // their albums' rows above their own row; the Artists list
            // checks the profile's menu.
            let views: &[SearchFilter] = if filter == SearchFilter::Artists {
                &[SearchFilter::Artists]
            } else {
                &[SearchFilter::All, filter]
            };
            for &selected in views {
                let (ctx, mut app) =
                    accessible_app(&format!("search-card-{selected:?}-{filter:?}"));
                app.saved.clear();
                app.search.filter = selected;
                // Keep matching artist subtitles in song rows out of the card lookup.
                // The lists interleaved, not the ranked hits.
                if let Loadable::Loaded(results) = &mut app.search.results {
                    let results = Arc::make_mut(results);
                    results.tracks = None;
                    results.top_hits.clear();
                    results.genres.clear();
                }
                let section = if selected == SearchFilter::All {
                    filter.label(crate::i18n::Locale::English)
                } else {
                    SearchFilter::All.label(crate::i18n::Locale::English)
                };
                check_card_menu(
                    &mut app,
                    &ctx,
                    crate::ui::search::show,
                    &section,
                    &title,
                    &uri,
                    &labels,
                );
                app.backend.shutdown();
            }
        }
    }

    #[test]
    fn home_json_hides_only_the_chosen_recommendation_shelves() {
        let (ctx, mut app) = accessible_app("home-json-visibility");
        let view = crate::ui::home::show;
        for (made_for_you, recommendations) in
            [(true, true), (false, true), (true, false), (false, false)]
        {
            app.settings.home = serde_json::from_str(&format!(
                r#"{{"made_for_you":{{"visible":{made_for_you}}},"recommendations":{{"visible":{recommendations}}}}}"#
            )).unwrap();
            view_frame(&ctx, &mut app, vec![], view);
            let text = view_frame(&ctx, &mut app, vec![], view);
            // With TIDAL's shelves shown, the later sections are below
            // the 800-point window; check them while the shelves are hidden.
            let below = [
                ("Recommended for you", recommendations),
                ("Recently played", true),
                ("Your top artists", true),
                ("Your top tracks", true),
            ];
            let checks = std::iter::once(("Your favorite artists", made_for_you))
                .chain(below.into_iter().filter(|_| !made_for_you));
            for (label, expected) in checks {
                assert_eq!(
                    text.iter().any(|(text, _)| text == label),
                    expected,
                    "{label}, made_for_you={made_for_you}, recommendations={recommendations}"
                );
            }
        }
        app.backend.shutdown();
    }

    #[test]
    fn home_cards_open_item_menus() {
        for (section, title, uri, labels) in [
            (
                "For you".to_string(),
                playlist(1).name,
                playlist(1).uri,
                vec!["Edit playlist", "Delete playlist"],
            ),
            (
                "For you".to_string(),
                playlist(0).name,
                playlist(0).uri,
                vec!["Unfollow"],
            ),
            (
                "Recently played".into(),
                track(5).name,
                track(5).uri,
                vec!["Play next", "Add to playlist", "Go to track radio"],
            ),
            (
                "Your top artists".into(),
                artist(1).name,
                artist(1).uri,
                vec!["Follow"],
            ),
        ] {
            let (ctx, mut app) = accessible_app(&format!("home-card-{section}-{title}"));
            // TIDAL's shelves come first; hide them so the later sections
            // are on screen.
            app.settings.home.made_for_you.visible = section == "For you";
            check_card_menu(
                &mut app,
                &ctx,
                crate::ui::home::show,
                &section,
                &title,
                &uri,
                &labels,
            );
            app.backend.shutdown();
        }
    }

    /// The hover Play button sits over the cover (dead centre on a round
    /// artist, bottom-left on the others); a right-click that lands on it
    /// must open the card's menu like one anywhere else on the card.
    #[test]
    fn right_click_on_a_home_card_cover_opens_its_menu_even_over_the_play_button() {
        for (section, title, round) in [
            ("Your favorite artists", artist(0).name, true),
            ("User playlists you\u{2019}ll love", playlist(0).name, false),
            ("My mixes", mix(0).name, false),
            ("Albums you\u{2019}ll enjoy", album(0).name, false),
        ] {
            let (ctx, mut app) = accessible_app(&format!("home-cover-menu-{section}"));
            // The same items sit on a later shelf too, as TIDAL's Home repeats
            // a favourite artist, and the shelves' uis share one id.
            if let Loadable::Loaded(shelves) = &mut app.home.shelves {
                Arc::make_mut(shelves).push(Shelf {
                    title: "More like these".into(),
                    items: vec![
                        ShelfItem::Artist(artist(0)),
                        ShelfItem::Playlist(playlist(0)),
                        ShelfItem::Mix(mix(0)),
                        ShelfItem::Album(album(0)),
                    ],
                    ..Default::default()
                });
            }
            let view = crate::ui::home::show;
            view_frame(&ctx, &mut app, vec![], view);
            let text = view_frame(&ctx, &mut app, vec![], view);
            let below = text
                .iter()
                .find(|(text, _)| text == section)
                .unwrap_or_else(|| panic!("{section}"))
                .1
                .bottom();
            let title_rect = text
                .iter()
                .find(|(text, rect)| text == &title && rect.top() >= below)
                .unwrap_or_else(|| panic!("{title} in {section}"))
                .1;
            let cover_bottom = title_rect.top() - 11.0;
            let left = if round {
                title_rect.center().x - 86.0
            } else {
                title_rect.left()
            };
            let spots = [
                (left + 86.0, cover_bottom - 86.0),
                (left + 26.0, cover_bottom - 26.0),
                (left + 60.0, cover_bottom - 120.0),
                (left + 150.0, cover_bottom - 40.0),
            ];
            for (x, y) in spots {
                let pos = egui::pos2(x, y);
                // Hover first, as a user does, so the Play button is up.
                view_frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)], view);
                view_frame(&ctx, &mut app, vec![], view);
                app.actions.clear();
                // The press and the release come in frames of their own, as
                // a window's events do.
                for pressed in [true, false] {
                    view_frame(
                        &ctx,
                        &mut app,
                        vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Secondary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        view,
                    );
                }
                let open = view_frame(&ctx, &mut app, vec![], view);
                assert!(
                    open.iter().any(|(text, _)| text == "Share"),
                    "{section}: right-click at {pos:?} opened no menu"
                );
                assert!(
                    app.actions
                        .iter()
                        .all(|action| matches!(action, Action::PrefetchPage(_))),
                    "{section}: right-click at {pos:?} must not navigate or play"
                );
                // Close it before the next spot.
                view_frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    view,
                );
                view_frame(&ctx, &mut app, vec![], view);
            }
            app.backend.shutdown();
        }
    }

    /// Collection > Save for Later: TIDAL's tabs, the empty text of each tab,
    /// and the rows of what is saved.
    #[test]
    fn save_for_later_page_shows_the_tabs_the_empty_texts_and_the_saved_rows() {
        use crate::save_later::{SaveTab, Saved};
        fn view(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::save_later::show(app, ui);
        }
        let has = |text: &[(String, egui::Rect)], wanted: &str| {
            text.iter().any(|(text, _)| text.starts_with(wanted))
        };
        let (ctx, mut app) = accessible_app("save-later-page");
        app.save_later.absorb(true, vec![], None);
        let text = view_frame(&ctx, &mut app, vec![], view);
        for label in [
            "Save for Later",
            "All",
            "Albums",
            "Tracks",
            "Artists",
            "Playlists",
            "Videos",
        ] {
            assert!(text.iter().any(|(text, _)| text == label), "no tab {label}");
        }
        assert!(has(&text, "You haven't saved anything for later"));

        let saved = album(0);
        app.save_later
            .details
            .insert(saved.uri.clone(), Saved::Album(saved.clone()));
        app.save_later.set_local(&saved.uri, true);
        let text = view_frame(&ctx, &mut app, vec![], view);
        assert!(has(&text, &saved.name), "the saved album has its card");
        assert!(!has(&text, "You haven't saved anything for later"));

        // An album does not fill the Tracks tab, which says so itself.
        app.save_later.tab = SaveTab::Tracks;
        let text = view_frame(&ctx, &mut app, vec![], view);
        assert!(!has(&text, &saved.name));
        assert!(has(&text, "You haven't saved any tracks for later"));
    }

    /// The right-click menu offers Save for Later, which reads "Remove from
    /// Save for Later" once the item is saved.
    #[test]
    fn menus_offer_save_for_later_and_flip_to_remove_when_saved() {
        fn menu(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::menus::album_menu(ui, app, &album(0));
        }
        let (ctx, mut app) = accessible_app("save-later-menu");
        // Before the list is known the menu still offers it, and drawing it
        // never asks TIDAL (the list is read at sign-in and on the page).
        let text = view_frame(&ctx, &mut app, vec![], menu);
        assert!(text.iter().any(|(text, _)| text == "Save for Later"));
        assert!(!app.save_later.loading, "a menu must not start a read");
        app.save_later.absorb(true, vec![], None);
        let text = view_frame(&ctx, &mut app, vec![], menu);
        assert!(text.iter().any(|(text, _)| text == "Save for Later"));
        assert!(
            !text
                .iter()
                .any(|(text, _)| text == "Remove from Save for Later")
        );
        app.save_later.set_local(&album(0).uri, true);
        let text = view_frame(&ctx, &mut app, vec![], menu);
        assert!(
            text.iter()
                .any(|(text, _)| text == "Remove from Save for Later")
        );
        assert!(!text.iter().any(|(text, _)| text == "Save for Later"));
    }

    /// Collection > Purchases with nothing bought: TIDAL's empty text and the
    /// button to Home's Uploads tab.
    #[test]
    fn purchases_page_with_nothing_bought_offers_the_uploads_tab() {
        fn view(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::purchases::show(app, ui);
        }
        let (ctx, mut app) = accessible_app("purchases-page");
        app.purchases = Loadable::Loaded(Default::default());
        let text = view_frame(&ctx, &mut app, vec![], view);
        for label in ["Purchases", "Albums", "Tracks", "View Uploads"] {
            assert!(text.iter().any(|(text, _)| text == label), "no {label}");
        }
        assert!(text.iter().any(|(text, _)| text
            == "You haven't purchased any albums yet. Discover music uploaded directly by artists."));
        let button = text
            .iter()
            .find(|(text, _)| text == "View Uploads")
            .unwrap()
            .1
            .center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(button, egui::PointerButton::Primary),
            view,
        );
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::OpenUploads))
        );
    }

    /// TIDAL's menus for the Home cards: an artist's reads Bio, Go to artist
    /// radio, Block, Credits, Follow, Save for Later, Share; a playlist of someone else's has
    /// the cover, the title and "19 Items (46:59)", then Play now, Shuffle,
    /// Play next, Follow, Save for Later, Share. (Report has no working
    /// implementation, so it is not offered, and a playlist cannot be
    /// blocked.)
    #[test]
    fn home_artist_and_playlist_card_menus_follow_tidals_items_and_order() {
        for (section, title, round, labels) in [
            (
                "Your favorite artists",
                artist(0).name,
                true,
                &[
                    "Bio",
                    "Go to artist radio",
                    "Block",
                    "Credits",
                    "Follow",
                    "Save for Later",
                    "Share",
                ][..],
            ),
            (
                "User playlists you\u{2019}ll love",
                playlist(0).name,
                false,
                &[
                    "Play now",
                    "Shuffle",
                    "Play next",
                    "Follow",
                    "Save for Later",
                    "Share",
                ][..],
            ),
        ] {
            let (ctx, mut app) = accessible_app(&format!("home-card-order-{section}"));
            if let Loadable::Loaded(shelves) = &mut app.home.shelves {
                for shelf in Arc::make_mut(shelves) {
                    for item in &mut shelf.items {
                        if let ShelfItem::Playlist(playlist) = item {
                            playlist.duration_secs = Some(46 * 60 + 59);
                        }
                    }
                }
            }
            app.saved.clear();
            let view = crate::ui::home::show;
            view_frame(&ctx, &mut app, vec![], view);
            let text = view_frame(&ctx, &mut app, vec![], view);
            let below = text
                .iter()
                .find(|(text, _)| text == section)
                .unwrap_or_else(|| panic!("{section}"))
                .1
                .bottom();
            let title_rect = text
                .iter()
                .find(|(text, rect)| text == &title && rect.top() >= below)
                .unwrap_or_else(|| panic!("{title} in {section}"))
                .1;
            let pos = title_rect.center();
            view_frame(
                &ctx,
                &mut app,
                pointer_click(pos, egui::PointerButton::Secondary),
                view,
            );
            let open = view_frame(&ctx, &mut app, vec![], view);
            let mut last = f32::MIN;
            for label in labels {
                let top = open
                    .iter()
                    .filter(|(text, rect)| text == label && rect.left() > pos.x - 40.0)
                    .map(|(_, rect)| rect.top())
                    .fold(f32::MAX, f32::min);
                assert!(top < f32::MAX, "{section}: no {label}");
                assert!(top > last, "{section}: {label} is out of order");
                last = top;
            }
            // Whether it is followed was never asked for a shelf card: opening
            // the menu asks, so Follow and Unfollow tell the truth.
            let uri = if round {
                artist(0).uri
            } else {
                playlist(0).uri
            };
            assert!(
                app.saved_pending.contains(&uri),
                "{section}: opening the menu must ask whether {uri} is followed"
            );
            let absent: &[&str] = if round {
                &["Report"]
            } else {
                &["Block", "Report"]
            };
            for absent in absent {
                assert!(
                    !open.iter().any(|(text, _)| text == absent),
                    "{section}: {absent} has no implementation and must not be offered"
                );
            }
            if round {
                // The menu asks for TIDAL's block list as it opens.
                assert_eq!(app.backend.take_blocked_artist_list_requests(), 1);
            }
            if round {
                // The Bio opens the dialog without opening the page.
                let bio = open
                    .iter()
                    .find(|(text, _)| text == "Bio")
                    .unwrap()
                    .1
                    .center();
                app.actions.clear();
                view_frame(
                    &ctx,
                    &mut app,
                    pointer_click(bio, egui::PointerButton::Primary),
                    view,
                );
                assert!(
                    matches!(app.actions.as_slice(), [Action::ShowArtistBio(id)] if id == &artist(0).id),
                    "{:?}",
                    app.actions
                );
            } else {
                let items = format!("{} Items (46:59)", playlist(0).track_total());
                assert!(
                    open.iter().any(|(text, _)| text == &items),
                    "{section}: the heading reads {items}"
                );
            }
            app.backend.shutdown();
        }
    }

    /// The artist page's More menu says Block for an artist TIDAL still plays
    /// and Unblock for one on the block list; clicking asks for the other.
    #[test]
    fn the_artist_page_menu_offers_block_or_unblock_by_state() {
        fn page(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::artist::show(app, ui, "art0");
        }
        let uri = artist(0).uri;
        assert_eq!(uri, "tidal:artist:art0");
        for blocked in [false, true] {
            let (ctx, mut app) = accessible_app(&format!("artist-block-menu-{blocked}"));
            app.handle_api(crate::backend::ApiResponse::BlockedArtists {
                result: Ok(if blocked { vec!["art0".into()] } else { vec![] }),
            });
            let painted = view_frame(&ctx, &mut app, vec![], page);
            let more = painted
                .iter()
                .find(|(text, _)| text == "More")
                .expect("the More button")
                .1
                .center();
            view_frame(
                &ctx,
                &mut app,
                pointer_click(more, egui::PointerButton::Primary),
                page,
            );
            let open = view_frame(&ctx, &mut app, vec![], page);
            let (shown, hidden) = if blocked {
                ("Unblock", "Block")
            } else {
                ("Block", "Unblock")
            };
            let item = open
                .iter()
                .find(|(text, _)| text == shown)
                .unwrap_or_else(|| panic!("the menu offers {shown}"))
                .1
                .center();
            assert!(!open.iter().any(|(text, _)| text == hidden), "not {hidden}");
            app.actions.clear();
            view_frame(
                &ctx,
                &mut app,
                pointer_click(item, egui::PointerButton::Primary),
                page,
            );
            assert!(
                matches!(
                    app.actions.as_slice(),
                    [Action::SetArtistBlocked { uri: u, blocked: b }] if u == &uri && *b == !blocked
                ),
                "{:?}",
                app.actions
            );
            app.backend.shutdown();
        }
    }

    /// TIDAL's Settings has "Blocked: View and edit your blocked content."
    /// under Playback; View opens the list, which names what is blocked and
    /// offers Unblock for each.
    #[test]
    fn settings_has_a_blocked_row_that_opens_the_list_of_blocked_content() {
        fn dialog(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::blocked::dialog(app, ui);
        }
        let (ctx, mut app) = accessible_app("settings-blocked");
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-filter"), "blocked".to_owned());
        });
        view_frame(&ctx, &mut app, vec![], crate::ui::settings::show);
        let painted = view_frame(&ctx, &mut app, vec![], crate::ui::settings::show);
        assert!(painted.iter().any(|(text, _)| text == "Blocked"));
        assert!(
            painted
                .iter()
                .any(|(text, _)| text == "View and edit your blocked content.")
        );
        let view = painted
            .iter()
            .find(|(text, _)| text == "View")
            .expect("the View button")
            .1
            .center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(view, egui::PointerButton::Primary),
            crate::ui::settings::show,
        );
        assert!(
            app.actions.iter().any(|a| matches!(a, Action::ShowBlocked)),
            "{:?}",
            app.actions
        );
        app.blocked_view = Loadable::Loaded(crate::artist_blocks::BlockedContent {
            artists: vec![crate::artist_blocks::BlockedArtist {
                id: "7".into(),
                name: "Band".into(),
            }],
            tracks: vec![crate::artist_blocks::BlockedTrack {
                id: "5".into(),
                title: "Song".into(),
                artist: "Someone".into(),
                hidden: false,
            }],
        });
        let painted = view_frame(&ctx, &mut app, vec![], dialog);
        for text in ["Band", "Song", "Someone", "Artists", "Songs"] {
            assert!(painted.iter().any(|(t, _)| t == text), "{text}");
        }
        let unblocks: Vec<_> = painted
            .iter()
            .filter(|(text, _)| text == "Unblock")
            .map(|(_, rect)| rect.center())
            .collect();
        assert_eq!(unblocks.len(), 2, "one Unblock per row");
        // Each Unblock sits level with its row's title (an artist), or with
        // the middle of the title and the artist under it (a song).
        let rect_of = |label: &str| painted.iter().find(|(t, _)| t == label).unwrap().1;
        let artist_title = rect_of("Band").center().y;
        assert!(
            (artist_title - unblocks[0].y).abs() < 1.0,
            "the artist's name is at {artist_title}, its Unblock at {}",
            unblocks[0].y
        );
        let song_block = (rect_of("Song").top() + rect_of("Someone").bottom()) / 2.0;
        assert!(
            (song_block - unblocks[1].y).abs() < 1.5,
            "the song's name and artist are centred on {song_block}, its Unblock on {}",
            unblocks[1].y
        );
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(unblocks[1], egui::PointerButton::Primary),
            dialog,
        );
        assert!(
            matches!(app.actions.as_slice(), [Action::UnblockTrack(uri)] if uri == "tidal:track:5"),
            "{:?}",
            app.actions
        );
        app.backend.shutdown();
    }

    /// The Blocked dialog is first drawn around a spinner while TIDAL
    /// answers; once the lists are in it grows to hold them, not the one
    /// row the spinner's card left room for (seen live with 29 artists).
    #[test]
    fn the_blocked_dialog_grows_when_a_long_list_arrives() {
        let (ctx, mut app) = accessible_app("blocked-grow");
        app.blocked_view = Loadable::Loading;
        app.dialog = Some(Dialog::Blocked);
        for _ in 0..3 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        app.blocked_view = Loadable::Loaded(crate::artist_blocks::BlockedContent {
            artists: (0..29)
                .map(|n| crate::artist_blocks::BlockedArtist {
                    id: n.to_string(),
                    name: format!("Artist {n}"),
                })
                .collect(),
            tracks: (0..5)
                .map(|n| crate::artist_blocks::BlockedTrack {
                    id: n.to_string(),
                    title: format!("Song {n}"),
                    artist: "Someone".into(),
                    hidden: false,
                })
                .collect(),
        });
        for _ in 0..4 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        let area = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("dialog")))
            .expect("the dialog's area");
        assert!(
            area.height() > 300.0,
            "the dialog is {} points high",
            area.height()
        );
        app.backend.shutdown();
    }

    fn frame_events(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            app.frame_ui(ui);
        });
        output.textures_delta.clear();
    }

    /// A toast is wide enough to avoid wrapping every word.
    #[test]
    fn a_toast_is_wide_enough_to_read() {
        let root = crate::paths::unique_test_dir("toast-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        // A short toast first: the toasts area remembers its size, and a
        // long toast used to inherit the narrow width and wrap inside it.
        app.toast("Saved");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        app.toasts.clear();
        app.toast("Wish You Were Here added to queue");
        // Two frames: an area sizes itself on its first one.
        let mut first = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
        first.textures_delta.clear();
        let mut output = ctx.run_ui(input, |ui| app.frame_ui(ui));
        output.textures_delta.clear();

        fn widest_toast_text(shape: &egui::epaint::Shape) -> Option<f32> {
            match shape {
                egui::epaint::Shape::Text(text)
                    if text.galley.job.text.contains("Wish You Were Here") =>
                {
                    Some(text.galley.rect.width())
                }
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().filter_map(widest_toast_text).next()
                }
                _ => None,
            }
        }
        let width = output
            .shapes
            .iter()
            .filter_map(|clipped| widest_toast_text(&clipped.shape))
            .next()
            .expect("the toast's text is painted");
        assert!(
            width > 150.0,
            "one word per line again: the toast text is only {width}px wide"
        );
        app.backend.shutdown();
    }

    /// The shortcuts are longer than a small window is tall, so the
    /// dialog scrolls them rather than running off the bottom with the
    /// Done button somewhere past the edge of the screen.
    #[test]
    fn the_shortcuts_dialog_fits_a_small_window() {
        let root = crate::paths::unique_test_dir("shortcuts-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.dialog = Some(Dialog::Shortcuts);

        let height = 420.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, height),
            )),
            ..Default::default()
        };
        // Two frames: the dialog sizes itself on the first.
        let mut first = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
        first.textures_delta.clear();
        let mut output = ctx.run_ui(input, |ui| app.frame_ui(ui));
        output.textures_delta.clear();

        let dialog = app.dialog_rect.expect("the dialog drew itself");
        let bottom = dialog.max.y;
        assert!(
            bottom <= height + 1.0,
            "the dialog runs {} pixels past the bottom of a {height}-tall window",
            bottom - height
        );
        app.backend.shutdown();
    }

    /// The embed card draws in both themes inside a small window.
    #[test]
    fn the_embed_dialog_renders_in_both_themes() {
        for light in [false, true] {
            let (ctx, mut app) = accessible_app("embed-dialog");
            app.settings.theme = if light {
                crate::settings::ThemeChoice::Light
            } else {
                crate::settings::ThemeChoice::Dark
            };
            app.dialog = Some(Dialog::Embed {
                uri: "tidal:video:441699021".into(),
                title: "Suzanne (Official Video)".into(),
                subtitle: "Leonard Cohen".into(),
                duration_ms: 236_000,
            });
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 700.0),
                    )),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| app.frame_ui(ui));
                out.textures_delta.clear();
            }
            let rect = app.dialog_rect.expect("the embed dialog drew itself");
            assert!(rect.max.y <= 701.0 && rect.width() > 300.0);
            app.backend.shutdown();
        }
    }

    #[test]
    fn inviting_a_collaborator_says_it_is_not_available_yet() {
        let (ctx, mut app) = accessible_app("invite-notice");
        let before = app.toasts.len();
        app.apply(Action::InviteCollaborator("pl1".into()), &ctx);
        assert_eq!(app.toasts.len(), before + 1);
        assert!(
            app.toasts
                .last()
                .unwrap()
                .message
                .contains("aren't available")
        );
        app.backend.shutdown();
    }

    /// Playlist forms stay sized to their content even on a tall window.
    #[test]
    fn playlist_dialogs_stay_compact_on_tall_windows() {
        for light in [false, true] {
            for size in [egui::vec2(1280.0, 800.0), egui::vec2(720.0, 1600.0)] {
                for edit in [false, true] {
                    for busy in [false, true] {
                        let (ctx, mut app) = accessible_app("compact-playlist-dialog");
                        app.settings.theme = if light {
                            crate::settings::ThemeChoice::Light
                        } else {
                            crate::settings::ThemeChoice::Dark
                        };
                        app.actions.push(Action::SettingsChanged);
                        app.playlist_busy = busy;
                        let input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        };
                        // The same modal area has already shown a longer dialog.
                        app.dialog = Some(Dialog::Shortcuts);
                        for _ in 0..3 {
                            let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                            output.textures_delta.clear();
                        }
                        app.dialog = None;
                        let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                        output.textures_delta.clear();
                        app.dialog = Some(if edit {
                            Dialog::EditPlaylist {
                                id: "pl1".into(),
                                name: "Long Way Home".into(),
                                description: "Songs for the road".into(),
                                public: Some(false),
                                cover: Default::default(),
                            }
                        } else {
                            Dialog::CreatePlaylist {
                                name: "Peak Masculinity".into(),
                                public: false,
                                add_uris: vec!["tidal:track:trk1".into()],
                                description: String::new(),
                            }
                        });
                        for _ in 0..3 {
                            let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                            output.textures_delta.clear();
                        }
                        let dialog = app.dialog_rect.expect("playlist dialog");
                        // TIDAL's cards: 486 high, a line more with songs to add.
                        let limit = 540.0;
                        assert!(
                            dialog.height() <= limit,
                            "edit={edit}, busy={busy}, light={light}, size={size:?}: {dialog:?}"
                        );
                        assert!(dialog.top() >= 0.0 && dialog.bottom() <= size.y);
                        app.backend.shutdown();
                    }
                }
            }
        }
    }

    /// Rule: the interface zoom control puts minus on the left and plus
    /// on the right. The setting row's control is right-to-left, which
    /// used to reverse the two buttons.
    #[test]
    fn interface_zoom_puts_minus_on_the_left() {
        let root = crate::paths::unique_test_dir("zoom-order-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.settings.zoom = 1.0;
        app.open(Page::Settings);

        let mut placed: Vec<(String, f32, f32)> = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        for _ in 0..2 {
            placed.clear();
            let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
            output.textures_delta.clear();
            fn walk(shape: &egui::epaint::Shape, placed: &mut Vec<(String, f32, f32)>) {
                match shape {
                    egui::epaint::Shape::Text(text) => {
                        placed.push((text.galley.job.text.clone(), text.pos.x, text.pos.y))
                    }
                    egui::epaint::Shape::Vec(shapes) => {
                        shapes.iter().for_each(|shape| walk(shape, placed))
                    }
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut placed);
            }
        }
        let percent = placed
            .iter()
            .find(|(text, _, _)| text == "100%")
            .unwrap_or_else(|| panic!("the zoom percent was never drawn: {placed:?}"));
        let on_row = |label: &str| -> f32 {
            placed
                .iter()
                .filter(|(text, _, y)| text == label && (y - percent.2).abs() < 8.0)
                .min_by(|a, b| (a.1 - percent.1).abs().total_cmp(&(b.1 - percent.1).abs()))
                .unwrap_or_else(|| panic!("{label} was never drawn next to 100%: {placed:?}"))
                .1
        };
        let minus = on_row("-");
        let plus = on_row("+");
        assert!(
            minus < percent.1 && percent.1 < plus,
            "zoom control should read minus, percent, plus; got - at {minus}, 100% at {}, + at {plus}",
            percent.1
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// Rule: side-panel headers stay on one line at their narrowest width.
    #[test]
    fn the_narrowest_panels_keep_their_headers_on_one_row() {
        let root = crate::paths::unique_test_dir("queue-header-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        populate(&mut app);
        app.settings.queue_width = crate::theme::SIDE_PANEL_MIN_WIDTH;
        app.settings.lyrics_width = crate::theme::SIDE_PANEL_MIN_WIDTH;
        app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
        app.lyrics_following = false;

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let drawn = |app: &mut App| {
            let mut placed = Vec::new();
            // A panel applies its requested width after the first frame.
            for _ in 0..2 {
                placed.clear();
                let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                output.textures_delta.clear();
                fn walk(shape: &egui::epaint::Shape, placed: &mut Vec<(String, egui::Rect)>) {
                    match shape {
                        egui::epaint::Shape::Text(text) => {
                            placed.push((text.galley.job.text.clone(), text.visual_bounding_rect()))
                        }
                        egui::epaint::Shape::Vec(shapes) => {
                            shapes.iter().for_each(|shape| walk(shape, placed))
                        }
                        _ => {}
                    }
                }
                for clipped in &output.shapes {
                    walk(&clipped.shape, &mut placed);
                }
            }
            placed
        };
        for (queue, lyrics) in [(false, false), (true, false), (false, true), (true, true)] {
            app.show_queue_panel = queue;
            app.show_lyrics_panel = lyrics;
            let placed = drawn(&mut app);
            // The queue floats with its title alone on its first row.
            if queue {
                assert!(placed.iter().any(|(text, _)| text == "Play queue"));
            }
            if lyrics {
                assert!(placed.iter().any(|(text, _)| text == "Lyrics"));
                // Paused, the song can be picked back up from the corner.
                assert!(placed.iter().any(|(text, _)| text == "Sync Lyrics"));
            }
        }
        app.backend.shutdown();
    }

    /// The queue names where the playing song comes from, on one row
    /// with its label: the playlist, or the song a radio is seeded by.
    /// With nothing reported, the row is not drawn.
    #[test]
    fn the_queue_names_where_the_song_plays_from() {
        let root = crate::paths::unique_test_dir("playing-from-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.show_queue_panel = true;

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let drawn = |app: &mut App| {
            let mut placed = Vec::new();
            // A panel applies its requested width after the first frame.
            for _ in 0..2 {
                placed.clear();
                let mut output = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                output.textures_delta.clear();
                fn walk(shape: &egui::epaint::Shape, placed: &mut Vec<(String, egui::Rect)>) {
                    match shape {
                        egui::epaint::Shape::Text(text) => {
                            placed.push((text.galley.job.text.clone(), text.visual_bounding_rect()))
                        }
                        egui::epaint::Shape::Vec(shapes) => {
                            shapes.iter().for_each(|shape| walk(shape, placed))
                        }
                        _ => {}
                    }
                }
                for clipped in &output.shapes {
                    walk(&clipped.shape, &mut placed);
                }
            }
            placed
        };
        let find = |placed: &[(String, egui::Rect)], label: &str| {
            placed
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{label} was never drawn: {placed:?}"))
        };

        // The name sits to the right of its label on one row. The sidebar
        // draws the same playlist name elsewhere, so look beside the label.
        let beside = |placed: &[(String, egui::Rect)], text: &str| {
            let label = find(placed, "Playing from: ");
            placed
                .iter()
                .find(|(drawn, rect)| {
                    drawn == text
                        && (rect.center().y - label.center().y).abs() < 10.0
                        && rect.left() >= label.right()
                })
                .unwrap_or_else(|| panic!("{text} was not drawn beside its label: {placed:?}"));
        };

        // The demo plays the second playlist.
        beside(&drawn(&mut app), "Late night focus");

        // A song radio is named after its song.
        if let Some(remote) = app.remote.as_mut() {
            remote.state.context = Some(Context {
                uri: "tidal:station:track:trk0".into(),
                kind: "station".into(),
            });
        }
        beside(&drawn(&mut app), "Rosewood Radio");

        // Nothing reported, nothing named.
        if let Some(remote) = app.remote.as_mut() {
            remote.state.context = None;
        }
        let placed = drawn(&mut app);
        assert!(
            !placed.iter().any(|(text, _)| text == "Playing from"),
            "the row hides without a context"
        );
        app.backend.shutdown();
    }

    /// The line being sung sits high in the lyrics, a fifth of the way
    /// down, in the panel and in full screen, so the lines to come fill
    /// the view.
    #[test]
    fn the_sung_line_sits_high_in_the_lyrics() {
        for fullscreen in [false, true] {
            let (ctx, mut app) = accessible_app(&format!("sung-line-{fullscreen}"));
            app.show_lyrics_panel = true;
            app.lyrics_fullscreen = Some(fullscreen);
            app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
            app.lyrics_following = true;
            let remote = app.remote.as_mut().unwrap();
            remote.state.is_playing = false;
            remote.state.progress_ms = Some(106_000);
            let mut centre = None;
            for frame in 0..60 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                centre = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text == "Somewhere past the county line" =>
                    {
                        Some(text.pos.y + text.galley.size().y / 2.0)
                    }
                    _ => None,
                });
            }
            let centre = centre.expect("the sung line is drawn");
            assert!(
                (60.0..320.0).contains(&centre),
                "fullscreen {fullscreen}: the sung line is at {centre} of 800"
            );
            app.backend.shutdown();
        }
    }

    /// Right-to-left lyrics are right-aligned and wrapped inside the panel:
    /// no part of an Arabic or Hebrew line is cut off at the left edge, in
    /// the side panel and in full screen.
    #[test]
    fn right_to_left_lyrics_stay_inside_the_panel() {
        let long = "مرحبا بكم في هذه الأغنية الطويلة جدا التي تتجاوز عرض اللوحة بكثير";
        let hebrew = "שלום עולם";
        for fullscreen in [None, Some(true)] {
            let (ctx, mut app) = accessible_app(&format!("rtl-lyrics-{fullscreen:?}"));
            app.show_lyrics_panel = true;
            app.lyrics_fullscreen = fullscreen;
            let mut lyrics = sample_lyrics();
            lyrics.lines[4].text = long.to_string();
            lyrics.lines[5].text = hebrew.to_string();
            app.lyrics = Loadable::Loaded(Some(lyrics));
            app.lyrics_following = true;
            let remote = app.remote.as_mut().unwrap();
            remote.state.is_playing = false;
            remote.state.progress_ms = Some(60_000);
            let mut seen = Vec::new();
            for frame in 0..60 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                seen.clear();
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && (text.galley.job.text.contains('م')
                            || text.galley.job.text.contains('ש'))
                    {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        seen.push((rect, shape.clip_rect));
                    }
                }
            }
            assert!(
                !seen.is_empty(),
                "fullscreen {fullscreen:?}: RTL lines drawn"
            );
            for (rect, clip) in seen {
                assert!(
                    rect.left() >= clip.left() - 0.5 && rect.right() <= clip.right() + 0.5,
                    "fullscreen {fullscreen:?}: RTL line {rect:?} is cut by {clip:?}"
                );
            }
            app.backend.shutdown();
        }
    }

    /// The line being sung is drawn in the text colour, not the accent, and
    /// the others are quieter: white on the dark theme, dark on the light one.
    #[test]
    fn the_sung_lyric_line_uses_the_text_colour_in_both_themes() {
        for light in [false, true] {
            let (ctx, mut app) = accessible_app(&format!("lyric-colour-{light}"));
            app.show_lyrics_panel = true;
            app.lyrics_fullscreen = None;
            app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
            app.lyrics_following = true;
            let remote = app.remote.as_mut().unwrap();
            remote.state.is_playing = false;
            remote.state.progress_ms = Some(106_000);
            ctx.set_theme(if light {
                egui::Theme::Light
            } else {
                egui::Theme::Dark
            });
            let palette = if light {
                crate::theme::Palette::light()
            } else {
                crate::theme::Palette::dark()
            };
            crate::theme::apply(&ctx, &palette);
            let mut sung = None;
            for frame in 0..60 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                sung = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text == "Somewhere past the county line" =>
                    {
                        text.galley.job.sections.first().map(|s| s.format.color)
                    }
                    _ => None,
                });
            }
            let sung = sung.expect("the sung line is drawn");
            let palette = app.palette;
            assert_ne!(sung, palette.accent, "light {light}: not the accent");
            assert_eq!(sung, palette.text, "light {light}: the text colour");
            app.backend.shutdown();
        }
    }

    /// Full screen is TIDAL's now-playing view: the cover on the left and the
    /// Lyrics tab in the column to its right (`now_playing::place`). A song
    /// without words keeps the tab, which says why in that same column.
    #[cfg(feature = "demo")]
    #[test]
    fn full_screen_lyrics_show_the_cover_beside_the_words_or_alone() {
        let find = |shapes: &[egui::epaint::ClippedShape], wanted: &str| {
            shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == wanted => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
        };
        // The column starts at 50.8% of the width and ends 12.15% from the
        // right edge; the cover sits left of it.
        let (column_left, column_right) = (1600.0 * 0.508, 1600.0 * (1.0 - 0.1215));
        for (surface, words) in [
            ("lyrics-fullscreen-view", true),
            ("lyrics-fullscreen-instrumental", false),
        ] {
            let (ctx, mut app) = accessible_app(surface);
            apply_flags(&mut app, None, Some(surface));
            let mut shapes = Vec::new();
            for frame in 0..30 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1600.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                shapes = output.shapes;
            }
            let lyric = find(&shapes, "Somewhere past the county line");
            let detail = find(&shapes, "No timed lyrics for this track.");
            if words {
                let lyric = lyric.expect("the words are drawn");
                assert!(lyric.left() > column_left, "beside the cover: {lyric:?}");
                assert!(lyric.right() < column_right, "inside the column: {lyric:?}");
            } else {
                assert!(lyric.is_none());
                let detail = detail.expect("why there are no words");
                assert!(detail.left() > column_left, "beside the cover: {detail:?}");
                assert!(
                    detail.right() < column_right,
                    "inside the column: {detail:?}"
                );
            }
            app.backend.shutdown();
        }
    }

    /// While lyrics load nothing is read yet, and when fetching them failed
    /// the failure and its retry sit in the lyrics column beside the cover.
    #[cfg(feature = "demo")]
    #[test]
    fn full_screen_lyrics_keep_the_column_beside_the_cover_until_there_are_words() {
        let beside_cover = |shapes: &[egui::epaint::ClippedShape], wanted: &str| {
            shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.contains(wanted) => {
                    shape.shape.visual_bounding_rect().left() > 1600.0 * 0.508
                }
                _ => false,
            })
        };
        for (state, shown) in [
            (Loadable::Loading, None),
            (
                Loadable::Failed("Connection interrupted".into()),
                Some(["Connection interrupted", "Try again"]),
            ),
        ] {
            let (ctx, mut app) = accessible_app("lyrics-centred-while-loading");
            apply_flags(&mut app, None, Some("lyrics-fullscreen-view"));
            app.lyrics = state;
            let mut shapes = Vec::new();
            for frame in 0..10 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1600.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.frame_ui(ui),
                );
                output.textures_delta.clear();
                shapes = output.shapes;
            }
            assert!(
                !beside_cover(&shapes, "Somewhere past the county line"),
                "no words yet"
            );
            for wanted in shown.into_iter().flatten() {
                assert!(beside_cover(&shapes, wanted), "{wanted} beside the cover");
            }
            app.backend.shutdown();
        }
    }

    #[test]
    fn fullscreen_lyrics_highlight_preserves_line_layout() {
        let root = crate::paths::unique_test_dir("lyrics-layout");
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.show_lyrics_panel = true;
        app.lyrics_fullscreen = Some(false);
        app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
        app.lyrics_following = false;
        let mut sizes = Vec::new();
        for (step, position) in [0, 41_000].into_iter().enumerate() {
            let remote = app.remote.as_mut().unwrap();
            remote.state.is_playing = false;
            remote.state.progress_ms = Some(position);
            for frame in 0..30 {
                let input = egui::RawInput {
                    time: Some(step as f64 + f64::from(frame) / 30.0),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| app.frame_ui(ui));
                output.textures_delta.clear();
                if frame == 29 {
                    let line = output
                        .shapes
                        .iter()
                        .find_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape
                                && text.galley.job.text
                                    == "Streetlights blinking down the river road"
                            {
                                Some((text.galley.rows.len(), text.galley.size()))
                            } else {
                                None
                            }
                        })
                        .expect("the lyric line is rendered");
                    sizes.push(line);
                }
            }
        }
        app.backend.shutdown();
        assert_eq!(
            sizes[0].0, sizes[1].0,
            "highlighting must not rewrap a line"
        );
        // A line only scales between its quiet and current sizes.
        let grown = sizes[1].1.x / sizes[0].1.x;
        assert!(
            (grown - 1.0 / crate::ui::lyrics_follow::LINE_SCALE_INACTIVE).abs() < 0.02
                || (grown - 1.0).abs() < 0.02,
            "a line scaled by {grown}"
        );
    }

    /// Every page, panel, and dialog lays out without panicking.
    #[test]
    fn every_surface_renders_headless() {
        let root = crate::paths::unique_test_dir("render-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        // These frames never advance the clock or take pictures.
        app.reveal_theme_changes = false;

        let pages = [
            Page::Home,
            Page::TopSongs,
            Page::Search,
            Page::LikedSongs,
            Page::Albums,
            Page::Artists,
            Page::Mixes,
            Page::Videos,
            Page::Playlist("pl1".into()),
            Page::Playlist("missing".into()),
            Page::Album("alb0".into()),
            Page::Artist("art0".into()),
            Page::Queue,
            Page::Settings,
        ];
        for page in pages {
            app.open(page.clone());
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert_eq!(app.page(), &page);
        }
        app.settings.sidebar_visible = false;
        frame(&ctx, &mut app);
        app.settings.sidebar_visible = true;
        app.show_queue_panel = true;
        frame(&ctx, &mut app);
        // Draw the Playing next section with a manual queue row.
        if let Loadable::Loaded(queue) = &app.queue
            && let Some(first) = queue.queue.first()
        {
            app.manual_queue = vec![first.uri().to_string()];
        }
        frame(&ctx, &mut app);
        app.manual_queue.clear();
        app.lyrics_uri = app.now_playing().map(|now| now.uri);
        app.lyrics = Loadable::Loaded(Some(sample_lyrics()));
        app.show_lyrics_panel = true;
        frame(&ctx, &mut app);
        app.lyrics_fullscreen = Some(false);
        frame(&ctx, &mut app);
        app.lyrics_fullscreen = None;
        app.show_lyrics_panel = false;
        for dialog in [
            Dialog::Shortcuts,
            Dialog::CreatePlaylist {
                name: "x".into(),
                public: true,
                add_uris: vec![],
                description: String::new(),
            },
            Dialog::EditPlaylist {
                cover: Default::default(),
                id: "pl1".into(),
                name: "x".into(),
                description: String::new(),
                public: Some(false),
            },
            Dialog::ConfirmDeletePlaylist {
                id: "pl1".into(),
                name: "x".into(),
                owned: true,
            },
            Dialog::ConfirmPlaylistDuplicates {
                position: None,
                playlist_id: "pl1".into(),
                playlist_name: "x".into(),
                items: vec![PlayableItem::Track(track(1))],
                duplicate_uris: vec!["tidal:track:trk1".into()],
            },
        ] {
            app.dialog = Some(dialog);
            frame(&ctx, &mut app);
        }
        app.settings.theme = crate::settings::ThemeChoice::Light;
        app.actions.push(Action::SettingsChanged);
        app.open(Page::Home);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert!(!app.palette.dark);
        app.lyrics_fullscreen = Some(false);
        frame(&ctx, &mut app);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// Virtual queue rows and library cards still draw a long list, and the
    /// library still asks for the next page when the end is near.
    #[test]
    fn a_long_virtual_queue_and_library_still_draw() {
        let root = crate::paths::unique_test_dir("virtual-long");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        if let Loadable::Loaded(queue) = &mut app.queue {
            let seed = queue.queue.clone();
            queue.queue = seed.iter().cloned().cycle().take(200).collect();
            app.manual_queue = queue
                .queue
                .iter()
                .take(80)
                .map(|item| item.uri().to_string())
                .collect();
        }
        app.show_queue_panel = true;
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        app.library.albums.items = (0..80)
            .map(|index| SavedAlbum {
                added_at: None,
                album: album(index % 8),
            })
            .collect();
        app.library.albums.next_offset = Some(80);
        app.library.albums.loaded_once = true;
        app.open(Page::Albums);
        app.actions.clear();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// A drag in flight renders, and releasing it over an owned playlist
    /// row lands in the same add-to-playlist plumbing the row menu uses.
    #[test]
    fn dropping_selected_songs_on_a_sidebar_playlist_adds_them_all() {
        drop_songs_on_sidebar(2);
    }

    #[test]
    fn dropping_a_song_on_a_sidebar_playlist_adds_it() {
        drop_songs_on_sidebar(1);
    }

    fn drop_songs_on_sidebar(count: usize) {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-drag-test-{}-{count}",
            std::process::id()
        ));
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.library
            .playlists
            .get_mut()
            .expect("the demo library")
            .retain(|playlist| playlist.id == "pl1");
        app.open(Page::Playlist("pl1".into()));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let before = app.playlist_pages["pl1"].items.items.len();
        let mut dragged = vec![
            PlayableItem::Track(Track {
                id: Some("not-in-demo-playlists".into()),
                uri: "tidal:track:not-in-demo-playlists".into(),
                name: "A new song".into(),
                ..Default::default()
            }),
            PlayableItem::Track(Track {
                id: Some("also-not-in-demo-playlists".into()),
                uri: "tidal:track:also-not-in-demo-playlists".into(),
                name: "Another new song".into(),
                ..Default::default()
            }),
        ];

        dragged.truncate(count);

        // Sweep a held track down the sidebar; somewhere along the sweep
        // the pointer crosses an owned playlist row, and releasing there
        // must mark the playlist edit busy through the existing plumbing.
        // Where exactly the rows sit depends on the loaded fonts, so the
        // sweep does not hardcode a row position.
        let mut dropped = false;
        for step in 0..40 {
            let pos = egui::pos2(120.0, 120.0 + step as f32 * 15.0);
            egui::DragAndDrop::set_payload(
                &ctx,
                DragTrack {
                    title: "A new song".into(),
                    image: None,
                    items: dragged.clone(),
                    from: None,
                    source_playlist: None,
                },
            );
            frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(!egui::DragAndDrop::has_any_payload(&ctx));
            if app.playlist_busy {
                dropped = true;
                break;
            }
        }
        assert!(dropped, "no sweep position landed on an owned playlist row");
        assert_eq!(app.playlist_pages["pl1"].items.items.len(), before + count);
        assert_eq!(
            app.playlist_pages["pl1"].items.items[before..]
                .iter()
                .map(|row| row.playable().unwrap().uri())
                .collect::<Vec<_>>(),
            dragged.iter().map(PlayableItem::uri).collect::<Vec<_>>()
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn selecting_album_rows_then_dragging_to_a_playlist_copies_display_order() {
        use egui::accesskit::Role;
        for sorted in [false, true] {
            let (ctx, mut app) = accessible_app(&format!("album-selection-drag-{sorted}"));
            let source = Page::Album("alb0".into());
            app.open(source.clone());
            let tracks = &mut app.album_pages.get_mut("alb0").unwrap().tracks;
            tracks.items.truncate(3);
            for (index, name) in ["Charlie", "Bravo", "Alpha"].iter().enumerate() {
                tracks.items[index].name = name.to_string();
                tracks.items[index].uri = format!("tidal:track:source{index}");
                tracks.items[index].id = Some(format!("source{index}"));
            }
            tracks.total = Some(3);
            tracks.next_offset = None;
            tracks.revision += 1;
            if sorted {
                app.table_sorts.insert(
                    source.clone(),
                    crate::model::TableSort {
                        column: crate::model::SortColumn::Title,
                        ascending: true,
                    },
                );
            }
            let draw = |app: &mut App, mut events: Vec<egui::Event>, modifiers| {
                events.insert(0, egui::Event::ModifiersChanged(modifiers));
                accessible_frame(&ctx, app, events)
            };
            draw(&mut app, vec![], egui::Modifiers::NONE);
            let tree = draw(&mut app, vec![], egui::Modifiers::NONE);
            let row = |name: &str| {
                let prefix = format!("Play {name},");
                let bounds = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == Role::Button
                            && node.label().is_some_and(|label| label.starts_with(&prefix))
                    })
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                egui::pos2(bounds.x0 as f32 + 130.0, bounds.y0 as f32 + 8.0)
            };
            let first = row(if sorted { "Alpha" } else { "Charlie" });
            let last = row(if sorted { "Charlie" } else { "Alpha" });
            // Pick in reverse order, using Cmd on macOS or Ctrl elsewhere.
            draw(
                &mut app,
                pointer_click(last, egui::PointerButton::Primary),
                egui::Modifiers::NONE,
            );
            assert_eq!(
                app.picked_rows(&source)
                    .map(|rows| rows.iter().copied().collect::<Vec<_>>()),
                Some(vec![2]),
                "first click at {last:?}, page {:?}",
                app.page()
            );
            let modifier = egui::Modifiers {
                command: true,
                mac_cmd: cfg!(target_os = "macos"),
                ctrl: !cfg!(target_os = "macos"),
                ..Default::default()
            };
            let mut click = pointer_click(first, egui::PointerButton::Primary);
            for event in &mut click {
                if let egui::Event::PointerButton { modifiers, .. } = event {
                    *modifiers = modifier;
                }
            }
            draw(&mut app, click, modifier);
            assert_eq!(
                app.picked_rows(&source)
                    .unwrap()
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
                [0, 2]
            );
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(last),
                    egui::Event::PointerButton {
                        pos: last,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
            );
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(last + egui::vec2(15.0, 0.0))],
                egui::Modifiers::NONE,
            );
            let payload =
                egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("drag from a selected row");
            let expected = if sorted {
                ["tidal:track:source2", "tidal:track:source0"]
            } else {
                ["tidal:track:source0", "tidal:track:source2"]
            };
            assert_eq!(
                payload
                    .items
                    .iter()
                    .map(PlayableItem::uri)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(payload.from, None);
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button && node.label() == Some("Late night focus")
                })
                .unwrap()
                .1
                .bounds()
                .unwrap();
            let target = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            let before = app.playlist_pages["pl1"].items.items.len();
            let playing = app.current_track_uri();
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(target)],
                egui::Modifiers::NONE,
            );
            draw(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                egui::Modifiers::NONE,
            );
            assert_eq!(
                app.playlist_pages["pl1"].items.items[before..]
                    .iter()
                    .map(|row| row.playable().unwrap().uri())
                    .collect::<Vec<_>>(),
                expected
            );
            let sent = app.backend.take_playlist_add_requests();
            assert!(
                matches!(sent.as_slice(), [crate::backend::ApiRequest::AddToPlaylist { playlist_id, uris, position: None, .. }] if playlist_id == "pl1" && uris == &expected)
            );
            assert_eq!(app.current_track_uri(), playing);
            app.backend.shutdown();
        }
    }

    /// Double-clicking a playable Library row plays its context. The first click still opens the page.
    #[test]
    fn double_clicking_a_sidebar_row_plays_its_context() {
        let (ctx, mut app) = accessible_app("sidebar-double-click");
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let name = sidebar_text(&painted, "Sunday morning").center();
        app.actions.clear();
        let [first, second] = double_click(name);
        view_frame(&ctx, &mut app, first, view);
        view_frame(&ctx, &mut app, second, view);
        assert_eq!(played_contexts(&app), ["tidal:playlist:pl2"]);
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::Open(Page::Playlist(id)) if id == "pl2")),
            "double click must still open the page"
        );
        app.backend.shutdown();
    }

    /// A single Library row click keeps navigating and never starts playback.
    #[test]
    fn single_clicking_a_sidebar_row_only_navigates() {
        let (ctx, mut app) = accessible_app("sidebar-single-click");
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let name = sidebar_text(&painted, "Sunday morning").center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(name, egui::PointerButton::Primary),
            view,
        );
        assert!(played_contexts(&app).is_empty());
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::Open(Page::Playlist(id)) if id == "pl2"))
        );
        app.backend.shutdown();
    }

    /// The cover play button sits on top of its row. A double click on it
    /// sends one play command, not one for each click plus the row's.
    #[test]
    fn double_clicking_a_sidebar_cover_plays_once() {
        let (ctx, mut app) = accessible_app("sidebar-double-click-cover");
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let name = sidebar_text(&painted, "Sunday morning");
        // The cover is the 44 point square centered 34 points left of the
        // name and on the row's centre, nine points below the name line.
        let cover = egui::pos2(name.left() - 34.0, name.center().y + 9.0);
        app.actions.clear();
        let [first, second] = double_click(cover);
        view_frame(&ctx, &mut app, first, view);
        assert_eq!(played_contexts(&app), ["tidal:playlist:pl2"]);
        view_frame(&ctx, &mut app, second, view);
        assert_eq!(
            played_contexts(&app),
            ["tidal:playlist:pl2"],
            "the second click must not play again"
        );
        app.backend.shutdown();
    }

    /// The cover's control follows its row: the playlist that is playing
    /// offers pause on its cover, not a restart of the context.
    #[test]
    fn clicking_the_cover_of_the_playing_playlist_pauses() {
        let (ctx, mut app) = accessible_app("sidebar-cover-pause");
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        // The demo's remote snapshot plays the "Late night focus" playlist.
        let name = sidebar_text(&painted, "Late night focus");
        let cover = egui::pos2(name.left() - 34.0, name.center().y + 9.0);
        app.actions.clear();
        let [click, _] = double_click(cover);
        view_frame(&ctx, &mut app, click, view);
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::TogglePlay)),
            "the playing playlist's cover pauses"
        );
        assert!(
            played_contexts(&app).is_empty(),
            "the playing playlist must not be started over"
        );
        app.backend.shutdown();
    }

    /// A paused context keeps the play glyph, and pressing it resumes where
    /// the song was instead of starting the context over.
    #[test]
    fn clicking_the_cover_of_the_paused_playlist_resumes() {
        let (ctx, mut app) = accessible_app("sidebar-cover-resume");
        // The demo's remote snapshot plays "Late night focus"; pause it.
        app.remote.as_mut().unwrap().state.is_playing = false;
        let view = crate::ui::sidebar::show;
        view_frame(&ctx, &mut app, vec![], view);
        let painted = view_frame(&ctx, &mut app, vec![], view);
        let name = sidebar_text(&painted, "Late night focus");
        let cover = egui::pos2(name.left() - 34.0, name.center().y + 9.0);
        app.actions.clear();
        let [click, _] = double_click(cover);
        view_frame(&ctx, &mut app, click, view);
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::TogglePlay)),
            "the paused playlist's cover resumes"
        );
        assert!(
            played_contexts(&app).is_empty(),
            "the paused playlist must not be started over"
        );
        app.backend.shutdown();
    }

    /// The cover and title in the bottom-left player are a song source, not
    /// just links. The sidebar can therefore receive the same complete row it
    /// receives when a table song is dragged.
    #[test]
    fn dragging_the_now_playing_song_supplies_a_playlist_row() {
        let root = crate::paths::unique_test_dir("now-playing-drag-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }

        let start = egui::pos2(40.0, 755.0);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
        );

        let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
            .expect("dragging the bottom-left song should create a song payload");
        assert_eq!(payload.items.len(), 1);
        assert_eq!(payload.items[0].uri(), "tidal:track:trk0");
        assert_eq!(payload.from, None, "this is an add, not a playlist move");

        egui::DragAndDrop::clear_payload(&ctx);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// Dropping a dragged song on the queue button in the player bar queues
    /// it, the same as the "Add to queue" menu item.
    #[test]
    fn dropping_a_dragged_song_on_the_queue_button_queues_it() {
        let (ctx, mut app) = accessible_app("queue-button-drop");
        let source_uri = app
            .queue
            .get()
            .unwrap()
            .currently_playing
            .clone()
            .unwrap()
            .uri()
            .to_string();

        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let button = accessible_node(&tree, "Queue", egui::accesskit::Role::Button);
        let bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == button)
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let end = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 / 2.0,
            (bounds.y0 + bounds.y1) as f32 / 2.0,
        );

        // Drag the now-playing song from the bottom-left player, same
        // starting point as the equivalent playlist-insert test.
        let start = egui::pos2(40.0, 755.0);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
        );
        let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
            .expect("dragging the now-playing song should create a payload");
        assert_eq!(payload.items[0].uri(), source_uri);

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(app.manual_queue, vec![source_uri]);
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message == "1 track added to queue"),
            "{:?}",
            app.toasts
        );
        app.backend.shutdown();
    }

    /// Dropping a dragged song anywhere on the open queue list (side panel
    /// or full page) queues it. The queue has no positional drop slots, so
    /// any point in the list works, not just the toggle button.
    #[test]
    fn dropping_a_dragged_song_on_the_open_queue_list_queues_it() {
        let (ctx, mut app) = accessible_app("queue-list-drop");
        app.show_queue_panel = true;
        let source_uri = app
            .queue
            .get()
            .unwrap()
            .currently_playing
            .clone()
            .unwrap()
            .uri()
            .to_string();
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }

        // A point well inside the queue side panel's body, clear of its
        // close/save buttons and tab chips.
        let end = egui::pos2(1100.0, 300.0);

        let start = egui::pos2(40.0, 755.0);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
        );
        let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
            .expect("dragging the now-playing song should create a payload");
        assert_eq!(payload.items[0].uri(), source_uri);

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(app.manual_queue, vec![source_uri]);
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message == "1 track added to queue"),
            "{:?}",
            app.toasts
        );
        app.backend.shutdown();
    }

    /// Seeds three manually queued songs at the front of "Playing next",
    /// named "Queued 0".."Queued 2", for the queue-reorder tests below.
    /// Also makes the local player the active target, the only case where
    /// the queue can be reordered or inserted into positionally.
    fn seed_queued_songs(app: &mut App) -> Vec<String> {
        app.local_ready = true;
        app.local.track = Some(crate::player::LocalTrack {
            uri: app.now_playing().unwrap().uri,
            ..Default::default()
        });
        app.local.playback = crate::player::Playback::Paused;
        let songs: Vec<Track> = (0..3)
            .map(|index| {
                let mut t = track(index);
                t.uri = format!("tidal:track:queued{index}");
                t.id = Some(format!("queued{index}"));
                t.name = format!("Queued {index}");
                t
            })
            .collect();
        if let Loadable::Loaded(queue) = &mut app.queue {
            for (offset, song) in songs.iter().enumerate() {
                queue
                    .queue
                    .insert(offset, PlayableItem::Track(song.clone()));
            }
        }
        let uris: Vec<String> = songs.iter().map(|song| song.uri.clone()).collect();
        app.manual_queue = uris.clone();
        uris
    }

    /// Dragging a song already in "Playing next" and dropping it elsewhere
    /// in the queue moves it there instead of adding a duplicate.
    #[test]
    fn dragging_a_queued_row_within_the_queue_reorders_it() {
        let (ctx, mut app) = accessible_app("queue-reorder");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row_rect = |name: &str| {
            let prefix = format!("Play {name},");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|text| text.starts_with(&prefix))
                })
                .unwrap_or_else(|| panic!("missing row {name}"))
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        let start_row = row_rect("Queued 0");
        let start = egui::pos2(start_row.left() + 80.0, start_row.center().y);
        // Dropped past the last queued row: moves to the end of "Playing next".
        let last_row = row_rect("Queued 2");
        let end = egui::pos2(last_row.left() + 130.0, last_row.bottom() - 1.0);

        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(15.0, -10.0))],
        );
        let payload =
            egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("a reorderable queue row drags");
        assert_eq!(
            payload.from,
            Some(("queue".to_string(), 0)),
            "the queue row must tag itself as the move source"
        );

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(
            app.manual_queue,
            vec![uris[1].clone(), uris[2].clone(), uris[0].clone()],
            "moved, not duplicated"
        );
        app.backend.shutdown();
    }

    /// Right-clicking a song under Playing next offers to take it out of
    /// the queue, and doing so removes that row; a Next up row offers the
    /// entry too (queue.md rule 11).
    #[test]
    fn a_queued_rows_menu_removes_it_from_the_queue() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("queue-remove");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        // The queue panel's own row: some of its songs are also on the
        // page behind it, and the panel opens against the right edge.
        let row_center = |tree: &egui::accesskit::TreeUpdate, name: &str| {
            let prefix = format!("Play {name},");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button
                        && node.label().is_some_and(|text| text.starts_with(&prefix))
                        && node
                            .bounds()
                            .is_some_and(|bounds| bounds.x0 >= 900.0 && bounds.y1 <= 800.0)
                })
                .unwrap_or_else(|| panic!("missing queue row {name}"))
                .1
                .bounds()
                .unwrap();
            egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            )
        };
        let has_remove = |tree: &egui::accesskit::TreeUpdate| {
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Remove from queue"))
        };

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let next_up = row_center(&tree, "Otomo");
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(next_up, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Play next", Role::Button);
        assert!(has_remove(&tree), "a Next up row can be removed");
        accessible_frame(
            &ctx,
            &mut app,
            vec![keyboard(egui::Key::Escape, egui::Modifiers::NONE)],
        );

        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let queued = row_center(&tree, "Queued 1");
        accessible_frame(
            &ctx,
            &mut app,
            pointer_click(queued, egui::PointerButton::Secondary),
        );
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let remove = accessible_node(&tree, "Remove from queue", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(
                remove,
                egui::accesskit::Action::Click,
                None,
            )],
        );
        assert_eq!(app.manual_queue, [uris[0].clone(), uris[2].clone()]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            !tree.nodes.iter().any(|(_, node)| node
                .label()
                .is_some_and(|text| text.starts_with("Play Queued 1,"))),
            "the removed row leaves the queue at once"
        );
        app.backend.shutdown();
    }

    /// If the playing song advances (consuming the front queued row) while a
    /// queued row is mid-drag, the drop must still act on the song that was
    /// actually picked up, not on whatever now sits at the drag's recorded
    /// start index.
    #[test]
    fn dragging_a_queued_row_while_next_advances_moves_the_dragged_song() {
        let (ctx, mut app) = accessible_app("queue-reorder-mid-drag-advance");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row_rect = |name: &str| {
            let prefix = format!("Play {name},");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|text| text.starts_with(&prefix))
                })
                .unwrap_or_else(|| panic!("missing row {name}"))
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        // "Queued 1" is dragged; row 0's slot is the drop target, captured
        // now so the drop still lands there once row 0 shifts up.
        let start_row = row_rect("Queued 1");
        let start = egui::pos2(start_row.left() + 80.0, start_row.center().y);
        let front_slot = row_rect("Queued 0");
        let end = egui::pos2(front_slot.left() + 130.0, front_slot.top() + 1.0);

        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(15.0, -10.0))],
        );
        let payload =
            egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("a reorderable queue row drags");
        assert_eq!(
            payload.from,
            Some(("queue".to_string(), 1)),
            "the queue row must tag itself as the move source"
        );

        // "Next" fires mid-drag: the front queued row is consumed and every
        // later row's index shifts down by one, so index 1 (the recorded
        // drag source) now names "Queued 2" instead of the dragged song.
        app.manual_queue.remove(0);
        if let Loadable::Loaded(queue) = &mut app.queue {
            queue.queue.remove(0);
        }

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        // The dragged song ("Queued 1") had already shifted to the front on
        // its own; dropping it back on the front slot is a no-op. Trusting
        // the stale index 1 would instead have moved "Queued 2" (the wrong
        // song) to the front.
        assert_eq!(
            app.manual_queue,
            vec![uris[1].clone(), uris[2].clone()],
            "the dragged song stays put instead of the wrong row moving"
        );
        app.backend.shutdown();
    }

    /// "Next up" plays from the current context, not from a list Tidalfast
    /// can rewrite, so it is never a drop target: dropping a queued row on
    /// it must not move or insert anything, even though the row sits inside
    /// the same scrollable list as "Playing next".
    #[test]
    fn dropping_a_queued_row_on_next_up_does_nothing() {
        let (ctx, mut app) = accessible_app("queue-reorder-next-up-not-a-target");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        // "Otomo" is also a demo library track shown elsewhere on the page
        // behind the queue side panel, so match on the queue panel's own
        // on-screen column (it opens flush against the right edge) rather
        // than the first node with a matching label.
        let row_rect = |name: &str| {
            let prefix = format!("Play {name},");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|text| text.starts_with(&prefix))
                        && node
                            .bounds()
                            .is_some_and(|bounds| bounds.x0 >= 900.0 && bounds.y1 <= 800.0)
                })
                .unwrap_or_else(|| panic!("missing on-screen queue row {name}"))
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        let start_row = row_rect("Queued 0");
        let start = egui::pos2(start_row.left() + 80.0, start_row.center().y);
        // "Otomo" is the first row of "Next up", from the default demo
        // queue past the three manually queued rows seeded above.
        let next_up_row = row_rect("Otomo");
        // The "Next up" heading sits just below Playing next's last row.
        let heading = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                [node.label(), node.value()]
                    .into_iter()
                    .flatten()
                    .any(|text| text.starts_with("Next up"))
            })
            .and_then(|(_, node)| node.bounds())
            .expect("the Next up heading");
        let heading = egui::Rect::from_min_max(
            egui::pos2(heading.x0 as f32, heading.y0 as f32),
            egui::pos2(heading.x1 as f32, heading.y1 as f32),
        );

        for end in [
            egui::pos2(next_up_row.left() + 130.0, next_up_row.center().y),
            egui::pos2(heading.left() + 130.0, heading.center().y),
            egui::pos2(heading.left() + 130.0, heading.top() + 1.0),
        ] {
            frame_events(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(start + egui::vec2(15.0, -10.0))],
            );
            egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("a reorderable queue row drags");

            frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            frame_events(&ctx, &mut app, vec![]);

            assert_eq!(
                app.manual_queue, uris,
                "Next up is never a drop target: nothing moves or gets inserted (dropped at {end:?})"
            );
        }
        app.backend.shutdown();
    }

    /// Holding a dragged queue row at the bottom edge of the open queue
    /// scrolls it, as the playlist table does, so a row can be moved past
    /// the rows that fit on screen.
    #[test]
    fn dragging_a_queued_row_scrolls_a_long_playing_next() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("queue-drag-scroll");
        app.show_queue_panel = true;
        seed_queued_songs(&mut app);
        let songs: Vec<Track> = (3..40)
            .map(|index| {
                let mut t = track(index);
                t.uri = format!("tidal:track:queued{index}");
                t.id = Some(format!("queued{index}"));
                t.name = format!("Queued {index}");
                t
            })
            .collect();
        if let Loadable::Loaded(queue) = &mut app.queue {
            for (offset, song) in songs.iter().enumerate() {
                queue
                    .queue
                    .insert(3 + offset, PlayableItem::Track(song.clone()));
            }
        }
        app.manual_queue
            .extend(songs.iter().map(|song| song.uri.clone()));
        let uris = app.manual_queue.clone();
        let on_screen = |tree: &egui::accesskit::TreeUpdate, name: &str| {
            let prefix = format!("Play {name},");
            tree.nodes.iter().find_map(|(_, node)| {
                (node.role() == Role::Button
                    && node.label().is_some_and(|text| text.starts_with(&prefix)))
                .then(|| node.bounds())
                .flatten()
                .filter(|bounds| bounds.x0 >= 900.0)
            })
        };
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            on_screen(&tree, "Queued 39").is_none(),
            "the last queued row starts out of view"
        );
        let first = on_screen(&tree, "Queued 0").expect("the first queued row is shown");
        let start = egui::pos2(first.x0 as f32 + 80.0, ((first.y0 + first.y1) / 2.0) as f32);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(15.0, -10.0))],
        );
        egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("a reorderable queue row drags");
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(egui::pos2(
                start.x,
                // The floating queue ends 20 above the bar (TIDAL desktop's),
                // inside 16.
                800.0 - crate::theme::PLAYER_BAR_HEIGHT - 42.0,
            ))],
        );
        for _ in 0..400 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            on_screen(&tree, "Queued 39").is_some(),
            "holding the drag at the bottom edge scrolls Playing next into view"
        );
        assert_eq!(
            app.manual_queue, uris,
            "scrolling alone must not reorder rows"
        );
        egui::DragAndDrop::clear_payload(&ctx);
        app.backend.shutdown();
    }

    /// With nothing manually queued yet, every row the open queue shows
    /// belongs to "Next up", so the panel offers no drop target at all:
    /// neither a Next up row nor its heading takes a dragged song. The
    /// player bar's Queue button still does.
    #[test]
    fn dropping_a_song_on_next_up_with_an_empty_playing_next_does_nothing() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("queue-empty-playing-next-not-a-target");
        app.show_queue_panel = true;
        // Make the local player the active target, like `seed_queued_songs`,
        // but leave "Playing next" empty.
        app.local_ready = true;
        app.local.track = Some(crate::player::LocalTrack {
            uri: app.now_playing().unwrap().uri,
            ..Default::default()
        });
        app.local.playback = crate::player::Playback::Paused;
        assert!(app.manual_queue.is_empty());
        assert!(app.queue_locally_reorderable());

        let source_uri = app
            .queue
            .get()
            .unwrap()
            .currently_playing
            .clone()
            .unwrap()
            .uri()
            .to_string();

        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let bounds_of = |id: egui::accesskit::NodeId| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(node, _)| *node == id)
                .unwrap()
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        // "Otomo" is the first row of the default demo queue's "Next up".
        // It is also a demo library track shown elsewhere on the page
        // behind the queue side panel, so match on the queue panel's own
        // on-screen column (it opens flush against the right edge) rather
        // than the first node with a matching label.
        let row = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == Role::Button
                    && node
                        .label()
                        .is_some_and(|text| text.starts_with("Play Otomo,"))
                    && node
                        .bounds()
                        .is_some_and(|bounds| bounds.x0 >= 900.0 && bounds.y1 <= 800.0)
            })
            .unwrap_or_else(|| panic!("missing on-screen queue row Otomo"))
            .0;
        let row = bounds_of(row);
        let heading = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                [node.label(), node.value()]
                    .into_iter()
                    .flatten()
                    .any(|text| text.starts_with("Next up"))
            })
            .expect("the Next up heading")
            .0;
        let heading = bounds_of(heading);
        // The player bar's Queue button, not the queue panel's Queue tab.
        let button = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == Role::Button
                    && node.label() == Some("Queue")
                    && node.bounds().is_some_and(|bounds| {
                        bounds.y0 >= f64::from(800.0 - crate::theme::PLAYER_BAR_HEIGHT)
                    })
            })
            .expect("the player bar's Queue button")
            .0;
        let button = bounds_of(button);

        let drag_to = |app: &mut App, end: egui::Pos2| {
            let start = egui::pos2(40.0, 755.0);
            frame_events(
                &ctx,
                app,
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame_events(
                &ctx,
                app,
                vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
            );
            let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
                .expect("dragging the now-playing song should create a payload");
            assert_eq!(payload.items[0].uri(), source_uri);
            frame_events(&ctx, app, vec![egui::Event::PointerMoved(end)]);
            frame_events(
                &ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            frame_events(&ctx, app, vec![]);
        };

        for end in [
            egui::pos2(row.left() + 130.0, row.center().y),
            egui::pos2(row.left() + 130.0, row.top() + 2.0),
            heading.center(),
            egui::pos2(heading.left() + 130.0, heading.top() - 2.0),
        ] {
            drag_to(&mut app, end);
            assert!(
                app.manual_queue.is_empty(),
                "Next up is never a drop target, even when Playing next has no rows of its own (dropped at {end:?})"
            );
        }

        drag_to(&mut app, button.center());
        assert_eq!(
            app.manual_queue,
            vec![source_uri],
            "the Queue button still queues the dropped song"
        );
        app.backend.shutdown();
    }

    /// Dropping a song from elsewhere at a specific row in "Playing next"
    /// inserts it there instead of always appending at the end.
    #[test]
    fn dropping_a_new_song_at_a_queue_position_inserts_it_there() {
        let (ctx, mut app) = accessible_app("queue-insert-position");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let row_rect = |name: &str| {
            let prefix = format!("Play {name},");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|text| text.starts_with(&prefix))
                })
                .unwrap_or_else(|| panic!("missing row {name}"))
                .1
                .bounds()
                .unwrap();
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        // Drop on top of "Queued 1": inserts before it.
        let target_row = row_rect("Queued 1");
        let end = egui::pos2(target_row.left() + 130.0, target_row.top() + 1.0);

        let start = egui::pos2(40.0, 755.0);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
        );
        let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
            .expect("dragging the now-playing song should create a payload");
        let dropped_uri = payload.items[0].uri().to_string();

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(
            app.manual_queue,
            vec![
                uris[0].clone(),
                dropped_uri,
                uris[1].clone(),
                uris[2].clone(),
            ],
        );
        app.backend.shutdown();
    }

    /// Without local playback active, neither the Web API nor the engine can
    /// reorder or insert into the live queue, so a drop still just appends,
    /// exactly like before this position-aware behavior existed.
    #[test]
    fn dropping_on_the_queue_without_local_playback_still_just_appends() {
        let (ctx, mut app) = accessible_app("queue-remote-fallback");
        app.show_queue_panel = true;
        let uris = seed_queued_songs(&mut app);
        app.local_ready = false;
        assert!(!app.queue_locally_reorderable());
        for _ in 0..3 {
            frame_events(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button
                    && node
                        .label()
                        .is_some_and(|text| text.starts_with("Play Queued 0,"))
            })
            .unwrap()
            .1
            .bounds()
            .unwrap();
        // Drop right on the first queued row; without local playback this
        // still appends at the end, ignoring the row it landed on.
        let end = egui::pos2(bounds.x0 as f32 + 130.0, bounds.y0 as f32 + 2.0);

        let start = egui::pos2(40.0, 755.0);
        frame_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, -10.0))],
        );
        let payload = egui::DragAndDrop::payload::<DragTrack>(&ctx)
            .expect("dragging the now-playing song should create a payload");
        assert_eq!(payload.from, None, "not reorderable, so not tagged as one");
        let dropped_uri = payload.items[0].uri().to_string();

        frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
        frame_events(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert_eq!(
            app.manual_queue,
            vec![
                uris[0].clone(),
                uris[1].clone(),
                uris[2].clone(),
                dropped_uri
            ],
        );
        app.backend.shutdown();
    }

    /// Pins are pins: dropping a pinned row at the top of the block
    /// reorders the pins themselves, and the rest of the shelf stays in
    /// its automatic order.
    #[test]
    fn dragging_within_the_pinned_block_reorders_it() {
        let root = crate::paths::unique_test_dir("reorder-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.settings.pinned_contexts =
            vec!["tidal:playlist:pl2".into(), "tidal:playlist:pl4".into()];
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }

        // Sweep from the top: the first slot inside the list drops the
        // dragged row above the first pinned playlist. Liked Songs is
        // still pinned but no longer drawn, so it keeps the front of the
        // pins. Where the list begins depends on the loaded fonts, so the
        // sweep does not hardcode it.
        let mut dropped = false;
        for step in 0..40 {
            let pos = egui::pos2(120.0, 100.0 + step as f32 * 10.0);
            egui::DragAndDrop::set_payload(
                &ctx,
                DragEntry {
                    uri: "tidal:playlist:pl4".into(),
                    title: "My New Arrivals".into(),
                    image: None,
                },
            );
            frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            egui::DragAndDrop::clear_payload(&ctx);
            let place = |key: &str| app.settings.pinned_contexts.iter().position(|k| k == key);
            if place("tidal:playlist:pl4") < place("tidal:playlist:pl2") {
                dropped = true;
                break;
            }
        }
        assert!(dropped, "no sweep position landed in the pinned block");
        assert_eq!(
            app.settings.pinned_contexts,
            vec![
                crate::settings::LIKED_SONGS_KEY.to_string(),
                "tidal:playlist:pl4".to_string(),
                "tidal:playlist:pl2".to_string(),
            ],
        );
        assert!(app.settings.sidebar_order.is_empty());
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// Reordering unpinned playlists creates a custom sidebar order.
    #[test]
    fn dropping_between_unpinned_playlists_creates_the_custom_order() {
        let root = crate::paths::unique_test_dir("unpinned-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        assert!(app.settings.pinned_contexts.is_empty());
        assert!(app.settings.sidebar_order.is_empty());
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }

        // Sweep from the top; the first slot inside the list is the one
        // above the first playlist (Liked Songs is no longer drawn here).
        let mut dropped = false;
        for step in 0..40 {
            let pos = egui::pos2(120.0, 100.0 + step as f32 * 10.0);
            egui::DragAndDrop::set_payload(
                &ctx,
                DragEntry {
                    uri: "tidal:playlist:pl4".into(),
                    title: "My New Arrivals".into(),
                    image: None,
                },
            );
            frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            egui::DragAndDrop::clear_payload(&ctx);
            if !app.settings.sidebar_order.is_empty() {
                dropped = true;
                break;
            }
        }
        assert!(dropped, "no sweep position landed in the playlist list");
        let expected: Vec<String> = [4]
            .into_iter()
            .chain((0..PLAYLISTS.len()).filter(|index| *index != 4))
            .map(|index| format!("tidal:playlist:pl{index}"))
            .collect();
        assert_eq!(app.settings.sidebar_order, expected);
        assert_eq!(
            app.settings.library_pins(),
            [crate::settings::LIKED_SONGS_KEY]
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queue_and_player_drags_insert_at_the_chosen_playlist_position() {
        for from_queue in [false, true] {
            for position in [0, 1, 4] {
                let (ctx, mut app) =
                    accessible_app(&format!("insert-drag-{from_queue}-{position}"));
                app.show_queue_panel = from_queue;
                app.open(Page::Playlist("pl1".into()));
                let items = &mut app.playlist_pages.get_mut("pl1").unwrap().items;
                items.items.truncate(4);
                for (index, row) in items.items.iter_mut().enumerate() {
                    if let Some(PlayableItem::Track(track)) = &mut row.item {
                        track.uri = format!("tidal:track:destination{index}");
                        track.id = Some(format!("destination{index}"));
                        track.name = format!("Destination {index}");
                    }
                }
                items.total = Some(4);
                items.next_offset = None;
                items.revision += 1;
                let queue_before: Vec<_> = app
                    .queue
                    .get()
                    .unwrap()
                    .queue
                    .iter()
                    .map(|item| item.uri().to_string())
                    .collect();
                let manual_before = app.manual_queue.clone();
                let playing_before = app.current_track_uri();
                let source_item = if from_queue {
                    app.queue.get().unwrap().queue[0].clone()
                } else {
                    app.queue.get().unwrap().currently_playing.clone().unwrap()
                };
                // TIDAL's header and filter field push the rows down;
                // scroll them clear of the player bar.
                frame_events(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(640.0, 400.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -240.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                for _ in 0..30 {
                    frame_events(&ctx, &mut app, vec![]);
                }
                accessible_frame(&ctx, &mut app, vec![]);
                let tree = accessible_frame(&ctx, &mut app, vec![]);
                let row_rect = |name: &str| {
                    let prefix = format!("Play {name},");
                    let bounds = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| {
                            node.role() == egui::accesskit::Role::Button
                                && node.label().is_some_and(|text| text.starts_with(&prefix))
                        })
                        .unwrap_or_else(|| panic!("missing row {name}"))
                        .1
                        .bounds()
                        .unwrap();
                    egui::Rect::from_min_max(
                        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                    )
                };
                let start = if from_queue {
                    let row = row_rect(source_item.name());
                    egui::pos2(row.left() + 80.0, row.center().y)
                } else {
                    egui::pos2(40.0, 755.0)
                };
                let row = row_rect(&format!("Destination {}", position.min(3)));
                let end = egui::pos2(
                    row.left() + 130.0,
                    if position == 4 {
                        row.bottom() - 1.0
                    } else {
                        row.top() + 1.0
                    },
                );
                frame_events(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(start),
                        egui::Event::PointerButton {
                            pos: start,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                frame_events(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerMoved(start + egui::vec2(15.0, -10.0))],
                );
                let payload =
                    egui::DragAndDrop::payload::<DragTrack>(&ctx).expect("real source drag");
                assert_eq!(payload.items[0].uri(), source_item.uri());
                assert_eq!(payload.from, None);
                frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
                frame_events(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                let rows = &app.playlist_pages["pl1"].items.items;
                assert_eq!(rows.len(), 5, "{from_queue} {position}");
                assert_eq!(rows[position].playable().unwrap().uri(), source_item.uri());
                let sent = app.backend.take_playlist_add_requests();
                assert!(
                    matches!(sent.as_slice(), [crate::backend::ApiRequest::AddToPlaylist { playlist_id, position: Some(at), uris, .. }] if playlist_id == "pl1" && *at == position as u32 && uris == &[source_item.uri()])
                );
                assert_eq!(app.current_track_uri(), playing_before);
                assert_eq!(app.manual_queue, manual_before);
                assert_eq!(
                    app.queue
                        .get()
                        .unwrap()
                        .queue
                        .iter()
                        .map(|item| item.uri().to_string())
                        .collect::<Vec<_>>(),
                    queue_before
                );
                app.backend.shutdown();
            }
        }
    }

    #[test]
    fn playlist_drop_targets_cover_empty_lists_and_preserve_editing_boundaries() {
        for mode in [
            "foreign",
            "empty",
            "multiple",
            "own-multiple",
            "readonly",
            "sorted",
            "filtered",
        ] {
            let (ctx, mut app) = accessible_app(&format!("playlist-drop-target-{mode}"));
            let mut target = track(0);
            target.name = "Drop target".into();
            target.uri = "tidal:track:destination".into();
            let rows = vec![(PlayableItem::Track(target), None, None)];
            if mode == "sorted" {
                app.table_sorts.insert(
                    Page::Playlist("pl1".into()),
                    crate::model::TableSort {
                        column: crate::model::SortColumn::Title,
                        ascending: true,
                    },
                );
            }
            let draw = |app: &mut App, events, empty| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(760.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        crate::ui::collection::table(
                            app,
                            ui,
                            crate::ui::collection::Table {
                                pagination: None,
                                items: if empty { &[] } else { &rows },
                                row_offset: if mode == "empty" { 0 } else { 100 },
                                context: crate::model::RowContext::Context {
                                    uri: "tidal:playlist:pl1".into(),
                                    editable_playlist: (mode != "readonly")
                                        .then(|| ("pl1".into(), None)),
                                },
                                show_album: false,
                                show_cover: true,
                                numbered: true,
                                show_added: false,
                                show_added_by: false,
                                page: Page::Playlist("pl1".into()),
                                loading: false,
                                error: None,
                                can_load_more: false,
                                filter: if mode == "filtered" {
                                    "Drop target"
                                } else {
                                    ""
                                },
                                items_revision: u64::from(empty),
                                volumes: &[],
                            },
                        );
                    },
                );
                output.textures_delta.clear();
                output.platform_output.accesskit_update.unwrap()
            };
            draw(&mut app, vec![], false);
            let tree = draw(&mut app, vec![], false);
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with("Play Drop target,"))
                })
                .unwrap()
                .1
                .bounds()
                .unwrap();
            let end = egui::pos2(
                bounds.x0 as f32 + 140.0,
                if mode == "empty" {
                    400.0
                } else {
                    bounds.y0 as f32 + 2.0
                },
            );
            let dragged =
                app.queue.get().unwrap().queue[..if matches!(mode, "multiple" | "own-multiple") {
                    2
                } else {
                    1
                }]
                    .to_vec();
            let expected: Vec<_> = dragged.iter().map(|item| item.uri().to_string()).collect();
            egui::DragAndDrop::set_payload(
                &ctx,
                DragTrack {
                    title: dragged[0].name().into(),
                    image: None,
                    items: dragged,
                    from: (mode == "foreign").then(|| ("pl2".into(), 5)),
                    source_playlist: match mode {
                        "foreign" => Some("pl2".into()),
                        "own-multiple" => Some("pl1".into()),
                        _ => None,
                    },
                },
            );
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                mode == "empty",
            );
            app.actions.clear();
            draw(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                mode == "empty",
            );
            if matches!(mode, "foreign" | "empty" | "multiple") {
                assert!(
                    matches!(app.actions.as_slice(), [Action::InsertInPlaylist { playlist_id, position, items }] if playlist_id == "pl1" && *position == if mode == "empty" { 0 } else { 100 } && items.iter().map(PlayableItem::uri).collect::<Vec<_>>() == expected),
                    "{mode}: {:?}",
                    app.actions
                );
            } else {
                assert!(
                    !app.actions.iter().any(|action| matches!(
                        action,
                        Action::InsertInPlaylist { .. } | Action::MoveInPlaylist { .. }
                    )),
                    "{mode}"
                );
            }
            app.backend.shutdown();
        }
    }

    #[test]
    fn dragging_at_playlist_edges_reaches_rows_beyond_the_viewport() {
        for upwards in [false, true] {
            let (ctx, mut app) = accessible_app(&format!("drag-scroll-{upwards}"));
            app.open(Page::Playlist("pl1".into()));
            // The page ends at its last song: TIDAL has nothing to suggest
            // here, so no Recommended songs lengthen it.
            app.playlist_pages
                .get_mut("pl1")
                .unwrap()
                .recommendations
                .as_mut()
                .unwrap()
                .total = 0;
            let items = &mut app.playlist_pages.get_mut("pl1").unwrap().items.items;
            let count = items.len();
            assert!(count > 20);
            for (index, name) in [(0, "First song"), (count - 1, "Last song")] {
                if let Some(PlayableItem::Track(track)) = &mut items[index].item {
                    track.name = name.into();
                }
            }
            let from = if upwards { count - 1 } else { 0 };
            let dragged_uri = items[from].playable().unwrap().uri().to_string();
            let mut time = 0.0;
            let mut draw = |app: &mut App, events, offset: Option<f32>| {
                time += 1.0 / 60.0;
                let mut result = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(760.0, 620.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("drag-scroll-playlist")
                            .auto_shrink([false, false]);
                        if let Some(offset) = offset {
                            scroll = scroll.vertical_scroll_offset(offset);
                        }
                        let shown =
                            scroll.show(ui, |ui| crate::ui::collection::playlist(app, ui, "pl1"));
                        result = Some((shown.state.offset.y, shown.inner_rect));
                    },
                );
                output.textures_delta.clear();
                let (offset, viewport) = result.unwrap();
                (
                    offset,
                    viewport,
                    output.platform_output.accesskit_update.unwrap(),
                )
            };
            let row = |tree: &egui::accesskit::TreeUpdate, name: &str| {
                let prefix = format!("Play {name},");
                let bounds = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == egui::accesskit::Role::Button
                            && node.label().is_some_and(|label| label.starts_with(&prefix))
                    })
                    .unwrap_or_else(|| panic!("missing {name}"))
                    .1
                    .bounds()
                    .unwrap();
                egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                )
            };
            draw(
                &mut app,
                vec![],
                Some(if upwards { 100_000.0 } else { 0.0 }),
            );
            let (start, viewport, tree) = draw(&mut app, vec![], None);
            let source = row(&tree, if upwards { "Last song" } else { "First song" });
            let pos = egui::pos2(source.left() + 160.0, source.top() + 18.0);
            assert!(viewport.contains(pos));
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                None,
            );
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(pos + egui::vec2(12.0, 0.0))],
                None,
            );
            assert!(
                egui::DragAndDrop::has_payload_of_type::<DragTrack>(&ctx),
                "the row must start a real drag"
            );
            let edge = egui::pos2(
                pos.x,
                if upwards {
                    viewport.top() + 2.0
                } else {
                    viewport.bottom() - 2.0
                },
            );
            draw(&mut app, vec![egui::Event::PointerMoved(edge)], None);
            for _ in 0..30 {
                draw(&mut app, vec![], None);
            }
            let (moved, _, _) = draw(&mut app, vec![], None);
            assert!(
                if upwards {
                    start - moved > 200.0
                } else {
                    moved - start > 200.0
                },
                "holding a stationary pointer at an edge must scroll"
            );
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(viewport.center())],
                None,
            );
            let (paused, _, _) = draw(&mut app, vec![], None);
            for _ in 0..10 {
                draw(&mut app, vec![], None);
            }
            let (still, _, _) = draw(&mut app, vec![], None);
            assert_eq!(paused, still, "leaving the edge stops scrolling");
            draw(&mut app, vec![egui::Event::PointerMoved(edge)], None);
            for _ in 0..300 {
                draw(&mut app, vec![], None);
            }
            let (_, _, tree) = draw(&mut app, vec![], None);
            let target = row(&tree, if upwards { "First song" } else { "Last song" });
            let pos = egui::pos2(
                pos.x,
                if upwards {
                    target.top() + 1.0
                } else {
                    target.bottom() - 1.0
                },
            );
            draw(&mut app, vec![egui::Event::PointerMoved(pos)], None);
            app.actions.clear();
            draw(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                None,
            );
            assert!(
                matches!(app.actions.as_slice(), [Action::MoveInPlaylist { playlist_id, from: actual_from, to }] if playlist_id == "pl1" && *actual_from == from as u32 && *to == if upwards { 0 } else { count as u32 })
            );
            for action in std::mem::take(&mut app.actions) {
                app.apply(action, &ctx);
            }
            let items = &app.playlist_pages["pl1"].items.items;
            assert_eq!(
                items[if upwards { 0 } else { count - 1 }]
                    .playable()
                    .unwrap()
                    .uri(),
                dragged_uri
            );
            app.backend.shutdown();
        }
    }

    #[test]
    fn dragging_a_library_entry_scrolls_to_offscreen_playlists() {
        use egui::accesskit::Role;
        let (ctx, mut app) = accessible_app("sidebar-drag-scroll");
        app.settings.pinned_contexts.clear();
        let playlists: Vec<_> = (0..60)
            .map(|index| {
                let mut playlist = playlist(1);
                playlist.id = format!("target{index}");
                playlist.uri = format!("tidal:playlist:target{index}");
                playlist.name = format!("Target {index}");
                playlist
            })
            .collect();
        app.settings.sidebar_order = playlists
            .iter()
            .map(|playlist| playlist.uri.clone())
            .collect();
        app.library.playlists = Loadable::Loaded(playlists);
        let original = app.settings.sidebar_order.clone();
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let first = accessible_node(&tree, "Target 0", Role::Button);
        let bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == first)
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let pos = egui::pos2(100.0, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        accessible_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        accessible_frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(pos + egui::vec2(12.0, 0.0))],
        );
        assert!(egui::DragAndDrop::has_payload_of_type::<DragEntry>(&ctx));
        accessible_frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(egui::pos2(
                112.0,
                800.0 - crate::theme::PLAYER_BAR_HEIGHT - 14.0,
            ))],
        );
        for _ in 0..400 {
            accessible_frame(&ctx, &mut app, vec![]);
        }
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Target 59", Role::Button);
        assert_eq!(
            app.settings.sidebar_order, original,
            "scrolling alone must not reorder entries"
        );
        egui::DragAndDrop::clear_payload(&ctx);
        app.backend.shutdown();
    }

    /// Dragging a row within an owned playlist's table moves it through
    /// the same MoveInPlaylist action the menu's move items use: the slot
    /// is TIDAL's insert-before, which the handler mirrors locally
    /// before asking the server.
    #[test]
    fn dragging_a_row_within_a_playlist_reorders_it() {
        let root = crate::paths::unique_test_dir("move-test");
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let ctx = egui::Context::default();
        let waker = crate::backend::Waker::default();
        waker.attach(&ctx);
        let mut app = App::new(
            &waker,
            dirs,
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.attach(&ctx);
        populate(&mut app);
        app.open(Page::Playlist("pl1".into()));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let order = |app: &App| -> Vec<String> {
            app.playlist_pages["pl1"]
                .items
                .items
                .iter()
                .filter_map(|item| item.playable().map(|playable| playable.uri().to_string()))
                .collect()
        };
        let original = order(&app);
        let from = 5usize;
        let held = |from: usize, uri: &str| DragTrack {
            title: "Closer".into(),
            image: None,
            items: vec![PlayableItem::Track(Track {
                uri: uri.to_string(),
                name: "Closer".into(),
                ..Default::default()
            })],
            from: Some(("pl1".into(), from as u32)),
            source_playlist: Some("pl1".into()),
        };

        // Sweep the held row down the page; above the table nothing
        // bites, and the first slot inside it lands the row above its old
        // place. Where the table begins depends on the loaded fonts, so
        // the sweep does not hardcode it.
        let mut landed = None;
        for step in 0..45 {
            let pos = egui::pos2(700.0, 120.0 + step as f32 * 15.0);
            egui::DragAndDrop::set_payload(&ctx, held(from, &original[from]));
            frame_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
            frame_events(
                &ctx,
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            egui::DragAndDrop::clear_payload(&ctx);
            if app.playlist_busy {
                landed = Some(pos);
                break;
            }
        }
        let landed = landed.expect("no sweep position landed inside the table");
        let drop_at = |ctx: &egui::Context, app: &mut App, payload: DragTrack| {
            egui::DragAndDrop::set_payload(ctx, payload);
            frame_events(ctx, app, vec![egui::Event::PointerMoved(landed)]);
            frame_events(
                ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos: landed,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            egui::DragAndDrop::clear_payload(ctx);
        };
        // The handler mirrored the move locally: the dragged row moved
        // up, everything else kept its order.
        let now = order(&app);
        let to = now
            .iter()
            .position(|uri| *uri == original[from])
            .expect("the dragged row vanished");
        assert!(to < from, "the row should have moved up, not to {to}");
        let mut expected = original.clone();
        let moved = expected.remove(from);
        expected.insert(to, moved);
        assert_eq!(now, expected);

        // Dropping the row on the same slot again moves nothing: the slot
        // is insert-before, so a row's own edges are a no-op. A slot sent
        // one row out would move it here.
        app.playlist_busy = false;
        drop_at(&ctx, &mut app, held(to, &expected[to]));
        assert!(!app.playlist_busy, "a row dropped on its own slot moved");
        assert_eq!(order(&app), expected);

        // A sorted view refuses the move: positions on screen no longer
        // match the server's.
        app.table_sorts.insert(
            Page::Playlist("pl1".into()),
            TableSort {
                column: SortColumn::Title,
                ascending: true,
            },
        );
        frame(&ctx, &mut app);
        drop_at(&ctx, &mut app, held(to, &expected[to]));
        assert!(!app.playlist_busy, "a sorted view accepted a move");
        assert_eq!(order(&app), expected);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    /// A song's menu holds TIDAL's entries in TIDAL's order (Add to queue
    /// right under Play next), then Tidalfast's Download, plugin entries last; Go to
    /// album is left out on the album's own page.
    #[test]
    fn a_track_menu_follows_tidals_order() {
        let (ctx, mut app) = accessible_app("track-menu-order");
        app.backend.set_offline(true);
        let item = PlayableItem::Track(track(0));
        let paint = |app: &mut App, context: &RowContext| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| crate::ui::widgets::item_menu(ui, app, &item, Some(context), Some(1)),
            );
            output.textures_delta.clear();
            menu_text(&output)
                .into_iter()
                .map(|(text, _)| match text.as_str() {
                    "Add to My Collection" | "Remove from My Collection" => "My Collection".into(),
                    "Download" | "Remove download" => "Download".into(),
                    _ => text,
                })
                .filter(|text| {
                    [
                        "Play next",
                        "Add to playlist",
                        "Remove from playlist",
                        "My Collection",
                        "Save for Later",
                        "Go to track radio",
                        "Go to album",
                        "Credits",
                        "Go to artist",
                        "Share",
                        "Add to queue",
                        "Download",
                        "Copy link",
                        "Open in TIDAL",
                        "Move up",
                    ]
                    .contains(&text.as_str())
                })
                .collect::<Vec<String>>()
        };
        let playlist = RowContext::Context {
            uri: "tidal:playlist:pl1".into(),
            editable_playlist: Some(("pl1".to_string(), None)),
        };
        assert_eq!(
            paint(&mut app, &playlist),
            [
                "Play next",
                "Add to queue",
                "Add to playlist",
                "Remove from playlist",
                "My Collection",
                "Save for Later",
                "Go to track radio",
                "Go to album",
                "Credits",
                "Go to artist",
                "Share",
                "Download",
            ]
        );
        let album_uri = track(0).album.unwrap().uri;
        let album = RowContext::Context {
            uri: album_uri,
            editable_playlist: None,
        };
        assert!(!paint(&mut app, &album).contains(&"Go to album".to_string()));
        app.backend.shutdown();
    }

    /// The labels of a menu drawn by `draw`, top to bottom, keeping only
    /// those in `keep` (My Collection and Download read the same either way).
    fn menu_labels(
        ctx: &egui::Context,
        app: &mut App,
        keep: &[&str],
        mut draw: impl FnMut(&mut egui::Ui, &mut App),
    ) -> Vec<String> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| draw(ui, app),
        );
        output.textures_delta.clear();
        let mut rows: Vec<(String, egui::Rect)> = menu_text(&output)
            .into_iter()
            .map(|(text, rect)| match text.as_str() {
                "Add to My Collection" | "Remove from My Collection" => {
                    ("My Collection".to_string(), rect)
                }
                "Download" | "Remove download" => ("Download".to_string(), rect),
                _ => (text, rect),
            })
            .filter(|(text, _)| keep.contains(&text.as_str()))
            .collect();
        rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        rows.into_iter().map(|(text, _)| text).collect()
    }

    /// An album's, a mix's and a video's menus hold TIDAL's entries in
    /// TIDAL's order (Add to queue right under Play next), then Tidalfast's
    /// own (Download, Play now and Open on TIDAL for a video, an album's
    /// Credits).
    #[test]
    fn album_mix_and_video_menus_follow_tidals_order() {
        let (ctx, mut app) = accessible_app("album-mix-video-menu-order");
        app.backend.set_offline(true);
        let keep = [
            "Play now",
            "Play next",
            "Add to playlist",
            "My Collection",
            "Save for Later",
            "Go to artist",
            "Share",
            "Add to queue",
            "Download",
            "Credits",
            "Open on TIDAL",
        ];
        let album = album(0);
        let labels = menu_labels(&ctx, &mut app, &keep, |ui, app| {
            crate::ui::menus::album_menu(ui, app, &album)
        });
        assert_eq!(
            labels,
            [
                "Play now",
                "Play next",
                "Add to queue",
                "Add to playlist",
                "My Collection",
                "Save for Later",
                "Go to artist",
                "Share",
                "Download",
                "Credits",
            ]
        );
        let mix = mix(0);
        let labels = menu_labels(&ctx, &mut app, &keep, |ui, app| {
            crate::ui::menus::mix_menu(ui, app, &mix)
        });
        assert_eq!(
            labels,
            [
                "Play next",
                "Add to playlist",
                "My Collection",
                "Share",
                "Download"
            ]
        );
        let mut video = video(0);
        video.artists = vec![artist_ref(0)];
        let mut width = 0.0;
        let labels = menu_labels(&ctx, &mut app, &keep, |ui, app| {
            crate::ui::videos::menu(ui, app, &video, std::slice::from_ref(&video));
            width = ui.min_rect().width();
        });
        assert!(
            (width - crate::ui::menus::MENU_WIDTH).abs() < 1.0,
            "a video's menu is {width} wide, as wide as the others"
        );
        assert_eq!(
            labels,
            [
                "Play next",
                "Add to queue",
                "Add to playlist",
                "My Collection",
                "Save for Later",
                "Go to artist",
                "Share",
                "Play now",
                "Open on TIDAL",
            ]
        );
        // The player bar's menu has no Play next or Add to queue, as TIDAL's;
        // the demo plays this song, so its Credits are offered.
        let item = PlayableItem::Track(track(0));
        let all = [keep.as_slice(), &["Go to track radio", "Go to album"]].concat();
        let labels = menu_labels(&ctx, &mut app, &all, |ui, app| {
            crate::ui::widgets::playing_menu(ui, app, &item, None)
        });
        assert_eq!(
            labels,
            [
                "Add to playlist",
                "My Collection",
                "Save for Later",
                "Go to track radio",
                "Go to album",
                "Credits",
                "Go to artist",
                "Share",
                "Download",
            ]
        );
        app.backend.shutdown();
    }

    #[test]
    fn playlist_menus_remove_by_uri_and_offer_no_moves() {
        let (ctx, mut app) = accessible_app("sorted-remove-menu");
        app.backend.set_offline(true);
        let item = PlayableItem::Track(track(0));
        let uri = item.uri().to_string();
        let editable = Some(("pl1".to_string(), None));
        let unsorted = RowContext::Context {
            uri: "tidal:playlist:pl1".into(),
            editable_playlist: editable.clone(),
        };
        let sorted = RowContext::View {
            uris: Arc::from([uri.clone()]),
            context_uri: "tidal:playlist:pl1".into(),
            editable_playlist: editable.clone(),
        };
        let paint = |ctx: &egui::Context,
                     app: &mut App,
                     item: &PlayableItem,
                     context: &RowContext,
                     events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::ui::widgets::item_menu(ui, app, item, Some(context), Some(1));
                },
            );
            output.textures_delta.clear();
            menu_text(&output)
        };
        // TIDAL's menu removes and never moves (rows are dragged instead),
        // in the playlist's own order and in a sorted view alike.
        for context in [&unsorted, &sorted] {
            let painted = paint(&ctx, &mut app, &item, context, vec![]);
            assert!(
                painted
                    .iter()
                    .any(|(text, _)| text == "Remove from playlist"),
                "the menu lost its removal"
            );
            for label in ["Move up", "Move down"] {
                assert!(
                    !painted.iter().any(|(text, _)| text == label),
                    "the menu must not offer {label}"
                );
            }
        }
        let painted = paint(&ctx, &mut app, &item, &sorted, vec![]);
        // Clicking the sorted removal removes that song by URI.
        let remove = painted
            .iter()
            .find(|(text, _)| text == "Remove from playlist")
            .unwrap()
            .1
            .center();
        app.actions.clear();
        paint(
            &ctx,
            &mut app,
            &item,
            &sorted,
            pointer_click(remove, egui::PointerButton::Primary),
        );
        assert!(
            matches!(app.actions.as_slice(), [Action::RemoveFromPlaylist { playlist_id, uris }] if playlist_id == "pl1" && uris == std::slice::from_ref(&uri))
        );
        // A view without edit rights offers no removal at all.
        let readonly = RowContext::View {
            uris: Arc::from([uri.clone()]),
            context_uri: "tidal:playlist:pl1".into(),
            editable_playlist: None,
        };
        let painted = paint(&ctx, &mut app, &item, &readonly, vec![]);
        assert!(
            !painted
                .iter()
                .any(|(text, _)| text == "Remove from playlist"),
            "a read-only view must not offer removal"
        );
        // The multi-select menu removes the whole selection by URI too.
        let songs = vec![PlayableItem::Track(track(0)), PlayableItem::Track(track(1))];
        let frame = |ctx: &egui::Context,
                     app: &mut App,
                     songs: &[PlayableItem],
                     events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 620.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::ui::widgets::picked_menu(
                        ui,
                        app,
                        songs,
                        Some(&("pl1".to_string(), None)),
                    );
                },
            );
            output.textures_delta.clear();
            menu_text(&output)
        };
        let text = frame(&ctx, &mut app, &songs, vec![]);
        let remove = text
            .iter()
            .find(|(text, _)| text == "Remove from playlist")
            .expect("multi-select menu lost its removal")
            .1
            .center();
        app.actions.clear();
        frame(
            &ctx,
            &mut app,
            &songs,
            pointer_click(remove, egui::PointerButton::Primary),
        );
        let expected: Vec<String> = songs.iter().map(|song| song.uri().to_string()).collect();
        assert!(
            matches!(app.actions.as_slice(), [Action::RemoveFromPlaylist { playlist_id, uris }] if playlist_id == "pl1" && *uris == expected)
        );
        app.backend.shutdown();
    }

    /// The custom order is a setting like any other: it survives the trip
    /// through the settings file, and older files without it stay in the
    /// automatic order.
    #[test]
    fn custom_sidebar_order_round_trips_through_settings() {
        let settings = Settings {
            sidebar_order: vec![
                "tidal:playlist:pl4".to_string(),
                "tidal:playlist:pl0".to_string(),
            ],
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.sidebar_order, settings.sidebar_order);
        let older: Settings = serde_json::from_str("{}").unwrap();
        assert!(older.sidebar_order.is_empty());
    }

    /// The sidebar's Explore row opens TIDAL's Explore page and stays lit
    /// on it.
    #[test]
    fn clicking_explore_in_the_sidebar_opens_the_explore_page() {
        use egui::accesskit::{Action as AccessibleAction, Role};
        let (ctx, mut app) = accessible_app("sidebar-explore-page");
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let explore = accessible_node(&tree, "Explore", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(explore, AccessibleAction::Click, None)],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(app.page(), &crate::ui::explore::page());
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        accessible_node(&tree, "Hip-Hop", Role::Button);
        accessible_node(&tree, "Clean Content", Role::Link);
        app.backend.shutdown();
    }

    /// The badges at the right end of the top bar sit in a right-to-left
    /// layout, which does not wrap and does not clip: anything that does not
    /// fit marches left over the search field. Draw the real bar and check
    /// that it never does.
    #[test]
    fn the_top_bar_badges_never_cover_the_search_field() {
        use crate::updates::{DownloadState, Installation, Kind, Prepared};
        use egui::accesskit::{Action as AccessibleAction, Role};
        // `widgets::search_field` insets its text this far from the pill's
        // left edge, so the pill reaches past the rect the field reports.
        // The badges sit to the left of the field, between the navigation
        // pill and the search pill.
        const FIELD_LEFT_INSET: f32 = 44.0;
        let (ctx, mut app) = accessible_app("topbar-badges");
        app.open(Page::Playlist("pl1".into()));
        for panel in [None, Some("queue"), Some("lyrics")] {
            app.show_queue_panel = panel == Some("queue");
            app.show_lyrics_panel = panel == Some("lyrics");
            for (label, state) in [
                (None, DownloadState::Idle),
                (Some("Update to 9.9.9"), DownloadState::Idle),
                (
                    Some("Downloading update…"),
                    DownloadState::Downloading {
                        received: 1,
                        total: 2,
                    },
                ),
                (
                    Some("Update ready"),
                    DownloadState::Ready(Box::new(Prepared::sample(
                        Installation {
                            executable: "/test/tidalfast".into(),
                            kind: Kind::Portable,
                        },
                        "9.9.9",
                    ))),
                ),
            ] {
                app.update_download = state;
                app.update = label.map(|_| crate::updates::Release {
                    version: "9.9.9".into(),
                    url: "https://example.invalid/releases".into(),
                });
                // Keep the original 760-point coverage without a right panel,
                // and the reported 1080-point size with one. Full-height panel
                // placement at 760 points is checked independently below.
                let widths: &[f32] = if panel.is_some() {
                    &[1080.0, 1120.0, 1200.0, 1280.0, 1440.0, 1600.0, 1920.0]
                } else {
                    &[
                        760.0, 800.0, 860.0, 900.0, 1000.0, 1080.0, 1200.0, 1280.0, 1440.0, 1600.0,
                        1920.0,
                    ]
                };
                for &width in widths {
                    let narrowest = width == widths[0];
                    let mut draw = || {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 620.0),
                                )),
                                ..Default::default()
                            },
                            |ui| app.frame_ui(ui),
                        );
                        output.textures_delta.clear();
                        output
                            .platform_output
                            .accesskit_update
                            .expect("screen-reader tree")
                    };
                    // The first frame settles the new window size.
                    draw();
                    let tree = draw();
                    let badge = |label: &str| {
                        tree.nodes
                            .iter()
                            .find(|(_, node)| {
                                node.role() == Role::Button
                                    && node.label().is_some_and(|name| name.starts_with(label))
                            })
                            .and_then(|(_, node)| node.bounds())
                            .map(|bounds| bounds.x1 as f32)
                    };
                    let field = ctx
                        .read_response(egui::Id::new("global-search"))
                        .expect("the search field")
                        .rect
                        .left()
                        - FIELD_LEFT_INSET;
                    // Collapsed to an icon a badge keeps its label for a screen
                    // reader, so it is found at every width.
                    if let Some(label) = label {
                        let release = badge(label).expect("the update badge");
                        assert!(
                            release <= field,
                            "the update badge covers {} px of the search field at {width} px",
                            release - field
                        );
                        if narrowest {
                            let button = accessible_node(&tree, label, Role::Button);
                            let mut output = ctx.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 620.0),
                                    )),
                                    events: vec![accessible_action(
                                        button,
                                        AccessibleAction::Click,
                                        None,
                                    )],
                                    ..Default::default()
                                },
                                |ui| app.frame_ui(ui),
                            );
                            output.textures_delta.clear();
                            assert!(
                                app.show_update,
                                "the collapsed {label} badge must open the updater"
                            );
                            app.show_update = false;
                        }
                    }
                }
            }
        }
        app.backend.shutdown();
    }
    #[test]
    fn saved_toggles_keep_their_saved_look_while_hovered() {
        // alb0 is saved, its first song is liked, and so is the song in
        // the player bar, so all three save toggles show their on state:
        // TIDAL's hearts, the player bar's too, are filled white.
        type Pick = fn(&egui::accesskit::Rect) -> bool;
        let targets: [(&str, &str, Pick); 3] = [
            ("album header", "Added", |_| true),
            ("song row", "Remove from My Collection", |b| b.y1 < 700.0),
            ("player bar", "Remove from My Collection", |b| b.y0 > 700.0),
        ];
        for theme in ["dark", "light"] {
            for (place, label, pick) in targets {
                let (ctx, mut app) = accessible_app(&format!("saved-hover-{theme}"));
                app.open(Page::Album("alb0".into()));
                app.settings.theme = if theme == "light" {
                    crate::settings::ThemeChoice::Light
                } else {
                    crate::settings::ThemeChoice::Dark
                };
                app.actions.push(Action::SettingsChanged);
                let mut frame = |events| {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1280.0, 800.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| app.frame_ui(ui),
                    );
                    output.textures_delta.clear();
                    output
                };
                frame(vec![]);
                let tree = frame(vec![])
                    .platform_output
                    .accesskit_update
                    .expect("screen-reader tree");
                let bounds = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| {
                        node.label() == Some(label) && node.role() == egui::accesskit::Role::Button
                    })
                    .filter_map(|(_, node)| node.bounds())
                    .find(pick)
                    .unwrap_or_else(|| panic!("missing {place} {label:?}"));
                let centre = egui::pos2(
                    (bounds.x0 + bounds.x1) as f32 / 2.0,
                    (bounds.y0 + bounds.y1) as f32 / 2.0,
                );
                frame(vec![egui::Event::PointerMoved(centre)]);
                let output = frame(vec![egui::Event::PointerMoved(centre)]);
                // Icons are textured rects tinted by their fill.
                let tints: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.brush.is_some()
                                && egui::Rect::from_min_max(
                                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                                )
                                .contains_rect(rect.rect) =>
                        {
                            Some(rect.fill)
                        }
                        _ => None,
                    })
                    .collect();
                // A page header's buttons fade under the pointer, as TIDAL's
                // do; the song and player bar hearts stay filled.
                assert!(
                    place == "album header" || tints.contains(&app.palette.text),
                    "a hovered {place} heart in {theme} must stay filled: {tints:?}"
                );
                assert!(
                    !tints.contains(&app.palette.accent_hover),
                    "{place} in {theme}"
                );
                app.backend.shutdown();
            }
        }
    }

    #[test]
    fn side_panels_keep_their_full_height_beside_the_page_toolbar() {
        for theme in ["dark", "light"] {
            let (ctx, mut app) = accessible_app(&format!("full-height-panels-{theme}"));
            app.open(Page::Playlist("pl1".into()));
            app.settings.theme = if theme == "light" {
                crate::settings::ThemeChoice::Light
            } else {
                crate::settings::ThemeChoice::Dark
            };
            app.actions.push(Action::SettingsChanged);
            // The queue floats over the page now; the lyrics panel docks.
            for panel in ["lyrics"] {
                app.show_queue_panel = panel == "queue";
                app.show_lyrics_panel = panel == "lyrics";
                for width in [760.0, 1080.0, 1600.0] {
                    for _ in 0..3 {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 800.0),
                                )),
                                ..Default::default()
                            },
                            |ui| app.frame_ui(ui),
                        );
                        output.textures_delta.clear();
                    }
                    let rect = |name: &str| {
                        egui::containers::panel::PanelState::load(&ctx, egui::Id::new(name))
                            .expect("the panel was drawn")
                            .outer_rect
                    };
                    let side = rect(&format!("{panel}-panel"));
                    let library = rect("sidebar");
                    let player = rect("player-bar");
                    assert_eq!(side.top(), library.top(), "{panel} at {width} in {theme}");
                    assert_eq!(side.top(), 0.0, "{panel} must start at the window top");
                    assert_eq!(side.bottom(), player.top());
                    let search = ctx.read_response(egui::Id::new("global-search")).unwrap();
                    assert!(side.top() < search.rect.top());
                }
            }
            app.backend.shutdown();
        }
    }

    /// #644: Go to song radio from a list names the radio after the song
    /// even when the song was never played or seen on a radio, instead of
    /// opening a page titled just "Radio" with no cover.
    #[test]
    fn song_radio_from_liked_songs_is_named_after_the_song() {
        let (ctx, mut app) = accessible_app("liked-song-radio");
        let view = crate::ui::collection::liked;
        let song = app
            .library
            .liked
            .items
            .last()
            .expect("a liked song")
            .track
            .clone();
        let seed = song.uri.clone();
        // Only songs that played or came back from a radio are cached.
        app.track_cache.clear();
        view_frame(&ctx, &mut app, vec![], view);
        let text = view_frame(&ctx, &mut app, vec![], view);
        let row = text
            .iter()
            .rev()
            .find(|(text, _)| text == &song.name)
            .unwrap_or_else(|| panic!("{} in Liked Songs", song.name))
            .1
            .center();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(row, egui::PointerButton::Secondary),
            view,
        );
        let text = view_frame(&ctx, &mut app, vec![], view);
        let radio = text
            .iter()
            .find(|(text, _)| text == "Go to track radio")
            .expect("the song's menu")
            .1
            .center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(radio, egui::PointerButton::Primary),
            view,
        );
        let actions = std::mem::take(&mut app.actions);
        assert!(
            matches!(actions.as_slice(), [Action::OpenSongRadio { uri, .. }] if uri == &seed),
            "the menu opens the song's radio: {actions:?}"
        );
        assert!(
            app.track_cache.is_empty(),
            "the menu leaves the cache to the action, applied after drawing"
        );
        for action in actions {
            app.apply(action, &ctx);
        }
        assert_eq!(app.page(), &Page::Radio(seed.clone()));
        assert_eq!(
            app.radio_name(&seed),
            Some(format!("{} Radio", song.name)),
            "the radio is named after the song"
        );
        assert_eq!(
            app.radio_pages[&seed].name,
            Some(format!("{} Radio", song.name)),
            "the page keeps the name"
        );
        app.backend.shutdown();
    }

    /// A song already cached from an album's song list, which comes without
    /// the album, takes the row's album for the radio's cover, while what
    /// the cache already knows is kept.
    #[test]
    fn song_radio_takes_the_rows_album_art_for_a_cached_song() {
        let (ctx, mut app) = accessible_app("song-radio-album-art");
        let song = app
            .library
            .liked
            .items
            .last()
            .expect("a liked song")
            .track
            .clone();
        assert!(
            song.album.as_ref().is_some_and(|a| !a.images.is_empty()),
            "the demo's liked song has album art"
        );
        let seed = song.uri.clone();
        let id = crate::util::uri_id(&seed).expect("a track id").to_owned();
        let mut cached = song.clone();
        cached.album = None;
        cached.name = "Cached name".into();
        app.track_cache.clear();
        app.track_cache.insert(id.clone(), cached);
        app.apply(
            Action::OpenSongRadio {
                uri: seed.clone(),
                track: Box::new(song.clone()),
            },
            &ctx,
        );
        assert_eq!(app.page(), &Page::Radio(seed.clone()));
        assert_eq!(
            app.radio_images(&seed),
            song.album.as_ref().unwrap().images,
            "the cover comes from the row's album"
        );
        assert_eq!(
            app.track_cache[&id].name, "Cached name",
            "the cached song is kept"
        );
        app.backend.shutdown();
    }

    /// A Feed release no page has loaded gets the same menu as any album:
    /// its own cover and artists in the heading, and the full item list.
    #[test]
    fn a_feed_release_menu_has_the_cover_the_artists_and_the_album_items() {
        use crate::api::models::{Feed, FeedEntry, FeedItem, ReleaseKind};
        let (ctx, mut app) = accessible_app("feed-menu");
        let mut release = album(5);
        release.id = "feedrelease".into();
        release.uri = "tidal:album:feedrelease".into();
        release.name = "Fresh Single".into();
        release.album_type = Some("single".into());
        assert!(!app.album_pages.contains_key("feedrelease"));
        let artist = release.artists[0].name.clone();
        let head = crate::ui::menus::MenuHead::album(&release, 38);
        assert!(head.image.is_some(), "the heading carries the row's cover");
        assert_eq!(head.title, "Fresh Single");
        assert_eq!(head.subtitle, artist);
        assert!(!head.artists.is_empty());
        app.feed = Loadable::Loaded(Feed {
            entries: vec![FeedEntry {
                item: FeedItem::Release {
                    album: release,
                    kind: ReleaseKind::Single,
                    explicit: false,
                },
                date: jiff::Zoned::now().date().to_string(),
                seen: true,
            }],
            unseen: 0,
        });
        let page: fn(&mut App, &mut egui::Ui) = crate::ui::feed::show;
        view_frame(&ctx, &mut app, vec![], page);
        let painted = view_frame(&ctx, &mut app, vec![], page);
        let pos = painted
            .iter()
            .find(|(text, _)| text == "Fresh Single")
            .expect("the row title")
            .1
            .center();
        app.actions.clear();
        view_frame(
            &ctx,
            &mut app,
            pointer_click(pos, egui::PointerButton::Secondary),
            page,
        );
        let open = view_frame(&ctx, &mut app, vec![], page);
        let has = |label: &str| open.iter().any(|(text, _)| text == label);
        assert!(
            open.iter()
                .filter(|(text, _)| text == "Fresh Single")
                .count()
                >= 2,
            "the menu heading repeats the title"
        );
        assert!(has(&artist), "the heading's subtitle names the artists");
        for label in [
            "Play now",
            "Play next",
            "Add to queue",
            "Add to My Collection",
            "Download",
            "Go to artist",
            "Credits",
            "Share",
        ] {
            assert!(has(label), "{label}");
        }
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::PrefetchPage(Page::Album(id)) if id == "feedrelease")),
            "the release's songs are read for Add to playlist"
        );
        app.backend.shutdown();
    }

    /// Every menu of an album or playlist the view holds goes through the
    /// shared builders, which take the cover and subtitle from the item.
    #[test]
    fn views_open_album_and_playlist_menus_through_the_shared_builders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        for entry in std::fs::read_dir(dir).expect("src/ui") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|ext| ext != "rs")
                || path.file_name().is_some_and(|name| name == "menus.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source");
            let flat: String = text.split_whitespace().collect();
            for needle in [
                "context_menu_items(ui,app,&album.uri",
                "context_menu_items(ui,app,&playlist.uri",
                "context_menu_items(ui,app,&track.uri",
                "context_menu_items(ui,app,album.uri",
                "context_menu_items(ui,app,&artist.uri",
                "context_menu_items(ui,app,&mix.uri",
                "context_menu_items(ui,app,artist.uri",
                "context_menu_items(ui,app,uri,name,None)",
            ] {
                assert!(
                    !flat.contains(needle),
                    "{} builds a menu without the item's data: {needle}",
                    path.display()
                );
            }
        }
    }

    /// The artist page's Top Tracks "View all" opens its own page (it does
    /// not expand in place); Back returns to the artist page at the scroll
    /// it was left at; the page's Play plays its own named context.
    #[test]
    fn view_all_opens_the_top_tracks_page_and_back_keeps_the_scroll() {
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1280.0, 700.0);
        let (ctx, mut app) = accessible_app("top-tracks-page");
        app.apply(Action::Open(Page::Artist("art0".into())), &ctx);
        for _ in 0..3 {
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
        }
        let find = |text: &[(String, egui::Rect)], label: &str| {
            text.iter()
                .find(|(t, _)| t == label)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{label} is not drawn"))
        };
        let top = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        let heading_y = find(&top, "Top Tracks").top();
        app.demo_scroll = Some(60.0);
        view_frame_sized(&ctx, &mut app, vec![], size, shell);
        app.demo_scroll = None;
        let scrolled = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        let scrolled_y = find(&scrolled, "Top Tracks").top();
        assert!(scrolled_y < heading_y - 30.0, "the page scrolled");
        let view_all = find(&scrolled, "View all").center();
        view_frame_sized(
            &ctx,
            &mut app,
            pointer_click(view_all, egui::PointerButton::Primary),
            size,
            shell,
        );
        let actions = std::mem::take(&mut app.actions);
        assert!(
            matches!(actions.as_slice(), [Action::Open(Page::ArtistTopTracks(id))] if id == "art0"),
            "View all opens the page: {actions:?}"
        );
        for action in actions {
            app.apply(action, &ctx);
        }
        assert_eq!(app.page(), &Page::ArtistTopTracks("art0".into()));
        let mut list = PagedList::default();
        list.absorb(0, page((0..30).map(track).collect()));
        app.artist_top_pages.insert(
            "art0".into(),
            crate::model::ArtistTopPage {
                tracks: list,
                generation: 0,
            },
        );
        for _ in 0..3 {
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
        }
        let listing = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        assert!(
            listing.iter().any(|(t, _)| t == "Top Tracks"),
            "the page is titled"
        );
        let play = find(&listing, "Play").center();
        app.actions.clear();
        view_frame_sized(
            &ctx,
            &mut app,
            pointer_click(play, egui::PointerButton::Primary),
            size,
            shell,
        );
        let actions = std::mem::take(&mut app.actions);
        assert!(
            actions.iter().any(|a| matches!(a,
                Action::PlayFromRow { context: RowContext::View { context_uri, .. }, .. }
                | Action::PlayContext { uri: context_uri, .. }
                if context_uri == "tidal:toptracks:art0")),
            "Play plays the page as its queue context: {actions:?}"
        );
        app.apply(Action::Back, &ctx);
        assert_eq!(app.page(), &Page::Artist("art0".into()));
        for _ in 0..3 {
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
        }
        let back = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        assert!(
            (find(&back, "Top Tracks").top() - scrolled_y).abs() < 1.0,
            "the artist page comes back at its scroll"
        );
        app.backend.shutdown();
    }
    /// Every artist section with more than a row of items has a "View all"
    /// that opens its own page, and Back returns to the artist page.
    #[test]
    fn artist_sections_view_all_open_their_own_pages() {
        use crate::model::ArtistSection;
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1280.0, 3200.0);
        let (ctx, mut app) = accessible_app("artist-sections");
        {
            let art = app.artist_pages.get_mut("art0").unwrap();
            for (filter, range) in [
                (DiscographyFilter::Albums, 0..16),
                (DiscographyFilter::Singles, 0..16),
                (DiscographyFilter::AppearsOn, 0..16),
            ] {
                let mut list = PagedList::default();
                list.absorb(0, page((range).map(album).collect()));
                art.albums.insert(filter.groups().to_string(), list);
            }
            art.related = Loadable::Loaded((1..20).map(artist).collect());
            art.videos.absorb(0, page((0..14).map(video).collect()));
        }
        app.apply(Action::Open(Page::Artist("art0".into())), &ctx);
        for _ in 0..4 {
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
        }
        let painted = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        let mut links: Vec<egui::Rect> = painted
            .iter()
            .filter(|(t, _)| t == "View all")
            .map(|(_, r)| *r)
            .collect();
        links.sort_by(|a, b| a.top().total_cmp(&b.top()));
        let mut opened = Vec::new();
        for link in links {
            app.actions.clear();
            view_frame_sized(
                &ctx,
                &mut app,
                pointer_click(link.center(), egui::PointerButton::Primary),
                size,
                shell,
            );
            for action in std::mem::take(&mut app.actions) {
                if let Action::Open(page) = action {
                    opened.push(page);
                }
            }
        }
        for section in [
            ArtistSection::Albums,
            ArtistSection::Singles,
            ArtistSection::AppearsOn,
            ArtistSection::Videos,
            ArtistSection::Related,
        ] {
            assert!(
                opened.contains(&Page::ArtistSection("art0".into(), section)),
                "{section:?} has a View all: {opened:?}"
            );
        }
        assert!(opened.contains(&Page::ArtistTopTracks("art0".into())));
        for section in [
            ArtistSection::Albums,
            ArtistSection::Singles,
            ArtistSection::Compilations,
            ArtistSection::AppearsOn,
            ArtistSection::Videos,
            ArtistSection::Related,
        ] {
            let target = Page::ArtistSection("art0".into(), section);
            assert_eq!(Page::decode(&target.encode()), Some(target.clone()));
            app.apply(Action::Open(target.clone()), &ctx);
            for _ in 0..3 {
                view_frame_sized(&ctx, &mut app, vec![], size, shell);
            }
            assert_eq!(app.page(), &target);
            app.apply(Action::Back, &ctx);
            assert_eq!(app.page(), &Page::Artist("art0".into()));
        }
        app.backend.shutdown();
    }

    /// The artist page's Playlists and Credits modules: both sections draw
    /// with a "View all" that opens its own page (Back returns), the Credits
    /// table shows BPM and a Camelot key, and a page without the modules
    /// shows neither. The sticky header appears once the page has scrolled.
    #[test]
    fn artist_playlists_and_credits_sections() {
        use crate::api::models::ArtistModules;
        use crate::model::ArtistSection;
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1280.0, 3200.0);
        let (ctx, mut app) = accessible_app("artist-modules");
        app.apply(Action::Open(Page::Artist("art0".into())), &ctx);
        let settle = |app: &mut App| {
            for _ in 0..4 {
                view_frame_sized(&ctx, app, vec![], size, shell);
            }
            view_frame_sized(&ctx, app, vec![], size, shell)
        };
        // No modules: no Credits section and no extra "View all" for them.
        app.artist_pages.get_mut("art0").unwrap().modules =
            Loadable::Loaded(ArtistModules::default());
        let bare = settle(&mut app);
        assert!(!bare.iter().any(|(t, _)| t == "Credits"), "Credits hides");
        let bare_links = bare.iter().filter(|(t, _)| t == "View all").count();
        // A page whose modules failed to load hides them the same.
        app.artist_pages.get_mut("art0").unwrap().modules = Loadable::Failed("no".into());
        let failed = settle(&mut app);
        assert!(!failed.iter().any(|(t, _)| t == "Credits"));

        app.artist_pages.get_mut("art0").unwrap().modules = Loadable::Loaded(ArtistModules {
            playlists: (0..14).map(playlist).collect(),
            playlists_more: Some("pages/data/p".into()),
            credits: (0..8).map(track).collect(),
            credits_more: Some("pages/data/c".into()),
        });
        let painted = settle(&mut app);
        assert!(painted.iter().any(|(t, _)| t == "Credits"));
        let links = painted.iter().filter(|(t, _)| t == "View all").count();
        assert_eq!(links, bare_links + 2, "Playlists and Credits each get one");
        let credits_top = painted
            .iter()
            .find(|(t, _)| t == "Credits")
            .map(|(_, r)| r.top())
            .expect("Credits heading");
        // Track 0 is C minor, which is 5A on the Camelot wheel.
        assert!(
            painted
                .iter()
                .any(|(t, r)| t == "5A" && r.top() > credits_top),
            "a Camelot pill in the Credits table"
        );

        let mut links: Vec<egui::Rect> = painted
            .iter()
            .filter(|(t, _)| t == "View all")
            .map(|(_, r)| *r)
            .collect();
        links.sort_by(|a, b| a.top().total_cmp(&b.top()));
        let mut opened = Vec::new();
        for link in links {
            app.actions.clear();
            view_frame_sized(
                &ctx,
                &mut app,
                pointer_click(link.center(), egui::PointerButton::Primary),
                size,
                shell,
            );
            for action in std::mem::take(&mut app.actions) {
                if let Action::Open(page) = action {
                    opened.push(page);
                }
            }
        }
        for section in [ArtistSection::Playlists, ArtistSection::Credits] {
            let target = Page::ArtistSection("art0".into(), section);
            assert!(opened.contains(&target), "{section:?} opens: {opened:?}");
            assert_eq!(Page::decode(&target.encode()), Some(target.clone()));
            app.apply(Action::Open(target.clone()), &ctx);
            {
                let page = app.artist_pages.get_mut("art0").unwrap();
                match section {
                    ArtistSection::Playlists => {
                        page.playlists_all = Loadable::Loaded((0..14).map(playlist).collect())
                    }
                    _ => page.credits_all = Loadable::Loaded((0..8).map(track).collect()),
                }
            }
            for _ in 0..3 {
                view_frame_sized(&ctx, &mut app, vec![], size, shell);
            }
            assert_eq!(app.page(), &target);
            app.apply(Action::Back, &ctx);
            assert_eq!(app.page(), &Page::Artist("art0".into()));
        }

        // The sticky header keeps a Play control on screen once scrolled.
        settle(&mut app);
        app.demo_scroll = Some(700.0);
        view_frame_sized(&ctx, &mut app, vec![], size, shell);
        let scrolled = view_frame_sized(&ctx, &mut app, vec![], size, shell);
        app.demo_scroll = None;
        assert!(
            scrolled.iter().any(|(t, r)| t == "Play" && r.top() < 120.0),
            "the sticky Play sits at the top"
        );
        app.backend.shutdown();
    }
    /// A multi-disc album lists each volume under its own "Volume N"
    /// heading and column header, numbered from 1 again; a single disc has
    /// no heading.
    #[test]
    fn a_multi_disc_album_lists_each_volume_under_its_own_heading() {
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1280.0, 1400.0);
        for discs in [1u32, 2] {
            let (ctx, mut app) = accessible_app(&format!("volumes-{discs}"));
            {
                let tracks = &mut app.album_pages.get_mut("alb0").unwrap().tracks;
                tracks.items.truncate(12);
                for (index, track) in tracks.items.iter_mut().enumerate() {
                    track.disc_number = Some(if discs == 2 && index >= 6 { 2 } else { 1 });
                }
                tracks.total = Some(12);
                tracks.next_offset = None;
                tracks.revision += 1;
            }
            app.open(Page::Album("alb0".into()));
            for _ in 0..3 {
                view_frame_sized(&ctx, &mut app, vec![], size, shell);
            }
            let painted = view_frame_sized(&ctx, &mut app, vec![], size, shell);
            let count = |label: &str| painted.iter().filter(|(t, _)| t == label).count();
            if discs == 1 {
                assert_eq!(count("Volume 1"), 0, "a single disc has no heading");
                assert_eq!(count("12"), 1, "numbered straight through");
            } else {
                let at = |label: &str| {
                    painted
                        .iter()
                        .find(|(t, _)| t == label)
                        .unwrap_or_else(|| panic!("{label} is not drawn"))
                        .1
                };
                assert!(at("Volume 1").top() < at("Volume 2").top());
                assert_eq!(count("TITLE"), 2, "each volume repeats the header");
                assert_eq!(count("6"), 2, "both volumes count up to 6");
                assert_eq!(count("7"), 0, "the second volume starts at 1 again");
                assert_eq!(count("12"), 0);
            }
            app.backend.shutdown();
        }
    }

    /// Once an album's or a playlist's header and controls have scrolled
    /// away, a bar with the title and the controls is pinned at the top of
    /// the page; before that there is none.
    #[test]
    fn a_pinned_bar_appears_once_the_header_has_scrolled_away() {
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1280.0, 800.0);
        for (page, title) in [
            (Page::Album("alb0".into()), album(0).name),
            (Page::Playlist("pl1".into()), playlist(1).name),
        ] {
            let (ctx, mut app) = accessible_app("pinned-bar");
            app.open(page.clone());
            for _ in 0..3 {
                view_frame_sized(&ctx, &mut app, vec![], size, shell);
            }
            let pinned = |painted: &[(String, egui::Rect)]| {
                let near_top = |label: &str| {
                    painted
                        .iter()
                        .any(|(t, r)| t == label && r.top() > 0.0 && r.top() < 160.0)
                };
                (near_top("Play") || near_top("Pause")) && near_top("Shuffle") && near_top(&title)
            };
            let top = view_frame_sized(&ctx, &mut app, vec![], size, shell);
            assert!(!pinned(&top), "{page:?}: no bar at the top");
            // The controls are still in view.
            app.demo_scroll = Some(200.0);
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
            let partway = view_frame_sized(&ctx, &mut app, vec![], size, shell);
            assert!(
                !pinned(&partway),
                "{page:?}: no bar while the controls show"
            );
            app.demo_scroll = Some(700.0);
            view_frame_sized(&ctx, &mut app, vec![], size, shell);
            let scrolled = view_frame_sized(&ctx, &mut app, vec![], size, shell);
            let near: Vec<_> = scrolled
                .iter()
                .filter(|(_, r)| r.top() > 0.0 && r.top() < 160.0)
                .map(|(t, _)| t.as_str())
                .collect();
            assert!(
                pinned(&scrolled),
                "{page:?}: the bar is pinned once scrolled: {near:?}"
            );
            app.demo_scroll = None;
            app.backend.shutdown();
        }
    }

    /// TIDAL's pinned bar on an album lists Credits between Add and Share,
    /// and a click opens the album's Credits window; a playlist's bar has
    /// no Credits. At every width the buttons stay clear of the title.
    #[test]
    fn the_pinned_bar_on_an_album_has_credits_between_add_and_share() {
        fn shell(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::show(app, ui);
        }
        let size = egui::vec2(1400.0, 900.0);
        for (page, credits) in [
            (Page::Album("alb0".into()), true),
            (Page::Playlist("pl1".into()), false),
        ] {
            let (ctx, mut app) = accessible_app("pinned-credits");
            app.open(page.clone());
            app.demo_scroll = Some(700.0);
            for _ in 0..3 {
                view_frame_sized(&ctx, &mut app, vec![], size, shell);
            }
            let painted = view_frame_sized(&ctx, &mut app, vec![], size, shell);
            let in_bar = |label: &str| {
                painted
                    .iter()
                    .find(|(t, r)| t == label && r.top() > 0.0 && r.top() < 160.0)
                    .map(|(_, r)| *r)
            };
            assert_eq!(in_bar("Credits").is_some(), credits, "{page:?}");
            if credits {
                let (added, credits_at, share) = (
                    in_bar("Added").expect("Added"),
                    in_bar("Credits").unwrap(),
                    in_bar("Share").expect("Share"),
                );
                assert!(added.center().x < credits_at.center().x, "after Add");
                assert!(credits_at.center().x < share.center().x, "before Share");
                view_frame_sized(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(credits_at.center()),
                        egui::Event::PointerButton {
                            pos: credits_at.center(),
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Default::default(),
                        },
                    ],
                    size,
                    shell,
                );
                view_frame_sized(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerButton {
                        pos: credits_at.center(),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }],
                    size,
                    shell,
                );
                assert!(
                    app.actions
                        .iter()
                        .any(|a| matches!(a, Action::ShowAlbumCredits(id) if id == "alb0"))
                        || matches!(app.page(), Page::AlbumCredits(id) if id == "alb0"),
                    "the bar's Credits opens the album credits"
                );
            }
            app.demo_scroll = None;
            app.backend.shutdown();
        }
    }

    /// A pinned bar's buttons never reach into the title's room, whatever
    /// the width and the button set; a title that fits whole is not cut
    /// while closing the buttons up (and then dropping Share) would leave it
    /// the room.
    #[test]
    fn the_pinned_bar_gives_a_long_title_the_room_that_is_free() {
        use crate::ui::sticky::{COVER, fit};
        let title = 235.0;
        // 520 points, six buttons: they close up and Share goes, and the
        // whole title shows; it used to be cut to 168 points.
        let fitted = fit(520.0, 6, true, title);
        let room =
            520.0 - COVER - 24.0 - (6.0 - f32::from(u8::from(!fitted.share))) * fitted.button_width;
        assert!(fitted.text && room >= title, "{fitted:?} leaves {room}");
        // A title too long for any width: the buttons close up all the way,
        // so it gets every free point.
        let long = fit(440.0, 6, true, 600.0);
        assert!(
            long.text && (long.button_width - 36.0).abs() < 0.1,
            "{long:?}"
        );
        // Room for the buttons as they are: they stay as wide as before.
        let wide = fit(620.0, 6, true, title);
        assert!(wide.share && wide.button_width >= 44.0, "{wide:?}");
        // Never wider than the bar, any width, any button count, any title.
        for avail in (280..1400).step_by(20) {
            for buttons in 4..9 {
                for title in [0.0, 120.0, 235.0, 400.0] {
                    let avail = avail as f32;
                    let fit = fit(avail, buttons, true, title);
                    let shown = buttons - usize::from(!fit.share);
                    let used = shown as f32 * fit.button_width;
                    let cover = COVER + 16.0;
                    let text = if fit.text { 140.0 } else { 0.0 };
                    assert!(
                        used + cover + text <= avail + 0.5 || fit.button_width <= 44.0 && !fit.text,
                        "{avail} {buttons} {title}: {fit:?}"
                    );
                }
            }
        }
    }

    /// In a cell too narrow for every artist, the names that fit whole are
    /// kept and an ellipsis ends the line; no name is cut after a comma.
    #[test]
    fn a_narrow_artist_cell_keeps_whole_names_and_ends_in_an_ellipsis() {
        let (ctx, mut app) = accessible_app("artist-cell");
        let person = |name: &str| ArtistRef {
            id: Some(name.to_lowercase()),
            name: name.to_string(),
            ..Default::default()
        };
        let artists = [person("Khruangbin"), person("Nils Frahm"), person("Bonobo")];
        for (width, expected) in [
            (
                400.0,
                vec!["Khruangbin", ", ", "Nils Frahm", ", ", "Bonobo"],
            ),
            (120.0, vec!["Khruangbin", ", \u{2026}"]),
            (60.0, vec!["Khruangbin"]),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.allocate_ui(egui::vec2(width, 24.0), |ui| {
                        ui.set_clip_rect(ui.max_rect());
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            crate::ui::widgets::artist_links_clipped(
                                ui,
                                &mut app,
                                &artists,
                                crate::theme::semibold(14.0),
                                egui::Color32::WHITE,
                            );
                        });
                    });
                },
            );
            output.textures_delta.clear();
            let mut runs = Vec::new();
            fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::epaint::Shape::Text(t) => out.push((
                        t.galley.job.text.clone(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            for shape in &output.shapes {
                walk(&shape.shape, &mut runs);
            }
            let painted: Vec<&str> = runs.iter().map(|(text, _)| text.as_str()).collect();
            assert_eq!(painted, expected, "{width} points wide");
            // Each run starts where the one before ended: no gap before
            // the ellipsis.
            for pair in runs.windows(2) {
                assert!(
                    (pair[1].1.left() - pair[0].1.right()).abs() < 0.6,
                    "{width}: {pair:?}"
                );
            }
        }
        app.backend.shutdown();
    }

    /// The pinned bar fades in over a fraction of a second and then asks for
    /// no repaint of its own: a settled page with the bar shown waits about as
    /// long as the same page without it.
    #[cfg(feature = "demo")]
    #[test]
    fn the_pinned_bar_asks_for_no_repaints_once_it_is_shown() {
        let settled = |scroll: Option<f32>| {
            let (ctx, mut app) = accessible_app("pinned-bar-idle");
            app.motion.force(false);
            crate::motion::apply(&ctx, false);
            // The demo's song plays on, and the player bar asks for a frame
            // at its progress bar's next visible step: a wait anywhere from
            // nothing to half a second, by where the wall clock has taken
            // the song. Paused, the page's own timers are what is left.
            app.remote.as_mut().unwrap().state.is_playing = false;
            app.open(Page::Album("alb0".into()));
            app.demo_scroll = scroll;
            let mut delay = std::time::Duration::ZERO;
            for frame in 0..60 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        time: Some(f64::from(frame) * 0.05),
                        ..Default::default()
                    },
                    |ui| crate::ui::show(&mut app, ui),
                );
                output.textures_delta.clear();
                delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            }
            app.backend.shutdown();
            delay
        };
        let without = settled(None);
        let with = settled(Some(700.0));
        // Wall-clock timers of the page itself differ a little between
        // runs; a bar that animates forever would ask for the next frame
        // within milliseconds.
        let slack = std::time::Duration::from_millis(150);
        assert!(
            with > std::time::Duration::from_millis(250) && with >= without.saturating_sub(slack),
            "the bar adds a repaint timer: {with:?} against {without:?}"
        );
    }

    /// AltPlay's library page: a plugin puts its own page first in `main`
    /// and hides the page it stands in for (`.mainContent`'s children).
    /// Its page is drawn across `main`, and while it hides the page, it is
    /// the page; without it the page shows as before.
    #[test]
    fn a_plugin_page_first_in_main_stands_in_for_the_page() {
        use crate::luna::dom::{DomOverlay, HostOverride, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let texts = |ctx: &egui::Context, app: &mut App| {
            let mut last = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                last = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            last
        };
        let page = VNode {
            id: 1,
            tag: "div".into(),
            attrs: vec![("id".into(), "altplay-page".into())],
            children: vec![VNode {
                id: 2,
                tag: "#text".into(),
                text: Some("Jellyfin library".into()),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let mut overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::MAIN.into(),
                position: Position::Prepend,
                nodes: vec![page],
            }],
            ..DomOverlay::default()
        };
        overlay.hosts.insert(
            keys::PAGE.into(),
            HostOverride {
                style: Style {
                    display: Some("none".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        let (ctx, mut app) = accessible_app("plugin-page");
        app.open(Page::Playlist("pl1".into()));
        plugin_dom::test_overlay(None);
        let native = texts(&ctx, &mut app);
        // The playlist header's own button (the sidebar lists the name too).
        let title = "Shuffle".to_string();
        assert!(native.iter().any(|t| t == &title), "{native:?}");
        plugin_dom::test_overlay(Some(overlay));
        let swapped = texts(&ctx, &mut app);
        plugin_dom::test_overlay(None);
        assert!(
            swapped.iter().any(|t| t == "Jellyfin library"),
            "the plugin's page shows: {swapped:?}"
        );
        assert!(
            !swapped.iter().any(|t| t == &title),
            "the page it hides does not: {swapped:?}"
        );
        app.backend.shutdown();
    }

    /// radiant-lyrics hides TIDAL's lines and appends its own word-by-word
    /// lines to the lines' container (`lyrics-lines`); that slot was never
    /// drawn, so the lyrics view went blank once it took over.
    #[cfg(feature = "demo")]
    #[test]
    fn a_plugins_lines_appended_to_the_lyrics_lines_are_drawn() {
        use crate::luna::dom::{DomOverlay, HostOverride, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let surface = "lyrics-fullscreen-view";
        let (ctx, mut app) = accessible_app(surface);
        apply_flags(&mut app, None, Some(surface));
        let texts = |ctx: &egui::Context, app: &mut App| {
            let mut last = Vec::new();
            for frame in 0..30 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        time: Some(f64::from(frame) / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1600.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| crate::ui::show(app, ui),
                );
                out.textures_delta.clear();
                last = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            last
        };
        let mut overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::LYRICS_LINES.into(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    attrs: vec![("class".into(), "rl-wbw-container".into())],
                    children: vec![VNode {
                        id: 2,
                        tag: "#text".into(),
                        text: Some("Its own word-by-word line".into()),
                        ..VNode::default()
                    }],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        };
        overlay.hosts.insert(
            keys::LYRICS.into(),
            HostOverride {
                style: Style {
                    display: Some("none".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        plugin_dom::test_overlay(None);
        let native = texts(&ctx, &mut app);
        assert!(
            native.iter().any(|t| t == "Somewhere past the county line"),
            "{native:?}"
        );
        plugin_dom::test_overlay(Some(overlay));
        let taken_over = texts(&ctx, &mut app);
        plugin_dom::test_overlay(None);
        assert!(
            taken_over.iter().any(|t| t == "Its own word-by-word line"),
            "the plugin's line shows: {taken_over:?}"
        );
        assert!(
            !taken_over
                .iter()
                .any(|t| t == "Somewhere past the county line"),
            "the lines it hides do not: {taken_over:?}"
        );
        app.backend.shutdown();
    }

    /// A theme that styles TIDAL's header pills (the cotton candy family,
    /// Catppuccin Mocha Blue) paints the playlist header's Play and Shuffle
    /// in its colours; Dark and Light without a theme draw them exactly as
    /// before, with Luna off or on.
    #[test]
    fn header_pills_take_the_themes_colours_only_while_it_sets_them() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let shapes = |ctx: &egui::Context, app: &mut App| {
            let mut last = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                last = out.shapes;
            }
            last
        };
        let pink = egui::Color32::from_rgb(0xff, 0xa8, 0xed);
        let blue = egui::Color32::from_rgb(0xa8, 0xee, 0xff);
        let ink = egui::Color32::from_rgb(0x01, 0x00, 0x06);
        let overlay = |version: u64, themed: bool| {
            let mut o = DomOverlay {
                version,
                ..DomOverlay::default()
            };
            if themed {
                let style = |f: &dyn Fn(&mut Style)| {
                    let mut s = Style::default();
                    f(&mut s);
                    HostOverride {
                        style: s,
                        ..HostOverride::default()
                    }
                };
                o.hosts.insert(
                    keys::PAGE_PLAY.into(),
                    style(&|s| s.background_color = Some("#a8eeff".into())),
                );
                o.hosts.insert(
                    keys::PAGE_SHUFFLE.into(),
                    style(&|s| s.background_color = Some("#ffa8ed".into())),
                );
                for key in [keys::PAGE_PLAY_LABEL, keys::PAGE_SHUFFLE_LABEL] {
                    o.hosts
                        .insert(key.into(), style(&|s| s.color = Some("#010006".into())));
                }
            }
            o
        };
        let fills = |shapes: &[egui::epaint::ClippedShape], color: egui::Color32| {
            shapes
                .iter()
                .filter(|c| {
                    matches!(&c.shape, egui::Shape::Rect(r)
                        if r.fill == color && r.rect.height() > 40.0 && r.rect.height() < 50.0)
                })
                .count()
        };
        let text_in = |shapes: &[egui::epaint::ClippedShape], color: egui::Color32| {
            shapes.iter().any(|c| {
                matches!(&c.shape, egui::Shape::Text(t)
                    if t.fallback_color == color && matches!(t.galley.text(), "Pause" | "Play" | "Shuffle"))
            })
        };
        for light in [false, true] {
            let (ctx, mut app) = accessible_app("header-pills");
            if let Some(remote) = app.remote.as_mut() {
                remote.state.is_playing = false;
            }
            app.settings.theme = if light {
                crate::settings::ThemeChoice::Light
            } else {
                crate::settings::ThemeChoice::Dark
            };
            app.actions.push(Action::SettingsChanged);
            app.open(Page::Playlist("pl1".into()));
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                out.textures_delta.clear();
            }
            plugin_dom::test_overlay(None);
            let off = shapes(&ctx, &mut app);
            assert_eq!(fills(&off, egui::Color32::WHITE), 1, "TIDAL's white Play");
            plugin_dom::test_overlay(Some(overlay(1, false)));
            let on = shapes(&ctx, &mut app);
            assert!(
                on == off,
                "Luna on without a theme draws the header as Luna off: light={light}"
            );
            plugin_dom::test_overlay(Some(overlay(2, true)));
            let themed = shapes(&ctx, &mut app);
            assert_eq!(fills(&themed, blue), 1, "the theme's Play: light={light}");
            assert_eq!(
                fills(&themed, pink),
                1,
                "the theme's Shuffle: light={light}"
            );
            assert_eq!(fills(&themed, egui::Color32::WHITE), 0);
            assert!(text_in(&themed, ink), "the theme's label colour");
            // Meteor Night's `* { color: white }` reaches the labels, which
            // TIDAL's own CSS leaves to inherit, but not the pills: TIDAL's
            // white Play stays, with white words.
            let mut catch_all = overlay(3, false);
            for key in [keys::PAGE_PLAY_LABEL, keys::PAGE_SHUFFLE_LABEL] {
                catch_all.hosts.insert(
                    key.into(),
                    HostOverride {
                        style: Style {
                            color: Some("#ffffff".into()),
                            ..Style::default()
                        },
                        ..HostOverride::default()
                    },
                );
            }
            plugin_dom::test_overlay(Some(catch_all));
            let caught = shapes(&ctx, &mut app);
            assert_eq!(
                fills(&caught, egui::Color32::WHITE),
                1,
                "TIDAL's white Play stays: light={light}"
            );
            assert!(
                caught != on,
                "the labels take the catch-all's white: light={light}"
            );
            plugin_dom::test_overlay(Some(overlay(4, false)));
            assert!(
                shapes(&ctx, &mut app) == on,
                "taking the theme off: light={light}"
            );
            plugin_dom::test_overlay(None);
            app.backend.shutdown();
        }
    }

    /// Dark and Light without a theme keep the shell exactly as it is: with
    /// Luna off or on without a theme the player bar keeps its native size
    /// and place. Only while a theme floats the bar and sidebar
    /// (Snusehunden's Tidaluna-Theme) do they move, and taking the theme
    /// off puts every shape the window draws back as it was.
    #[test]
    fn shell_geometry_changes_only_while_a_theme_sets_it() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let shapes = |ctx: &egui::Context, app: &mut App| {
            let mut last = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                last = out.shapes;
            }
            last
        };
        let bar = |ctx: &egui::Context| {
            egui::containers::panel::PanelState::load(ctx, egui::Id::new("player-bar"))
                .expect("the player bar")
                .outer_rect
        };
        let overlay = |version: u64, themed: bool| {
            let mut o = DomOverlay {
                version,
                ..DomOverlay::default()
            };
            if themed {
                let style = |f: fn(&mut Style)| {
                    let mut s = Style::default();
                    f(&mut s);
                    HostOverride {
                        style: s,
                        ..HostOverride::default()
                    }
                };
                o.hosts.insert(
                    keys::FOOTER.into(),
                    style(|s| {
                        s.position = Some("fixed".into());
                        s.bottom = Some("2vh".into());
                        s.width = Some("98vw".into());
                        s.border_radius = Some("15px".into());
                    }),
                );
                o.hosts.insert(
                    keys::SIDEBAR.into(),
                    style(|s| {
                        s.top = Some("2vh".into());
                        s.border_radius = Some("0 20px 20px 0".into());
                    }),
                );
            }
            o
        };
        for light in [false, true] {
            let (ctx, mut app) = accessible_app("theme-geometry");
            // A still frame: the progress bar does not move between frames.
            if let Some(remote) = app.remote.as_mut() {
                remote.state.is_playing = false;
            }
            app.settings.theme = if light {
                crate::settings::ThemeChoice::Light
            } else {
                crate::settings::ThemeChoice::Dark
            };
            app.actions.push(Action::SettingsChanged);
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| app.frame_ui(ui));
                out.textures_delta.clear();
            }
            assert_eq!(app.palette.dark, !light);
            plugin_dom::test_overlay(None);
            let off = shapes(&ctx, &mut app);
            assert_eq!(
                bar(&ctx),
                egui::Rect::from_min_max(
                    egui::pos2(0.0, size.y - crate::theme::PLAYER_BAR_HEIGHT),
                    size.to_pos2()
                ),
                "the native bar spans the window's foot"
            );
            // Luna on, no theme: the native geometry too.
            plugin_dom::test_overlay(Some(overlay(1, false)));
            assert!(plugin_dom::theme_layout(&ctx).is_none());
            let on = shapes(&ctx, &mut app);
            assert_eq!(bar(&ctx).height(), crate::theme::PLAYER_BAR_HEIGHT);
            assert!(
                on == off,
                "Luna on without a theme draws as Luna off: light={light}"
            );
            plugin_dom::test_overlay(Some(overlay(2, true)));
            let layout = plugin_dom::theme_layout(&ctx).expect("the theme floats the shell");
            assert_eq!(layout.player_bar.unwrap().gaps.bottom, 18);
            let themed = shapes(&ctx, &mut app);
            assert!(themed != off, "the theme moves the shell");
            assert_eq!(bar(&ctx).height(), crate::theme::PLAYER_BAR_HEIGHT + 18.0);
            plugin_dom::test_overlay(Some(overlay(3, false)));
            assert!(
                shapes(&ctx, &mut app) == on,
                "taking the theme off: light={light}"
            );
            plugin_dom::test_overlay(None);
            assert!(
                shapes(&ctx, &mut app) == off,
                "Luna off again: light={light}"
            );
            app.backend.shutdown();
        }
    }

    /// CatJam appends its cat (52 points tall, `position: relative; top:
    /// -52px`) to the player bar's cover figure. In TIDAL it flows in under
    /// the cover picture and is lifted back over it: the cat covers the
    /// cover, inside the bar. It stood above the bar, then (anchored at the
    /// cover's foot) was cut to the strip that had been inside the window.
    #[test]
    fn an_element_appended_to_the_bar_cover_and_lifted_lies_over_the_cover() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let cat = egui::Color32::from_rgb(7, 8, 9);
        let (ctx, mut app) = accessible_app("catjam-cover");
        super::play_here(&mut app);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::COVER.into(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 1,
                    tag: "div".into(),
                    style: Style {
                        position: Some("relative".into()),
                        top: Some("-52px".into()),
                        left: Some("-8px".into()),
                        margin: Some("0 0 0 8px".into()),
                        height: Some("52px".into()),
                        width: Some("68px".into()),
                        background_color: Some("rgb(7, 8, 9)".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(&mut app, ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        fn walk(
            shape: &egui::Shape,
            clip: egui::Rect,
            fill: egui::Color32,
            out: &mut Vec<egui::Rect>,
        ) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, fill, out)),
                egui::Shape::Rect(r) if r.fill == fill => out.push(r.rect.intersect(clip)),
                _ => {}
            }
        }
        let mut seen = Vec::new();
        for s in &shapes {
            walk(&s.shape, s.clip_rect, cat, &mut seen);
        }
        let bar_top = size.y - crate::theme::PLAYER_BAR_HEIGHT;
        assert_eq!(seen.len(), 1, "{seen:?}");
        let r = seen[0];
        assert!(
            r.top() > bar_top && r.bottom() < size.y,
            "inside the bar: {r:?}"
        );
        assert!(
            (r.height() - 52.0).abs() < 1.0,
            "the whole cat shows: {r:?}"
        );
        assert!(r.left() < 40.0, "over the cover: {r:?}");
        app.backend.shutdown();
    }

    /// A theme's cards behind the player bar's title and artists
    /// (`._trackContent_`) and its right-hand buttons
    /// (`._utilityButtons_`), the cotton candy family's light rounded
    /// boxes, are painted round them, padding included, and the quality
    /// pill stays outside the buttons' card. No theme, no card.
    #[test]
    fn theme_cards_sit_behind_the_player_bars_title_and_buttons() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let title_card = egui::Color32::from_rgb(1, 2, 3);
        let buttons_card = egui::Color32::from_rgb(4, 5, 6);
        fn walk(shape: &egui::Shape, fill: egui::Color32, found: &mut Vec<(Rect, CornerRadius)>) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, fill, found)),
                egui::Shape::Rect(r) if r.fill == fill => found.push((r.rect, r.corner_radius)),
                _ => {}
            }
        }
        use egui::{CornerRadius, Rect};
        let filled = |shapes: &[egui::epaint::ClippedShape], fill: egui::Color32| {
            let mut found = Vec::new();
            for s in shapes {
                walk(&s.shape, fill, &mut found);
            }
            found
        };
        let (ctx, mut app) = accessible_app("theme-cards");
        super::play_here(&mut app);
        app.local.playback = crate::player::Playback::Paused;
        app.local.format = Some(crate::tidal_engine::StreamFormat {
            codec: "flac".into(),
            sample_rate: 44_100,
            bit_depth: Some(16),
            channels: 2,
            quality: Some(crate::tidal_engine::Quality::Lossless),
            ..Default::default()
        });
        let mut shapes = || {
            let mut last = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(&mut app, ui));
                out.textures_delta.clear();
                last = out.shapes;
            }
            last
        };
        let card = |fill: egui::Color32| {
            let s = Style {
                background_color: Some(format!("rgb({}, {}, {})", fill.r(), fill.g(), fill.b())),
                border_radius: Some(".67em".into()),
                padding: Some("4px".into()),
                ..Style::default()
            };
            HostOverride {
                style: s,
                ..HostOverride::default()
            }
        };
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        plugin_dom::test_overlay(Some(o.clone()));
        let plain = shapes();
        assert!(filled(&plain, title_card).is_empty());
        o.version = 2;
        o.hosts.insert(keys::TRACK_CONTENT.into(), card(title_card));
        o.hosts
            .insert(keys::UTILITY_BUTTONS.into(), card(buttons_card));
        plugin_dom::test_overlay(Some(o));
        let themed = shapes();
        let bar_top = size.y - crate::theme::PLAYER_BAR_HEIGHT;
        let title = filled(&themed, title_card);
        assert_eq!(title.len(), 1, "one card behind the title");
        let (rect, radius) = title[0];
        assert_eq!(radius, egui::CornerRadius::same(9));
        assert!(
            rect.top() > bar_top && rect.left() > 60.0 && rect.width() > 40.0,
            "{rect:?}"
        );
        let buttons = filled(&themed, buttons_card);
        assert_eq!(buttons.len(), 1, "one card behind the buttons");
        let (rect, _) = buttons[0];
        assert!(
            rect.top() > bar_top && rect.left() > size.x / 2.0,
            "{rect:?}"
        );
        assert!(
            rect.right() < size.x - 121.0,
            "the pill stays outside the card: {rect:?}"
        );
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
    }

    /// Lunallena's header is a tab over the page's top, 30vw wide and
    /// centred, pulled 80 up so that only its foot shows, with its controls
    /// hidden until the pointer is on it; its sidebar slides 47 out to the
    /// left. Without the theme neither moves.
    #[test]
    fn a_themes_header_tab_and_sliding_sidebar_are_drawn_only_while_it_is_applied() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        fn texts(shape: &egui::Shape, word: &str, found: &mut Vec<egui::Pos2>) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, word, found)),
                egui::Shape::Text(t) if t.galley.text() == word => found.push(t.pos),
                _ => {}
            }
        }
        let explore = |shapes: &[egui::epaint::ClippedShape]| {
            let mut found = Vec::new();
            for s in shapes {
                texts(&s.shape, "Explore", &mut found);
            }
            found.first().copied().expect("the Explore row")
        };
        let tab_id = egui::Id::new("theme-header-tab");
        let (ctx, mut app) = accessible_app("theme-header-tab");
        let shapes = |ctx: &egui::Context, app: &mut App| {
            let mut last = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                last = out.shapes;
            }
            last
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        let plain = shapes(&ctx, &mut app);
        assert!(ctx.memory(|m| m.area_rect(tab_id)).is_none());
        let native = explore(&plain);
        let mut o = DomOverlay {
            version: 2,
            ..DomOverlay::default()
        };
        let host = |f: fn(&mut Style)| {
            let mut s = Style::default();
            f(&mut s);
            HostOverride {
                style: s,
                ..HostOverride::default()
            }
        };
        o.hosts.insert(
            keys::HEADER.into(),
            host(|s| {
                s.margin_top = Some("-80px".into());
                s.width = Some("30vw".into());
                s.left = Some("50%".into());
                s.transform = Some("translateX(-50%)".into());
                s.padding_top = Some("45px".into());
                s.background_color = Some("rgba(50, 50, 50, 0.5)".into());
            }),
        );
        o.hosts
            .insert(keys::SEARCH.into(), host(|s| s.opacity = Some("0".into())));
        o.hosts.insert(
            keys::SIDEBAR.into(),
            host(|s| s.left = Some("-47px".into())),
        );
        plugin_dom::test_overlay(Some(o));
        let themed = shapes(&ctx, &mut app);
        if cfg!(target_os = "macos") {
            // The traffic lights sit in the bar's row on macOS, so the bar
            // stays and no tab is drawn (topbar::header_tab).
            assert!(ctx.memory(|m| m.area_rect(tab_id)).is_none());
        } else {
            let tab = ctx
                .memory(|m| m.area_rect(tab_id))
                .expect("the header is a tab over the page");
            assert!((tab.width() - 420.0).abs() < 1.0, "{tab:?}");
            let sidebar = 240.0;
            let page_centre = sidebar + (size.x - sidebar) / 2.0;
            // Centred on the page (its scroll area, without the scroll bar).
            assert!((tab.center().x - page_centre).abs() < 20.0, "{tab:?}");
            assert_eq!(tab.top(), -80.0, "pulled up over the window top");
        }
        let slid = explore(&themed);
        assert_eq!(slid.x, native.x - 47.0, "the sidebar slid out");
        assert_eq!(slid.y, native.y);
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
    }

    /// Lunallena lays `#main` under the sidebar (a negative `left` the width
    /// of the sidebar, `min-width: 100vw`) and slides the sidebar out over
    /// it: the page then starts at the window's left edge and the sidebar
    /// floats over it. A page pulled left but not kept as wide as the
    /// window, and no theme, leave it beside the sidebar.
    #[test]
    fn a_themes_page_reaches_under_a_sidebar_that_floats_over_it() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let (ctx, mut app) = accessible_app("theme-page-under-sidebar");
        // Where the page's Back button is: the page's left edge.
        let back = |ctx: &egui::Context, app: &mut App| {
            let mut update = None;
            for _ in 0..3 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                update = out.platform_output.accesskit_update;
            }
            let tree = update.expect("screen-reader tree");
            let node = accessible_node(&tree, "Back", egui::accesskit::Role::Button);
            let (_, node) = tree.nodes.iter().find(|(id, _)| *id == node).unwrap();
            node.bounds().expect("the Back button's bounds").x0 as f32
        };
        let themed = |version: u64, main: fn(&mut Style)| {
            let host = |f: fn(&mut Style)| {
                let mut s = Style::default();
                f(&mut s);
                HostOverride {
                    style: s,
                    ..HostOverride::default()
                }
            };
            let mut o = DomOverlay {
                version,
                hover: true,
                ..DomOverlay::default()
            };
            o.hosts.insert(
                keys::SIDEBAR.into(),
                host(|s| s.left = Some("-217px".into())),
            );
            o.hosts.insert(keys::MAIN.into(), host(main));
            o
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        // Let the first frames' animations finish.
        for _ in 0..4 {
            back(&ctx, &mut app);
        }
        let beside = back(&ctx, &mut app);
        // Pulled left only: a gap would open on its right, so it stays.
        plugin_dom::test_overlay(Some(themed(2, |s| s.left = Some("-240px".into()))));
        let left_only = back(&ctx, &mut app);
        plugin_dom::test_overlay(Some(themed(3, |s| {
            s.left = Some("-240px".into());
            s.min_width = Some("100vw".into());
        })));
        let under = back(&ctx, &mut app);
        // The strip the sidebar leaves is over it for `:hover` (the page under
        // it is not what the pointer is on), and the page beside it is not.
        let hover_at = |ctx: &egui::Context, app: &mut App, at: egui::Pos2| {
            let mut raw = input.clone();
            raw.events.push(egui::Event::PointerMoved(at));
            // The first frame's end sends the anchors the next one hits.
            for _ in 0..2 {
                let mut out = ctx.run_ui(raw.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                plugin_dom::end_frame(ctx);
            }
            plugin_dom::test_hover()
        };
        let strip = hover_at(&ctx, &mut app, egui::pos2(10.0, 30.0));
        let page = hover_at(&ctx, &mut app, egui::pos2(700.0, 400.0));
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 4,
            ..DomOverlay::default()
        }));
        let plain = back(&ctx, &mut app);
        let all = format!("{beside} {left_only} {under} {plain}");
        assert!(beside > 200.0, "beside the sidebar: {all}");
        assert_eq!(left_only, beside, "{all}");
        assert!(under < 60.0, "the page starts at the window's edge: {all}");
        let in_sidebar = |t: &Option<crate::luna::dom::DomTarget>| matches!(t, Some(crate::luna::dom::DomTarget::Host(k)) if k.starts_with("sidebar"));
        assert!(in_sidebar(&strip), "the strip is the sidebar: {strip:?}");
        assert!(!in_sidebar(&page), "{page:?}");
        assert_eq!(plain, beside, "no theme: beside it again: {all}");
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
    }

    /// StarryNight hides the now-playing view's cover backdrop and gives
    /// `#nowPlaying` its sky gradient: the view shows the gradient, not only
    /// its first colour (black, where TIDAL shows the sky's blue foot).
    #[test]
    fn a_hidden_now_playing_backdrop_shows_the_views_own_gradient() {
        use crate::luna::dom::{DomOverlay, HostOverride};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            ..Default::default()
        };
        fn colours(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| colours(s, out)),
                egui::Shape::Mesh(m) => out.extend(m.vertices.iter().map(|v| v.color)),
                _ => {}
            }
        }
        let (ctx, mut app) = accessible_app("np-sky");
        apply_flags(&mut app, None, Some("now-playing"));
        // StarryNight: TIDAL's cover backdrop hidden, `#nowPlaying` a sky
        // from black at the top to #142b44 at the foot.
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        let mut hidden = HostOverride::default();
        hidden.style.display = Some("none".into());
        o.hosts.insert(keys::NP_BACKGROUND.into(), hidden);
        let mut sky = HostOverride::default();
        sky.style.background_image =
            Some("linear-gradient(180deg, #000000 0, #142b44 100%)".into());
        o.hosts.insert(keys::NOW_PLAYING.into(), sky);
        plugin_dom::test_overlay(Some(o));
        let mut found = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(&mut app, ui));
            out.textures_delta.clear();
            found.clear();
            out.shapes
                .iter()
                .for_each(|s| colours(&s.shape, &mut found));
        }
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
        assert!(
            found.contains(&egui::Color32::from_rgb(0x14, 0x2b, 0x44)),
            "the sky's blue foot is drawn"
        );
    }

    /// Lunallena's sidebar glass is `rgba(50, 50, 50, 0.5)` under a
    /// `backdrop-filter: blur()`. Out over the page it would show the page's
    /// text plainly (the content behind is not blurred), so the glass is laid
    /// over the window's colour and opaque there. Beside the page, or without
    /// a backdrop blur, the fill stays as the theme gives it.
    #[test]
    fn a_blurred_glass_sidebar_over_the_page_hides_the_page_under_it() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys;
        use crate::ui::plugin_dom;
        let size = egui::vec2(1400.0, 900.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        fn fills(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32)>) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| fills(s, out)),
                egui::Shape::Rect(r) => out.push((r.rect, r.fill)),
                _ => {}
            }
        }
        let (ctx, mut app) = accessible_app("theme-glass-sidebar");
        // The alphas of the sidebar's fills: about 240 wide, tall, and the
        // theme's dark grey (a fill from the opaque window colour is not).
        let glass = |ctx: &egui::Context, app: &mut App| {
            let mut found = Vec::new();
            for _ in 0..4 {
                let mut out = ctx.run_ui(input.clone(), |ui| crate::ui::show(app, ui));
                out.textures_delta.clear();
                found.clear();
                out.shapes.iter().for_each(|s| fills(&s.shape, &mut found));
            }
            found
                .iter()
                .filter(|(r, _)| (230.0..250.0).contains(&r.width()) && r.height() > 400.0)
                .filter(|(_, c)| c.r() < 70 && c.g() < 70 && c.b() < 70 && c.a() > 100)
                .map(|(_, c)| c.a())
                .collect::<Vec<u8>>()
        };
        let themed = |version: u64, blur: bool, over: bool| {
            let host = |f: &dyn Fn(&mut Style)| {
                let mut s = Style::default();
                f(&mut s);
                HostOverride {
                    style: s,
                    ..HostOverride::default()
                }
            };
            let mut o = DomOverlay {
                version,
                ..DomOverlay::default()
            };
            o.hosts.insert(
                keys::SIDEBAR.into(),
                host(&|s| {
                    s.background_color = Some("rgba(50, 50, 50, 0.5)".into());
                    if blur {
                        s.backdrop_filter = Some("blur(35px) saturate(2.5)".into());
                    }
                }),
            );
            if over {
                o.hosts.insert(
                    keys::MAIN.into(),
                    host(&|s| {
                        s.left = Some("-240px".into());
                        s.min_width = Some("100vw".into());
                    }),
                );
            }
            o
        };
        plugin_dom::test_overlay(Some(themed(1, true, false)));
        let beside = glass(&ctx, &mut app);
        plugin_dom::test_overlay(Some(themed(2, false, true)));
        let unblurred = glass(&ctx, &mut app);
        plugin_dom::test_overlay(Some(themed(3, true, true)));
        let over = glass(&ctx, &mut app);
        let all = format!("{beside:?} {unblurred:?} {over:?}");
        assert!(
            !beside.is_empty() && beside.iter().all(|a| *a < 200),
            "{all}"
        );
        assert!(
            !unblurred.is_empty() && unblurred.iter().all(|a| *a < 200),
            "{all}"
        );
        assert!(!over.is_empty() && over.iter().all(|a| *a == 255), "{all}");
        plugin_dom::test_overlay(None);
        app.backend.shutdown();
    }

    fn texts(painted: &[(String, egui::Rect)]) -> Vec<&str> {
        painted.iter().map(|(text, _)| text.as_str()).collect()
    }

    /// The album page leaves TIDAL's review to its Credits page (it
    /// was shown twice), and the header offers Credits.
    #[test]
    fn an_album_page_leaves_its_review_to_credits_and_offers_credits() {
        use egui::accesskit::{Action as Access, Role};
        fn album(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        let (ctx, mut app) = accessible_app("album-review");
        assert!(
            matches!(app.album_pages["alb0"].review, Loadable::Loaded(Some(_))),
            "the demo album has a review"
        );
        view_frame(&ctx, &mut app, vec![], album);
        let painted = view_frame(&ctx, &mut app, vec![], album);
        let all = texts(&painted);
        assert!(!all.contains(&"Review"), "{all:?}");
        assert!(
            !all.iter()
                .any(|text| text.contains("Recorded over two winters")),
            "the review is not under the songs"
        );
        assert!(!all.contains(&"Read more"), "{all:?}");
        // The Credits button opens the album's Credits page (no dialog) and
        // asks nothing while the credits are held.
        app.open(Page::Album("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let credits = accessible_node(&tree, "Credits", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(credits, Access::Click, None)],
        );
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(*app.page(), Page::AlbumCredits("alb0".into()));
        assert!(app.dialog.is_none());
        assert!(matches!(
            app.album_pages["alb0"].credits,
            Loadable::Loaded(_)
        ));
        app.backend.shutdown();
    }

    fn album_view(app: &mut App, ui: &mut egui::Ui) {
        crate::ui::collection::album(app, ui, "alb0");
    }

    /// TIDAL's shelves come under the album's footer (date, copyright), in
    /// its order, each titled in the app's words with View all.
    #[test]
    fn an_album_page_shows_tidals_shelves_under_its_footer_in_order() {
        let (ctx, mut app) = accessible_app("album-shelves");
        view_frame(&ctx, &mut app, vec![], album_view);
        let painted = view_frame(&ctx, &mut app, vec![], album_view);
        let y = |label: &str| {
            painted
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("{label:?} is not drawn: {:?}", texts(&painted)))
                .1
                .top()
        };
        let footer = painted
            .iter()
            .find(|(text, _)| text.starts_with("© "))
            .expect("the copyright line")
            .1
            .top();
        let artist = album(0).artists[0].name.clone();
        let headings = [
            y("Other versions"),
            y(&format!("More albums by {artist}")),
            y("Related albums"),
            y("Related artists"),
        ];
        assert!(footer < headings[0], "the shelves are under the footer");
        assert!(
            headings.windows(2).all(|pair| pair[0] < pair[1]),
            "{headings:?}"
        );
        let view_all = painted.iter().filter(|(t, _)| t == "View all").count();
        assert_eq!(view_all, 4, "each shelf has View all");
        app.backend.shutdown();
    }

    /// A shelf with no cards draws no heading, and a page with none draws
    /// no shelf at all.
    #[test]
    fn an_empty_album_shelf_has_no_heading() {
        let (ctx, mut app) = accessible_app("album-shelves-empty");
        let page = app.album_pages.get_mut("alb0").unwrap();
        let Loadable::Loaded(shelves) = &mut page.shelves else {
            panic!("the demo album has its shelves");
        };
        shelves.retain(|entry| entry.kind != AlbumShelfKind::RelatedAlbums);
        shelves
            .iter_mut()
            .find(|entry| entry.kind == AlbumShelfKind::OtherVersions)
            .unwrap()
            .shelf
            .items
            .clear();
        view_frame(&ctx, &mut app, vec![], album_view);
        let painted = view_frame(&ctx, &mut app, vec![], album_view);
        let all = texts(&painted);
        assert!(!all.contains(&"Other versions"), "{all:?}");
        assert!(!all.contains(&"Related albums"), "{all:?}");
        assert!(all.contains(&"Related artists"), "{all:?}");
        app.album_pages.get_mut("alb0").unwrap().shelves = Loadable::Loaded(Vec::new());
        let painted = view_frame(&ctx, &mut app, vec![], album_view);
        let all = texts(&painted);
        assert!(!all.contains(&"Related artists"), "{all:?}");
        assert!(!all.contains(&"View all"), "{all:?}");
        app.backend.shutdown();
    }

    /// The shelves are asked for once the footer is about to come into
    /// view, not when the page opens, and only once.
    #[test]
    fn the_album_shelves_are_read_when_the_footer_nears_the_view() {
        let (ctx, mut app) = accessible_app("album-shelves-lazy");
        app.album_pages.get_mut("alb0").unwrap().shelves = Loadable::NotLoaded;
        // A window the footer is far below: nothing is asked.
        view_frame_sized(
            &ctx,
            &mut app,
            vec![],
            egui::vec2(1280.0, 300.0),
            album_view,
        );
        assert!(
            matches!(app.album_pages["alb0"].shelves, Loadable::NotLoaded),
            "the footer is not near"
        );
        view_frame(&ctx, &mut app, vec![], album_view);
        assert!(
            app.album_pages["alb0"].shelves.is_loading(),
            "the footer is in view"
        );
        app.backend.shutdown();
    }

    /// A shelf's View all opens the page TIDAL has behind it, headed with
    /// the shelf's name.
    #[test]
    fn an_album_shelf_view_all_opens_its_page() {
        let (ctx, mut app) = accessible_app("album-shelves-all");
        // Tall enough for every shelf to be on screen.
        let size = egui::vec2(1280.0, 3400.0);
        for _ in 0..3 {
            view_frame_sized(&ctx, &mut app, vec![], size, album_view);
        }
        let painted = view_frame_sized(&ctx, &mut app, vec![], size, album_view);
        let related = painted
            .iter()
            .find(|(t, _)| t == "Related artists")
            .unwrap()
            .1;
        let view_all = painted
            .iter()
            .filter(|(t, _)| t == "View all")
            .map(|(_, rect)| *rect)
            .find(|rect| (rect.center().y - related.center().y).abs() < 8.0)
            .expect("View all on the Related artists heading");
        view_frame_sized(
            &ctx,
            &mut app,
            pointer_click(view_all.center(), egui::PointerButton::Primary),
            size,
            album_view,
        );
        let actions = std::mem::take(&mut app.actions);
        let path = "pages/single-module-page/demo/related-artists";
        assert!(
            matches!(actions.as_slice(), [Action::Open(Page::Shelf(p))] if p == path),
            "{actions:?}"
        );
        assert_eq!(
            app.page_titles.get(path).map(String::as_str),
            Some("Related artists")
        );
        app.backend.shutdown();
    }

    fn album_credits_page(app: &mut App, ui: &mut egui::Ui) {
        crate::ui::album_credits::page(app, ui, "alb0");
    }

    /// The album's Credits page is TIDAL's: the album's header dated in
    /// full and without a Credits button, the Credits and Info tabs, and a
    /// card for each song with its number and title over its roles in
    /// capitals, the people of a role by name, a code name spelled as words.
    #[test]
    fn the_album_credits_page_shows_a_card_for_each_song() {
        let (ctx, mut app) = accessible_app("album-credits-page");
        app.open(Page::AlbumCredits("alb0".into()));
        view_frame(&ctx, &mut app, vec![], album_credits_page);
        let painted = view_frame(&ctx, &mut app, vec![], album_credits_page);
        let all = texts(&painted);
        let album = app.album_pages["alb0"].album.get().unwrap().clone();
        let date = crate::ui::activity::long_date(
            app.locale,
            album.release_date.as_deref().expect("a release date"),
        );
        for expected in [
            album.name.as_str(),
            &date.to_uppercase(),
            "Credits",
            "Info",
            "1",
            "Rosewood",
            "6",
            "PRODUCER",
            "COMPOSER",
            "ADDITIONAL PRODUCER",
            "MIXING ENGINEER",
            "FEATURED ARTIST",
            "Nils Frahm, Simon Green",
            "Andreya Triana, Grey Reverend, Szjerdene",
        ] {
            assert!(all.contains(&expected), "{expected:?} missing in {all:?}");
        }
        assert!(!all.contains(&"ADDITIONALPRODUCER"));
        // The tab is the one "Credits": the header has no Credits button.
        assert_eq!(
            all.iter().filter(|text| **text == "Credits").count(),
            1,
            "{all:?}"
        );
        // The release's own roles are the Info tab's, not the cards'.
        assert!(!all.contains(&"PRIMARY ARTIST"), "{all:?}");
        // Before the credits arrive: a spinner, no cards.
        let credits = std::mem::take(&mut app.album_pages.get_mut("alb0").unwrap().credits);
        app.album_pages.get_mut("alb0").unwrap().credits = Loadable::Loading;
        let painted = view_frame(&ctx, &mut app, vec![], album_credits_page);
        assert!(!texts(&painted).contains(&"PRODUCER"));
        // After a failure: the way to try again.
        app.album_pages.get_mut("alb0").unwrap().credits = Loadable::Failed("offline".into());
        let painted = view_frame(&ctx, &mut app, vec![], album_credits_page);
        assert!(texts(&painted).contains(&"Try again"));
        app.album_pages.get_mut("alb0").unwrap().credits = credits;
        app.backend.shutdown();
    }

    /// Info: TIDAL's review in full with where it comes from, then
    /// "Additional Credits", the release's own roles over their people.
    #[test]
    fn the_album_credits_info_tab_shows_the_review_and_the_release_roles() {
        use egui::accesskit::{Action as Access, Role};
        let (ctx, mut app) = accessible_app("album-credits-info");
        app.open(Page::AlbumCredits("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        let info = accessible_node(&tree, "Info", Role::Button);
        accessible_frame(
            &ctx,
            &mut app,
            vec![accessible_action(info, Access::Click, None)],
        );
        assert_eq!(
            app.album_pages["alb0"].credits_tab,
            crate::model::CreditsTab::Info
        );
        view_frame(&ctx, &mut app, vec![], album_credits_page);
        let painted = view_frame(&ctx, &mut app, vec![], album_credits_page);
        let all = texts(&painted);
        for expected in [
            "Album Review",
            "(Album review from TiVo)",
            "Additional Credits",
            "PRIMARY ARTIST",
            "EXECUTIVE PRODUCER",
            "RECORD LABEL",
            "Peter Roberts",
            "Ninja Tune",
        ] {
            assert!(all.contains(&expected), "{expected:?} missing in {all:?}");
        }
        // The review in full: its last words too, no Read more.
        assert!(
            all.iter()
                .any(|text| text.contains("most coherent of the producer")),
            "{all:?}"
        );
        assert!(!all.contains(&"Read more"));
        // No song cards on this tab.
        assert!(!all.contains(&"Rosewood"), "{all:?}");
        // Opening Credits again shows the Credits tab.
        app.actions.push(Action::ShowAlbumCredits("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        assert_eq!(
            app.album_pages["alb0"].credits_tab,
            crate::model::CreditsTab::Credits
        );
        app.backend.shutdown();
    }

    /// Opening Credits asks TIDAL once and shows the page; an offline app
    /// fails the credits rather than spinning for ever.
    #[test]
    fn opening_album_credits_reads_them_once() {
        let (ctx, mut app) = accessible_app("album-credits-open");
        app.album_pages.get_mut("alb0").unwrap().credits = Loadable::NotLoaded;
        app.actions.push(Action::ShowAlbumCredits("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(matches!(app.album_pages["alb0"].credits, Loadable::Loading));
        assert_eq!(*app.page(), Page::AlbumCredits("alb0".into()));
        assert!(app.dialog.is_none());
        app.network_reachable = false;
        app.album_pages.get_mut("alb0").unwrap().credits = Loadable::NotLoaded;
        app.actions.push(Action::ShowAlbumCredits("alb0".into()));
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(matches!(
            app.album_pages["alb0"].credits,
            Loadable::Failed(_)
        ));
        app.backend.shutdown();
    }

    /// The album header puts the artist's own picture before the name, from
    /// the album's answer, even for an artist the app has read nowhere
    /// else (The Weeknd's album showed the grey placeholder).
    #[test]
    fn the_album_header_shows_the_artists_picture_from_the_album() {
        fn album_page(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::album(app, ui, "alb0");
        }
        const PICTURE: &str =
            "https://resources.tidal.com/images/5ac8f8f2/f98c/43c4/8f49/6ccebe2eba26/160x160.jpg";
        let (ctx, mut app) = accessible_app("album-avatar");
        if let Loadable::Loaded(album) = &mut app.album_pages.get_mut("alb0").unwrap().album {
            album.artists = vec![ArtistRef {
                id: Some("3652822".into()),
                name: "J. Cole".into(),
                uri: Some("tidal:artist:3652822".into()),
                picture: Some(PICTURE.into()),
            }];
        }
        assert!(app.known_artist("3652822").is_none());
        view_frame(&ctx, &mut app, vec![], album_page);
        let hero = crate::ui::sticky::last_hero(&ctx).expect("the header's note");
        assert_eq!(hero.avatar, Some(Some(PICTURE.to_string())));
        // The Credits page's header shows it too.
        view_frame(&ctx, &mut app, vec![], album_credits_page);
        let hero = crate::ui::sticky::last_hero(&ctx).expect("the header's note");
        assert_eq!(hero.avatar, Some(Some(PICTURE.to_string())));
        app.backend.shutdown();
    }

    /// Under a playlist the user can edit, TIDAL's Recommended songs are
    /// listed with an Add button each; adding one makes it a row of the
    /// playlist at once and takes it off the suggestions.
    #[test]
    fn recommended_songs_add_to_the_playlist_at_once() {
        use egui::accesskit::{Action as Access, Role};
        let (ctx, mut app) = accessible_app("recommended-songs");
        app.open(Page::Playlist("pl1".into()));
        // A window tall enough to show the whole page.
        let tall = |ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 4000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.expect("tree")
        };
        tall(&ctx, &mut app, vec![]);
        let tree = tall(&ctx, &mut app, vec![]);
        let first = super::track(200);
        let adds = format!("Add {} to the playlist", first.name);
        let before = app.playlist_pages["pl1"].items.items.len();
        let add = accessible_node(&tree, &adds, Role::Button);
        tall(
            &ctx,
            &mut app,
            vec![accessible_action(add, Access::Click, None)],
        );
        tall(&ctx, &mut app, vec![]);
        let page = &app.playlist_pages["pl1"];
        assert_eq!(
            page.items.items.len(),
            before + 1,
            "the row appears at once"
        );
        let last = page.items.items.last().and_then(|item| item.playable());
        assert_eq!(last.map(PlayableItem::uri), Some(first.uri.as_str()));
        let held = page.recommendations.as_ref().unwrap();
        assert_eq!(held.songs.len(), 9, "it leaves the suggestions");
        assert!(held.songs.iter().all(|song| song.uri != first.uri));
        app.backend.shutdown();
    }

    /// Songs a playlist holds are not suggested, a playlist the user
    /// cannot edit shows no suggestions and asks for none, and Refresh
    /// asks for the next ten.
    #[test]
    fn recommended_songs_follow_the_playlist_and_refresh() {
        fn playlist(app: &mut App, ui: &mut egui::Ui) {
            crate::ui::collection::playlist(app, ui, "pl1");
        }
        let (ctx, mut app) = accessible_app("recommended-rules");
        // A suggestion the playlist already holds is not offered.
        let held = super::track(3);
        app.playlist_pages
            .get_mut("pl1")
            .unwrap()
            .recommendations
            .as_mut()
            .unwrap()
            .songs
            .push(held.clone());
        view_frame(&ctx, &mut app, vec![], playlist);
        let painted = view_frame(&ctx, &mut app, vec![], playlist);
        assert!(texts(&painted).contains(&"Recommended songs"));
        let rows = painted
            .iter()
            .filter(|(text, rect)| text == &held.name && rect.top() > 2000.0)
            .count();
        assert_eq!(rows, 0, "{} is already in the playlist", held.name);
        // Refresh asks for the next ten; the ones shown stay meanwhile.
        app.actions.push(Action::LoadPlaylistRecommendations {
            id: "pl1".into(),
            refresh: true,
        });
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(app.playlist_pages["pl1"].recommendations_loading);
        assert!(app.playlist_pages["pl1"].recommendations.is_some());
        // Not editable: no section, and nothing is asked.
        app.playlist_pages
            .get_mut("pl1")
            .unwrap()
            .recommendations_loading = false;
        app.playlist_pages.get_mut("pl1").unwrap().recommendations = None;
        if let Loadable::Loaded(playlist) = &mut app.playlist_pages.get_mut("pl1").unwrap().playlist
        {
            playlist.owner.id = Some("someone-else".into());
        }
        let painted = view_frame(&ctx, &mut app, vec![], playlist);
        accessible_frame(&ctx, &mut app, vec![]);
        assert!(!texts(&painted).contains(&"Recommended songs"));
        assert!(!app.playlist_pages["pl1"].recommendations_loading);
        app.backend.shutdown();
    }

    /// Every text the whole window draws, with where.
    fn window_texts(
        ctx: &egui::Context,
        app: &mut App,
        size: egui::Vec2,
    ) -> Vec<(String, egui::Rect)> {
        view_frame_sized(ctx, app, vec![], size, |app, ui| app.frame_ui(ui))
    }

    /// TIDAL's play queue lies beside the page, not over it: with the panel
    /// open the page and its pinned bar give up its width, so the pinned
    /// bar's buttons and the table stay in view left of the panel. The top
    /// bar keeps its width, and closing the panel gives the room back.
    #[test]
    fn the_open_play_queue_takes_its_width_from_the_page() {
        let (ctx, mut app) = accessible_app("queue-beside-page");
        app.open(Page::Playlist("pl1".into()));
        // The bar pins once the page's own controls have scrolled away.
        app.demo_scroll = Some(700.0);
        let size = egui::vec2(1400.0, 900.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let panel_left = crate::ui::queue::floating_rect(&ctx, screen).left();
        let pinned_buttons_right = |painted: &[(String, egui::Rect)]| {
            painted
                .iter()
                .filter(|(text, rect)| text == "More" && rect.top() < 200.0)
                .map(|(_, rect)| rect.right())
                .fold(f32::MIN, f32::max)
        };
        app.show_queue_panel = false;
        window_texts(&ctx, &mut app, size);
        let closed = window_texts(&ctx, &mut app, size);
        let closed_right = pinned_buttons_right(&closed);
        assert!(
            closed_right > panel_left,
            "with the queue closed the page runs under where its panel would be: {closed_right}"
        );
        app.show_queue_panel = true;
        window_texts(&ctx, &mut app, size);
        let open = window_texts(&ctx, &mut app, size);
        let open_right = pinned_buttons_right(&open);
        assert!(open_right > 0.0, "the pinned bar still shows its buttons");
        assert!(
            open_right <= panel_left,
            "the pinned bar's buttons end at {open_right}, under the queue panel at {panel_left}"
        );
        // Closing it again returns the page its width.
        app.show_queue_panel = false;
        window_texts(&ctx, &mut app, size);
        let reopened = window_texts(&ctx, &mut app, size);
        assert_eq!(pinned_buttons_right(&reopened), closed_right);
        app.backend.shutdown();
    }

    /// Opening or closing the play queue narrows or widens the page; the
    /// page keeps where it was scrolled to (the pinned bar stays pinned).
    #[test]
    fn the_play_queue_opening_keeps_the_page_where_it_was_scrolled() {
        let (ctx, mut app) = accessible_app("queue-keeps-scroll");
        app.open(Page::Playlist("pl1".into()));
        app.demo_scroll = None;
        let size = egui::vec2(1400.0, 900.0);
        let pinned = |painted: &[(String, egui::Rect)]| {
            painted
                .iter()
                .any(|(text, rect)| text == "More" && rect.top() < 200.0)
        };
        let frame = |ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>| {
            view_frame_sized(ctx, app, events, size, |app, ui| app.frame_ui(ui))
        };
        frame(&ctx, &mut app, vec![]);
        // The page's own controls scroll away and the bar pins.
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(600.0, 500.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -900.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let mut painted = Vec::new();
        for _ in 0..30 {
            painted = frame(&ctx, &mut app, vec![]);
        }
        assert!(pinned(&painted), "the page scrolled and its bar pinned");
        for open in [true, false, true] {
            app.show_queue_panel = open;
            for _ in 0..4 {
                painted = frame(&ctx, &mut app, vec![]);
            }
            assert!(
                pinned(&painted),
                "the page jumped back to the top with the queue {}",
                if open { "opened" } else { "closed" }
            );
        }
        app.backend.shutdown();
    }

    /// TIDAL draws the Recommended songs as rows of the playlist's own
    /// table, one line each: the title, artist, album and time start where
    /// the table's cells above them do, and a centred Refresh list button
    /// sits below the list (no refresh icon at the heading).
    #[test]
    fn recommended_songs_lie_in_the_tables_columns_with_a_refresh_list_button() {
        let (ctx, mut app) = accessible_app("recommended-columns");
        app.open(Page::Playlist("pl1".into()));
        let size = egui::vec2(1280.0, 4000.0);
        window_texts(&ctx, &mut app, size);
        let painted = window_texts(&ctx, &mut app, size);
        let page = &app.playlist_pages["pl1"];
        let PlayableItem::Track(last) = page
            .items
            .items
            .last()
            .and_then(|item| item.playable())
            .unwrap();
        let last = last.clone();
        let suggested = page.recommendations.as_ref().unwrap().songs[0].clone();
        let heading = painted
            .iter()
            .find(|(text, _)| text == "Recommended songs")
            .expect("the heading")
            .1;
        // The table's last row ends just above the heading, the first
        // suggestion starts just below it.
        let in_table = |text: &str| {
            painted
                .iter()
                .find(|(shown, rect)| {
                    shown == text
                        && rect.top() < heading.top()
                        && rect.top() > heading.top() - 120.0
                })
                .unwrap_or_else(|| panic!("{text:?} is not in the table's last row"))
                .1
        };
        let in_list = |text: &str| {
            painted
                .iter()
                .find(|(shown, rect)| {
                    shown == text
                        && rect.top() > heading.bottom()
                        && rect.top() < heading.bottom() + 60.0
                })
                .unwrap_or_else(|| panic!("{text:?} is not in the first suggestion's row"))
                .1
        };
        let time = crate::util::format_duration_ms;
        let pairs = [
            ("title", in_table(&last.name), in_list(&suggested.name)),
            (
                "artist",
                in_table(&last.artists[0].name),
                in_list(&suggested.artists[0].name),
            ),
            (
                "album",
                in_table(&last.album.as_ref().unwrap().name),
                in_list(&suggested.album.as_ref().unwrap().name),
            ),
            (
                "time",
                in_table(&time(last.duration_ms)),
                in_list(&time(suggested.duration_ms)),
            ),
        ];
        for (column, table, list) in pairs {
            assert!(
                (table.left() - list.left()).abs() < 0.5,
                "{column}: the table's cell starts at {} but the suggestion's at {}",
                table.left(),
                list.left()
            );
        }
        // One line a row: the artist shares the title's line.
        let title = in_list(&suggested.name);
        let artist = in_list(&suggested.artists[0].name);
        assert!((title.center().y - artist.center().y).abs() < 2.0);
        // Refresh list below the list; the heading holds no refresh icon.
        let refresh = painted
            .iter()
            .find(|(text, _)| text == "Refresh list")
            .expect("a Refresh list button")
            .1;
        let last_suggested = page.recommendations.as_ref().unwrap().songs.last().unwrap();
        let lowest_row = painted
            .iter()
            .filter(|(text, rect)| rect.top() > heading.bottom() && text == &last_suggested.name)
            .map(|(_, rect)| rect.bottom())
            .fold(f32::MIN, f32::max);
        assert!(refresh.top() > lowest_row);
        let tree = accessible_frame(&ctx, &mut app, vec![]);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Refresh")),
            "the heading no longer carries a refresh icon"
        );
        app.backend.shutdown();
    }

    /// A song added from Recommended into a playlist whose rows carry no
    /// dates must look like every other row: the table's columns are decided
    /// once for the table (and its Recommended rows), never by whether one
    /// row has a date, and the new row carries no date TIDAL would not give.
    #[test]
    fn an_added_recommended_song_lies_in_the_columns_of_the_other_rows() {
        use egui::accesskit::{Action as Access, Role};
        let (ctx, mut app) = accessible_app("recommended-undated-add");
        app.open(Page::Playlist("pl1".into()));
        {
            let page = app.playlist_pages.get_mut("pl1").unwrap();
            for item in &mut page.items.items {
                item.added_at = None;
            }
            page.items.revision = page.items.revision.wrapping_add(1);
        }
        let size = egui::vec2(1280.0, 4000.0);
        let run = |ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| app.frame_ui(ui),
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.expect("tree")
        };
        run(&ctx, &mut app, vec![]);
        let tree = run(&ctx, &mut app, vec![]);
        let first = super::track(200);
        let add = accessible_node(
            &tree,
            &format!("Add {} to the playlist", first.name),
            Role::Button,
        );
        run(
            &ctx,
            &mut app,
            vec![accessible_action(add, Access::Click, None)],
        );
        window_texts(&ctx, &mut app, size);
        let painted = window_texts(&ctx, &mut app, size);
        let page = &app.playlist_pages["pl1"];
        assert_eq!(
            page.items
                .items
                .last()
                .and_then(|item| item.added_at.clone()),
            None,
            "the new row has no date the other rows lack"
        );
        assert!(
            !painted.iter().any(|(text, _)| text == "DATE ADDED"),
            "an undated playlist has no Date added column"
        );
        let n = page.items.items.len();
        let track_of = |index: usize| match page.items.items[index].playable().unwrap() {
            PlayableItem::Track(track) => track.clone(),
        };
        let (new, old) = (track_of(n - 1), track_of(n - 2));
        let suggested = page.recommendations.as_ref().unwrap().songs[0].clone();
        let heading = painted
            .iter()
            .find(|(text, _)| text == "Recommended songs")
            .expect("the heading")
            .1;
        // Left edge of `text` in the table row `rows_up` rows above the
        // new (last) one, or of the first suggestion when `rows_up` is None.
        // Demo songs repeat, so the new row is the lowest match above the heading.
        let new_top = painted
            .iter()
            .filter(|(shown, rect)| shown == &new.name && rect.top() < heading.top())
            .map(|(_, rect)| rect.top())
            .fold(f32::MIN, f32::max);
        let left = |text: &str, rows_up: Option<f32>| {
            painted
                .iter()
                .find(|(shown, rect)| {
                    shown == text
                        && match rows_up {
                            Some(up) => (rect.top() - (new_top - 48.0 * up)).abs() < 12.0,
                            None => {
                                rect.top() > heading.bottom()
                                    && rect.top() < heading.bottom() + 60.0
                            }
                        }
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{text:?} not found; heading {heading:?} new_top {new_top}; {:?}",
                        painted
                            .iter()
                            .filter(|(shown, _)| shown == text)
                            .collect::<Vec<_>>()
                    )
                })
                .1
                .left()
        };
        let album = |track: &Track| track.album.as_ref().unwrap().name.clone();
        let same = |label: &str, a: f32, b: f32| {
            assert!((a - b).abs() < 0.5, "{label}: {a} against {b}");
        };
        let table_artist = left(&old.artists[0].name, Some(1.0));
        let table_album = left(&album(&old), Some(1.0));
        same(
            "new row artist",
            left(&new.artists[0].name, Some(0.0)),
            table_artist,
        );
        same("new row album", left(&album(&new), Some(0.0)), table_album);
        same(
            "suggestion artist",
            left(&suggested.artists[0].name, None),
            table_artist,
        );
        same(
            "suggestion album",
            left(&album(&suggested), None),
            table_album,
        );
        app.backend.shutdown();
    }
}
