//! TIDAL Upload: the account's own albums and singles, on the documented
//! JSON:API at `openapi.tidal.com/v2` (see `docs/_reference/tidal-api.md`,
//! "Upload"). The web player's flow, read from its bundle:
//!
//! ```text
//! GET    artists?filter[owners.id]=me                      the account's artist
//! GET    albums?filter[owners.id]=<userId>&include=artists,coverArt
//! GET    albums/{id}/relationships/items?include=items,items.sourceFile
//! POST   albums        {albumType, title} + artists        a new single/EP/album
//! POST   tracks        {accessType, title} + albums, artists
//! POST   trackSourceFiles {md5Hash, size} + track          -> uploadLink
//! PUT    <uploadLink.href> with uploadLink.meta.headers    the file itself
//! PATCH  albums/{id}   {title, albumType, accessType}
//! PATCH  tracks/{id}   {title}
//! PATCH  tracks/{id}/relationships/albums [{albums}]       move a track
//! PATCH  albums/{id}/relationships/items [{tracks}] + meta.positionIndex
//! PATCH  artists/{id}  {name}
//! POST   artworks      {mediaType: IMAGE, sourceFile {md5Hash, size}}
//! PATCH  albums/{id}/relationships/coverArt [{artworks}]
//! GET    albums/{id}/relationships/suggestedCoverArts      the spectrograms
//! DELETE tracks/{id}, albums/{id}
//! ```
//!
//! The file's own upload (the `PUT`) is not here: it streams from disk on a
//! thread of the backend's (`backend.rs`), outside the JSON client.

use reqwest::Method;
use serde_json::{Value, json};

use super::{Call, Result, TidalClient};
use crate::api::ApiError;

/// Who can see and play an upload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Access {
    Public,
    #[default]
    Unlisted,
    /// Flagged by moderation; shown like Unlisted, never chosen.
    Private,
}

impl Access {
    pub fn parse(text: &str) -> Self {
        match text {
            "PUBLIC" => Self::Public,
            "PRIVATE" => Self::Private,
            _ => Self::Unlisted,
        }
    }

    pub fn api(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Unlisted => "UNLISTED",
            Self::Private => "PRIVATE",
        }
    }

    /// What a new track in an album of this access gets (TIDAL's web
    /// player: a private album's tracks are Unlisted).
    pub fn for_new_track(self) -> Self {
        match self {
            Self::Public => Self::Public,
            _ => Self::Unlisted,
        }
    }
}

/// Single, EP or Album.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlbumType {
    #[default]
    Single,
    Ep,
    Album,
}

impl AlbumType {
    pub fn parse(text: &str) -> Self {
        match text {
            "ALBUM" => Self::Album,
            "EP" => Self::Ep,
            _ => Self::Single,
        }
    }

    pub fn api(self) -> &'static str {
        match self {
            Self::Single => "SINGLE",
            Self::Ep => "EP",
            Self::Album => "ALBUM",
        }
    }

    /// TIDAL's web player picks the type of a new release by how many
    /// files it starts with: 1-2 Single, 3-6 EP, 7 or more Album.
    pub fn for_tracks(count: usize) -> Self {
        match count {
            0..=2 => Self::Single,
            3..=6 => Self::Ep,
            _ => Self::Album,
        }
    }
}

/// The account's artist: the name uploads are released under.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadArtist {
    pub id: String,
    pub name: String,
}

/// One of the account's uploaded releases.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UploadAlbum {
    pub id: String,
    pub title: String,
    pub album_type: AlbumType,
    pub access: Access,
    pub artist_id: Option<String>,
    pub artist_name: String,
    /// The cover at the size nearest 640, when it has one.
    pub cover: Option<String>,
    pub tracks: u32,
    pub explicit: bool,
}

/// Where a track's file is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileState {
    /// Not uploaded yet, or still being processed or scanned.
    #[default]
    Preparing,
    /// Processed, and still being scanned by moderation.
    Scanning,
    Ready,
    /// Flagged or taken down by moderation.
    Flagged,
    /// Processing failed.
    Failed,
}

