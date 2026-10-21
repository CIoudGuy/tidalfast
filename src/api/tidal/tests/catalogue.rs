//! Playing an artist goes past their top tracks into the rest of their
//! own catalogue, once each song.

use super::*;
use crate::api::tidal_catalog::TidalCatalog;
use crate::tidal_engine::Catalog as _;

const ARTIST: &str = "9";

fn song(id: u32, isrc: &str, artist: &str) -> String {
    format!(
        r#"{{"id":{id},"title":"Song {id}","duration":200,"isrc":"{isrc}","allowStreaming":true,"streamReady":true,"artists":[{{"id":{artist},"name":"A"}}],"artist":{{"id":{artist},"name":"A"}}}}"#
    )
}

fn page(items: &[String], total: usize) -> String {
    format!(
        r#"{{"limit":100,"offset":0,"totalNumberOfItems":{total},"items":[{}]}}"#,
        items.join(",")
    )
}

fn release(id: u32) -> String {
    format!(r#"{{"id":{id},"title":"Release {id}","type":"ALBUM"}}"#)
}

/// An artist page module's "View all" (`v2/artist/<MODULE>/view-all`).
fn module(releases: &[String]) -> String {
    let items: Vec<String> = releases
        .iter()
        .map(|release| format!(r#"{{"type":"ALBUM","data":{release}}}"#))
        .collect();
    format!(
        r#"{{"title":"Albums","itemLayout":"GRID","items":[{}]}}"#,
        items.join(",")
    )
}

#[tokio::test]
async fn an_artist_plays_top_tracks_then_their_albums_without_repeats() {
    // 25 top tracks (1-25), then two albums of 60 new songs each. The first
    // repeats five top tracks; the second repeats a top track under another
    // id (same ISRC) and holds a song of someone else's.
    let top: Vec<String> = (1..=25)
        .map(|id| song(id, &format!("ISRC{id}"), ARTIST))
        .collect();
    let mut first: Vec<String> = (1..=5)
        .map(|id| song(id, &format!("ISRC{id}"), ARTIST))
        .collect();
    first.extend((100..160).map(|id| song(id, &format!("ISRC{id}"), ARTIST)));
    let mut second = vec![song(300, "isrc7", ARTIST), song(301, "OTHER", "77")];
    second.extend((200..260).map(|id| song(id, &format!("ISRC{id}"), ARTIST)));
    // The releases are the artist page's: its Albums (no compilations or
    // live albums), then its EP & Singles, empty here, which falls back to
    // `artists/{id}/albums` (empty too).
    let (base, seen) = script(vec![
        reply("200 OK", "", &page(&top, 25)),
        reply("200 OK", "", &module(&[release(1), release(2)])),
        reply("200 OK", "", &module(&[])),
        reply("200 OK", "", &module(&[])),
        reply("200 OK", "", &page(&first, first.len())),
        reply("200 OK", "", &page(&second, second.len())),
        reply("200 OK", "", &module(&[])),
        reply("200 OK", "", &page(&[], 0)),
    ])
    .await;
    let catalog = TidalCatalog::new(
        Arc::new(client(&base, refresher())),
        tokio::runtime::Handle::current(),
    );
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut pages: Vec<Vec<String>> = Vec::new();
        let result = catalog.context_pages(&format!("tidal:artist:{ARTIST}"), &mut |page| {
            pages.push(page.into_iter().map(|track| track.uri).collect());
            true
        });
        let _ = sender.send((result.is_ok(), pages));
    });
    let (ok, pages) = receiver.await.unwrap();
    assert!(ok);
    // The top tracks come first, as the first page.
    assert_eq!(pages[0].len(), 25);
    assert_eq!(pages[0][0], "tidal:track:1");
    let all: Vec<&String> = pages.iter().flatten().collect();
    let unique: std::collections::HashSet<&&String> = all.iter().collect();
    assert_eq!(all.len(), unique.len(), "no song twice");
    assert_eq!(all.len(), 25 + 120);
    assert!(!all.iter().any(|uri| *uri == "tidal:track:301"));
    assert!(!all.iter().any(|uri| *uri == "tidal:track:300"));
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /artists/9/toptracks?"));
    assert!(seen[1].starts_with("GET /artist/ARTIST_ALBUMS/view-all?"));
    assert!(seen[1].contains("artistId=9"));
    assert!(seen[2].starts_with("GET /artist/ARTIST_COMPILATIONS/view-all?"));
    assert!(seen[3].starts_with("GET /artist/ARTIST_LIVE_ALBUMS/view-all?"));
    assert!(seen[6].starts_with("GET /artist/ARTIST_TOP_SINGLES/view-all?"));
    assert!(seen[7].starts_with("GET /artists/9/albums?"));
    assert!(seen[7].contains("filter=EPSANDSINGLES"));
}
