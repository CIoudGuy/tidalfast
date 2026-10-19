//! TIDAL's web search: `v2/search/` for the results page (ranked
//! `topHits`, genres, videos and uploads besides the usual lists) and
//! `v2/suggestions/` for the dropdown while typing.
//!
//! Both answers were seen in the web player's network log but are not
//! documented, so they are read tolerantly: every part is optional, and a
//! part that does not parse is left out rather than failing the search.
//! With `TIDALFAST_DUMP_SEARCH=<directory>`, each raw answer is written
//! there for checking the shapes against the parser.

use serde::Deserialize;
use serde_json::Value;

use super::convert::{self, RawAlbum, RawArtist, RawPage, RawPlaylist, RawTrack, RawVideo};
use super::{LOCALE, SEARCH_LIMIT};
use crate::api::models::{
    Page, SearchGenre, SearchResults, SearchSuggestion, SearchSuggestions, TopHit,
};

/// The kinds the results page asks for, in TIDAL's spelling.
pub(super) const TYPES: &str = "ARTISTS,ALBUMS,TRACKS,VIDEOS,PLAYLISTS,UPLOADS";

/// Writes `value` to the dump directory when one is set.
pub(super) fn dump(what: &str, query: &str, value: &Value) {
    let Some(dir) = std::env::var_os("TIDALFAST_DUMP_SEARCH") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let safe: String = query
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    let path = dir.join(format!("{what}-{safe}.json"));
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap_or_default()));
    if let Err(error) = written {
        log::warn!("search dump {}: {error}", path.display());
    }
}

fn page<R: for<'de> Deserialize<'de>, U>(
    value: &Value,
    key: &str,
    convert: impl FnMut(R) -> U,
) -> Option<Page<U>> {
    let raw: RawPage<R> = serde_json::from_value(value.get(key)?.clone())
        .map_err(|error| log::debug!("search: {key} did not parse: {error}"))
        .ok()?;
    Some(raw.map(SEARCH_LIMIT, convert))
}

/// One ranked hit: `{type, value}` with the value in v1's shape.
pub(super) fn hit(kind: &str, value: &Value) -> Option<TopHit> {
    let value = value.clone();
    Some(match kind {
        "ARTISTS" | "ARTIST" => TopHit::Artist(convert::artist(
            serde_json::from_value::<RawArtist>(value).ok()?,
        )),
        "ALBUMS" | "ALBUM" => TopHit::Album(convert::album(
            serde_json::from_value::<RawAlbum>(value).ok()?,
        )),
        "TRACKS" | "TRACK" => TopHit::Track(convert::track(
            serde_json::from_value::<RawTrack>(value).ok()?,
        )),
        "UPLOADS" | "UPLOAD" => TopHit::Upload(convert::track(
            serde_json::from_value::<RawTrack>(value).ok()?,
        )),
        "PLAYLISTS" | "PLAYLIST" => TopHit::Playlist(convert::playlist(
            serde_json::from_value::<RawPlaylist>(value).ok()?,
        )),
        "VIDEOS" | "VIDEO" => TopHit::Video(convert::video(
            serde_json::from_value::<RawVideo>(value).ok()?,
        )),
        _ => return None,
    })
}

