//! TIDAL links as they arrive from outside: the desktop's URL handler, the
//! command line, and a second launch handing one to the running instance.
//!
//! Every shape TIDAL hands out comes back as the one canonical URI,
//! `tidal:<kind>:<id>` or `tidal:search:<encoded query>`, or nothing when it
//! is not something the app can open.

/// Resource pages. Search links carry text instead of a resource id.
const KINDS: [&str; 6] = ["track", "album", "artist", "playlist", "mix", "upload"];

/// The canonical form of a context URI reported as playing. TIDAL contexts
/// already have the one shape; this keeps call sites that compare contexts
/// strict.
pub fn canonical_context_uri(uri: &str) -> String {
    uri.to_owned()
}

/// The longest `tidaluna:` link handed to plugins. An OAuth redirect carries a
/// code and a state, a few hundred bytes; this leaves room and stops a
/// runaway link from filling the control channel or a plugin's memory.
pub const PLUGIN_LINK_MAX: usize = 2048;

const PLUGIN_SCHEME: &str = "tidaluna://";

/// Whether `text` is a `tidaluna://` link (the scheme in any case, as URL
/// schemes are case-insensitive), whatever follows it.
pub fn is_plugin_link(text: &str) -> bool {
    text.trim()
        .get(..PLUGIN_SCHEME.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(PLUGIN_SCHEME))
}

/// A `tidaluna://` link for plugins (TidaLuna's own scheme: a plugin
/// waits on `tidaLuna://login-callback?code=...` after an
/// OAuth sign-in), exactly as it arrived, or `None` when it is something
/// else, empty after the scheme, longer than [`PLUGIN_LINK_MAX`], or holds a
/// space or control character. It is never a TIDAL link, and its query
/// (which can carry a sign-in code) must stay out of every log.
pub fn plugin_link(text: &str) -> Option<String> {
    let text = text.trim();
    let valid = is_plugin_link(text)
        && text.len() > PLUGIN_SCHEME.len()
        && text.len() <= PLUGIN_LINK_MAX
        && !text.chars().any(|c| c.is_control() || c.is_whitespace());
    valid.then(|| text.to_owned())
}

/// The canonical `tidal:<kind>:<id>` behind `text`, or `None` when it is not
/// a link to a track, album, artist, playlist, mix, or search.
///
/// Accepted: `tidal:track:ID`, `tidal://track/ID`, `tidal://browse/track/ID`,
/// `https://tidal.com/browse/track/ID`, `https://tidal.com/track/ID`,
/// `https://tidal.com/upload/ID` (a release of the account's own Upload page),
/// `https://listen.tidal.com/track/ID`, each with or without a query string,
/// and `…/album/ID/track/ID`, which is the track.
pub fn parse(text: &str) -> Option<String> {
    if let Some(query) = search_query(text) {
        return Some(format!(
            "tidal:search:{}",
            percent_encoding::utf8_percent_encode(&query, percent_encoding::NON_ALPHANUMERIC)
        ));
    }
    let text = text.trim();
    let mut segments: Vec<&str> = if let Some(rest) = text.strip_prefix("tidal://") {
        let mut segments = path_segments(rest);
        if segments.first().is_some_and(|host| is_web_host(host)) {
            segments.remove(0);
        }
        segments
    } else if let Some(rest) = text.strip_prefix("tidal:") {
        rest.split(':').filter(|part| !part.is_empty()).collect()
    } else {
        let rest = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"))?;
        let mut segments = path_segments(rest);
        if !segments.first().is_some_and(|host| is_web_host(host)) {
            return None;
        }
        segments.remove(0);
        segments
    };
    // tidal.com/browse/track/ID
    if segments
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case("browse"))
    {
        segments.remove(0);
    }
    // listen.tidal.com/album/ID/track/ID names the track.
    if segments.len() >= 4 && segments[2].eq_ignore_ascii_case("track") {
        segments.drain(..2);
    }
    let [kind, id, ..] = segments.as_slice() else {
        return None;
    };
    let kind = kind.to_ascii_lowercase();
    if !KINDS.contains(&kind.as_str()) || !is_id(id) {
        return None;
    }
    Some(format!("tidal:{kind}:{id}"))
}

/// Decodes a search link once: `tidal:search:<encoded>`, or a web search
/// address with its words in `q=` (`listen.tidal.com/search?q=…`) or in the
/// path (`tidal.com/search/…`). A path's `+` is literal, not a form-encoded
/// space. Canonical links encode the whole query so delimiters and Unicode
/// survive command-line, D-Bus, Apple Event and line-based socket delivery.
pub fn search_query(text: &str) -> Option<String> {
    let text = text.trim();
    let encoded = if text.starts_with("tidal:") && !text.starts_with("tidal://") {
        let (kind, query) = text.strip_prefix("tidal:")?.split_once(':')?;
        if !kind.eq_ignore_ascii_case("search") {
            return None;
        }
        query.to_owned()
    } else {
        let address = if let Some(rest) = text.strip_prefix("tidal://") {
            match rest.split_once('/') {
                Some((host, path)) if is_web_host(host) => path,
                _ => rest,
            }
        } else {
            let rest = text
                .strip_prefix("https://")
                .or_else(|| text.strip_prefix("http://"))?;
            let (host, path) = rest.split_once('/')?;
            if !is_web_host(host) {
                return None;
            }
            path
        };
        let path_end = address.find(['?', '#']).unwrap_or(address.len());
        let path = &address[..path_end];
        let path = path
            .strip_prefix("browse/")
            .unwrap_or(path)
            .trim_end_matches('/');
        let (kind, rest) = path.split_once('/').unwrap_or((path, ""));
        if !kind.eq_ignore_ascii_case("search") {
            return None;
        }
        let from_query = address[path_end..]
            .trim_start_matches('?')
            .split(['&', '#'])
            .find_map(|pair| pair.strip_prefix("q="))
            .map(|query| query.replace('+', " "));
        // `search/tracks?q=...` names a filter, not the words.
        from_query.unwrap_or_else(|| rest.to_owned())
    };
    let query = percent_encoding::percent_decode_str(&encoded)
        .decode_utf8()
        .ok()?;
    (!query.chars().any(char::is_control)).then(|| query.into_owned())
}

