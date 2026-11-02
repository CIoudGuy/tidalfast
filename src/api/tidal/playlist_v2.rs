//! Reading a playlist that holds a video.
//!
//! TIDAL's v1 `GET playlists/{uuid}/items` answers 500 whenever the page it
//! would return contains a video (live 2026-10-18; its own web player shows
//! "Server error" too). The openapi v2 relationship still lists songs and
//! videos, so a failed v1 read is repeated there and the answer is turned into
//! the v1 shape (`{ items: [{ item, type, created }], ... }`), which the rest
//! of the client, and the Luna plugins reading `playlists/{id}/items`, already
//! understand.
//!
//! ```text
//! GET /playlists/{id}/relationships/items?include=items,items.artists,
//!     items.albums,items.albums.coverArt,items.thumbnailArt[&page[cursor]=..]
//! ```
//!
//! The relationship pages by cursor, 20 entries at a time, so the whole list
//! is read once and kept for a short while: the app asks for it 100 entries
//! at a time and a cursor walk per page would repeat itself. Playlists
//! without a video never get here.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{Value, json};

use super::jsonapi::{Document, artwork_uuid, duration_seconds, next_cursor, text_id};
use super::{ApiError, Call, Result, TidalClient};

/// How long a list read through v2 is reused.
const LIST_TTL: Duration = Duration::from_secs(30);
/// How long a playlist is read through v2 without trying v1 first.
const BROKEN_TTL: Duration = Duration::from_secs(600);
/// Pages of 20 entries; a playlist is capped at 5000 entries by TIDAL.
const MAX_PAGES: usize = 300;
const INCLUDE: &str = "items,items.artists,items.albums,items.albums.coverArt,items.thumbnailArt";

#[derive(Default)]
pub(crate) struct V2Playlists {
    entries: HashMap<String, (Instant, std::sync::Arc<Vec<Value>>)>,
    broken: HashMap<String, Instant>,
}

