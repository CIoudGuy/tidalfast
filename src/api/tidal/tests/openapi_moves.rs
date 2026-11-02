//! Features read and written through openapi, against a scripted local
//! server: what is asked, and what comes back from TIDAL's JSON:API shapes
//! (trimmed from live answers). Nothing here reaches a real account.

use super::*;

const SIMILAR_PAGE_ONE: &str = r#"{
  "data": [{"id": "1", "type": "artists"}, {"id": "2", "type": "artists"}, {"id": "9", "type": "artists"}],
  "included": [
    {"id": "1", "type": "artists",
     "attributes": {"name": "D12", "popularity": 0.86245,
       "externalLinks": [{"href": "https://instagram.com/x", "meta": {"type": "INSTAGRAM"}},
                         {"href": "https://tidal.com/browse/artist/1", "meta": {"type": "TIDAL_SHARING"}}]},
     "relationships": {"profileArt": {"data": [{"id": "art1", "type": "artworks"}]}}},
    {"id": "2", "type": "artists", "attributes": {"name": "Obie Trice"},
     "relationships": {"profileArt": {"data": []}}},
    {"id": "art1", "type": "artworks", "attributes": {"files": [
      {"href": "https://resources.tidal.com/images/b6be7de9/3b30/409a/9c47/7bae9f332398/750x750.jpg",
       "meta": {"height": 750, "width": 750}}]}}
  ],
  "links": {"meta": {"nextCursor": "3nI1Esi"}, "self": "/artists/17275/relationships/similarArtists"}
}"#;

const SIMILAR_PAGE_TWO: &str = r#"{
  "data": [{"id": "3", "type": "artists"}],
  "included": [{"id": "3", "type": "artists", "attributes": {"name": "Dr. Dre"}}],
  "links": {"self": "/artists/17275/relationships/similarArtists"}
}"#;

const SUGGESTED_VIDEOS: &str = r#"{
  "data": [{"id": "142481460", "type": "videos"}, {"id": "441699021", "type": "videos"}],
  "included": [
    {"id": "142481460", "type": "videos",
     "attributes": {"title": "Non e vero", "duration": "PT3M11S", "explicit": true, "releaseDate": "2020-05-23"},
     "relationships": {"artists": {"data": [{"id": "6068727", "type": "artists"}]},
                       "thumbnailArt": {"data": [{"id": "thumb", "type": "artworks"}]}}},
    {"id": "441699021", "type": "videos", "attributes": {"title": "The video itself", "duration": "PT1S"}},
    {"id": "6068727", "type": "artists", "attributes": {"name": "Singer"}},
    {"id": "thumb", "type": "artworks", "attributes": {"files": [
      {"href": "https://resources.tidal.com/images/b2e21110/d424/41db/8e86/351f92b177d1/1280x720.jpg",
       "meta": {"height": 720, "width": 1280}}]}}
  ],
  "links": {"self": "/videos/441699021/relationships/suggestedVideos"}
}"#;

#[tokio::test]
async fn related_artists_are_tidals_similar_artists_read_by_cursor() {
    let (base, seen) = script(vec![
        reply("200 OK", "", SIMILAR_PAGE_ONE),
        reply("200 OK", "", SIMILAR_PAGE_TWO),
    ])
    .await;
    let client = client(&base, refresher());
    let artists = client.related_artists("17275").await.unwrap();
    let names: Vec<&str> = artists.iter().map(|artist| artist.name.as_str()).collect();
    assert_eq!(names, ["D12", "Obie Trice", "Dr. Dre"]);
    assert_eq!(artists[0].uri, "tidal:artist:1");
    assert_eq!(artists[0].popularity, Some(86));
    assert_eq!(
        artists[0].external_urls.tidal.as_deref(),
        Some("https://tidal.com/browse/artist/1")
    );
    assert!(
        artists[0].images[0]
            .url
            .contains("/images/b6be7de9/3b30/409a/9c47/7bae9f332398/"),
        "{:?}",
        artists[0].images
    );
    assert!(artists[1].images.is_empty());
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /artists/17275/relationships/similarArtists?"),
        "{}",
        seen[0]
    );
    assert!(seen[1].contains("page%5Bcursor%5D=3nI1Esi"), "{}", seen[1]);
}

#[tokio::test]
async fn related_artists_stop_at_the_shelfs_length() {
    let many: Vec<Value> = (0..30)
        .map(|n| serde_json::json!({"id": n.to_string(), "type": "artists"}))
        .collect();
    let included: Vec<Value> = (0..30)
        .map(|n| serde_json::json!({"id": n.to_string(), "type": "artists", "attributes": {"name": "A"}}))
        .collect();
    let page = serde_json::json!({
        "data": many, "included": included, "links": {"meta": {"nextCursor": "more"}}
    })
    .to_string();
    let (base, seen) = script(vec![reply("200 OK", "", &page), reply("200 OK", "", &page)]).await;
    let client = client(&base, refresher());
    let artists = client.related_artists("1").await.unwrap();
    assert_eq!(artists.len(), RELATED_ARTISTS);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn an_artist_with_no_similar_artists_is_an_empty_shelf_and_a_failure_an_error() {
    let (base, _) = script(vec![
        reply("404 Not Found", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
        reply("500 Internal Server Error", "", "{}"),
    ])
    .await;
    let client = client(&base, refresher());
    assert!(client.related_artists("1").await.unwrap().is_empty());
    assert!(client.related_artists("2").await.is_err());
}

#[tokio::test]
async fn similar_videos_are_tidals_suggested_videos() {
    let (base, seen) = script(vec![reply("200 OK", "", SUGGESTED_VIDEOS)]).await;
    let client = client(&base, refresher());
    let videos = client.similar_videos("441699021", None).await.unwrap();
    assert_eq!(videos.len(), 1, "the video itself is left out");
    let video = &videos[0];
    assert_eq!(video.id, "142481460");
    assert_eq!(video.name, "Non e vero");
    assert_eq!(video.artist, "Singer");
    assert_eq!(video.artists[0].id.as_deref(), Some("6068727"));
    assert_eq!(video.duration, 191);
    assert!(video.explicit);
    assert_eq!(video.release_date.as_deref(), Some("2020-05-23"));
    assert!(
        video.images[0]
            .url
            .contains("/images/b2e21110/d424/41db/8e86/351f92b177d1/"),
        "{:?}",
        video.images
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /videos/441699021/relationships/suggestedVideos?"),
        "{}",
        seen[0]
    );
}
