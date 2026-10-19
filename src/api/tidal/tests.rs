use super::*;
use crate::auth::tidal::tests::{FakeRefresher, grant};
use std::sync::atomic::{AtomicUsize, Ordering};

mod catalogue;
mod creators;
mod dnd;
mod folders;
mod playlist_video;

fn fixture<T: DeserializeOwned>(text: &str) -> T {
    serde_json::from_str(text).unwrap()
}

#[test]
fn a_track_maps_with_uri_artists_album_art_and_availability() {
    let track = convert::track(fixture(include_str!("fixtures/track.json")));
    assert_eq!(track.id.as_deref(), Some("77646168"));
    assert_eq!(track.uri, "tidal:track:77646168");
    assert_eq!(
        track.name,
        "Get Lucky (feat. Pharrell Williams & Nile Rodgers)"
    );
    assert_eq!(track.duration_ms, 369_000);
    assert_eq!(track.track_number, Some(8));
    assert_eq!(track.disc_number, Some(1));
    assert_eq!(track.is_playable, Some(true));
    assert_eq!(track.external_ids.isrc.as_deref(), Some("USQX91300108"));
    assert_eq!(track.artist_names(), "Daft Punk, Pharrell Williams");
    assert_eq!(
        track.artists[1].uri.as_deref(),
        Some("tidal:artist:3532225")
    );
    let album = track.album.as_ref().unwrap();
    assert_eq!(album.uri, "tidal:album:77646164");
    assert_eq!(album.images.len(), 5);
    assert!(track.image(600).unwrap().ends_with("/640x640.jpg"));
    assert_eq!(
        track.external_urls.tidal.as_deref(),
        Some("https://www.tidal.com/track/77646168")
    );
}

#[test]
fn an_album_maps_and_its_tracks_keep_unavailable_ones_greyed() {
    let album = convert::album(fixture(include_str!("fixtures/album.json")));
    assert_eq!(album.uri, "tidal:album:77646164");
    assert_eq!(album.album_type.as_deref(), Some("album"));
    assert_eq!(album.total_tracks, Some(13));
    assert_eq!(album.year(), Some("2013"));
    assert_eq!(album.artists[0].name, "Daft Punk");
    // The header's avatar: the album's artist carries its picture.
    assert_eq!(
        album.artists[0].picture.as_deref(),
        Some("https://resources.tidal.com/images/5e5f5c1b/0f34/4b62/9b54/0d6cbe4ec2a1/160x160.jpg")
    );
    assert_eq!(album.copyrights[0].text, "(P) 2013 Daft Life Limited");
    let page: RawPage<RawTrack> = fixture(include_str!("fixtures/album_tracks.json"));
    let page = page.map(100, convert::track);
    assert_eq!(page.items.len(), 2);
    // A song's artists hold no picture: no row draws one.
    assert!(
        page.items
            .iter()
            .flat_map(|track| &track.artists)
            .all(|artist| artist.picture.is_none())
    );
    assert_eq!(page.items[1].is_playable, Some(false));
    assert_eq!(page.next_offset(), None);
}

#[test]
fn playlist_items_keep_positions_and_show_videos_as_video_rows() {
    let page: RawPage<Wrapped<RawTrack>> = fixture(include_str!("fixtures/playlist_items.json"));
    let items = playlist_items_page(page, 100);
    assert_eq!(items.items.len(), 4);
    let video = items.items[1].playable().expect("the video has a row");
    assert_eq!(video.uri(), "tidal:video:12345");
    assert_eq!(video.name(), "Get Lucky (Video)");
    assert_eq!(video.duration_ms(), 250_000);
    assert_eq!(
        items.items[2].playable().unwrap().uri(),
        "tidal:track:77646169"
    );
    let page: RawPage<Wrapped<RawTrack>> = fixture(include_str!("fixtures/playlist_items.json"));
    let tracks = tracks_page(page, 100);
    assert_eq!(tracks.items.len(), 3);
    assert_eq!(tracks.total, 4);
    let page: RawPage<Wrapped<RawTrack>> = fixture(include_str!("fixtures/playlist_items.json"));
    let playable = playlist_tracks_page(page, 100);
    assert_eq!(playable.items.len(), 4, "a playlist plays its videos too");
    assert_eq!(playable.items[1].uri, "tidal:video:12345");
    assert_eq!(playable.total, 4);
}

#[test]
fn playlists_know_their_owner_and_art() {
    let playlist = convert::playlist(fixture(include_str!("fixtures/playlist.json")));
    assert_eq!(
        playlist.uri,
        "tidal:playlist:11111111-aaaa-4bbb-8ccc-000000000003"
    );
    assert_eq!(playlist.name, "My Friday");
    assert!(playlist.owned_by("12345678"));
    assert_eq!(playlist.track_total(), 3);
    assert_eq!(playlist.public, Some(false));
    assert_eq!(playlist.images[0].width, Some(1080), "square art preferred");
    assert_eq!(playlist.images[0].height, Some(1080));
}

