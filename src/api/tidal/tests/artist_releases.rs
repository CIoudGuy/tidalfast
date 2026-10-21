//! An artist page's release shelves list what TIDAL's own page lists: its
//! modules' versions (for J. Cole the explicit one of each release, and
//! both Come Up Mixtapes) in its order, not every version that
//! `artists/{id}/albums` returns.

use super::*;

fn release(id: u32, title: &str, explicit: bool) -> String {
    format!(
        r#"{{"id":{id},"title":"{title}","type":"ALBUM","explicit":{explicit},"releaseDate":"2025-04-29"}}"#
    )
}

/// An artist page module's "View all" (`v2/artist/<MODULE>/view-all`).
fn module(releases: &[String]) -> String {
    let items: Vec<String> = releases
        .iter()
        .map(|release| {
            format!(
                r#"{{"type":"ALBUM","following":false,"latestRelease":false,"data":{release}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"title":"Albums","itemLayout":"GRID","filters":[],"items":[{}]}}"#,
        items.join(",")
    )
}

fn v1_page(releases: &[String]) -> String {
    format!(
        r#"{{"limit":50,"offset":0,"totalNumberOfItems":{},"items":[{}]}}"#,
        releases.len(),
        releases.join(",")
    )
}

/// J. Cole's Albums shelf as TIDAL showed it live (2026-10-22): The
/// Fall-Off, two Come Up Mixtapes and Might Delete Later, each explicit,
/// then the Compilations and Live albums modules. `artists/{id}/albums`
/// lists four Fall-Offs (two clean) and a clean Might Delete Later before
/// the explicit ones, so Tidalfast showed clean versions without the E and
/// one Come Up.
#[tokio::test]
async fn the_albums_shelf_lists_tidals_versions_in_its_order() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            &module(&[
                release(496439179, "The Fall-Off", true),
                release(432959631, "The Come Up Mixtape Vol. 1", true),
                release(400414937, "The Come Up Mixtape Vol. 1", true),
                release(355491608, "Might Delete Later", true),
            ]),
        ),
        reply(
            "200 OK",
            "",
            &module(&[release(402165729, "Friday Night Lights", true)]),
        ),
        reply(
            "200 OK",
            "",
            &module(&[release(
                345547906,
                "Forest Hills Drive: Live from Fayetteville, NC",
                true,
            )]),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let page = client
        .artist_albums("3652822", "album", 0, 50)
        .await
        .unwrap();
    let ids: Vec<&str> = page.items.iter().map(|album| album.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "496439179",
            "432959631",
            "400414937",
            "355491608",
            "402165729",
            "345547906"
        ]
    );
    assert!(page.items.iter().all(|album| album.explicit));
    let kinds: Vec<Option<&str>> = page
        .items
        .iter()
        .map(|album| album.album_type.as_deref())
        .collect();
    assert_eq!(
        kinds,
        [
            Some("album"),
            Some("album"),
            Some("album"),
            Some("album"),
            Some("compilation"),
            Some("live")
        ]
    );
    assert_eq!(page.total, 6);
    assert_eq!(page.next_offset(), None);
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /artist/ARTIST_ALBUMS/view-all?"));
    for wanted in [
        "artistId=3652822",
        "limit=50",
        "offset=0",
        "deviceType=BROWSER",
        "platform=WEB",
    ] {
        assert!(seen[0].contains(wanted), "{wanted} in {}", seen[0]);
    }
    assert!(seen[1].starts_with("GET /artist/ARTIST_COMPILATIONS/view-all?"));
    assert!(seen[2].starts_with("GET /artist/ARTIST_LIVE_ALBUMS/view-all?"));
}

/// EP & Singles and Appears On page as TIDAL pages a module's "View all":
/// 50 at a time, more while a page comes back full.
#[tokio::test]
async fn a_long_module_is_read_fifty_at_a_time() {
    let full: Vec<String> = (0..50)
        .map(|index| release(1000 + index, &format!("Single {index}"), index % 2 == 0))
        .collect();
    let (base, seen) = script(vec![
        reply("200 OK", "", &module(&full)),
        reply("200 OK", "", &module(&[release(2000, "Last", false)])),
    ])
    .await;
    let client = client(&base, refresher());
    let first = client.artist_albums("9", "single", 0, 50).await.unwrap();
    assert_eq!(first.items.len(), 50);
    assert!(first.items[0].explicit && !first.items[1].explicit);
    assert_eq!(first.next_offset(), Some(50));
    let second = client.artist_albums("9", "single", 50, 50).await.unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.next_offset(), None);
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /artist/ARTIST_TOP_SINGLES/view-all?"));
    assert!(seen[1].contains("offset=50"));
}

/// An artist whose page lists no releases (TIDAL answers an uploader's own
/// artist page empty) is read from `artists/{id}/albums`, one version of
/// each title, the explicit one where there is one.
#[tokio::test]
async fn an_empty_artist_page_falls_back_to_one_version_of_each_title() {
    let (base, seen) = script(vec![
        reply("200 OK", "", &module(&[])),
        reply("200 OK", "", &module(&[])),
        reply("200 OK", "", &module(&[])),
        reply(
            "200 OK",
            "",
            &v1_page(&[
                release(1, "Demo Sessions", false),
                release(2, "Demo Sessions", true),
                release(3, "Basement Tapes", false),
            ]),
        ),
    ])
    .await;
    let client = client(&base, refresher());
    let page = client.artist_albums("9", "album", 0, 50).await.unwrap();
    let ids: Vec<&str> = page.items.iter().map(|album| album.id.as_str()).collect();
    assert_eq!(ids, ["2", "3"]);
    let seen = seen.lock().unwrap();
    assert!(seen[3].starts_with("GET /artists/9/albums?"));
    assert!(seen[3].contains("filter=ALBUMS"));
}
