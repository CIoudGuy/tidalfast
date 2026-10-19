//! TIDAL's page documents (`pages/home`, `pages/explore`, genre and mood
//! pages, "View all" data pages) as a flat list of shelves.
//!
//! A page is rows of modules; each list module becomes one [`Shelf`].
//! Unknown module and item types are skipped, never an error: TIDAL adds new
//! ones without notice.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::convert::{self, RawAlbum, RawArtist, RawMix, RawPlaylist, RawTrack, RawVideo};
use crate::api::models::{
    Album, AlbumShelf, AlbumShelfKind, Artist, CardMark, Image, Mix, PageLink, Playlist, Promo,
    Shelf, ShelfItem, ShelfLayout, Track, Video,
};

fn decode<T: DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

/// An entity of a known kind (`ALBUM`, `album`, `ALBUM_LIST`...) as a card.
fn item(kind: &str, value: &Value) -> Option<ShelfItem> {
    let kind = kind.to_ascii_uppercase();
    let kind = kind.trim_end_matches("_LIST").trim_end_matches('S');
    let item = match kind {
        "ALBUM" => ShelfItem::Album(convert::album(decode::<RawAlbum>(value)?)),
        "PLAYLIST" => ShelfItem::Playlist(convert::playlist(decode::<RawPlaylist>(value)?)),
        "MIX" => ShelfItem::Mix(convert::mix(decode::<RawMix>(value)?)),
        "ARTIST" => ShelfItem::Artist(convert::artist(decode::<RawArtist>(value)?)),
        "TRACK" => ShelfItem::Track(convert::track(decode::<RawTrack>(value)?)),
        "VIDEO" => ShelfItem::Video(convert::video(decode::<RawVideo>(value)?)),
        _ => return None,
    };
    let empty = match &item {
        ShelfItem::Album(album) => album.id.is_empty(),
        ShelfItem::Playlist(playlist) => playlist.id.is_empty(),
        ShelfItem::Mix(mix) => mix.id.is_empty(),
        ShelfItem::Artist(artist) => artist.id.is_empty(),
        ShelfItem::Track(track) => track.id.is_none(),
        ShelfItem::Video(video) => video.id.is_empty(),
    };
    (!empty).then_some(item)
}

/// `{type, item}` (mixed lists, highlights, v2 feeds) or `{type, data}`.
fn wrapped(value: &Value) -> Option<ShelfItem> {
    let kind = value.get("type")?.as_str()?;
    let inner = value.get("item").or_else(|| value.get("data"))?;
    item(kind, inner)
}

