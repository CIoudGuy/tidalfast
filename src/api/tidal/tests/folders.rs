//! Playlist folders, against a scripted local server. Nothing here reaches
//! a real account.

use super::*;

#[tokio::test]
async fn folders_are_read_with_the_playlists_each_holds() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            r#"{"items":[
                {"itemType":"FOLDER","data":{"id":"f-1","name":"Road trip","trn":"trn:folder:f-1"}},
                {"itemType":"PLAYLIST","data":{"uuid":"root-pl","name":"Loose"}}
            ],"cursor":null,"totalNumberOfItems":2}"#,
        ),
        reply(
            "200 OK",
            "",
            r#"{"items":[
                {"itemType":"PLAYLIST","data":{"uuid":"pl-1","name":"One"}},
                {"itemType":"PLAYLIST","data":{"uuid":"pl-2","name":"Two"}}
            ],"cursor":null,"totalNumberOfItems":2}"#,
        ),
    ])
    .await;
    let folders = client(&base, refresher()).playlist_folders().await.unwrap();
    assert_eq!(
        folders,
        [PlaylistFolder {
            id: "f-1".into(),
            name: "Road trip".into(),
            parent_id: None,
            playlist_ids: vec!["pl-1".into(), "pl-2".into()],
        }]
    );
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /my-collection/playlists/folders?"));
    assert!(seen[0].contains("folderId=root"));
    assert!(seen[1].contains("folderId=f-1"));
}

#[tokio::test]
async fn moving_playlists_sends_their_trns_and_the_target_folder() {
    let (base, seen) = script(vec![reply("200 OK", "", "{}"), reply("200 OK", "", "{}")]).await;
    let client = client(&base, refresher());
    client
        .move_playlists_to_folder(&["a".into(), "b".into()], Some("f-1"))
        .await
        .unwrap();
    client
        .move_playlists_to_folder(&["a".into()], None)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("PUT /my-collection/playlists/folders/move?"));
    assert!(seen[0].contains("folderId=f-1"));
    assert!(
        seen[0].contains("trn%3Aplaylist%3Aa%2Ctrn%3Aplaylist%3Ab"),
        "{}",
        seen[0]
    );
    assert!(seen[1].contains("folderId=root"));
}

#[tokio::test]
async fn creating_renaming_and_deleting_a_folder_keeps_its_playlists() {
    let (base, seen) = script(vec![
        reply("200 OK", "", "{}"),
        reply("200 OK", "", "{}"),
        reply("200 OK", "", "{}"),
        reply("200 OK", "", r#"{"items":[],"cursor":null}"#),
        reply("200 OK", "", "{}"),
    ])
    .await;
    let client = client(&base, refresher());
    client.create_folder("Trips").await.unwrap();
    client.rename_folder("f-1", "Journeys").await.unwrap();
    client.delete_folder("f-1", &["pl-1".into()]).await.unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("PUT /my-collection/playlists/folders/create-folder?"));
    assert!(seen[0].contains("name=Trips"));
    assert!(seen[1].starts_with("PUT /my-collection/playlists/folders/rename?"));
    assert!(seen[1].contains("trn=trn%3Afolder%3Af-1"));
    assert!(
        seen[2].starts_with("PUT /my-collection/playlists/folders/move?")
            && seen[2].contains("folderId=root"),
        "its playlists go to the top level first"
    );
    assert!(seen[3].starts_with("GET /my-collection/playlists/folders?"));
    assert!(seen[3].contains("folderId=f-1"));
    assert!(seen[4].starts_with("PUT /my-collection/playlists/folders/remove?"));
    assert!(seen[4].contains("trns=trn%3Afolder%3Af-1"));
}

#[tokio::test]
async fn a_folder_that_still_holds_playlists_is_not_deleted() {
    // The move answers 200, but the folder still lists a playlist.
    let (base, seen) = script(vec![
        reply("200 OK", "", "{}"),
        reply(
            "200 OK",
            "",
            r#"{"items":[{"itemType":"PLAYLIST","data":{"uuid":"pl-1"}}],"cursor":null}"#,
        ),
    ])
    .await;
    let result = client(&base, refresher())
        .delete_folder("f-1", &["pl-1".into()])
        .await;
    assert!(matches!(result, Err(ApiError::FolderNotEmpty)));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "no remove call was sent");
    assert!(
        !seen
            .iter()
            .any(|request| request.contains("/folders/remove"))
    );
}