#[test]
fn favourites_map_with_dates_and_paging() {
    let page: RawPage<Wrapped<RawTrack>> = fixture(include_str!("fixtures/favorites_tracks.json"));
    let saved = saved_tracks_page(page, 2);
    assert_eq!(saved.items.len(), 2);
    assert_eq!(saved.total, 5);
    assert_eq!(
        saved.items[0].added_at.as_deref(),
        Some("2026-10-01T12:00:00.000+0000")
    );
    assert_eq!(saved.next_offset(), Some(2));
    let albums: RawPage<Wrapped<RawAlbum>> =
        fixture(include_str!("fixtures/favorites_albums.json"));
    let albums = albums.map(50, |wrapped| wrapped.item.map(convert::album).unwrap());
    assert_eq!(albums.items[0].name, "Random Access Memories");
    let ids: FavouriteIds = fixture(include_str!("fixtures/favorites_ids.json"));
    let none = HashSet::new();
    assert!(ids.contains("tidal:track:77646168", &none));
    assert!(ids.contains("tidal:album:77646164", &none));
    assert!(ids.contains("tidal:artist:8847", &none));
    assert!(ids.contains("tidal:playlist:11111111-aaaa-4bbb-8ccc-000000000003", &none));
    assert!(!ids.contains("tidal:track:1", &none));
    assert!(!ids.contains("other:track:77646168", &none));
    let mixes = HashSet::from(["m1".to_string()]);
    assert!(ids.contains("tidal:mix:m1", &mixes));
}

#[test]
fn mixes_in_both_shapes_map() {
    let page: RawPage<RawMix> = fixture(include_str!("fixtures/favorites_mixes.json"));
    let mixes = page.map(50, convert::mix);
    assert_eq!(mixes.items[0].name, "My Mix 1");
    assert_eq!(
        mixes.items[0].subtitle.as_deref(),
        Some("Daft Punk, Justice and more")
    );
    assert_eq!(mixes.items[0].images.len(), 3);
    assert_eq!(mixes.items[1].uri, "tidal:mix:00221");
    assert_eq!(mixes.items[1].name, "Get Lucky Radio");
    assert_eq!(mixes.items[1].mix_type.as_deref(), Some("TRACK_MIX"));
    assert_eq!(mixes.items[1].images[0].width, Some(1500));
    let items: RawPage<Wrapped<RawTrack>> = fixture(include_str!("fixtures/mix_items.json"));
    assert_eq!(tracks_page(items, 100).items.len(), 2);
}

#[test]
fn search_maps_every_section_and_the_top_hit() {
    let results = search_results(fixture(include_str!("fixtures/search.json")));
    assert_eq!(results.tracks.as_ref().unwrap().total, 300);
    assert_eq!(results.tracks.as_ref().unwrap().next_offset(), Some(10));
    let artist = &results.artists.as_ref().unwrap().items[0];
    assert!(!artist.images.is_empty(), "album cover fallback");
    let album = &results.albums.as_ref().unwrap().items[0];
    assert_eq!(album.artists[0].name, "Daft Punk", "artists without artist");
    let playlist = &results.playlists.as_ref().unwrap().items[0];
    assert_eq!(playlist.owner.display_name.as_deref(), Some("TIDAL"));
    assert_eq!(playlist.owner.id, None);
    assert_eq!(playlist.description, None);
    assert!(
        !playlist.images.is_empty(),
        "wide image when there is no square one"
    );
    let Some(TopHit::Artist(top)) = &results.top_hit else {
        panic!("top hit is the artist")
    };
    assert_eq!(top.uri, "tidal:artist:8847");
    assert!(!results.is_empty());
}

#[test]
fn synced_lyrics_become_sorted_timed_lines() {
    let lyrics = lyrics(fixture(include_str!("fixtures/lyrics.json"))).unwrap();
    assert!(lyrics.synced);
    let times: Vec<Option<u32>> = lyrics.lines.iter().map(|line| line.at_ms).collect();
    assert_eq!(
        times,
        [Some(28_120), Some(31_400), Some(62_340), Some(138_560)]
    );
    assert_eq!(lyrics.lines[3].text, "We are up all night to get lucky");
    let plain = super::lyrics(RawLyrics {
        lyrics: Some("one\ntwo".into()),
        subtitles: Some(String::new()),
    })
    .unwrap();
    assert!(!plain.synced);
    assert_eq!(plain.lines.len(), 2);
    assert!(super::lyrics(RawLyrics::default()).is_none());
}

#[test]
fn identity_documents_decode() {
    let session: SessionInfo = fixture(include_str!("fixtures/sessions.json"));
    assert_eq!(session.user_id, 12345678);
    assert_eq!(session.country_code, "NO");
    let _: Value = fixture(include_str!("fixtures/user.json"));
    let _: Value = fixture(include_str!("fixtures/subscription.json"));
}

#[test]
fn errors_read_like_sentences() {
    let error = ErrorBody::parse(r#"{"status":403,"subStatus":4035,"userMessage":"x"}"#);
    assert!(readable(403, &error).contains("country"));
    let error = ErrorBody::parse(
        r#"{"status":401,"subStatus":4005,"userMessage":"Asset is not ready for playback"}"#,
    );
    assert!(!error.token_expired());
    assert!(readable(401, &error).contains("not available"));
    let error = ErrorBody::parse(
        r#"{"status":401,"subStatus":11002,"userMessage":"The token has expired. (Expired on time)"}"#,
    );
    assert!(error.token_expired());
    assert_eq!(
        readable(500, &ErrorBody::default()),
        "TIDAL answered HTTP 500."
    );
}

#[test]
fn release_groups_follow_include_groups() {
    use ReleaseGroup::*;
    assert_eq!(
        ReleaseGroup::parse("album,single,compilation,appears_on"),
        [Albums, EpsAndSingles, Compilations]
    );
    assert_eq!(ReleaseGroup::parse("single"), [EpsAndSingles]);
    assert_eq!(ReleaseGroup::parse(""), [Albums]);
}

#[test]
fn uris_are_checked_by_scheme_and_kind() {
    assert_eq!(uri_id("tidal:track:1", "track"), Some("1"));
    assert_eq!(uri_id("tidal:album:1", "track"), None);
    assert_eq!(uri_id("other:track:1", "track"), None);
    assert_eq!(uri_id("tidal:track:", "track"), None);
}

// ---- transport, against a scripted local server --------------------------------

/// Serves `replies` in order, one per connection, and records each request
/// head so a test can inspect what was sent.
async fn script(replies: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 16 * 1024];
            let read = socket.read(&mut buffer).await.unwrap();
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buffer[..read]).into_owned());
            socket.write_all(reply.as_bytes()).await.unwrap();
            let _ = socket.shutdown().await;
        }
    });
    (base, seen)
}