/// One track of an uploaded release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UploadTrack {
    pub id: String,
    pub title: String,
    pub access: Access,
    pub duration_secs: Option<u32>,
    pub file: FileState,
    pub number: u32,
}

/// Where and how the file goes (`uploadLink`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadLink {
    pub href: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
}

/// One page of a JSON:API list and the cursor of the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
}

/// A suggested cover (TIDAL's spectrogram artwork).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuggestedCover {
    pub artwork_id: String,
    pub image: Option<String>,
    pub background: Option<String>,
    pub foreground: Option<String>,
}

/// A change to a release's details; `None` leaves the field as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumChange {
    pub title: Option<String>,
    pub album_type: Option<AlbumType>,
    pub access: Option<Access>,
}

impl AlbumChange {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.album_type.is_none() && self.access.is_none()
    }
}

fn id_of(value: &Value) -> Option<String> {
    match value {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// `included` entries by `(type, id)`.
fn find<'a>(document: &'a Value, kind: &str, id: &str) -> Option<&'a Value> {
    document["included"].as_array()?.iter().find(|entry| {
        entry["type"].as_str() == Some(kind) && id_of(&entry["id"]).as_deref() == Some(id)
    })
}

/// The next page's cursor: `links.meta.nextCursor`, else `page[cursor]` of
/// `links.next`.
fn next_cursor(document: &Value) -> Option<String> {
    if let Some(cursor) = document["links"]["meta"]["nextCursor"].as_str() {
        return (!cursor.is_empty()).then(|| cursor.to_string());
    }
    let link = document["links"]["next"].as_str()?;
    let (_, query) = link.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "page%5Bcursor%5D" || key == "page[cursor]").then(|| {
            percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .into_owned()
        })
    })
}

/// The file of an artwork nearest `width` (TIDAL lists several sizes).
fn artwork_href(artwork: &Value, width: u64) -> Option<String> {
    let files = artwork["attributes"]["files"].as_array()?;
    files
        .iter()
        .filter_map(|file| {
            let href = file["href"].as_str()?;
            let w = file["meta"]["width"].as_u64().unwrap_or(0);
            let h = file["meta"]["height"].as_u64().unwrap_or(w);
            // Square files first: TIDAL also lists wide crops (1280x720).
            let wide = u64::from(h != w) * 100_000;
            Some((w.abs_diff(width) + wide, href))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, href)| href.to_string())
}

/// ISO 8601 durations as TIDAL writes them (`PT3M5S`, `PT1H2M`, `PT12.5S`).
pub fn parse_duration(text: &str) -> Option<u32> {
    let rest = text.strip_prefix("PT")?;
    let mut total = 0.0_f64;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' | 'M' | 'S' => {
                let value: f64 = number.parse().ok()?;
                number.clear();
                total += value
                    * match c {
                        'H' => 3600.0,
                        'M' => 60.0,
                        _ => 1.0,
                    };
            }
            _ => return None,
        }
    }
    number.is_empty().then_some(total.round() as u32)
}

