//! JSON:API documents, as `openapi.tidal.com/v2` answers them.
//!
//! A document's `data` holds the resources asked for, or only their
//! identifiers on a `relationships/...` read, with `include`d resources in
//! `included`. A resource names its relations as identifiers under
//! `relationships.<name>.data`. Pages go by cursor: `links.meta.nextCursor`,
//! also written into `links.next` as `page[cursor]`.
//!
//! ```text
//! GET /artists/17275/relationships/similarArtists?include=similarArtists.profileArt
//! ```
//!
//! An identifier without its resource is left out rather than failing the
//! answer. Ids arrive as strings, sometimes as numbers.

use std::collections::HashMap;

use serde_json::Value;

/// A document with its `included` resources indexed by type and id.
pub(crate) struct Document<'a> {
    root: &'a Value,
    included: HashMap<(&'a str, String), &'a Value>,
}

impl<'a> Document<'a> {
    pub fn new(root: &'a Value) -> Self {
        let included = root["included"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|resource| Some(((resource["type"].as_str()?, id_of(resource)?), resource)))
            .collect();
        Self { root, included }
    }

    /// `data`: a list, or the one resource of a single read.
    pub fn data(&self) -> Vec<&'a Value> {
        match &self.root["data"] {
            Value::Array(entries) => entries.iter().collect(),
            Value::Null => Vec::new(),
            single => vec![single],
        }
    }

    /// The included resource an identifier names.
    pub fn resource(&self, identifier: &Value) -> Option<&'a Value> {
        let kind = identifier["type"].as_str()?;
        self.included.get(&(kind, id_of(identifier)?)).copied()
    }

    /// The included resources of a relation, in the relation's order.
    pub fn related(&self, resource: &Value, name: &str) -> Vec<&'a Value> {
        identifiers(resource, name)
            .into_iter()
            .filter_map(|identifier| self.resource(identifier))
            .collect()
    }

    /// `data` resolved to the included resources, for a `relationships/...`
    /// read.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn resources(&self) -> Vec<&'a Value> {
        self.data()
            .into_iter()
            .filter_map(|identifier| self.resource(identifier))
            .collect()
    }
}

/// A resource's or identifier's id as text.
pub(crate) fn id_of(resource: &Value) -> Option<String> {
    text_id(&resource["id"])
}

pub(crate) fn text_id(id: &Value) -> Option<String> {
    match id {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The identifiers of a relation; one identifier for a to-one relation.
pub(crate) fn identifiers<'v>(resource: &'v Value, name: &str) -> Vec<&'v Value> {
    match &resource["relationships"][name]["data"] {
        Value::Array(entries) => entries.iter().collect(),
        Value::Null => Vec::new(),
        single => vec![single],
    }
}

/// The next page's cursor: `links.meta.nextCursor`, else `page[cursor]` of
/// `links.next`. Decoded, as it goes back as a query value.
pub(crate) fn next_cursor(document: &Value) -> Option<String> {
    let decode = |cursor: &str| {
        percent_encoding::percent_decode_str(cursor)
            .decode_utf8_lossy()
            .into_owned()
    };
    if let Some(cursor) = document["links"]["meta"]["nextCursor"].as_str() {
        return (!cursor.is_empty()).then(|| decode(cursor));
    }
    let (_, query) = document["links"]["next"].as_str()?.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "page%5Bcursor%5D" || key == "page[cursor]").then(|| decode(value))
    })
}