/// A promotion card points at an artifact by id, with a 550x400 image.
fn promotion(value: &Value) -> Option<ShelfItem> {
    let kind = value.get("type")?.as_str()?;
    let id = match value.get("artifactId")? {
        Value::String(id) => id.clone(),
        Value::Number(id) => id.to_string(),
        _ => return None,
    };
    let name = value
        .get("header")
        .or_else(|| value.get("shortHeader"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let images: Vec<Image> = value
        .get("imageId")
        .and_then(Value::as_str)
        .map(|uuid| Image {
            url: convert::image_url(uuid, "550x400"),
            width: Some(550),
            height: Some(400),
        })
        .into_iter()
        .collect();
    Some(match kind {
        "ALBUM" => ShelfItem::Album(Album {
            uri: convert::uri("album", &id),
            id,
            name,
            images,
            ..Default::default()
        }),
        "PLAYLIST" => ShelfItem::Playlist(Playlist {
            uri: convert::uri("playlist", &id),
            id,
            name,
            images,
            ..Default::default()
        }),
        "ARTIST" => ShelfItem::Artist(Artist {
            uri: convert::uri("artist", &id),
            id,
            name,
            images,
            ..Default::default()
        }),
        "MIX" => ShelfItem::Mix(Mix {
            uri: convert::uri("mix", &id),
            id,
            name,
            images,
            ..Default::default()
        }),
        "TRACK" => ShelfItem::Track(Track {
            uri: convert::uri("track", &id),
            id: Some(id),
            name,
            ..Default::default()
        }),
        "VIDEO" => ShelfItem::Video(Video {
            id,
            name,
            images,
            ..Default::default()
        }),
        _ => return None,
    })
}

fn items_of(module: &Value) -> Vec<&Value> {
    module
        .get("pagedList")
        .and_then(|list| list.get("items"))
        .or_else(|| module.get("items"))
        .or_else(|| module.get("highlights"))
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

/// The kinds of list module this client shows.
const LIST_MODULES: &[&str] = &[
    "ALBUM_LIST",
    "PLAYLIST_LIST",
    "MIX_LIST",
    "ARTIST_LIST",
    "TRACK_LIST",
    "VIDEO_LIST",
    "MIXED_TYPES_LIST",
    "HIGHLIGHT_MODULE",
    "ITEM_LIST_WITH_ROLES",
    "FEATURED_PROMOTIONS",
    "MULTIPLE_TOP_PROMOTIONS",
];

/// The cards of a module's entries.
fn module_items(kind: &str, entries: Vec<&Value>) -> Vec<ShelfItem> {
    match kind {
        "ALBUM_LIST" | "PLAYLIST_LIST" | "MIX_LIST" | "ARTIST_LIST" | "TRACK_LIST"
        | "VIDEO_LIST" => entries
            .into_iter()
            .filter_map(|value| item(kind, value).or_else(|| wrapped(value)))
            .collect(),
        "MIXED_TYPES_LIST" | "HIGHLIGHT_MODULE" => entries
            .into_iter()
            .filter_map(|value| wrapped(value).or_else(|| value.get("item").and_then(wrapped)))
            .collect(),
        "ITEM_LIST_WITH_ROLES" => entries
            .into_iter()
            .filter_map(|value| value.get("item").and_then(|inner| item("ARTIST", inner)))
            .collect(),
        "FEATURED_PROMOTIONS" | "MULTIPLE_TOP_PROMOTIONS" => {
            entries.into_iter().filter_map(promotion).collect()
        }
        _ => Vec::new(),
    }
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

/// A `PAGE_LINKS_CLOUD` (pills) or `PAGE_LINKS` (icon links) module.
fn links_shelf(kind: &str, module: &Value) -> Option<Shelf> {
    let links: Vec<PageLink> = items_of(module)
        .into_iter()
        .filter_map(|value| {
            let title = value.get("title")?.as_str()?.trim().to_string();
            let path = non_empty(value.get("apiPath"))?;
            (!title.is_empty()).then(|| PageLink {
                title,
                path: normalize_path(&path),
                icon: non_empty(value.get("icon")),
            })
        })
        .collect();
    if links.is_empty() {
        return None;
    }
    Some(Shelf {
        title: module
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        more: non_empty(module.get("showMore").and_then(|more| more.get("apiPath")))
            .or_else(|| non_empty(module.get("viewAll")))
            .map(|path| normalize_path(&path)),
        links,
        cloud: kind == "PAGE_LINKS_CLOUD",
        ..Default::default()
    })
}

fn shelf(module: &Value) -> Option<Shelf> {
    let kind = module.get("type")?.as_str()?;
    if matches!(kind, "PAGE_LINKS_CLOUD" | "PAGE_LINKS") {
        return links_shelf(kind, module);
    }
    if !LIST_MODULES.contains(&kind) {
        return None;
    }
    let items = module_items(kind, items_of(module));
    let more = non_empty(module.get("showMore").and_then(|more| more.get("apiPath")))
        .or_else(|| non_empty(module.get("viewAll")));
    let title = module
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !items.is_empty() {
        // Promotions (Explore's Videos and genre pages' "Featured") also
        // come as TIDAL's wide editorial cards: kicker, title, text.
        let promos = if matches!(kind, "FEATURED_PROMOTIONS" | "MULTIPLE_TOP_PROMOTIONS") {
            items_of(module).into_iter().filter_map(magazine).collect()
        } else {
            Vec::new()
        };
        return Some(Shelf {
            title,
            kind: kind.to_string(),
            items,
            more,
            promos,
            ..Default::default()
        });
    }
    // A module TIDAL sends without its list: the list is at the paged
    // list's `dataApiPath`, else behind "View all".
    let source = non_empty(
        module
            .get("pagedList")
            .and_then(|list| list.get("dataApiPath")),
    )
    .or_else(|| more.clone())?;
    Some(Shelf {
        title,
        kind: kind.to_string(),
        items: Vec::new(),
        more,
        source: Some(source),
        pending: true,
        ..Default::default()
    })
}

/// The cards of a page fetched for a shelf sent without them: a data page
/// (`{items}`) or a page of modules, whose cards are put together.
pub(crate) fn shelf_items(page: &Value) -> Vec<ShelfItem> {
    let mut items: Vec<ShelfItem> = Vec::new();
    for shelf in shelves(page).into_iter().chain(feed_shelves(page)) {
        if !shelf.pending {
            items.extend(shelf.items);
        }
    }
    if items.is_empty() {
        // A v2 "view all" lists `{type, data}` entries without modules.
        items = page
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(wrapped)
            .collect();
    }
    items
}

/// TIDAL's v2 home feed (`home/feed/static`): `items` are modules
/// (`SHORTCUT_LIST`, `HORIZONTAL_LIST`, `HORIZONTAL_LIST_WITH_CONTEXT`,
/// `TRACK_LIST`, ...), each listing `{type, data}` cards. Every module that
/// lists cards becomes a shelf, whatever its type; unknown card types are
/// skipped. The `SHORTCUT_LIST` module is kept with the
/// [`ShelfLayout::Shortcuts`] layout: Home draws it as the tile grid at the
/// top, never as a shelf.
pub(crate) fn feed_shelves(feed: &Value) -> Vec<Shelf> {
    let Some(modules) = feed.get("items").and_then(Value::as_array) else {
        return Vec::new();
    };
    modules.iter().filter_map(feed_shelf).collect()
}

fn feed_shelf(module: &Value) -> Option<Shelf> {
    // Modules list cards; cards themselves are never shelves.
    let entries = module.get("items")?.as_array()?;
    if module.get("data").is_some() {
        return None;
    }
    let mut title = module
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    // "Because you liked" names what it is about in its header.
    if let Some(about) = module
        .get("header")
        .and_then(|header| header.get("data").or_else(|| header.get("item")))
        .and_then(|data| data.get("title").or_else(|| data.get("name")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        && !title.contains(about)
    {
        title = if title.is_empty() {
            about.to_string()
        } else {
            format!("{title} {about}")
        };
    }
    let mut items = Vec::new();
    let mut marks = Vec::new();
    let mut promos = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(Value::as_str) == Some("MAGAZINE") {
            promos.extend(entry.get("data").and_then(magazine));
        } else if let Some(item) = wrapped(entry) {
            items.push(item);
            marks.push(card_mark(entry.get("data").or_else(|| entry.get("item"))));
        }
    }
    if marks.iter().all(|mark| *mark == CardMark::default()) {
        marks.clear();
    }
    let layout = match module.get("type").and_then(Value::as_str) {
        Some("COMPACT_GRID_CARD") => ShelfLayout::Compact,
        Some("SHORTCUT_LIST") => ShelfLayout::Shortcuts,
        _ => ShelfLayout::Cards,
    };
    let subtitle = non_empty(module.get("subtitle"));
    let module_id = non_empty(module.get("moduleId")).unwrap_or_default();
    let info = module
        .get("icons")
        .and_then(Value::as_array)
        .is_some_and(|icons| {
            icons
                .iter()
                .any(|icon| icon.as_str() == Some("SPOTLIGHT_INFO"))
        });
    let more = non_empty(module.get("viewAll"))
        .or_else(|| non_empty(module.get("showMore").and_then(|more| more.get("apiPath"))))
        .map(|path| v2_path(&path));
    if !promos.is_empty() {
        return Some(Shelf {
            title,
            more,
            subtitle,
            info,
            promos,
            module_id,
            ..Default::default()
        });
    }
    if items.is_empty() {
        // Listed empty with a "view all": the cards load from there.
        let source = more.clone()?;
        return Some(Shelf {
            title,
            items,
            more,
            source: Some(source),
            pending: true,
            layout,
            module_id,
            ..Default::default()
        });
    }
    Some(Shelf {
        title,
        items,
        more,
        layout,
        subtitle,
        info,
        marks,
        module_id,
        ..Default::default()
    })
}

/// The badges of a card's entity: an upload, picked by the editors, sold.
fn card_mark(data: Option<&Value>) -> CardMark {
    let flag = |key: &str| {
        data.and_then(|data| data.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    CardMark {
        upload: flag("upload"),
        spotlighted: flag("spotlighted"),
        purchase: flag("payToStream"),
    }
}

/// A `MAGAZINE` card (the Uploads tab's "Featured"): a promotion with a
/// kicker (`header`), a title (`shortHeader`) and a line (`shortSubHeader`).
fn magazine(data: &Value) -> Option<Promo> {
    let text = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    let title = text("shortHeader");
    let mut item = promotion(data)?;
    if !title.is_empty() {
        match &mut item {
            ShelfItem::Album(album) => album.name = title.clone(),
            ShelfItem::Playlist(playlist) => playlist.name = title.clone(),
            ShelfItem::Artist(artist) => artist.name = title.clone(),
            ShelfItem::Mix(mix) => mix.name = title.clone(),
            ShelfItem::Track(track) => track.name = title.clone(),
            ShelfItem::Video(video) => video.name = title.clone(),
        }
    }
    let image = data
        .get("imageId")
        .and_then(Value::as_str)
        .filter(|uuid| !uuid.is_empty())
        .map(|uuid| convert::image_url(uuid, "550x400"));
    Some(Promo {
        kicker: text("header").to_uppercase(),
        title,
        text: text("shortSubHeader"),
        image,
        item,
    })
}

/// The cursor of the feed's next page, if any.
pub(crate) fn feed_cursor(feed: &Value) -> Option<String> {
    non_empty(feed.get("page").and_then(|page| page.get("cursor")))
}

/// A v2 path marked so [`TidalClient::page_path`] sends it to the v2 host.
fn v2_path(path: &str) -> String {
    let path = path.trim().trim_start_matches('/');
    if path.starts_with("http") || path.starts_with("v1/") || path.starts_with("pages/") {
        return path.to_string();
    }
    let path = path.strip_prefix("v2/").unwrap_or(path);
    format!("v2/{path}")
}

/// Every list module of a page, in order.
pub(crate) fn shelves(page: &Value) -> Vec<Shelf> {
    let modules = page
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("modules").and_then(Value::as_array))
        .flatten();
    // A data page ("View all") is one paged list without rows.
    if page.get("rows").is_none() && page.get("items").is_some() {
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let items: Vec<ShelfItem> = page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| {
                wrapped(value).or_else(|| {
                    // Data pages list bare entities; guess the kind by shape.
                    let kind = if value.get("uuid").is_some() {
                        "PLAYLIST"
                    } else if value.get("numberOfTracks").is_some() || value.get("cover").is_some()
                    {
                        if value.get("duration").is_some() && value.get("album").is_some() {
                            "TRACK"
                        } else {
                            "ALBUM"
                        }
                    } else if value.get("album").is_some() {
                        "TRACK"
                    } else if value.get("mixType").is_some() || value.get("mixImages").is_some() {
                        "MIX"
                    } else if value.get("picture").is_some() {
                        "ARTIST"
                    } else {
                        return None;
                    };
                    item(kind, value)
                })
            })
            .collect();
        return if items.is_empty() {
            Vec::new()
        } else {
            vec![Shelf {
                title,
                items,
                ..Default::default()
            }]
        };
    }
    modules.filter_map(shelf).collect()
}

/// The Playlists and Credits modules of an artist page (`pages/artist`).
/// Told apart by module type where TIDAL sends it (a playlist list is
/// Playlists in every language), by English title otherwise; the artist's
/// own top tracks are a track list too, so Credits needs its title or the
/// BPM and key only its tracks carry. Either is empty when the page lacks it.
pub(crate) fn artist_modules(shelves: &[Shelf]) -> crate::api::models::ArtistModules {
    let mut modules = crate::api::models::ArtistModules::default();
    let tracks_of = |shelf: &Shelf| -> Vec<crate::api::models::Track> {
        shelf
            .items
            .iter()
            .filter_map(|item| match item {
                ShelfItem::Track(track) => Some(track.clone()),
                _ => None,
            })
            .collect()
    };
    let mut track_lists = Vec::new();
    for shelf in shelves {
        let title = shelf.title.to_lowercase();
        let playlists = shelf.kind == "PLAYLIST_LIST"
            || (shelf.kind.is_empty() || shelf.kind == "MIXED_TYPES_LIST")
                && title.contains("playlist");
        if playlists && modules.playlists.is_empty() {
            modules.playlists = shelf
                .items
                .iter()
                .filter_map(|item| match item {
                    ShelfItem::Playlist(playlist) => Some(playlist.clone()),
                    _ => None,
                })
                .collect();
            modules.playlists_more = shelf.more.clone();
        } else if title.contains("credit") && modules.credits.is_empty() {
            modules.credits = tracks_of(shelf);
            modules.credits_more = shelf.more.clone();
        } else if shelf.kind == "TRACK_LIST" {
            track_lists.push(shelf);
        }
    }
    // Without a title to go by, Credits is the track list with BPM and key.
    if modules.credits.is_empty()
        && let Some(shelf) = track_lists
            .iter()
            .find(|shelf| tracks_of(shelf).iter().any(|t| t.audio_analysis.is_some()))
    {
        modules.credits = tracks_of(shelf);
        modules.credits_more = shelf.more.clone();
    }
    modules
}

/// The shelves under an album page's footer (`pages/album`), told apart
/// the way TIDAL's page builds them: the artist list is Related Artists;
/// the album list asked with an `artistId` is More Albums by the artist;
/// of the other album lists, the one TIDAL titles for its versions is Other
/// Versions and the rest are Related Albums. A module without cards is left
/// out, and the rest come in TIDAL's order.
pub(crate) fn album_shelves(shelves: Vec<Shelf>) -> Vec<AlbumShelf> {
    let mut found: Vec<AlbumShelf> = shelves
        .into_iter()
        .filter(|shelf| !shelf.items.is_empty())
        .filter_map(|shelf| {
            let artist_id = shelf.more.as_deref().and_then(|path| {
                let (_, rest) = path.split_once("artistId=")?;
                let id = rest.split('&').next().unwrap_or_default();
                (!id.is_empty()).then(|| id.to_string())
            });
            let kind = match shelf.kind.as_str() {
                "ARTIST_LIST" => AlbumShelfKind::RelatedArtists,
                "ALBUM_LIST" if artist_id.is_some() => AlbumShelfKind::MoreByArtist,
                "ALBUM_LIST" if shelf.title.to_lowercase().contains("version") => {
                    AlbumShelfKind::OtherVersions
                }
                "ALBUM_LIST" => AlbumShelfKind::RelatedAlbums,
                _ => return None,
            };
            Some(AlbumShelf {
                kind,
                artist_id,
                shelf,
            })
        })
        .collect();
    found.sort_by_key(|entry| entry.kind);
    // One shelf of each other kind: the first TIDAL sends. An album by two
    // main artists has a More Albums shelf for each.
    let mut seen = Vec::new();
    found.retain(|entry| {
        entry.kind == AlbumShelfKind::MoreByArtist || {
            let new = !seen.contains(&entry.kind);
            seen.push(entry.kind);
            new
        }
    });
    found
}

/// A page path from a `showMore` / `viewAll` link, relative to the v1 base:
/// leading slashes, the host and a `v1/` prefix are dropped. A `v2/`
/// prefix is kept: that path is on the v2 host.
pub(crate) fn normalize_path(path: &str) -> String {
    let mut path = path.trim();
    for host in [
        "https://api.tidal.com",
        "https://tidal.com",
        "https://listen.tidal.com",
    ] {
        if let Some(rest) = path.strip_prefix(host) {
            path = rest;
        }
    }
    let path = path.trim_start_matches('/');
    path.strip_prefix("v1/").unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_page_becomes_shelves_skipping_unknown_modules() {
        let page: Value = serde_json::from_str(include_str!("fixtures/page_home.json")).unwrap();
        let shelves = shelves(&page);
        let titles: Vec<&str> = shelves.iter().map(|shelf| shelf.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Featured",
                "Your Mixes",
                "New Albums",
                "Popular Playlists",
                "Suggested New Tracks",
                "Videos",
                "Recently Played"
            ]
        );
        let ShelfItem::Playlist(promoted) = &shelves[0].items[0] else {
            panic!("promotion is a playlist")
        };
        assert_eq!(
            promoted.uri,
            "tidal:playlist:11111111-aaaa-4bbb-8ccc-000000000003"
        );
        assert_eq!(promoted.images[0].width, Some(550));
        let ShelfItem::Mix(mix) = &shelves[1].items[0] else {
            panic!("mix")
        };
        assert_eq!(mix.uri, "tidal:mix:0011a3f1f4f6d36dd5a4c2e9a1c6b0");
        assert_eq!(mix.name, "My Mix 1");
        assert_eq!(mix.images[0].width, Some(1500), "largest mix image first");
        let ShelfItem::Album(album) = &shelves[2].items[0] else {
            panic!("album")
        };
        assert_eq!(album.uri, "tidal:album:77646164");
        assert_eq!(album.artists[0].name, "Daft Punk");
        assert_eq!(shelves[2].more.as_deref(), Some("pages/data/new-albums"));
        assert!(matches!(shelves[3].items[0], ShelfItem::Playlist(_)));
        assert!(matches!(shelves[4].items[0], ShelfItem::Track(_)));
        assert!(matches!(shelves[5].items[0], ShelfItem::Video(_)));
        // A mixed list keeps an album, a mix and a video.
        assert_eq!(shelves[6].items.len(), 3);
    }

    #[test]
    fn explore_links_become_pill_rows_and_an_icon_grid() {
        let page = serde_json::json!({
            "title": "Explore",
            "rows": [
                {"modules": [{
                    "type": "PAGE_LINKS_CLOUD",
                    "title": "Genres",
                    "showMore": {"title": "View all", "apiPath": "pages/genre_page"},
                    "pagedList": {"items": [
                        {"title": "Hip-Hop", "icon": "", "apiPath": "/v1/pages/genre_hip_hop"},
                        {"title": "", "apiPath": "pages/genre_blank"},
                        {"title": "Pop", "apiPath": "pages/genre_pop", "imageId": null}
                    ]}
                }]},
                {"modules": [{
                    "type": "PAGE_LINKS",
                    "title": "",
                    "pagedList": {"items": [
                        {"title": "New", "icon": "new", "apiPath": "pages/new"},
                        {"title": "Clean Content", "apiPath": "pages/clean"}
                    ]}
                }]}
            ]
        });
        let shelves = shelves(&page);
        assert_eq!(shelves.len(), 2);
        assert!(shelves[0].cloud);
        assert_eq!(shelves[0].title, "Genres");
        assert_eq!(shelves[0].more.as_deref(), Some("pages/genre_page"));
        let titles: Vec<&str> = shelves[0].links.iter().map(|l| l.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Hip-Hop", "Pop"],
            "a link without a name is skipped"
        );
        assert_eq!(shelves[0].links[0].path, "pages/genre_hip_hop");
        assert_eq!(shelves[0].links[0].icon, None);
        assert!(!shelves[1].cloud);
        assert_eq!(shelves[1].links[0].icon.as_deref(), Some("new"));
    }

    #[test]
    fn paths_from_links_are_relative_to_v1() {
        assert_eq!(normalize_path("/v1/pages/genre_rnb"), "pages/genre_rnb");
        assert_eq!(normalize_path("pages/data/x?a=b"), "pages/data/x?a=b");
        assert_eq!(
            normalize_path("https://api.tidal.com/v1/pages/data/x"),
            "pages/data/x"
        );
        assert_eq!(normalize_path("v2/home/pages/x"), "v2/home/pages/x");
    }

    #[test]
    fn a_module_sent_without_its_list_is_a_pending_shelf() {
        let page: Value =
            serde_json::from_str(include_str!("fixtures/page_home_lazy.json")).unwrap();
        let shelves = shelves(&page);
        let titles: Vec<&str> = shelves.iter().map(|shelf| shelf.title.as_str()).collect();
        // The unknown module and the empty one with nowhere to load from
        // are left out.
        assert_eq!(titles, ["Recommended for you", "New Albums"]);
        assert!(shelves[0].pending);
        assert!(shelves[0].items.is_empty());
        assert_eq!(
            shelves[0].source.as_deref(),
            Some("pages/data/aaaaaaaa-0000-4000-8000-000000000002?limit=20"),
            "the paged list's own path comes before View all"
        );
        assert_eq!(
            shelves[0].more.as_deref(),
            Some("pages/data/aaaaaaaa-0000-4000-8000-000000000001")
        );
        assert!(!shelves[1].pending);
        assert_eq!(shelves[1].items.len(), 1);
    }

    #[test]
    fn a_data_page_gives_a_pending_shelf_its_cards() {
        let page: Value =
            serde_json::from_str(include_str!("fixtures/page_data_playlists.json")).unwrap();
        let items = shelf_items(&page);
        assert_eq!(items.len(), 2);
        let ShelfItem::Playlist(first) = &items[0] else {
            panic!("playlist")
        };
        assert_eq!(first.name, "Sample Playlist One");
    }

    #[test]
    fn the_v2_feed_becomes_shelves_skipping_unknown_cards() {
        let feed: Value = serde_json::from_str(include_str!("fixtures/home_feed_v2.json")).unwrap();
        let shelves = feed_shelves(&feed);
        let titles: Vec<&str> = shelves.iter().map(|shelf| shelf.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Shortcuts",
                "Recommended for you",
                "Because you liked Sample Record",
                "Recently played",
                "Your mixes",
            ]
        );
        // The shortcut module is the tile grid at the top of Home, not a shelf.
        assert_eq!(shelves[0].layout, ShelfLayout::Shortcuts);
        assert!(
            shelves[1..]
                .iter()
                .all(|shelf| shelf.layout != ShelfLayout::Shortcuts)
        );
        // The deep link is skipped; the mix and the playlist stay.
        assert_eq!(shelves[0].items.len(), 2);
        let ShelfItem::Mix(mix) = &shelves[0].items[0] else {
            panic!("mix")
        };
        assert_eq!(mix.name, "My Daily Discovery");
        assert_eq!(
            shelves[1].more.as_deref(),
            Some("v2/home/pages/recommended_playlists")
        );
        // The video is kept (it opens on TIDAL's website).
        assert_eq!(shelves[2].items.len(), 2);
        assert!(matches!(shelves[3].items[0], ShelfItem::Track(_)));
        assert!(shelves[4].pending);
        assert_eq!(
            shelves[4].source.as_deref(),
            Some("v2/home/pages/your_mixes")
        );
        assert_eq!(feed_cursor(&feed), None);
        assert!(super::shelves(&feed).is_empty(), "a feed is not a v1 page");
    }

    /// Explore's Videos page: "WATCH" promotions as editorial cards and
    /// music videos as video cards.
    #[test]
    fn the_videos_page_has_watch_cards_and_video_shelves() {
        let page = serde_json::json!({"rows": [
            {"modules": [{
                "type": "MULTIPLE_TOP_PROMOTIONS",
                "title": "Featured",
                "items": [{
                    "header": "Watch",
                    "shortHeader": "LP Giobbi on SESSIONS",
                    "shortSubHeader": "Austin-based electronica musician",
                    "imageId": "aa-bb",
                    "type": "VIDEO",
                    "artifactId": 123
                }]
            }]},
            {"modules": [{
                "type": "VIDEO_LIST",
                "title": "New Music Videos",
                "pagedList": {"items": [
                    {"id": 7, "title": "Stan", "duration": 400, "imageId": "cc-dd",
                     "artists": [{"id": 1, "name": "Eminem"}], "type": "Music Video"}
                ]}
            }]}
        ]});
        let shelves = shelves(&page);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].promos.len(), 1);
        assert_eq!(shelves[0].promos[0].kicker, "WATCH");
        assert_eq!(shelves[0].promos[0].title, "LP Giobbi on SESSIONS");
        assert!(matches!(&shelves[0].promos[0].item, ShelfItem::Video(video) if video.id == "123"));
        let ShelfItem::Video(video) = &shelves[1].items[0] else {
            panic!("a video")
        };
        assert_eq!((video.id.as_str(), video.name.as_str()), ("7", "Stan"));
        assert_eq!(video.duration, 400);
    }

    #[test]
    fn garbage_never_fails_a_page() {
        for text in [
            "{}",
            "[]",
            "null",
            r#"{"items": 3}"#,
            r#"{"items": [1, "x", null]}"#,
        ] {
            let value: Value = serde_json::from_str(text).unwrap();
            assert!(feed_shelves(&value).is_empty());
            assert!(shelves(&value).is_empty());
            assert!(shelf_items(&value).is_empty());
        }
    }
    /// An artist page lists Playlists and Credits among its modules, as
    /// tidalapi's and tiddl's `pages/artist` fixtures do; a page without
    /// them gives neither.
    #[test]
    fn an_artist_page_gives_its_playlists_and_credits() {
        let page = serde_json::json!({ "rows": [ { "modules": [
            { "type": "ARTIST_HEADER", "title": "" },
            { "type": "ALBUM_LIST", "title": "Albums", "pagedList": { "items": [
                { "id": 1, "title": "One", "type": "ALBUM" } ] } },
            { "type": "PLAYLIST_LIST", "title": "Playlists",
              "showMore": { "title": "View all", "apiPath": "pages/data/abc" },
              "pagedList": { "items": [
                { "uuid": "p-1", "title": "This Is Bonobo", "type": "EDITORIAL",
                  "creator": {}, "numberOfTracks": 50 } ] } },
            { "type": "TRACK_LIST", "title": "Credits",
              "showMore": { "title": "View all", "apiPath": "pages/data/def" },
              "pagedList": { "items": [
                { "id": 7, "title": "Kerala", "duration": 270,
                  "audioAnalysisAttributes": { "bpm": "120", "key": "A", "keyScale": "MINOR" },
                  "artist": { "id": 2, "name": "Bonobo" },
                  "artists": [ { "id": 2, "name": "Bonobo" } ],
                  "album": { "id": 3, "title": "Migration" } } ] } }
        ] } ] });
        let modules = artist_modules(&shelves(&page));
        assert_eq!(modules.playlists.len(), 1);
        assert_eq!(modules.playlists[0].id, "p-1");
        assert_eq!(modules.playlists_more.as_deref(), Some("pages/data/abc"));
        assert_eq!(modules.credits.len(), 1);
        assert_eq!(modules.credits_more.as_deref(), Some("pages/data/def"));
        assert!(
            modules.credits[0].audio_analysis.is_some(),
            "the credits keep BPM and key for their columns"
        );
        let bare = serde_json::json!({ "rows": [ { "modules": [
            { "type": "ALBUM_LIST", "title": "Albums", "pagedList": { "items": [
                { "id": 1, "title": "One", "type": "ALBUM" } ] } } ] } ] });
        assert_eq!(
            artist_modules(&shelves(&bare)),
            crate::api::models::ArtistModules::default()
        );
    }

    /// Playlists are found by module type, whatever the language of the
    /// titles; Credits by title, else as the track list with BPM and key.
    #[test]
    fn artist_modules_match_by_type_before_title() {
        let page = serde_json::json!({ "rows": [ { "modules": [
            { "type": "TRACK_LIST", "title": "Top Tracks", "pagedList": { "items": [
                { "id": 1, "title": "Plain", "duration": 100, "album": { "id": 3, "title": "A" } } ] } },
            { "type": "PLAYLIST_LIST", "title": "Wiedergabelisten", "pagedList": { "items": [
                { "uuid": "p-1", "title": "This Is", "type": "EDITORIAL", "creator": {} } ] } },
            { "type": "TRACK_LIST", "title": "Mitwirkende", "pagedList": { "items": [
                { "id": 7, "title": "Kerala", "duration": 270, "bpm": 120, "key": "A",
                  "keyScale": "MINOR", "album": { "id": 3, "title": "Migration" } } ] } }
        ] } ] });
        let modules = artist_modules(&shelves(&page));
        assert_eq!(modules.playlists.len(), 1);
        assert_eq!(modules.credits.len(), 1);
        assert_eq!(modules.credits[0].name, "Kerala");
        let analysis = modules.credits[0].audio_analysis.as_ref().unwrap();
        assert_eq!(analysis.bpm, Some(120));
        assert_eq!(analysis.camelot(), "8A");
    }

    fn album_page() -> Value {
        serde_json::from_str(include_str!("fixtures/page_album.json")).unwrap()
    }

    fn album_kinds(page: &Value) -> Vec<AlbumShelfKind> {
        album_shelves(shelves(page))
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    }

    /// The Fall-Off (live, 2026-10-18): the page's four list modules are
    /// Other versions, More Albums by J. Cole, Related Albums and Related
    /// Artists, in that order, each with the page behind its View all.
    #[test]
    fn an_album_page_becomes_its_four_shelves_in_order() {
        let found = album_shelves(shelves(&album_page()));
        let kinds: Vec<AlbumShelfKind> = found.iter().map(|entry| entry.kind).collect();
        assert_eq!(
            kinds,
            [
                AlbumShelfKind::OtherVersions,
                AlbumShelfKind::MoreByArtist,
                AlbumShelfKind::RelatedAlbums,
                AlbumShelfKind::RelatedArtists,
            ]
        );
        assert_eq!(found[1].artist_id.as_deref(), Some("3652822"));
        assert_eq!(found[0].artist_id, None);
        for entry in &found {
            assert_eq!(entry.shelf.items.len(), 3, "{:?}", entry.kind);
            let more = entry.shelf.more.as_deref().unwrap();
            assert!(more.starts_with("pages/single-module-page/"), "{more}");
        }
        assert!(matches!(found[0].shelf.items[0], ShelfItem::Album(_)));
        assert!(matches!(found[3].shelf.items[0], ShelfItem::Artist(_)));
    }

    /// A module that arrives without cards is no shelf (no empty heading).
    #[test]
    fn an_album_shelf_without_cards_is_left_out() {
        let mut page = album_page();
        let rows = page["rows"].as_array_mut().unwrap();
        // Other versions emptied.
        let list = rows[2]["modules"][0]["pagedList"].as_object_mut().unwrap();
        list.insert("items".into(), serde_json::json!([]));
        list.remove("dataApiPath");
        rows[2]["modules"][0]["showMore"] = Value::Null;
        assert_eq!(
            album_kinds(&page),
            [
                AlbumShelfKind::MoreByArtist,
                AlbumShelfKind::RelatedAlbums,
                AlbumShelfKind::RelatedArtists
            ]
        );
    }

    /// An album TIDAL has no other versions of has no such module: the first
    /// album list without an artist is Related Albums.
    #[test]
    fn related_albums_are_not_mistaken_for_other_versions() {
        let mut page = album_page();
        page["rows"].as_array_mut().unwrap().remove(2);
        assert_eq!(
            album_kinds(&page),
            [
                AlbumShelfKind::MoreByArtist,
                AlbumShelfKind::RelatedAlbums,
                AlbumShelfKind::RelatedArtists
            ]
        );
    }

    /// An album by two main artists (Revenge Of The Dreamers III) has a
    /// More Albums shelf for each.
    #[test]
    fn an_album_by_two_artists_has_a_shelf_for_each() {
        let mut page = album_page();
        let rows = page["rows"].as_array_mut().unwrap();
        let mut second = rows[3].clone();
        let more = second["modules"][0]["showMore"]["apiPath"]
            .as_str()
            .unwrap()
            .replace("artistId=3652822", "artistId=15390768");
        second["modules"][0]["showMore"]["apiPath"] = Value::String(more);
        rows.insert(3, second);
        let found = album_shelves(shelves(&page));
        let by: Vec<Option<&str>> = found
            .iter()
            .filter(|entry| entry.kind == AlbumShelfKind::MoreByArtist)
            .map(|entry| entry.artist_id.as_deref())
            .collect();
        assert_eq!(by, [Some("15390768"), Some("3652822")]);
    }
}