fn reply(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
}

fn client(base: &str, refresher: Arc<FakeRefresher>) -> TidalClient {
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let client = TidalClient::with_bases(http, Arc::new(NetActivity::default()), base, base, base);
    let far = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    client.set_tokens(Some(TidalTokens::new(grant("stale", far), refresher, None)));
    client
}

fn refresher() -> Arc<FakeRefresher> {
    Arc::new(FakeRefresher {
        calls: AtomicUsize::new(0),
        fail: false,
    })
}

#[tokio::test]
async fn an_expired_token_is_refreshed_once_and_the_request_retried() {
    let (base, seen) = script(vec![
        reply(
            "401 Unauthorized",
            "",
            r#"{"status":401,"subStatus":11002,"userMessage":"The token has expired. (Expired on time)"}"#,
        ),
        reply("200 OK", "", include_str!("fixtures/track.json")),
    ])
    .await;
    let refresher = refresher();
    let client = client(&base, refresher.clone());
    let track = client.track("77646168").await.unwrap();
    assert_eq!(track.uri, "tidal:track:77646168");
    assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
    let seen = seen.lock().unwrap();
    assert!(seen[0].contains("authorization: Bearer stale"));
    assert!(seen[1].contains("authorization: Bearer fresh-1"));
    assert!(seen[1].starts_with("GET /tracks/77646168?countryCode=US "));
    assert!(seen[1].contains("x-tidal-client-version"));
}

#[tokio::test]
async fn an_availability_401_is_not_mistaken_for_expiry() {
    let not_ready =
        r#"{"status":401,"subStatus":4005,"userMessage":"Asset is not ready for playback"}"#;
    let (base, _) = script(vec![
        reply("401 Unauthorized", "", not_ready),
        reply("401 Unauthorized", "", not_ready),
    ])
    .await;
    let refresher = refresher();
    let error = client(&base, refresher.clone())
        .playback_info("1", Quality::Lossless)
        .await
        .unwrap_err();
    assert_eq!(error.status(), Some(401));
    assert_eq!(refresher.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rate_limits_are_waited_out() {
    let (base, seen) = script(vec![
        reply("429 Too Many Requests", "Retry-After: 2\r\n", "{}"),
        reply("200 OK", "", include_str!("fixtures/sessions.json")),
    ])
    .await;
    let session = client(&base, refresher()).session().await.unwrap();
    assert_eq!(session.country_code, "NO");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn playlist_edits_send_the_etag_as_if_none_match() {
    let etag = "ETag: \"1700000000000\"\r\n";
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            etag,
            r#"{"limit":1,"offset":0,"totalNumberOfItems":4,"items":[]}"#,
        ),
        reply(
            "200 OK",
            "",
            r#"{"lastUpdated":1700000000001,"addedItemIds":[1]}"#,
        ),
    ])
    .await;
    let updated = client(&base, refresher())
        .add_playlist_items(
            "p-1",
            &["tidal:track:1".into(), "tidal:track:2".into()],
            None,
        )
        .await
        .unwrap();
    assert_eq!(updated.as_deref(), Some("1700000000001"));
    let seen = seen.lock().unwrap();
    assert!(seen[1].starts_with("POST /playlists/p-1/items?countryCode=US "));
    assert!(seen[1].contains("if-none-match: \"1700000000000\""));
    assert!(seen[1].contains("trackIds=1%2C2&toIndex=4"));
}

/// Live 2026-10-18: `POST users/{id}/favorites/videos` with `videoIds`
/// saves a video, `DELETE .../videos/{id}` takes it out again.
#[tokio::test]
async fn a_video_is_saved_with_a_post_of_its_video_id_and_removed_with_a_delete() {
    let (base, seen) = script(vec![reply("200 OK", "", ""), reply("200 OK", "", "")]).await;
    let client = client(&base, refresher());
    client
        .save(&["tidal:video:441699021".into()])
        .await
        .unwrap();
    client
        .unsave(&["tidal:video:441699021".into()])
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("POST /users/"), "{}", seen[0]);
    assert!(seen[0].contains("/favorites/videos?"), "{}", seen[0]);
    assert!(seen[0].contains("videoIds=441699021"), "{}", seen[0]);
    assert!(seen[1].starts_with("DELETE /users/"), "{}", seen[1]);
    assert!(
        seen[1].contains("/favorites/videos/441699021?"),
        "{}",
        seen[1]
    );
}