/// The path of a web address split at slashes, its query and fragment
/// dropped, empty segments (a trailing slash) with them.
fn path_segments(rest: &str) -> Vec<&str> {
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    rest[..end]
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn is_web_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "tidal.com" | "www.tidal.com" | "listen.tidal.com" | "desktop.tidal.com"
    )
}

/// TIDAL ids are numbers (tracks, albums, artists), UUIDs (playlists) or
/// alphanumeric strings (mixes); anything else on a link is not one.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tidaluna_link_is_for_plugins_and_never_a_tidal_link() {
        let callback = "tidaLuna://login-callback?code=abc&state=xyz";
        assert_eq!(plugin_link(callback).as_deref(), Some(callback));
        assert_eq!(
            plugin_link("  TIDALUNA://login-callback?code=1\n").as_deref(),
            Some("TIDALUNA://login-callback?code=1")
        );
        assert!(is_plugin_link(callback));
        assert!(parse(callback).is_none());
        for bad in [
            "tidaluna://",
            "tidaluna:login-callback",
            "tidal://login-callback",
            "https://tidaluna.example/login-callback",
            "tidaluna://a b",
            "tidaluna://a\nb",
            "",
        ] {
            assert!(plugin_link(bad).is_none(), "{bad:?}");
        }
        let long = format!("tidaluna://x?code={}", "a".repeat(PLUGIN_LINK_MAX));
        assert!(plugin_link(&long).is_none());
        // A multi-byte character across the scheme's length does not panic.
        assert!(!is_plugin_link("tidalun\u{e9}\u{e9}\u{e9}"));
    }

    /// Every shape TIDAL's apps and site hand out lands on the one URI the
    /// app navigates by.
    #[test]
    fn every_link_shape_becomes_the_one_uri() {
        for (link, uri) in [
            ("tidal:track:77646170", "tidal:track:77646170"),
            ("tidal://track/77646170", "tidal:track:77646170"),
            ("tidal://browse/album/77646168", "tidal:album:77646168"),
            (
                "https://tidal.com/browse/track/77646170?u",
                "tidal:track:77646170",
            ),
            ("https://tidal.com/track/77646170", "tidal:track:77646170"),
            (
                "https://listen.tidal.com/album/77646168/track/77646170",
                "tidal:track:77646170",
            ),
            ("https://listen.tidal.com/artist/3346", "tidal:artist:3346"),
            (
                "https://tidal.com/browse/playlist/0e5f6c2a-1b2c-4d3e-9f40-123456789abc",
                "tidal:playlist:0e5f6c2a-1b2c-4d3e-9f40-123456789abc",
            ),
            (
                "https://listen.tidal.com/mix/0123456789abcdef0123456789abcd",
                "tidal:mix:0123456789abcdef0123456789abcd",
            ),
            ("  tidal:album:77646168\n", "tidal:album:77646168"),
            (
                "https://tidal.com/upload/12345678-90ab-4cde-8f01-234567890abc",
                "tidal:upload:12345678-90ab-4cde-8f01-234567890abc",
            ),
        ] {
            assert_eq!(parse(link).as_deref(), Some(uri), "{link}");
        }
    }

    #[test]
    fn other_links_are_refused() {
        for link in [
            "",
            "tidal:video:123",
            "tidal:track:",
            "tidal:track:12 34",
            "https://example.com/browse/track/1",
            "https://example.com/track/4uLU6hMCjMI75M1A2tKUQC",
            "other:track:4uLU6hMCjMI75M1A2tKUQC",
            "https://tidal.com/",
            "https://tidal.com/browse/track/1;rm",
        ] {
            assert!(parse(link).is_none(), "{link}");
        }
    }

    #[test]
    fn search_links_keep_their_words() {
        assert_eq!(
            search_query("https://listen.tidal.com/search?q=daft+punk").as_deref(),
            Some("daft punk")
        );
        assert_eq!(
            search_query("https://tidal.com/search/%E6%9D%B1%E4%BA%AC").as_deref(),
            Some("東京")
        );
        assert_eq!(
            parse("https://listen.tidal.com/search/tracks?q=a%26b").as_deref(),
            Some("tidal:search:a%26b")
        );
        let canonical = parse("tidal:search:what%20a%20day").unwrap();
        assert_eq!(search_query(&canonical).as_deref(), Some("what a day"));
        // A bare search page clears the search.
        assert_eq!(
            search_query("https://listen.tidal.com/search").as_deref(),
            Some("")
        );
        assert!(search_query("tidal:search:a%0Ab").is_none());
    }

    #[test]
    fn contexts_compare_as_they_are() {
        assert_eq!(
            canonical_context_uri("tidal:playlist:abc"),
            "tidal:playlist:abc"
        );
    }
}
