//! TIDAL's Save for Later (`openapi.tidal.com/v2/userCollectionSaveForLaters`),
//! a JSON:API relationship of the signed-in user:
//!
//! ```text
//! GET    /userCollectionSaveForLaters/me/relationships/items?include=items
//! POST   /userCollectionSaveForLaters/me/relationships/items   {"data":[{"id","type"}]}
//! DELETE /userCollectionSaveForLaters/me/relationships/items   {"data":[{"id","type"}]}
//! ```
//!
//! `type` is `albums`, `tracks`, `artists`, `playlists` or `videos`. The
//! list answers the identifiers only; the page asks the catalogue for the
//! items it draws.

use reqwest::Method;
use serde_json::{Value, json};

use super::{Call, Result, TidalClient};
use crate::save_later::{SaveKind, Saved};

/// One page of the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveLaterPage {
    pub items: Vec<(SaveKind, String)>,
    /// The cursor of the next page, when there is one.
    pub next: Option<String>,
}

/// A list answer: its items in TIDAL's order (unknown kinds skipped) and the
/// next page's cursor, read from `links.next` (`page[cursor]=...`).
pub(crate) fn parse(document: &Value) -> SaveLaterPage {
    let items = document["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let kind = SaveKind::from_api_type(entry["type"].as_str()?)?;
            let id = match &entry["id"] {
                Value::String(id) => id.clone(),
                Value::Number(id) => id.to_string(),
                _ => return None,
            };
            (!id.is_empty()).then_some((kind, id))
        })
        .collect();
    let next = document["links"]["next"]
        .as_str()
        .and_then(|link| link.split_once('?').map(|(_, query)| query))
        .and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "page%5Bcursor%5D" || key == "page[cursor]").then(|| value.to_string())
            })
        });
    SaveLaterPage { items, next }
}

/// The body of a write: one identifier.
pub(crate) fn write_body(kind: SaveKind, id: &str) -> Value {
    json!({ "data": [{ "id": id, "type": kind.api_type() }] })
}

impl TidalClient {
    fn save_later_url(&self) -> String {
        format!(
            "{}/userCollectionSaveForLaters/me/relationships/items",
            self.openapi_base
        )
    }

    /// One page of the saved items.
    pub async fn save_later(&self, cursor: Option<&str>) -> Result<SaveLaterPage> {
        let mut call = Call::new(Method::GET, self.save_later_url()).query("include", "items");
        if let Some(cursor) = cursor {
            call = call.query("page[cursor]", cursor);
        }
        let document: Value = self.get(call).await?;
        Ok(parse(&document))
    }

    /// What a row of the page draws, read from the catalogue.
    pub async fn save_later_detail(&self, uri: &str) -> Result<Saved> {
        let (kind, id) = crate::save_later::parse_uri(uri)
            .ok_or_else(|| super::ApiError::Decode("not an item Save for Later keeps".into()))?;
        Ok(match kind {
            SaveKind::Album => Saved::Album(self.album(&id).await?),
            SaveKind::Track => Saved::Track(self.track(&id).await?),
            SaveKind::Artist => Saved::Artist(self.artist(&id).await?),
            SaveKind::Playlist => Saved::Playlist(self.playlist(&id).await?),
            SaveKind::Video => Saved::Video(self.video(&id).await?),
        })
    }