#[test]
fn the_favourite_ids_say_whether_a_video_is_saved() {
    let ids: FavouriteIds = fixture(r#"{"VIDEO":["441699021"],"TRACK":["5"]}"#);
    let none = HashSet::new();
    assert!(ids.contains("tidal:video:441699021", &none));
    assert!(!ids.contains("tidal:video:5", &none));
    assert!(ids.contains("tidal:track:5", &none));
}

/// Live 2026-10-18: the v1 `trackIds` field takes songs only; a video goes
/// in through the JSON:API relationship the web player uses.
#[tokio::test]
async fn a_video_is_added_to_a_playlist_through_the_json_api_relationship() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        r#"{"data":[{"id":"441699021","type":"videos"}],"meta":{"skipped":[]}}"#,
    )])
    .await;
    let updated = client(&base, refresher())
        .add_playlist_items("p-1", &["tidal:video:441699021".into()], None)
        .await
        .unwrap();
    assert_eq!(updated, None);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "no playlist read for a video");
    assert!(
        seen[0].starts_with("POST /playlists/p-1/relationships/items"),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains(r#""id":"441699021""#), "{}", seen[0]);
    assert!(seen[0].contains(r#""type":"videos""#), "{}", seen[0]);
    assert!(seen[0].contains(r#""onDuplicates":"ADD""#), "{}", seen[0]);
}

/// Live 2026-10-18: once a playlist holds a video, TIDAL's v1 read of it
/// answers 500, and the edit that adds songs starts with that read. A song
/// for the end of the list then goes in through the JSON:API relationship.
#[tokio::test]
async fn a_song_is_added_through_the_relationship_when_the_v1_read_fails() {
    // Each read is retried three times before it is given up: the items,
    // then the playlist itself.
    let (base, seen) = script(vec![
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("200 OK", "", r#"{"data":[{"id":"5","type":"tracks"}]}"#),
    ])
    .await;
    client(&base, refresher())
        .add_playlist_items("p-1", &["tidal:track:5".into()], None)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /playlists/p-1/items"),
        "{}",
        seen[0]
    );
    assert!(
        seen[8].starts_with("POST /playlists/p-1/relationships/items"),
        "{}",
        seen[8]
    );
    assert!(seen[8].contains(r#""type":"tracks""#), "{}", seen[8]);
}

#[tokio::test]
async fn a_song_is_blocked_with_a_post_of_its_track_id() {
    let (base, seen) = script(vec![reply("200 OK", "", "")]).await;
    client(&base, refresher())
        .block_track("tidal:track:100122415")
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("POST /users/"), "{}", seen[0]);
    assert!(
        seen[0].contains("/blocks/tracks?countryCode=US "),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains("trackId=100122415"), "{}", seen[0]);
    assert!(
        !seen[0].contains("trackIds"),
        "TIDAL answers 404 to the plural"
    );
}

#[tokio::test]
async fn an_artist_is_blocked_with_a_post_of_its_artist_id_and_unblocked_with_a_delete() {
    let (base, seen) = script(vec![
        reply("200 OK", "", ""),
        reply("204 No Content", "", ""),
        reply("404 Not Found", "", "{}"),
    ])
    .await;
    let client = client(&base, refresher());
    client
        .set_artist_blocked("tidal:artist:3532225", true)
        .await
        .unwrap();
    client
        .set_artist_blocked("tidal:artist:3532225", false)
        .await
        .unwrap();
    // Already not blocked is what was asked for.
    client
        .set_artist_blocked("tidal:artist:3532225", false)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("POST /users/"), "{}", seen[0]);
    assert!(
        seen[0].contains("/blocks/artists?countryCode=US "),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains("artistId=3532225"), "{}", seen[0]);
    assert!(
        seen[1].starts_with("DELETE /users/") && seen[1].contains("/blocks/artists/3532225?"),
        "{}",
        seen[1]
    );
}

#[tokio::test]
async fn the_blocked_artists_are_read_page_after_page() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            r#"{"limit":100,"offset":0,"totalNumberOfItems":3,"items":[
                {"created":"2026-10-17","item":{"id":11,"name":"A"},"type":"ARTIST"},
                {"created":"2026-10-17","item":{"id":22,"name":"B"},"type":"ARTIST"}]}"#,
        ),
        reply(
            "200 OK",
            "",
            r#"{"limit":100,"offset":2,"totalNumberOfItems":3,"items":[
                {"created":"2026-10-17","item":{"id":33,"name":"C"},"type":"ARTIST"}]}"#,
        ),
    ])
    .await;
    let ids = client(&base, refresher())
        .blocked_artist_ids()
        .await
        .unwrap();
    assert_eq!(ids, ["11", "22", "33"]);
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /users/"), "{}", seen[0]);
    assert!(seen[0].contains("/blocks/artists?"), "{}", seen[0]);
    assert!(seen[1].contains("offset=2"), "{}", seen[1]);
}

#[tokio::test]
async fn blocked_songs_and_artists_are_read_with_names_and_a_song_is_unblocked() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            r#"{"limit":100,"offset":0,"totalNumberOfItems":1,"items":[
                {"created":"2026-10-17","item":{"id":5,"title":"Song","artist":{"id":1,"name":"Someone"}},"type":"TRACK"}]}"#,
        ),
        reply(
            "200 OK",
            "",
            r#"{"limit":100,"offset":0,"totalNumberOfItems":1,"items":[
                {"created":"2026-10-17","item":{"id":11,"name":"Band"},"type":"ARTIST"}]}"#,
        ),
        reply("204 No Content", "", ""),
    ])
    .await;
    let client = client(&base, refresher());
    let tracks = client.blocked_tracks().await.unwrap();
    assert_eq!(
        tracks,
        [BlockedTrack {
            id: "5".into(),
            title: "Song".into(),
            artist: "Someone".into(),
            hidden: false
        }]
    );
    let artists = client.blocked_artists().await.unwrap();
    assert_eq!(artists[0].id, "11");
    assert_eq!(artists[0].name, "Band");
    client.unblock_track("tidal:track:5").await.unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].contains("/blocks/tracks?"), "{}", seen[0]);
    assert!(seen[1].contains("/blocks/artists?"), "{}", seen[1]);
    assert!(
        seen[2].starts_with("DELETE /users/") && seen[2].contains("/blocks/tracks/5?"),
        "{}",
        seen[2]
    );
}

