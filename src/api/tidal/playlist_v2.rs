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

/// The `page[cursor]` of a document's `links.next`.
pub(crate) fn next_cursor(document: &Value) -> Option<String> {
    let query = document["links"]["next"].as_str()?.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "page%5Bcursor%5D" || key == "page[cursor]").then(|| value.to_string())
    })
}

fn text_id(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// `PT3M45S`, `PT1H2M3S` as seconds.
pub(crate) fn iso_seconds(text: &str) -> Option<u64> {
    let rest = text.strip_prefix("PT")?;
    let mut total = 0.0_f64;
    let mut number = String::new();
    for character in rest.chars() {
        if character.is_ascii_digit() || character == '.' {
            number.push(character);
            continue;
        }
        let value: f64 = number.parse().ok()?;
        number.clear();
        total += value
            * match character {
                'H' => 3600.0,
                'M' => 60.0,
                'S' => 1.0,
                _ => return None,
            };
    }
    number.is_empty().then_some(total.round() as u64)
}

/// The image id (`xxxxxxxx-xxxx-...`) behind an artwork's file address
/// (`https://resources.tidal.com/images/xxxxxxxx/xxxx/.../1280x1280.jpg`).
fn artwork_uuid(artwork: &Value) -> Option<String> {
    let files = artwork["attributes"]["files"].as_array()?;
    files.iter().find_map(|file| {
        let href = file["href"].as_str()?;
        let path = href.split_once("/images/")?.1;
        let parts: Vec<&str> = path.split('/').collect();
        // five uuid groups, then the file name
        (parts.len() == 6).then(|| parts[..5].join("-"))
    })
}

/// The v1-shaped entries of a v2 document, in its order.
pub(crate) fn entries_of(document: &Value) -> Vec<Value> {
    let included: HashMap<(String, String), &Value> = document["included"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|resource| {
            Some((
                (
                    resource["type"].as_str()?.to_string(),
                    text_id(&resource["id"])?,
                ),
                resource,
            ))
        })
        .collect();
    let related = |resource: &Value, name: &str| -> Vec<&Value> {
        resource["relationships"][name]["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|link| {
                included
                    .get(&(link["type"].as_str()?.to_string(), text_id(&link["id"])?))
                    .copied()
            })
            .collect()
    };
    let artists = |resource: &Value| -> Vec<Value> {
        related(resource, "artists")
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
    document["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let kind = link["type"].as_str()?;
            let id = text_id(&link["id"])?;
            let resource = included.get(&(kind.to_string(), id.clone())).copied();
            let attributes = resource.map(|resource| &resource["attributes"]);
            let attribute = |name: &str| attributes.map_or(Value::Null, |a| a[name].clone());
            let duration = attributes
                .and_then(|a| a["duration"].as_str())
                .and_then(iso_seconds);
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
                        if let Some(album) = related(resource, "albums").first() {
                            let cover = related(album, "coverArt")
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
                        item["imageId"] = related(resource, "thumbnailArt")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_durations_are_read_in_seconds() {
        assert_eq!(iso_seconds("PT3M45S"), Some(225));
        assert_eq!(iso_seconds("PT1H2M3S"), Some(3723));
        assert_eq!(iso_seconds("PT45S"), Some(45));
        assert_eq!(iso_seconds("PT2M"), Some(120));
        assert_eq!(iso_seconds("3M"), None);
        assert_eq!(iso_seconds("PT3X"), None);
    }

    #[test]
    fn the_next_cursor_is_read_from_the_link() {
        let document = json!({"links": {"next": "/playlists/p/relationships/items?page%5Bcursor%5D=abc&include=items"}});
        assert_eq!(next_cursor(&document).as_deref(), Some("abc"));
        assert_eq!(next_cursor(&json!({"links": {}})), None);
    }
}
