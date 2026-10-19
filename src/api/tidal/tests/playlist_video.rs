//! Playlists that hold a video: TIDAL's v1 item read answers 500 for them
//! (live 2026-10-18), so they are read through openapi v2.

use super::*;

const FAIL: &str = "500 Internal Server Error";

/// A v2 relationship page: a song, a video and a song, in that order.
fn document(next: Option<&str>) -> String {
    let links = next.map_or(String::new(), |cursor| {
        format!(
            r#","links":{{"next":"/playlists/p-1/relationships/items?page%5Bcursor%5D={cursor}&include=items"}}"#
        )
    });
    format!(
        r#"{{"data":[
            {{"id":"5","type":"tracks","meta":{{"addedAt":"2026-10-18T09:00:00Z","itemId":"a"}}}},
            {{"id":"441699021","type":"videos","meta":{{"addedAt":"2026-10-18T09:01:00Z","itemId":"b"}}}},
            {{"id":"6","type":"tracks","meta":{{"addedAt":"2026-10-18T09:02:00Z","itemId":"c"}}}}],
          "included":[
            {{"id":"5","type":"tracks","attributes":{{"title":"First","duration":"PT3M45S","explicit":true}},
              "relationships":{{"artists":{{"data":[{{"id":"1","type":"artists"}}]}},"albums":{{"data":[{{"id":"9","type":"albums"}}]}}}}}},
            {{"id":"6","type":"tracks","attributes":{{"title":"Third","duration":"PT2M"}},
              "relationships":{{"artists":{{"data":[{{"id":"1","type":"artists"}}]}}}}}},
            {{"id":"441699021","type":"videos","attributes":{{"title":"The Video","duration":"PT4M10S"}},
              "relationships":{{"artists":{{"data":[{{"id":"1","type":"artists"}}]}},"thumbnailArt":{{"data":[{{"id":"t1","type":"artworks"}}]}}}}}},
            {{"id":"1","type":"artists","attributes":{{"name":"Daft Punk"}}}},
            {{"id":"9","type":"albums","attributes":{{"title":"RAM"}},"relationships":{{"coverArt":{{"data":[{{"id":"c1","type":"artworks"}}]}}}}}},
            {{"id":"c1","type":"artworks","attributes":{{"files":[{{"href":"https://resources.tidal.com/images/aaaaaaaa/bbbb/cccc/dddd/eeeeeeeeeeee/1280x1280.jpg"}}]}}}},
            {{"id":"t1","type":"artworks","attributes":{{"files":[{{"href":"https://resources.tidal.com/images/11111111/2222/3333/4444/555555555555/640x360.jpg"}}]}}}}
          ]{links}}}"#
    )
}

fn failing_reads(count: usize) -> Vec<String> {
    (0..count).map(|_| reply(FAIL, "", "{}")).collect()
}