#[tokio::test]
async fn a_playlist_is_made_public_or_private_with_a_v2_put() {
    let (base, seen) = script(vec![
        reply("200 OK", "", "{}"),
        reply("200 OK", "", "{}"),
        reply("403 Forbidden", "", "{}"),
    ])
    .await;
    let client = client(&base, refresher());
    client
        .update_playlist("p-1", None, None, Some(true))
        .await
        .unwrap();
    client
        .update_playlist("p-1", None, None, Some(false))
        .await
        .unwrap();
    assert!(
        client
            .update_playlist("p-1", None, None, Some(false))
            .await
            .is_err(),
        "a refusal is an error, so the shown state can be put back"
    );
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("PUT /playlists/p-1/set-public"));
    assert!(seen[1].starts_with("PUT /playlists/p-1/set-private"));
}

#[tokio::test]
async fn playback_info_becomes_a_stream() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("fixtures/playbackinfo_bts_flac.json"),
    )])
    .await;
    let stream = client(&base, refresher())
        .playback_info("77646168", Quality::HiResLossless)
        .await
        .unwrap();
    assert_eq!(
        stream.quality,
        Quality::Lossless,
        "what TIDAL gave, not what was asked"
    );
    assert!(seen.lock().unwrap()[0].contains("audioquality=HI_RES_LOSSLESS"));
}

const NOT_ENTITLED: &str =
    r#"{"errors":[{"status":"403","code":"CLIENT_NOT_ENTITLED","detail":"no"}]}"#;

#[tokio::test]
async fn lossless_is_16_bit_flac_from_track_manifests() {
    // TIDAL answers `playbackinfopostpaywall?audioquality=LOSSLESS` with AAC
    // 320 for this client, so Lossless asks `trackManifests` up to FLAC.
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("fixtures/trackmanifest_flac.json"),
    )])
    .await;
    let stream = client(&base, refresher())
        .playback_info("257665679", Quality::Lossless)
        .await
        .unwrap();
    assert_eq!(stream.quality, Quality::Lossless);
    assert_eq!(stream.codec, "flac");
    assert_eq!(stream.bit_depth, Some(16));
    assert_eq!(stream.sample_rate, Some(44100));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].starts_with("GET /trackManifests/257665679?"));
    assert!(seen[0].contains("manifestType=MPEG_DASH"));
    assert!(seen[0].contains("uriScheme=DATA"));
    assert!(seen[0].contains("formats=HEAACV1&formats=AACLC&formats=FLAC "));
    assert!(
        !seen[0].contains("FLAC_HIRES"),
        "Lossless never asks hi-res"
    );
}

#[tokio::test]
async fn lossless_falls_back_to_playbackinfo_when_track_manifests_refuses() {
    let (base, seen) = script(vec![
        reply("403 Forbidden", "", NOT_ENTITLED),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_flac.json"),
        ),
    ])
    .await;
    let stream = client(&base, refresher())
        .playback_info("1", Quality::Lossless)
        .await
        .unwrap();
    assert_eq!(stream.quality, Quality::Lossless);
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /trackManifests/1?"));
    assert!(seen[1].contains("audioquality=LOSSLESS"));
}

#[tokio::test]
async fn protected_track_manifests_are_skipped_after_the_first() {
    // The PKCE client gets Widevine from trackManifests but 16-bit FLAC
    // from playbackinfo LOSSLESS (measured 2026-10-14).
    let drm = r#"{"data":{"attributes":{"trackPresentation":"FULL","formats":["FLAC"],"uri":"data:application/dash+xml;base64,","drmData":{"drmSystem":"WIDEVINE"}}}}"#;
    let (base, seen) = script(vec![
        reply("200 OK", "", drm),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_flac.json"),
        ),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_flac.json"),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    for id in ["1", "2"] {
        let stream = client.playback_info(id, Quality::Lossless).await.unwrap();
        assert_eq!(stream.quality, Quality::Lossless);
    }
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /trackManifests/1?"));
    assert!(seen[1].contains("audioquality=LOSSLESS"));
    assert!(
        seen[2].starts_with("GET /tracks/2/playbackinfopostpaywall?"),
        "no second trackManifests request"
    );
}

#[tokio::test]
async fn a_lossy_max_answer_for_a_cd_quality_track_does_not_give_up_on_max() {
    // A track with no hi-res master: playbackinfo answers HI_RES_LOSSLESS
    // with AAC, trackManifests with the 16-bit FLAC it has. The next track
    // still asks for Max.
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_aac.json"),
        ),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/trackmanifest_flac.json"),
        ),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_dash_hires.json"),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let stream = client
        .playback_info("77646168", Quality::HiResLossless)
        .await
        .unwrap();
    assert_eq!(stream.quality, Quality::Lossless);
    assert_eq!(stream.bit_depth, Some(16));
    let next = client
        .playback_info("257665679", Quality::HiResLossless)
        .await
        .unwrap();
    assert_eq!(next.quality, Quality::HiResLossless);
    let seen = seen.lock().unwrap();
    assert!(seen[0].contains("audioquality=HI_RES_LOSSLESS"));
    assert!(seen[1].starts_with("GET /trackManifests/77646168?"));
    assert!(seen[1].contains("formats=FLAC_HIRES"));
    assert!(seen[2].contains("audioquality=HI_RES_LOSSLESS"));
}