/// ISO 8601 durations as TIDAL writes them (`PT3M5S`, `PT1H2M`, `PT12.5S`),
/// in seconds.
pub(crate) fn duration_seconds(text: &str) -> Option<u32> {
    let rest = text.strip_prefix("PT")?;
    let mut total = 0.0_f64;
    let mut number = String::new();
    for character in rest.chars() {
        match character {
            '0'..='9' | '.' => number.push(character),
            'H' | 'M' | 'S' => {
                let value: f64 = number.parse().ok()?;
                number.clear();
                total += value
                    * match character {
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

/// The image id (`xxxxxxxx-xxxx-...`) behind an artwork's file address
/// (`https://resources.tidal.com/images/xxxxxxxx/xxxx/.../1280x1280.jpg`).
pub(crate) fn artwork_uuid(artwork: &Value) -> Option<String> {
    let files = artwork["attributes"]["files"].as_array()?;
    files.iter().find_map(|file| {
        let href = file["href"].as_str()?;
        let path = href.split_once("/images/")?.1;
        let parts: Vec<&str> = path.split('/').collect();
        // five uuid groups, then the file name
        (parts.len() == 6).then(|| parts[..5].join("-"))
    })
}

/// The file of an artwork nearest `width` (TIDAL lists several sizes).
pub(crate) fn artwork_href(artwork: &Value, width: u64) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document() -> Value {
        json!({
            "data": [
                {"id": "1", "type": "artists"},
                {"id": 2, "type": "artists"},
                {"id": "gone", "type": "artists"}
            ],
            "included": [
                {"id": "2", "type": "artists", "attributes": {"name": "Two"}},
                {"id": 1, "type": "artists", "attributes": {"name": "One"},
                 "relationships": {
                    "profileArt": {"data": [{"id": "art", "type": "artworks"}]},
                    "owner": {"data": {"id": "2", "type": "artists"}},
                    "empty": {"links": {"self": "/x"}}
                 }},
                {"id": "art", "type": "artworks", "attributes": {"files": [
                    {"href": "https://resources.tidal.com/images/aa/bb/cc/dd/ee/160x160.jpg", "meta": {"width": 160, "height": 160}},
                    {"href": "https://resources.tidal.com/images/aa/bb/cc/dd/ee/750x750.jpg", "meta": {"width": 750, "height": 750}}
                ]}}
            ]
        })
    }

    #[test]
    fn relationship_reads_resolve_in_order_and_leave_out_what_is_missing() {
        let root = document();
        let document = Document::new(&root);
        let names: Vec<_> = document
            .resources()
            .iter()
            .map(|artist| artist["attributes"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["One", "Two"]);
        assert_eq!(document.data().len(), 3);
    }

    #[test]
    fn relations_resolve_to_many_and_to_one_and_none() {
        let root = document();
        let document = Document::new(&root);
        let one = document.resources()[0];
        assert_eq!(document.related(one, "profileArt").len(), 1);
        assert_eq!(
            document.related(one, "owner")[0]["attributes"]["name"],
            "Two"
        );
        assert!(document.related(one, "empty").is_empty());
        assert!(document.related(one, "unknown").is_empty());
    }

    #[test]
    fn a_single_read_has_its_one_resource_as_data() {
        let root = json!({"data": {"id": "1", "type": "tracks"}});
        assert_eq!(Document::new(&root).data().len(), 1);
        let null = json!({"data": null});
        assert!(Document::new(&null).data().is_empty());
        let nothing = json!({});
        assert!(Document::new(&nothing).data().is_empty());
    }

    #[test]
    fn the_cursor_comes_from_the_meta_then_the_link_decoded() {
        let cursor = |links: Value| next_cursor(&json!({ "links": links }));
        assert_eq!(
            cursor(json!({"meta": {"nextCursor": "a%3Db"}})).as_deref(),
            Some("a=b")
        );
        assert_eq!(
            cursor(json!({"next": "/x?include=y&page%5Bcursor%5D=a%3Db"})).as_deref(),
            Some("a=b")
        );
        assert_eq!(
            cursor(json!({"next": "/x?page[cursor]=abc"})).as_deref(),
            Some("abc")
        );
        assert_eq!(cursor(json!({"self": "/x"})), None);
        assert_eq!(cursor(json!({"meta": {"nextCursor": ""}})), None);
    }

    #[test]
    fn durations_read_in_seconds() {
        assert_eq!(duration_seconds("PT3M5S"), Some(185));
        assert_eq!(duration_seconds("PT1H2M"), Some(3720));
        assert_eq!(duration_seconds("PT12.6S"), Some(13));
        assert_eq!(duration_seconds("PT0S"), Some(0));
        assert_eq!(duration_seconds("3:05"), None);
        assert_eq!(duration_seconds("PT3"), None);
    }

    #[test]
    fn artwork_gives_its_uuid_and_the_nearest_file() {
        let root = document();
        let document = Document::new(&root);
        let art = document.related(document.resources()[0], "profileArt")[0];
        assert_eq!(artwork_uuid(art).as_deref(), Some("aa-bb-cc-dd-ee"));
        assert!(artwork_href(art, 200).unwrap().ends_with("160x160.jpg"));
    }
}