pub(crate) fn parse_albums(document: &Value) -> Paged<UploadAlbum> {
    let items = document["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["type"].as_str().unwrap_or("albums") == "albums")
        .filter_map(|entry| {
            let id = id_of(&entry["id"])?;
            let attributes = &entry["attributes"];
            let artist_id = entry["relationships"]["artists"]["data"]
                .as_array()
                .and_then(|artists| artists.first())
                .and_then(|artist| id_of(&artist["id"]));
            let artist_name = artist_id
                .as_deref()
                .and_then(|artist| find(document, "artists", artist))
                .and_then(|artist| artist["attributes"]["name"].as_str())
                .unwrap_or_default()
                .to_string();
            let cover = entry["relationships"]["coverArt"]["data"]
                .as_array()
                .and_then(|art| art.first())
                .and_then(|art| id_of(&art["id"]))
                .and_then(|art| find(document, "artworks", &art))
                .and_then(|art| artwork_href(art, 640));
            Some(UploadAlbum {
                id,
                title: attributes["title"].as_str().unwrap_or_default().to_string(),
                album_type: AlbumType::parse(attributes["albumType"].as_str().unwrap_or("")),
                access: Access::parse(attributes["accessType"].as_str().unwrap_or("")),
                artist_id,
                artist_name,
                cover,
                tracks: attributes["numberOfItems"].as_u64().unwrap_or(0) as u32,
                explicit: attributes["explicit"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    Paged {
        items,
        next: next_cursor(document),
    }
}

fn file_state(source: Option<&Value>) -> FileState {
    let Some(status) = source.map(|source| &source["attributes"]["status"]) else {
        return FileState::Preparing;
    };
    let technical = status["technicalFileStatus"].as_str().unwrap_or("");
    let moderation = status["moderationFileStatus"].as_str().unwrap_or("");
    if matches!(technical, "ERROR" | "DELETED") {
        FileState::Failed
    } else if matches!(moderation, "FLAGGED" | "TAKEN_DOWN") {
        FileState::Flagged
    } else if technical == "OK" && matches!(moderation, "NOT_MODERATED" | "SCANNING") {
        FileState::Scanning
    } else if technical == "OK" {
        FileState::Ready
    } else {
        FileState::Preparing
    }
}

pub(crate) fn parse_tracks(document: &Value) -> Paged<UploadTrack> {
    let items = document["data"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, entry)| entry["type"].as_str() == Some("tracks"))
        .filter_map(|(index, entry)| {
            let id = id_of(&entry["id"])?;
            let track = find(document, "tracks", &id);
            let attributes = track.map(|track| &track["attributes"]);
            let source = track
                .and_then(|track| id_of(&track["relationships"]["sourceFile"]["data"]["id"]))
                .and_then(|source| find(document, "trackSourceFiles", &source));
            Some(UploadTrack {
                title: attributes
                    .and_then(|a| a["title"].as_str())
                    .unwrap_or_default()
                    .to_string(),
                access: Access::parse(
                    attributes
                        .and_then(|a| a["accessType"].as_str())
                        .unwrap_or(""),
                ),
                duration_secs: attributes
                    .and_then(|a| a["duration"].as_str())
                    .and_then(parse_duration)
                    .filter(|secs| *secs > 0),
                file: file_state(source),
                number: entry["meta"]["trackNumber"]
                    .as_u64()
                    .map_or(index as u32 + 1, |n| n as u32),
                id,
            })
        })
        .collect();
    Paged {
        items,
        next: next_cursor(document),
    }
}

pub(crate) fn parse_artist(document: &Value) -> Option<UploadArtist> {
    let entry = match &document["data"] {
        Value::Array(entries) => entries.first()?,
        entry @ Value::Object(_) => entry,
        _ => return None,
    };
    Some(UploadArtist {
        id: id_of(&entry["id"])?,
        name: entry["attributes"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

pub(crate) fn parse_upload_link(attributes: &Value) -> Option<UploadLink> {
    let link = &attributes["uploadLink"];
    let href = link["href"].as_str().filter(|href| !href.is_empty())?;
    let headers = link["meta"]["headers"]
        .as_object()
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(UploadLink {
        href: href.to_string(),
        method: link["meta"]["method"].as_str().unwrap_or("PUT").to_string(),
        headers,
    })
}

pub(crate) fn parse_suggested(document: &Value) -> Vec<SuggestedCover> {
    document["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let artwork_id = id_of(&entry["id"])?;
            Some(SuggestedCover {
                image: find(document, "artworks", &artwork_id)
                    .and_then(|art| artwork_href(art, 320)),
                background: entry["meta"]["backgroundColor"]
                    .as_str()
                    .map(str::to_string),
                foreground: entry["meta"]["foregroundColor"]
                    .as_str()
                    .map(str::to_string),
                artwork_id,
            })
        })
        .collect()
}

// ---- request bodies (the shapes are the OpenAPI spec's) ----------------------

pub(crate) fn new_album_body(title: &str, album_type: AlbumType, artist_id: &str) -> Value {
    json!({"data": {
        "type": "albums",
        "attributes": {"albumType": album_type.api(), "title": title},
        "relationships": {"artists": {"data": [{"id": artist_id, "type": "artists"}]}},
    }})
}

pub(crate) fn new_track_body(
    title: &str,
    access: Access,
    album_id: &str,
    artist_id: &str,
) -> Value {
    json!({"data": {
        "type": "tracks",
        "attributes": {"accessType": access.api(), "title": title},
        "relationships": {
            "albums": {"data": [{"id": album_id, "type": "albums"}]},
            "artists": {"data": [{"id": artist_id, "type": "artists"}]},
        },
    }})
}

pub(crate) fn source_file_body(track_id: &str, md5_hex: &str, size: u64) -> Value {
    json!({"data": {
        "type": "trackSourceFiles",
        "attributes": {"md5Hash": md5_hex, "size": size},
        "relationships": {"track": {"data": {"id": track_id, "type": "tracks"}}},
    }})
}

pub(crate) fn artwork_body(md5_hex: &str, size: u64) -> Value {
    json!({"data": {
        "type": "artworks",
        "attributes": {"mediaType": "IMAGE", "sourceFile": {"md5Hash": md5_hex, "size": size}},
    }})
}

pub(crate) fn album_change_body(id: &str, change: &AlbumChange) -> Value {
    let mut attributes = serde_json::Map::new();
    if let Some(title) = &change.title {
        attributes.insert("title".into(), json!(title));
    }
    if let Some(album_type) = change.album_type {
        attributes.insert("albumType".into(), json!(album_type.api()));
    }
    if let Some(access) = change.access {
        attributes.insert("accessType".into(), json!(access.api()));
    }
    json!({"data": {"id": id, "type": "albums", "attributes": attributes}})
}

pub(crate) fn track_title_body(id: &str, title: &str) -> Value {
    json!({"data": {"id": id, "type": "tracks", "attributes": {"title": title}}})
}

pub(crate) fn artist_name_body(id: &str, name: &str) -> Value {
    json!({"data": {"id": id, "type": "artists", "attributes": {"name": name}}})
}

pub(crate) fn one(kind: &str, id: &str) -> Value {
    json!({"data": [{"id": id, "type": kind}]})
}

pub(crate) fn position_body(track_id: &str, position: u32) -> Value {
    json!({
        "data": [{"id": track_id, "type": "tracks"}],
        "meta": {"positionIndex": position},
    })
}

/// The id of a created resource (`data.id`).
fn created_id(body: &str) -> Result<(String, Value)> {
    let document: Value =
        serde_json::from_str(body).map_err(|error| ApiError::Decode(error.to_string()))?;
    let id = id_of(&document["data"]["id"])
        .ok_or_else(|| ApiError::Decode("TIDAL answered without an id".into()))?;
    Ok((id, document))
}

impl TidalClient {
    fn openapi(&self, path: &str) -> String {
        format!("{}/{path}", self.openapi_base)
    }

    async fn openapi_write(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<String> {
        let mut call = Call::new(method, self.openapi(path));
        if let Some(body) = body {
            call = call.json_api(body);
        }
        Ok(self.send(call).await?.body)
    }

    /// The account's artist, `None` when it has none yet (it never
    /// uploaded and never chose a name).
    pub async fn upload_artist(&self) -> Result<Option<UploadArtist>> {
        let document: Value = self
            .get(Call::new(Method::GET, self.openapi("artists")).query("filter[owners.id]", "me"))
            .await?;
        Ok(parse_artist(&document))
    }

    /// One page of the account's uploaded releases, newest first.
    pub async fn upload_albums(&self, cursor: Option<&str>) -> Result<Paged<UploadAlbum>> {
        let mut call = Call::new(Method::GET, self.openapi("albums"))
            .query("filter[owners.id]", self.user_id()?)
            .query("include", "artists,coverArt")
            .query("sort", "-createdAt");
        if let Some(cursor) = cursor {
            call = call.query("page[cursor]", cursor);
        }
        let document: Value = self.get(call).await?;
        Ok(parse_albums(&document))
    }

    /// One page of a release's tracks with their files' states.
    pub async fn upload_tracks(
        &self,
        album_id: &str,
        cursor: Option<&str>,
    ) -> Result<Paged<UploadTrack>> {
        let mut call = Call::new(
            Method::GET,
            self.openapi(&format!("albums/{album_id}/relationships/items")),
        )
        .query("include", "items,items.sourceFile");
        if let Some(cursor) = cursor {
            call = call.query("page[cursor]", cursor);
        }
        let document: Value = self.get(call).await?;
        Ok(parse_tracks(&document))
    }

    /// A new release under the account's artist; its id.
    pub async fn create_upload_album(
        &self,
        title: &str,
        album_type: AlbumType,
        artist_id: &str,
    ) -> Result<String> {
        let body = self
            .openapi_write(
                Method::POST,
                "albums",
                Some(&new_album_body(title, album_type, artist_id)),
            )
            .await?;
        Ok(created_id(&body)?.0)
    }

    /// A new track in a release; its id.
    pub async fn create_upload_track(
        &self,
        title: &str,
        access: Access,
        album_id: &str,
        artist_id: &str,
    ) -> Result<String> {
        let body = self
            .openapi_write(
                Method::POST,
                "tracks",
                Some(&new_track_body(title, access, album_id, artist_id)),
            )
            .await?;
        Ok(created_id(&body)?.0)
    }

    /// Announces a track's file; where to send it.
    pub async fn create_track_source(
        &self,
        track_id: &str,
        md5_hex: &str,
        size: u64,
    ) -> Result<UploadLink> {
        let body = self
            .openapi_write(
                Method::POST,
                "trackSourceFiles",
                Some(&source_file_body(track_id, md5_hex, size)),
            )
            .await?;
        let (_, document) = created_id(&body)?;
        parse_upload_link(&document["data"]["attributes"])
            .ok_or_else(|| ApiError::Decode("TIDAL gave no upload address".into()))
    }

    /// Announces a cover image; its artwork id and where to send it.
    pub async fn create_artwork(&self, md5_hex: &str, size: u64) -> Result<(String, UploadLink)> {
        let body = self
            .openapi_write(Method::POST, "artworks", Some(&artwork_body(md5_hex, size)))
            .await?;
        let (id, document) = created_id(&body)?;
        let link = parse_upload_link(&document["data"]["attributes"]["sourceFile"])
            .ok_or_else(|| ApiError::Decode("TIDAL gave no upload address".into()))?;
        Ok((id, link))
    }

    /// Sends a small file (a cover) to its upload address. The address is
    /// signed: no token goes with it, and it is never logged.
    pub async fn put_upload(
        &self,
        link: &UploadLink,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        let http = self.http.client().map_err(ApiError::Network)?;
        let method = Method::from_bytes(link.method.as_bytes())
            .map_err(|_| ApiError::Decode("the upload method".into()))?;
        let mut request = http
            .request(method, &link.href)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes);
        for (key, value) in &link.headers {
            request = request.header(key.as_str(), value.as_str());
        }
        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Network(error.without_url().to_string()))?;
        let status = response.status().as_u16();
        log::debug!("TIDAL artwork PUT answered {status}");
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(ApiError::Status {
                status,
                message: format!("Upload failed ({status})"),
            })
        }
    }

    pub async fn set_upload_cover(&self, album_id: &str, artwork_id: &str) -> Result<()> {
        self.openapi_write(
            Method::PATCH,
            &format!("albums/{album_id}/relationships/coverArt"),
            Some(&one("artworks", artwork_id)),
        )
        .await?;
        Ok(())
    }

    pub async fn suggested_covers(&self, album_id: &str) -> Result<Vec<SuggestedCover>> {
        let document: Value = self
            .get(
                Call::new(
                    Method::GET,
                    self.openapi(&format!(
                        "albums/{album_id}/relationships/suggestedCoverArts"
                    )),
                )
                .query("include", "suggestedCoverArts"),
            )
            .await?;
        Ok(parse_suggested(&document))
    }

    pub async fn change_upload_album(&self, album_id: &str, change: &AlbumChange) -> Result<()> {
        if change.is_empty() {
            return Ok(());
        }
        self.openapi_write(
            Method::PATCH,
            &format!("albums/{album_id}"),
            Some(&album_change_body(album_id, change)),
        )
        .await?;
        Ok(())
    }

    pub async fn rename_upload_track(&self, track_id: &str, title: &str) -> Result<()> {
        self.openapi_write(
            Method::PATCH,
            &format!("tracks/{track_id}"),
            Some(&track_title_body(track_id, title)),
        )
        .await?;
        Ok(())
    }

    pub async fn move_upload_track(&self, track_id: &str, album_id: &str) -> Result<()> {
        self.openapi_write(
            Method::PATCH,
            &format!("tracks/{track_id}/relationships/albums"),
            Some(&one("albums", album_id)),
        )
        .await?;
        Ok(())
    }

    /// Puts a track at `position` (1-based) in its release.
    pub async fn reorder_upload_track(
        &self,
        album_id: &str,
        track_id: &str,
        position: u32,
    ) -> Result<()> {
        self.openapi_write(
            Method::PATCH,
            &format!("albums/{album_id}/relationships/items"),
            Some(&position_body(track_id, position)),
        )
        .await?;
        Ok(())
    }

    pub async fn rename_upload_artist(&self, artist_id: &str, name: &str) -> Result<()> {
        self.openapi_write(
            Method::PATCH,
            &format!("artists/{artist_id}"),
            Some(&artist_name_body(artist_id, name)),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_upload_track(&self, track_id: &str) -> Result<()> {
        self.openapi_write(Method::DELETE, &format!("tracks/{track_id}"), None)
            .await?;
        Ok(())
    }

    pub async fn delete_upload_album(&self, album_id: &str) -> Result<()> {
        self.openapi_write(Method::DELETE, &format!("albums/{album_id}"), None)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_tidals_iso_form() {
        assert_eq!(parse_duration("PT3M5S"), Some(185));
        assert_eq!(parse_duration("PT1H2M"), Some(3720));
        assert_eq!(parse_duration("PT12.6S"), Some(13));
        assert_eq!(parse_duration("PT0S"), Some(0));
        assert_eq!(parse_duration("3:05"), None);
        assert_eq!(parse_duration("PT3"), None);
    }

    #[test]
    fn a_new_release_takes_its_type_from_how_many_files_it_starts_with() {
        assert_eq!(AlbumType::for_tracks(1), AlbumType::Single);
        assert_eq!(AlbumType::for_tracks(2), AlbumType::Single);
        assert_eq!(AlbumType::for_tracks(3), AlbumType::Ep);
        assert_eq!(AlbumType::for_tracks(6), AlbumType::Ep);
        assert_eq!(AlbumType::for_tracks(7), AlbumType::Album);
        assert_eq!(Access::Private.for_new_track(), Access::Unlisted);
        assert_eq!(Access::Public.for_new_track(), Access::Public);
    }

    #[test]
    fn the_album_list_reads_names_covers_and_counts_from_included() {
        let document = json!({
            "data": [
                {"id": "500", "type": "albums",
                 "attributes": {"title": "Demo", "albumType": "EP", "accessType": "PUBLIC",
                                "numberOfItems": 3, "explicit": true},
                 "relationships": {
                    "artists": {"data": [{"id": "77", "type": "artists"}]},
                    "coverArt": {"data": [{"id": "art1", "type": "artworks"}]}}},
                {"id": 501, "type": "albums",
                 "attributes": {"title": "Fresh", "albumType": "SINGLE", "accessType": "UNLISTED",
                                "numberOfItems": 0},
                 "relationships": {"artists": {"data": [{"id": "77", "type": "artists"}]},
                                   "coverArt": {"data": []}}}
            ],
            "included": [
                {"id": "77", "type": "artists", "attributes": {"name": "Me"}},
                {"id": "art1", "type": "artworks", "attributes": {"files": [
                    {"href": "https://img/80.jpg", "meta": {"width": 80, "height": 80}},
                    {"href": "https://img/640.jpg", "meta": {"width": 640, "height": 640}},
                    {"href": "https://img/1280.jpg", "meta": {"width": 1280, "height": 1280}}]}}
            ],
            "links": {"self": "/albums", "meta": {"nextCursor": "abc"}}
        });
        let page = parse_albums(&document);
        assert_eq!(page.next.as_deref(), Some("abc"));
        assert_eq!(page.items.len(), 2);
        let first = &page.items[0];
        assert_eq!(first.title, "Demo");
        assert_eq!(first.album_type, AlbumType::Ep);
        assert_eq!(first.access, Access::Public);
        assert_eq!(first.artist_name, "Me");
        assert_eq!(first.cover.as_deref(), Some("https://img/640.jpg"));
        assert_eq!(first.tracks, 3);
        assert!(first.explicit);
        let second = &page.items[1];
        assert_eq!(second.id, "501");
        assert_eq!(second.access, Access::Unlisted);
        assert_eq!(second.cover, None);
    }

    #[test]
    fn tracks_read_their_files_state_and_duration() {
        let document = json!({
            "data": [
                {"id": "1", "type": "tracks", "meta": {"trackNumber": 1, "volumeNumber": 1}},
                {"id": "2", "type": "tracks", "meta": {"trackNumber": 2, "volumeNumber": 1}},
                {"id": "3", "type": "tracks"}
            ],
            "included": [
                {"id": "1", "type": "tracks", "attributes": {"title": "One", "duration": "PT2M",
                    "accessType": "UNLISTED"},
                 "relationships": {"sourceFile": {"data": {"id": "s1", "type": "trackSourceFiles"}}}},
                {"id": "2", "type": "tracks", "attributes": {"title": "Two", "duration": "PT0S",
                    "accessType": "PUBLIC"},
                 "relationships": {"sourceFile": {"data": {"id": "s2", "type": "trackSourceFiles"}}}},
                {"id": "3", "type": "tracks", "attributes": {"title": "Three"}},
                {"id": "s1", "type": "trackSourceFiles", "attributes": {"status":
                    {"technicalFileStatus": "OK", "moderationFileStatus": "OK"}}},
                {"id": "s2", "type": "trackSourceFiles", "attributes": {"status":
                    {"technicalFileStatus": "PROCESSING", "moderationFileStatus": "NOT_MODERATED"}}}
            ]
        });
        let tracks = parse_tracks(&document).items;
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].title, "One");
        assert_eq!(tracks[0].duration_secs, Some(120));
        assert_eq!(tracks[0].file, FileState::Ready);
        assert_eq!(tracks[1].file, FileState::Preparing);
        assert_eq!(tracks[1].duration_secs, None);
        assert_eq!(tracks[1].access, Access::Public);
        assert_eq!(tracks[2].file, FileState::Preparing);
        // Seen live (2026-10-18): moderation FLAGGED and ERROR with the file
        // processed, and a fresh upload still being scanned.
        let status = |technical: &str, moderation: &str| {
            file_state(Some(&json!({"attributes": {"status": {
                "technicalFileStatus": technical, "moderationFileStatus": moderation}}})))
        };
        assert_eq!(status("OK", "FLAGGED"), FileState::Flagged);
        assert_eq!(status("OK", "ERROR"), FileState::Ready);
        assert_eq!(status("OK", "SCANNING"), FileState::Scanning);
        assert_eq!(status("ERROR", "OK"), FileState::Failed);
        assert_eq!(tracks[2].number, 3);
    }

    #[test]
    fn the_upload_link_keeps_its_method_and_headers() {
        let attributes = json!({"uploadLink": {"href": "https://up/x?sig=1",
            "meta": {"method": "PUT", "headers": {"x-amz-meta-a": "b", "Content-MD5": "c"}}}});
        let link = parse_upload_link(&attributes).unwrap();
        assert_eq!(link.href, "https://up/x?sig=1");
        assert_eq!(link.method, "PUT");
        assert!(link.headers.contains(&("x-amz-meta-a".into(), "b".into())));
        assert_eq!(parse_upload_link(&json!({})), None);
        let bare = parse_upload_link(&json!({"uploadLink": {"href": "https://up/y", "meta": {}}}));
        assert_eq!(bare.unwrap().method, "PUT");
    }

    #[test]
    fn the_artist_is_the_first_one_the_account_owns() {
        let document =
            json!({"data": [{"id": "77", "type": "artists", "attributes": {"name": "Me"}}]});
        assert_eq!(
            parse_artist(&document),
            Some(UploadArtist {
                id: "77".into(),
                name: "Me".into()
            })
        );
        assert_eq!(parse_artist(&json!({"data": []})), None);
    }

    #[test]
    fn write_bodies_have_the_specs_shapes() {
        assert_eq!(
            new_album_body("Song", AlbumType::Single, "77"),
            json!({"data": {"type": "albums",
                "attributes": {"albumType": "SINGLE", "title": "Song"},
                "relationships": {"artists": {"data": [{"id": "77", "type": "artists"}]}}}})
        );
        assert_eq!(
            new_track_body("Song", Access::Unlisted, "500", "77"),
            json!({"data": {"type": "tracks",
                "attributes": {"accessType": "UNLISTED", "title": "Song"},
                "relationships": {
                    "albums": {"data": [{"id": "500", "type": "albums"}]},
                    "artists": {"data": [{"id": "77", "type": "artists"}]}}}})
        );
        assert_eq!(
            source_file_body("9", "d41d8cd98f00b204e9800998ecf8427e", 0),
            json!({"data": {"type": "trackSourceFiles",
                "attributes": {"md5Hash": "d41d8cd98f00b204e9800998ecf8427e", "size": 0},
                "relationships": {"track": {"data": {"id": "9", "type": "tracks"}}}}})
        );
        assert_eq!(
            album_change_body(
                "500",
                &AlbumChange {
                    title: Some("New".into()),
                    album_type: Some(AlbumType::Ep),
                    access: None,
                }
            ),
            json!({"data": {"id": "500", "type": "albums",
                "attributes": {"title": "New", "albumType": "EP"}}})
        );
        assert_eq!(
            one("albums", "501"),
            json!({"data": [{"id": "501", "type": "albums"}]})
        );
        assert_eq!(
            position_body("9", 2),
            json!({"data": [{"id": "9", "type": "tracks"}], "meta": {"positionIndex": 2}})
        );
        assert_eq!(
            artist_name_body("77", "Name"),
            json!({"data": {"id": "77", "type": "artists", "attributes": {"name": "Name"}}})
        );
        assert_eq!(
            artwork_body("abc", 10),
            json!({"data": {"type": "artworks",
                "attributes": {"mediaType": "IMAGE", "sourceFile": {"md5Hash": "abc", "size": 10}}}})
        );
        assert_eq!(
            track_title_body("9", "T"),
            json!({"data": {"id": "9", "type": "tracks", "attributes": {"title": "T"}}})
        );
    }

    #[test]
    fn suggested_covers_read_ids_colours_and_images() {
        let document = json!({
            "data": [{"id": "a1", "type": "artworks",
                      "meta": {"backgroundColor": "#000000", "foregroundColor": "#ffffff"}}],
            "included": [{"id": "a1", "type": "artworks", "attributes": {"files": [
                {"href": "https://img/320.jpg", "meta": {"width": 320}}]}}]
        });
        let covers = parse_suggested(&document);
        assert_eq!(covers.len(), 1);
        assert_eq!(covers[0].artwork_id, "a1");
        assert_eq!(covers[0].image.as_deref(), Some("https://img/320.jpg"));
        assert_eq!(covers[0].foreground.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn the_next_cursor_comes_from_the_link_too() {
        let document = json!({"links": {"next": "/albums?page%5Bcursor%5D=a%3Db&x=1"}});
        assert_eq!(next_cursor(&document).as_deref(), Some("a=b"));
        assert_eq!(next_cursor(&json!({"links": {}})), None);
    }
}
