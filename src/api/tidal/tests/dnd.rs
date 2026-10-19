//! The playlist writes behind drag and drop, against a scripted local server.

use super::*;

#[tokio::test]
async fn moving_a_playlist_item_posts_its_index_and_target() {
    let etag = "ETag: \"7\"\r\n";
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            etag,
            r#"{"limit":1,"offset":0,"totalNumberOfItems":4,"items":[]}"#,
        ),
        reply("200 OK", "", r#"{"lastUpdated":8}"#),
    ])
    .await;
    let updated = client(&base, refresher())
        .reorder_playlist("p-1", 3, 0, None)
        .await
        .unwrap();
    assert_eq!(updated.as_deref(), Some("8"));
    let seen = seen.lock().unwrap();
    assert!(seen[1].starts_with("POST /playlists/p-1/items/3?"));
    assert!(seen[1].contains("if-none-match: \"7\""));
    assert!(seen[1].contains("toIndex=0"));
}

#[tokio::test]
async fn removing_playlist_indices_deletes_from_the_end() {
    let etag = "ETag: \"7\"\r\n";
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            etag,
            r#"{"limit":1,"offset":0,"totalNumberOfItems":4,"items":[]}"#,
        ),
        reply("200 OK", "", r#"{"lastUpdated":9}"#),
    ])
    .await;
    client(&base, refresher())
        .remove_playlist_indices("p-1", &[1, 3])
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen[1].starts_with("DELETE /playlists/p-1/items/3%2C1?")
            || seen[1].starts_with("DELETE /playlists/p-1/items/3,1?"),
        "{}",
        seen[1]
    );
}

#[tokio::test]
async fn dropping_an_album_lists_its_songs_page_by_page() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        r#"{"limit":100,"offset":0,"totalNumberOfItems":2,"items":[{"id":11,"title":"A"},{"id":12,"title":"B"}]}"#,
    )])
    .await;
    let tracks = client(&base, refresher())
        .context_tracks("tidal:album:99")
        .await
        .unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].uri, "tidal:track:11");
    assert!(seen.lock().unwrap()[0].starts_with("GET /albums/99/tracks?"));
}

/// A mix's songs are read from its items, for Play next on a mix whose
/// page was never opened.
#[tokio::test]
async fn a_mix_lists_its_songs() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        r#"{"limit":100,"offset":0,"totalNumberOfItems":2,"items":[{"type":"track","item":{"id":21,"title":"A"}},{"type":"track","item":{"id":22,"title":"B"}}]}"#,
    )])
    .await;
    let tracks = client(&base, refresher())
        .context_tracks("tidal:mix:abc")
        .await
        .unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[1].uri, "tidal:track:22");
    assert!(seen.lock().unwrap()[0].starts_with("GET /mixes/abc/items?"));
}

#[tokio::test]
async fn an_artist_has_no_songs_to_add() {
    let (base, seen) = script(vec![]).await;
    let tracks = client(&base, refresher())
        .context_tracks("tidal:artist:abc")
        .await
        .unwrap();
    assert!(tracks.is_empty());
    assert!(seen.lock().unwrap().is_empty());
}