    /// Saves or removes one item.
    pub async fn set_save_later(&self, kind: SaveKind, id: &str, saved: bool) -> Result<()> {
        let method = if saved { Method::POST } else { Method::DELETE };
        self.send(Call::new(method, self.save_later_url()).json_api(&write_body(kind, id)))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_ids_and_the_next_cursor() {
        let document = json!({
            "data": [
                {"id": "496871321", "type": "videos"},
                {"id": "5", "type": "albums"},
                {"id": "9", "type": "mixes"},
                {"id": 7, "type": "tracks"}
            ],
            "links": {"next": "/userCollectionSaveForLaters/me/relationships/items?page%5Bcursor%5D=abc&include=items"}
        });
        let page = parse(&document);
        assert_eq!(
            page.items,
            vec![
                (SaveKind::Video, "496871321".to_string()),
                (SaveKind::Album, "5".to_string()),
                (SaveKind::Track, "7".to_string()),
            ]
        );
        assert_eq!(page.next.as_deref(), Some("abc"));
        assert_eq!(parse(&json!({"data": []})), SaveLaterPage::default());
    }

    #[test]
    fn a_write_names_the_id_and_its_json_api_type() {
        assert_eq!(
            write_body(SaveKind::Video, "496871321"),
            json!({"data": [{"id": "496871321", "type": "videos"}]})
        );
    }
}

/// What the account bought: Collection > Purchases.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Purchases {
    pub albums: Vec<crate::api::models::Album>,
    pub tracks: Vec<crate::api::models::Track>,
    /// Entries TIDAL listed that could not be read as an album or song.
    pub unreadable: usize,
}

/// `GET /v2/marketplace/v1/user-purchases` answers
/// `{"albums": [...], "tracks": [...]}`. Only the empty answer has been
/// seen (no purchases on the test account); a listed entry is read as a v1
/// album or track, bare or under `item`, `album` or `track`.
pub(crate) fn parse_purchases(document: &Value) -> Purchases {
    fn entries<T>(
        document: &Value,
        key: &str,
        wrapper: &str,
        out: &mut Vec<T>,
        unreadable: &mut usize,
        read: impl Fn(Value) -> Option<T>,
    ) {
        for entry in document[key].as_array().into_iter().flatten() {
            let inner = [wrapper, "item"]
                .iter()
                .find_map(|name| entry.get(*name).filter(|value| value.is_object()))
                .unwrap_or(entry);
            match read(inner.clone()) {
                Some(item) => out.push(item),
                None => *unreadable += 1,
            }
        }
    }
    let mut purchases = Purchases::default();
    entries(
        document,
        "albums",
        "album",
        &mut purchases.albums,
        &mut purchases.unreadable,
        |value| {
            serde_json::from_value::<super::convert::RawAlbum>(value)
                .ok()
                .map(super::convert::album)
                .filter(|album| !album.id.is_empty())
        },
    );
    entries(
        document,
        "tracks",
        "track",
        &mut purchases.tracks,
        &mut purchases.unreadable,
        |value| {
            serde_json::from_value::<super::convert::RawTrack>(value)
                .ok()
                .map(super::convert::track)
                .filter(|track| track.id.is_some())
        },
    );
    purchases
}

impl TidalClient {
    /// The account's purchases (`marketplace/v1/user-purchases`; the same
    /// session token as the rest of v2 is accepted).
    pub async fn purchases(&self) -> Result<Purchases> {
        let document: Value = self
            .get(Call::new(
                Method::GET,
                self.v2("marketplace/v1/user-purchases"),
            ))
            .await?;
        Ok(parse_purchases(&document))
    }
}

#[cfg(test)]
mod purchases_tests {
    use super::*;

    #[test]
    fn no_purchases_is_the_empty_answer_tidal_sends() {
        let purchases = parse_purchases(&json!({"albums": [], "tracks": []}));
        assert_eq!(purchases, Purchases::default());
    }

    #[test]
    fn a_listed_album_or_song_is_read_and_a_strange_entry_is_counted() {
        let purchases = parse_purchases(&json!({
            "albums": [{"item": {"id": 5, "title": "Bought"}}, {"nothing": true}],
            "tracks": [{"id": 9, "title": "Song"}]
        }));
        assert_eq!(purchases.albums.len(), 1);
        assert_eq!(purchases.albums[0].name, "Bought");
        assert_eq!(purchases.tracks.len(), 1);
        assert_eq!(purchases.unreadable, 1);
    }
}