fn hits(list: Option<&Value>) -> Vec<TopHit> {
    list.and_then(Value::as_array)
        .map(|hits| {
            hits.iter()
                .filter_map(|entry| {
                    hit(
                        entry.get("type").and_then(Value::as_str).unwrap_or(""),
                        entry.get("value")?,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The results page from a `v2/search/` answer.
pub(super) fn results(value: &Value) -> SearchResults {
    let top_hits = hits(value.get("topHits"));
    let genres = value
        .get("genres")
        .and_then(Value::as_array)
        .map(|genres| {
            genres
                .iter()
                .filter_map(|genre| {
                    let title = genre.get("title").and_then(Value::as_str)?;
                    let api_path = genre
                        .get("apiPath")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    (!title.is_empty()).then(|| SearchGenre {
                        title: title.to_string(),
                        api_path: api_path.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    SearchResults {
        tracks: page(value, "tracks", convert::track),
        artists: page(value, "artists", convert::artist),
        albums: page(value, "albums", convert::album),
        playlists: page(value, "playlists", convert::playlist),
        videos: page(value, "videos", convert::video),
        uploads: page(value, "uploads", convert::track),
        top_hit: top_hits.first().cloned(),
        top_hits,
        genres,
        did_you_mean: did_you_mean(value),
    }
}

/// TIDAL's `didYouMean`: the corrected query, kept only when it says
/// something.
fn did_you_mean(value: &Value) -> Option<String> {
    let found = value.get("didYouMean")?;
    let text = match found {
        Value::String(text) => text.as_str(),
        Value::Object(object) => ["query", "text", "value", "suggestion"]
            .iter()
            .find_map(|key| object.get(*key).and_then(Value::as_str))?,
        Value::Array(list) => list.iter().find_map(Value::as_str)?,
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The dropdown from a `v2/suggestions/` answer.
pub fn suggestions(query: &str, value: &Value) -> SearchSuggestions {
    let suggestions = value
        .get("suggestions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let text = entry.get("query").and_then(Value::as_str)?;
                    let highlights = entry
                        .get("highlights")
                        .and_then(Value::as_array)
                        .map(|spans| {
                            spans
                                .iter()
                                .filter_map(|span| {
                                    let start = span.get("start")?.as_u64()? as usize;
                                    let length = span.get("length")?.as_u64()? as usize;
                                    Some((start, length))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(SearchSuggestion {
                        query: text.to_string(),
                        highlights,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    SearchSuggestions {
        query: query.to_string(),
        suggestions,
        direct_hits: hits(value.get("directHits")),
    }
}

/// The query string for a `v2/search/` call, besides `query` and `types`.
pub(super) fn search_params() -> [(&'static str, String); 8] {
    [
        ("limit", "50".into()),
        ("offset", "0".into()),
        ("includeContributors", "true".into()),
        ("includeDidYouMean", "true".into()),
        ("includeUserPlaylists", "true".into()),
        ("supportsUserData", "true".into()),
        ("locale", LOCALE.into()),
        ("deviceType", "BROWSER".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_ranked_hits_genres_and_every_list() {
        let answer = json!({
            "artists": {"items": [{"id": 1, "name": "Eminem"}], "totalNumberOfItems": 1},
            "tracks": {"items": [{"id": 2, "title": "Without Me", "duration": 290}]},
            "videos": {"items": [{"id": 3, "title": "Stan", "duration": 400}]},
            "uploads": {"items": [{"id": 4, "title": "Emi", "duration": 100}]},
            "playlists": "not a page",
            "topHits": [
                {"type": "ARTISTS", "value": {"id": 1, "name": "Eminem"}},
                {"type": "UPLOADS", "value": {"id": 4, "title": "Emi"}},
                {"type": "SOMETHING_NEW", "value": {}},
                {"type": "TRACKS", "value": {"id": 2, "title": "Without Me"}, "tabPosition": 3},
                {"type": "VIDEOS", "value": {"id": 3, "title": "Stan"}}
            ],
            "genres": [{"title": "LoFi", "apiPath": "pages/genre_lofi"}, {"title": ""}]
        });
        let results = results(&answer);
        assert_eq!(results.top_hits.len(), 4);
        assert!(matches!(results.top_hits[0], TopHit::Artist(_)));
        assert!(matches!(results.top_hits[1], TopHit::Upload(_)));
        assert!(matches!(results.top_hits[3], TopHit::Video(_)));
        assert!(matches!(results.top_hit, Some(TopHit::Artist(_))));
        assert_eq!(results.genres.len(), 1);
        assert_eq!(results.genres[0].api_path, "pages/genre_lofi");
        assert!(results.playlists.is_none());
        assert_eq!(results.videos.map(|page| page.items.len()), Some(1));
        assert_eq!(results.uploads.map(|page| page.items.len()), Some(1));
    }

    #[test]
    fn reads_suggestions_with_highlights_and_direct_hits() {
        let answer = json!({
            "history": [],
            "suggestions": [
                {"query": "emily jane white", "highlights": [{"start": 0, "length": 3}]},
                {"query": "emilia"}
            ],
            "directHits": [{"type": "ALBUMS", "value": {"id": 5, "title": "The Eminem Show"}}],
            "suggestionUuid": "x"
        });
        let found = suggestions("emi", &answer);
        assert_eq!(found.query, "emi");
        assert_eq!(found.suggestions.len(), 2);
        assert_eq!(found.suggestions[0].highlights, vec![(0, 3)]);
        assert!(found.suggestions[1].highlights.is_empty());
        assert!(matches!(found.direct_hits[..], [TopHit::Album(_)]));
    }

    #[test]
    fn reads_a_realistic_suggestions_answer() {
        let answer: Value =
            serde_json::from_str(include_str!("fixtures/suggestions.json")).unwrap();
        let found = suggestions("eminem", &answer);
        let texts: Vec<_> = found.suggestions.iter().map(|s| s.query.as_str()).collect();
        assert_eq!(
            texts,
            [
                "eminem",
                "eminem lose yourself",
                "eminem without me",
                "eminem mockingbird",
                "eminem the real slim shady"
            ]
        );
        assert_eq!(found.suggestions[1].highlights, vec![(0, 6)]);
        assert!(matches!(
            found.direct_hits[..],
            [
                TopHit::Artist(_),
                TopHit::Track(_),
                TopHit::Album(_),
                TopHit::Playlist(_)
            ]
        ));
    }

    #[test]
    fn an_empty_or_strange_answer_is_empty_not_an_error() {
        let results = results(&json!({}));
        assert!(results.top_hits.is_empty() && results.tracks.is_none());
        let found = suggestions("x", &json!([1, 2]));
        assert!(found.suggestions.is_empty() && found.direct_hits.is_empty());
    }

    /// TIDAL's `didYouMean` is the corrected query as a plain string (seen
    /// live, 2026-10-17: "eminme" answers "eminem"); other shapes and empty
    /// text mean no suggestion.
    #[test]
    fn reads_the_did_you_mean_query() {
        let read = |value: Value| results(&value).did_you_mean;
        assert_eq!(
            read(json!({"didYouMean": "eminem"})).as_deref(),
            Some("eminem")
        );
        assert_eq!(
            read(json!({"didYouMean": " eminem "})).as_deref(),
            Some("eminem")
        );
        assert_eq!(
            read(json!({"didYouMean": {"query": "eminem"}})).as_deref(),
            Some("eminem")
        );
        for none in [
            json!({}),
            json!({"didYouMean": ""}),
            json!({"didYouMean": null}),
            json!({"didYouMean": 3}),
        ] {
            assert_eq!(read(none), None);
        }
    }
}