impl V2Playlists {
    fn lock(client: &TidalClient) -> std::sync::MutexGuard<'_, V2Playlists> {
        client
            .v2_playlists
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TidalClient {
    /// The playlist's items were edited: forget what was read of it.
    pub(super) fn forget_playlist_read(&self, id: &str) {
        V2Playlists::lock(self).entries.remove(id);
    }

    /// Whether `id` failed in v1 recently, so v2 is asked directly.
    pub(super) fn playlist_is_v1_broken(&self, id: &str) -> bool {
        let mut state = V2Playlists::lock(self);
        match state.broken.get(id) {
            Some(at) if at.elapsed() < BROKEN_TTL => true,
            Some(_) => {
                state.broken.remove(id);
                false
            }
            None => false,
        }
    }

    /// A page of the playlist in the v1 shape, from the v2 relationship.
    pub(super) async fn playlist_items_via_v2(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<String> {
        let cached = {
            let state = V2Playlists::lock(self);
            state
                .entries
                .get(id)
                .filter(|(at, _)| at.elapsed() < LIST_TTL)
                .map(|(_, entries)| entries.clone())
        };
        let entries = match cached {
            Some(entries) => entries,
            None => {
                let entries = std::sync::Arc::new(self.read_playlist_v2(id).await?);
                let mut state = V2Playlists::lock(self);
                state.broken.insert(id.to_string(), Instant::now());
                state
                    .entries
                    .insert(id.to_string(), (Instant::now(), entries.clone()));
                entries
            }
        };
        Ok(page_body(&entries, offset, limit))
    }

    async fn read_playlist_v2(&self, id: &str) -> Result<Vec<Value>> {
        let url = format!("{}/playlists/{id}/relationships/items", self.openapi_base);
        let mut entries = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut call = Call::new(Method::GET, url.clone()).query("include", INCLUDE);
            if let Some(cursor) = &cursor {
                call = call.query("page[cursor]", cursor);
            }
            let document: Value = self.get(call).await?;
            let mut page = entries_of(&document);
            let missing: Vec<(bool, String)> = page
                .iter()
                .filter(|entry| entry["item"]["title"].as_str().is_none_or(str::is_empty))
                .filter_map(|entry| {
                    Some((
                        entry["type"] == "video",
                        entry["item"]["id"].as_str()?.to_string(),
                    ))
                })
                .collect();
            if !missing.is_empty() {
                // The document left an entry without its attributes: the
                // catalogue has them.
                for (video, item) in missing {
                    let found = if video {
                        self.video_json(&item).await
                    } else {
                        self.track_json(&item).await
                    };
                    if let Ok(found) = found
                        && let Some(slot) = page
                            .iter_mut()
                            .find(|entry| entry["item"]["id"].as_str() == Some(item.as_str()))
                    {
                        slot["item"] = found;
                    }
                }
            }
            let empty = page.is_empty();
            entries.extend(page);
            cursor = next_cursor(&document);
            if cursor.is_none() || empty {
                return Ok(entries);
            }
        }
        Err(ApiError::Decode("the playlist is too long to read".into()))
    }

    async fn track_json(&self, id: &str) -> Result<Value> {
        self.get(Call::new(Method::GET, self.v1(&format!("tracks/{id}"))))
            .await
    }

    async fn video_json(&self, id: &str) -> Result<Value> {
        self.get(Call::new(Method::GET, self.v1(&format!("videos/{id}"))))
            .await
    }
}

/// `{ limit, offset, totalNumberOfItems, items }` for the slice asked for.
fn page_body(entries: &[Value], offset: u32, limit: u32) -> String {
    let start = (offset as usize).min(entries.len());
    let end = start.saturating_add(limit as usize).min(entries.len());
    json!({
        "limit": limit,
        "offset": offset,
        "totalNumberOfItems": entries.len(),
        "items": &entries[start..end],
    })
    .to_string()
}

/// The v1-shaped entries of a v2 document, in its order.
pub(crate) fn entries_of(document: &Value) -> Vec<Value> {
    let document = Document::new(document);
    let artists = |resource: &Value| -> Vec<Value> {
        document
            .related(resource, "artists")
            .into_iter()
            .map(|artist| {
                json!({
                    "id": text_id(&artist["id"]),
                    "name": artist["attributes"]["name"],
                    "type": "MAIN",
                })
            })
            .collect()
    };
    document
        .data()
        .into_iter()
        .filter_map(|link| {
            let kind = link["type"].as_str()?;
            let id = text_id(&link["id"])?;
            let resource = document.resource(link);
            let attributes = resource.map(|resource| &resource["attributes"]);
            let attribute = |name: &str| attributes.map_or(Value::Null, |a| a[name].clone());
            let duration = attributes
                .and_then(|a| a["duration"].as_str())
                .and_then(duration_seconds);
            let mut item = json!({
                "id": id,
                "title": attribute("title"),
                "version": attribute("version"),
                "duration": duration,
                "explicit": attribute("explicit"),
            });
            let created = link["meta"]["addedAt"].clone();
            let entry_type = match kind {
                "tracks" => {
                    if let Some(resource) = resource {
                        let artists = artists(resource);
                        item["artist"] = artists.first().cloned().unwrap_or(Value::Null);
                        item["artists"] = Value::Array(artists);
                        if let Some(album) = document.related(resource, "albums").first() {
                            let cover = document
                                .related(album, "coverArt")
                                .first()
                                .and_then(|art| artwork_uuid(art));
                            item["album"] = json!({
                                "id": text_id(&album["id"]),
                                "title": album["attributes"]["title"],
                                "cover": cover,
                            });
                        }
                    }
                    "track"
                }
                "videos" => {
                    if let Some(resource) = resource {
                        let artists = artists(resource);
                        item["artist"] = artists.first().cloned().unwrap_or(Value::Null);
                        item["artists"] = Value::Array(artists);
                        item["imageId"] = document
                            .related(resource, "thumbnailArt")
                            .first()
                            .and_then(|art| artwork_uuid(art))
                            .map_or(Value::Null, Value::String);
                        item["releaseDate"] = attribute("releaseDate");
                    }
                    "video"
                }
                _ => return None,
            };
            Some(json!({ "item": item, "type": entry_type, "created": created }))
        })
        .collect()
}