#[tokio::test]
async fn a_lossy_answer_to_max_is_asked_again_as_lossless() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_aac.json"),
        ),
        reply("403 Forbidden", "", NOT_ENTITLED),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_flac.json"),
        ),
        reply("403 Forbidden", "", NOT_ENTITLED),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/playbackinfo_bts_flac.json"),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let stream = client
        .playback_info("1", Quality::HiResLossless)
        .await
        .unwrap();
    assert_eq!(stream.quality, Quality::Lossless);
    client
        .playback_info("2", Quality::HiResLossless)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].contains("audioquality=HI_RES_LOSSLESS"));
    assert!(seen[1].starts_with("GET /trackManifests/1?"));
    assert!(seen[2].contains("audioquality=LOSSLESS"));
    assert!(
        seen[3].starts_with("GET /trackManifests/2?") && !seen[3].contains("FLAC_HIRES"),
        "later requests skip the refused Max"
    );
    assert!(seen[4].contains("audioquality=LOSSLESS"));
}

#[tokio::test]
async fn not_signed_in_is_an_error_not_a_request() {
    let client = TidalClient::new(reqwest::Client::new(), Arc::new(NetActivity::default()));
    assert!(matches!(
        client.track("1").await,
        Err(ApiError::NotSignedIn)
    ));
}

#[test]
fn track_credits_keep_roles_in_order_and_link_tidal_artists() {
    let json = serde_json::json!([
        {"type": "Producer", "contributors": [{"name": "Dr. Dre", "id": 3346}, {"name": "Trevor Lawrence Jr."}]},
        {"type": "Composer", "contributors": [{"name": "Marshall Mathers", "id": "17275"}]},
        {"type": "Empty", "contributors": []}
    ]);
    let credits = super::parse_credits(&json);
    assert_eq!(credits.roles.len(), 2);
    assert_eq!(credits.roles[0].role, "Producer");
    assert_eq!(credits.roles[0].contributors[0].id.as_deref(), Some("3346"));
    assert_eq!(credits.roles[0].contributors[1].id, None);
    assert_eq!(
        credits.roles[1].contributors[0].id.as_deref(),
        Some("17275")
    );
    let (year, label) = super::release_and_label(&serde_json::json!({
        "streamStartDate": "2009-12-21T00:00:00.000+0000",
        "copyright": "\u{2117} 2009 Aftermath Records"
    }));
    assert_eq!(year.as_deref(), Some("2009"));
    assert_eq!(label.as_deref(), Some("\u{2117} 2009 Aftermath Records"));
}

#[tokio::test]
async fn home_reads_the_v2_feed_as_the_web_player_does() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("fixtures/home_feed_v2.json"),
    )])
    .await;
    let client = client(&base, refresher());
    let shelves = client.home().await.unwrap();
    assert_eq!(shelves[1].title, "Recommended for you");
    assert!(shelves[4].pending, "filled afterwards, shelf by shelf");
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /home/feed/static?countryCode=US&deviceType=BROWSER&platform=WEB&locale=en_US&limit=20&timeOffset="));
    assert!(seen[0].contains("x-tidal-client-version"));
}