#[tokio::test]
async fn a_playlist_with_a_video_is_read_through_v2_and_kept_for_the_next_page() {
    let mut replies = failing_reads(4);
    replies.push(reply("200 OK", "", &document(Some("next"))));
    replies.push(reply("200 OK", "", &document(None)));
    let (base, seen) = script(replies).await;
    let client = client(&base, refresher());
    let page = client.playlist_items("p-1", 0, 100).await.unwrap();
    assert_eq!(page.total, 6);
    let uris: Vec<&str> = page
        .items
        .iter()
        .map(|item| item.playable().unwrap().uri())
        .collect();
    assert_eq!(
        uris,
        [
            "tidal:track:5",
            "tidal:video:441699021",
            "tidal:track:6",
            "tidal:track:5",
            "tidal:video:441699021",
            "tidal:track:6"
        ]
    );
    let song = page.items[0].playable().unwrap();
    assert_eq!(song.name(), "First");
    assert_eq!(song.duration_ms(), 225_000);
    assert_eq!(song.subtitle(), "Daft Punk");
    assert!(
        song.image(640)
            .is_some_and(|url| url.contains("aaaaaaaa/bbbb/cccc/dddd/eeeeeeeeeeee")),
        "the album art of the song"
    );
    assert_eq!(
        page.items[0].added_at.as_deref(),
        Some("2026-10-18T09:00:00Z")
    );
    let video = page.items[1].playable().unwrap();
    assert_eq!(video.name(), "The Video");
    assert_eq!(video.duration_ms(), 250_000);
    assert!(
        video
            .image(640)
            .is_some_and(|url| url.contains("11111111/2222/3333/4444/555555555555")),
        "the thumbnail of the video"
    );
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 6, "four failed v1 reads, two v2 pages");
        assert!(
            seen[0].starts_with("GET /playlists/p-1/items"),
            "{}",
            seen[0]
        );
        assert!(
            seen[4].starts_with("GET /playlists/p-1/relationships/items"),
            "{}",
            seen[4]
        );
        assert!(seen[5].contains("page%5Bcursor%5D=next"), "{}", seen[5]);
    }
    // The next page, and the songs for the engine, come from what was read:
    // no further request, and no v1 attempt for a playlist known to fail.
    let rest = client.playlist_items("p-1", 4, 100).await.unwrap();
    assert_eq!(rest.items.len(), 2);
    let playable = client.playlist_tracks("p-1", 0, 100).await.unwrap();
    assert_eq!(playable.items.len(), 6);
    assert_eq!(playable.items[1].uri, "tidal:video:441699021");
    assert_eq!(seen.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn a_playlist_without_a_video_costs_no_v2_request() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("../fixtures/playlist_items.json"),
    )])
    .await;
    let page = client(&base, refresher())
        .playlist_items("p-1", 0, 100)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 4);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].starts_with("GET /playlists/p-1/items"),
        "{}",
        seen[0]
    );
}

#[tokio::test]
async fn a_failing_v2_read_leaves_the_v1_error() {
    let mut replies = failing_reads(4);
    replies.extend(failing_reads(4));
    let (base, _) = script(replies).await;
    let error = client(&base, refresher())
        .playlist_items("p-1", 0, 100)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ApiError::Status { status: 500, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_edit_takes_the_etag_from_the_playlist_when_v1_cannot_list_it() {
    let mut replies = failing_reads(4);
    replies.push(reply(
        "200 OK",
        "ETag: \"77\"\r\n",
        r#"{"uuid":"p-1","title":"T","numberOfTracks":2,"numberOfVideos":1}"#,
    ));
    replies.push(reply("200 OK", "", r#"{"lastUpdated":5}"#));
    let (base, seen) = script(replies).await;
    let updated = client(&base, refresher())
        .reorder_playlist("p-1", 2, 0, None)
        .await
        .unwrap();
    assert_eq!(updated.as_deref(), Some("5"));
    let seen = seen.lock().unwrap();
    assert!(seen[4].starts_with("GET /playlists/p-1?"), "{}", seen[4]);
    assert!(
        seen[5].starts_with("POST /playlists/p-1/items/2"),
        "{}",
        seen[5]
    );
    assert!(seen[5].contains("if-none-match: \"77\""), "{}", seen[5]);
}

#[tokio::test]
async fn a_plugin_reading_the_items_gets_the_v2_page_in_the_v1_shape() {
    let mut replies = failing_reads(4);
    replies.push(reply("200 OK", "", &document(None)));
    let (base, _) = script(replies).await;
    let (status, body, etag) = client(&base, refresher())
        .v1_call(
            "GET",
            "playlists/p-1/items",
            &[("limit", "100".to_string()), ("offset", "0".to_string())],
            &[],
            None,
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(etag, None);
    let page: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(page["totalNumberOfItems"], 3);
    assert_eq!(page["items"][1]["type"], "video");
    assert_eq!(page["items"][1]["item"]["id"], "441699021");
    assert_eq!(page["items"][0]["item"]["title"], "First");
}