#[tokio::test]
async fn home_falls_back_to_the_v1_page_and_keeps_lazy_modules() {
    let (base, seen) = script(vec![
        reply("404 Not Found", "", r#"{"status":404,"subStatus":2001}"#),
        reply("200 OK", "", include_str!("fixtures/page_home_lazy.json")),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/page_data_playlists.json"),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let shelves = client.home().await.unwrap();
    assert_eq!(shelves.len(), 2);
    assert!(shelves[0].pending);
    let items = client
        .shelf_items(shelves[0].source.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    let seen = seen.lock().unwrap();
    assert!(seen[1].starts_with("GET /pages/home?"));
    assert!(
        seen[2].starts_with(
            "GET /pages/data/aaaaaaaa-0000-4000-8000-000000000002?countryCode=US&deviceType=BROWSER&locale=en_US&limit=20 "
        ),
        "{}",
        seen[2]
    );
}

#[tokio::test]
async fn a_page_fills_its_lazy_modules_before_answering() {
    let (base, _) = script(vec![
        reply("200 OK", "", include_str!("fixtures/page_home_lazy.json")),
        reply(
            "200 OK",
            "",
            include_str!("fixtures/page_data_playlists.json"),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let shelves = client.page_path("pages/genre_sample").await.unwrap();
    assert_eq!(shelves.len(), 2);
    assert!(!shelves[0].pending);
    assert_eq!(shelves[0].items.len(), 2);
}

#[test]
fn a_favourite_video_maps_with_artists_length_and_wide_thumbnails() {
    let wrapped: convert::Wrapped<convert::RawVideo> = fixture(
        r#"{"created":"2025-01-02T03:04:05.000+0000","item":{"id":123456,"title":"Live Session","duration":212,
            "imageId":"aaaa-bbbb-cccc","artists":[{"id":1,"name":"One"},{"id":2,"name":"Two"}]}}"#,
    );
    let video = convert::video(wrapped.item.unwrap());
    assert_eq!(video.id, "123456");
    assert_eq!(video.name, "Live Session");
    assert_eq!(video.artist, "One, Two");
    assert_eq!(video.duration, 212);
    assert_eq!(video.images.len(), 4);
    assert_eq!(video.images[0].width, Some(750));
    assert!(video.images[0].url.ends_with("/aaaa/bbbb/cccc/750x500.jpg"));
}

#[test]
fn a_tracks_tempo_and_key_come_from_its_audio_analysis() {
    let analysed = |attributes: &str| {
        let json = format!(r#"{{"id": 1, "title": "T", "audioAnalysisAttributes": {attributes}}}"#);
        convert::track(fixture(&json)).audio_analysis
    };
    let found = analysed(r#"{"bpm": "112", "key": "C", "keyScale": "MINOR"}"#).unwrap();
    assert_eq!(found.bpm, Some(112));
    assert_eq!(found.camelot(), "5A");
    let number = analysed(r#"{"bpm": 86.4, "key": "UNKNOWN", "keyScale": "MAJOR"}"#).unwrap();
    assert_eq!(number.bpm, Some(86));
    assert_eq!(number.camelot(), "-");
    assert_eq!(analysed("{}"), None);
    assert_eq!(analysed("null"), None);
}

#[test]
fn a_track_carries_tidals_ai_label_and_defaults_to_not_ai() {
    let track =
        |extra: &str| convert::track(fixture(&format!(r#"{{"id": 1, "title": "T"{extra}}}"#)));
    assert!(track(r#", "ai": true"#).ai);
    assert!(!track(r#", "ai": false"#).ai);
    assert!(!track("").ai);
    assert!(!track(r#", "ai": null"#).ai);
}

#[tokio::test]
async fn a_playlist_download_lists_its_songs_on_the_runtime_and_skips_videos() {
    use crate::api::tidal_catalog::TidalCatalog;
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("fixtures/playlist_items.json"),
    )])
    .await;
    let catalog = TidalCatalog::new(
        Arc::new(client(&base, refresher())),
        tokio::runtime::Handle::current(),
    );
    let tracks = catalog
        .context_tracks_async("tidal:playlist:11111111-aaaa-4bbb-8ccc-000000000001")
        .await
        .unwrap();
    assert_eq!(tracks.unplayable, 1);
    let tracks = tracks.tracks;
    assert_eq!(
        tracks.len(),
        2,
        "a video and an unavailable song are not kept"
    );
    assert_eq!(tracks[0].uri, "tidal:track:77646168");
    assert!(
        seen.lock().unwrap()[0]
            .starts_with("GET /playlists/11111111-aaaa-4bbb-8ccc-000000000001/items?")
    );
}

#[tokio::test]
async fn the_blocking_catalogue_refuses_the_blocking_pool_so_downloads_do_not_use_it() {
    use crate::api::tidal_catalog::TidalCatalog;
    use crate::tidal_engine::Catalog as _;
    let (base, _) = script(vec![]).await;
    let catalog = TidalCatalog::new(
        Arc::new(client(&base, refresher())),
        tokio::runtime::Handle::current(),
    );
    let error = tokio::task::spawn_blocking(move || catalog.context_tracks("tidal:playlist:p"))
        .await
        .unwrap()
        .unwrap_err();
    assert!(format!("{error:#}").contains("inside the async runtime"));
}

#[tokio::test]
async fn an_unlistable_playlist_reports_what_tidal_said() {
    use crate::api::tidal_catalog::TidalCatalog;
    let (base, _) = script(vec![reply(
        "404 Not Found",
        "",
        r#"{"status":404,"subStatus":2002,"userMessage":"Playlist not found"}"#,
    )])
    .await;
    let catalog = TidalCatalog::new(
        Arc::new(client(&base, refresher())),
        tokio::runtime::Handle::current(),
    );
    let error = catalog
        .context_tracks_async("tidal:playlist:gone")
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("TIDAL could not find this"),
        "{error:#}"
    );
}

/// One page of a playlist of `total` songs, `limit` at a time from `offset`.
fn playlist_page(offset: u32, limit: u32, total: u32) -> String {
    let items: Vec<String> = (offset..(offset + limit).min(total))
        .map(|n| {
            let playable = n % 100 != 99;
            format!(
                r#"{{"type":"track","created":"2026-01-01T00:00:00.000+0000","item":{{"id":{id},"title":"Song {n}","duration":200,"allowStreaming":{playable},"streamReady":{playable},"trackNumber":1,"volumeNumber":1,"artist":{{"id":1,"name":"A"}},"artists":[{{"id":1,"name":"A"}}],"album":{{"id":2,"title":"B","cover":null}}}}}}"#,
                id = 1000 + n
            )
        })
        .collect();
    format!(
        r#"{{"limit":{limit},"offset":{offset},"totalNumberOfItems":{total},"items":[{}]}}"#,
        items.join(",")
    )
}

/// A 1,200-song playlist is read in full, page after page, through a rate
/// limit in the middle, with the songs TIDAL will not give counted.
#[tokio::test]
async fn a_huge_playlist_is_read_to_the_end_through_a_rate_limit() {
    use crate::api::tidal_catalog::TidalCatalog;
    let mut replies = Vec::new();
    for page in 0..12 {
        if page == 5 {
            replies.push(reply("429 Too Many Requests", "Retry-After: 0\r\n", "{}"));
        }
        replies.push(reply("200 OK", "", &playlist_page(page * 100, 100, 1200)));
    }
    let (base, seen) = script(replies).await;
    let catalog = TidalCatalog::new(
        Arc::new(client(&base, refresher())),
        tokio::runtime::Handle::current(),
    );
    let found = catalog
        .context_tracks_async("tidal:playlist:big")
        .await
        .unwrap();
    assert_eq!(found.tracks.len() as u32 + found.unplayable, 1200);
    assert_eq!(found.unplayable, 12, "one song in a hundred is unavailable");
    assert_eq!(found.tracks[0].uri, "tidal:track:1000");
    assert_eq!(found.tracks.last().unwrap().uri, "tidal:track:2198");
    assert_eq!(seen.lock().unwrap().len(), 13, "12 pages and one retry");
}

#[test]
fn an_openapi_artist_gives_its_follower_count() {
    let document: Value = fixture(include_str!("fixtures/openapi_artist.json"));
    assert_eq!(followers_of(&document), Some(1_951_000));
    let without: Value =
        fixture(r#"{"data":{"attributes":{"name":"X","numberOfFollowers":null}}}"#);
    assert_eq!(followers_of(&without), None);
    assert_eq!(followers_of(&Value::Null), None);
}

/// A plugin's own call gets TIDAL's error answer as sent (status, subStatus,
/// userMessage), not a body rebuilt from the readable message (an album's
/// 404 read "TIDAL has no radio for this.").
#[tokio::test]
async fn a_plugins_call_gets_tidals_error_body_as_sent() {
    let sent =
        r#"{"status":404,"subStatus":2001,"userMessage":"Album with id 395361139 not found"}"#;
    let (base, _) = script(vec![reply("404 Not Found", "", sent)]).await;
    let tidal = client(&base, refresher());
    let (status, body, etag) = tidal
        .v1_call("GET", "albums/395361139", &[], &[], None)
        .await
        .unwrap();
    assert_eq!(status, 404);
    assert_eq!(body, sent);
    assert_eq!(etag, None);
    // A 401 that is not an expired token (an availability error) too.
    let sent = r#"{"status":401,"subStatus":4005,"userMessage":"Asset is not ready for playback"}"#;
    let (base, _) = script(vec![reply("401 Unauthorized", "", sent)]).await;
    let tidal = client(&base, refresher());
    let (status, body, _) = tidal
        .v1_call("GET", "tracks/1/playbackinfo", &[], &[], None)
        .await
        .unwrap();
    assert_eq!((status, body.as_str()), (401, sent));
}

#[test]
fn album_item_credits_list_each_track_with_its_roles_and_skip_videos() {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/album_items_credits.json")).unwrap();
    let (tracks, total) = super::parse_album_items_credits(&json);
    assert_eq!(total, 3);
    assert_eq!(tracks.len(), 2, "the video is left out");
    assert_eq!((tracks[0].volume, tracks[0].number), (1, 1));
    assert_eq!(tracks[0].title, "Intro");
    // A role with nobody in it is dropped, the rest keep TIDAL's order.
    let roles: Vec<&str> = tracks[0]
        .roles
        .iter()
        .map(|credit| credit.role.as_str())
        .collect();
    assert_eq!(roles, ["Producer", "Composer", "AdditionalProducer"]);
    assert_eq!(
        tracks[0].roles[1].contributors[1].id.as_deref(),
        Some("3652822")
    );
    assert_eq!(tracks[0].roles[1].contributors[0].id, None);
    // The song itself, for the card's artists line and menu.
    let song = tracks[0].track.as_ref().expect("the song");
    assert_eq!(song.uri, "tidal:track:209237068");
    assert_eq!(song.artists[0].name, "Ada Example");
    // The second disc, and a version in the title.
    assert_eq!((tracks[1].volume, tracks[1].number), (2, 1));
    assert_eq!(tracks[1].title, "Quiet Hours (Live)");
    assert!(tracks[1].roles.is_empty());
}

#[tokio::test]
async fn an_album_review_is_read_with_its_source_and_a_404_means_none() {
    let (base, seen) = script(vec![
        reply("200 OK", "", include_str!("fixtures/album_review.json")),
        reply("404 Not Found", "", r#"{"status":404,"subStatus":2001}"#),
    ])
    .await;
    let client = client(&base, refresher());
    let review = client.album_review("209237067").await.unwrap().unwrap();
    assert_eq!(review.source.as_deref(), Some("TiVo"));
    let paragraphs = crate::api::bio::parse(&review.text);
    assert_eq!(paragraphs.len(), 1);
    // The links open the album and the artist; the stray bracket stays text.
    let links: Vec<_> = paragraphs[0]
        .iter()
        .filter_map(|piece| match piece {
            crate::api::bio::BioPiece::Link { text, target } => Some((text.as_str(), target)),
            _ => None,
        })
        .collect();
    assert_eq!(links.len(), 3);
    assert_eq!(
        links[1],
        (
            "Ada Example",
            &crate::api::bio::BioTarget::Artist("3652822".into())
        )
    );
    assert!(paragraphs[0].iter().any(|piece| matches!(
        piece,
        crate::api::bio::BioPiece::Text(text) if text.ends_with("vinyl.] ~ A Reviewer")
    )));
    assert!(client.album_review("1").await.unwrap().is_none());
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /albums/209237067/review?countryCode=US "));
}

#[tokio::test]
async fn playlist_recommendations_are_a_page_of_tracks_with_the_total() {
    let (base, seen) = script(vec![reply(
        "200 OK",
        "",
        include_str!("fixtures/playlist_recommendations.json"),
    )])
    .await;
    let client = client(&base, refresher());
    let page = client
        .playlist_recommendations("11111111-aaaa-4bbb-8ccc-000000000002", 10, 10)
        .await
        .unwrap();
    assert_eq!(page.total, 100);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].uri, "tidal:track:332900812");
    assert_eq!(page.items[0].artists.len(), 2);
    assert!(page.items[1].explicit);
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with(
            "GET /playlists/11111111-aaaa-4bbb-8ccc-000000000002/recommendations/items?countryCode=US&limit=10&offset=10 "
        ),
        "{}",
        seen[0]
    );
}
