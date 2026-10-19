//! Headphone corrections from the AutoEQ project
//! (github.com/jaakkopasanen/AutoEq).
//!
//! Nothing is fetched until the search is opened: then the project's
//! results index, a Markdown list of about nine thousand measurements, is
//! downloaded from GitHub's raw file host and kept for a week; picking a
//! headphone downloads its `ParametricEQ.txt`, kept as long as the cache.
//! No account, no other host, nothing about the listener is sent.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// Where AutoEQ's results live.
const RESULTS: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";
/// How long the downloaded index is used before it is fetched again.
const INDEX_FRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const INDEX_FILE: &str = "INDEX.md";
/// The most results a search shows.
pub const MAX_RESULTS: usize = 50;

/// One measured headphone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headphone {
    pub name: String,
    /// Who measured it, and on what rig when the index says.
    pub source: String,
    /// The results folder, URL-encoded as the index gives it.
    path: String,
}

impl Headphone {
    /// Where its parametric profile is.
    pub fn profile_url(&self) -> String {
        let folder = self.path.rsplit('/').next().unwrap_or_default();
        format!("{RESULTS}/{}/{folder}%20ParametricEQ.txt", self.path)
    }

    /// A file name for the cached profile, the same for the same folder.
    fn cache_name(&self) -> String {
        let safe: String = self
            .path
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("{safe}.txt")
    }
}

/// Reads the index's `- [Name](./source/kind/Name) by source on rig` lines.
pub fn parse_index(text: &str) -> Vec<Headphone> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix("- [")?;
            let (name, rest) = line.split_once("](")?;
            let (path, rest) = rest.split_once(')')?;
            // A name with brackets closes later: "(ANC Off))".
            let (path, rest) = if path.matches('(').count() > path.matches(')').count() {
                let more = rest.find(')')?;
                (format!("{path}){}", &rest[..more]), &rest[more + 1..])
            } else {
                (path.to_string(), rest)
            };
            let path = path.strip_prefix("./")?.to_string();
            if path.contains("..") || path.contains("://") {
                return None;
            }
            let source = rest.trim().strip_prefix("by ").unwrap_or(rest.trim());
            Some(Headphone {
                name: name.to_string(),
                source: source.to_string(),
                path,
            })
        })
        .collect()
}

/// Headphones whose name holds every word of `query`, best first: names
/// that start with the query, then the rest in index order.
pub fn search<'a>(index: &'a [Headphone], query: &str) -> Vec<&'a Headphone> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let query = query.trim().to_lowercase();
    let mut found: Vec<&Headphone> = index
        .iter()
        .filter(|headphone| {
            let name = headphone.name.to_lowercase();
            words.iter().all(|word| name.contains(word))
        })
        .collect();
    found.sort_by_key(|headphone| !headphone.name.to_lowercase().starts_with(&query));
    found.truncate(MAX_RESULTS);
    found
}

fn fresh(path: &Path, within: Option<Duration>) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if let Some(within) = within {
        let age = SystemTime::now()
            .duration_since(meta.modified().ok()?)
            .unwrap_or_default();
        if age > within {
            return None;
        }
    }
    std::fs::read_to_string(path).ok()
}

fn keep(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("part");
    if std::fs::write(&temporary, text).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

async fn download(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("AutoEQ: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("AutoEQ answered {}", response.status()));
    }
    response
        .text()
        .await
        .map_err(|error| format!("AutoEQ: {error}"))
}

/// The index, from the cache while it is fresh, else from GitHub (the
/// stale copy when GitHub cannot be reached).
pub async fn index(http: &reqwest::Client, cache_dir: &Path) -> Result<Vec<Headphone>, String> {
    let path = cache_dir.join(INDEX_FILE);
    if let Some(text) = fresh(&path, Some(INDEX_FRESH)) {
        return Ok(parse_index(&text));
    }
    match download(http, &format!("{RESULTS}/{INDEX_FILE}")).await {
        Ok(text) => {
            keep(&path, &text);
            Ok(parse_index(&text))
        }
        Err(error) => fresh(&path, None)
            .map(|text| parse_index(&text))
            .ok_or(error),
    }
}

/// A headphone's parametric profile, as text for [`super::apo::parse`].
pub async fn profile(
    http: &reqwest::Client,
    cache_dir: &Path,
    headphone: &Headphone,
) -> Result<String, String> {
    let path = cache_dir.join("profiles").join(headphone.cache_name());
    if let Some(text) = fresh(&path, None) {
        return Ok(text);
    }
    let text = download(http, &headphone.profile_url()).await?;
    super::apo::parse(&text).map_err(|error| format!("AutoEQ profile: {error}"))?;
    keep(&path, &text);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "# Index
This is a list of all equalization profiles.

- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045
- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990
- [Sennheiser HD 650](./oratory1990/over-ear/Sennheiser%20HD%20650) by oratory1990
- [Bad](../../etc/passwd) by nobody
- [Sennheiser HD 600](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600) by crinacle on GRAS 43AG-7
";

    #[test]
    fn the_index_lists_every_measurement_with_its_source() {
        let index = parse_index(INDEX);
        assert_eq!(index.len(), 4);
        assert_eq!(index[0].name, "1MORE Aero (ANC Off)");
        assert_eq!(index[0].source, "HypetheSonics on GRAS RA0045");
        assert_eq!(
            index[0].path,
            "HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)"
        );
        assert_eq!(
            index[1].profile_url(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/oratory1990/over-ear/Sennheiser%20HD%20600/Sennheiser%20HD%20600%20ParametricEQ.txt"
        );
        assert_ne!(index[1].cache_name(), index[3].cache_name());
    }

    #[test]
    fn a_search_needs_every_word_and_puts_prefixes_first() {
        let index = parse_index(INDEX);
        let names = |query| -> Vec<String> {
            search(&index, query)
                .iter()
                .map(|headphone| format!("{} / {}", headphone.name, headphone.source))
                .collect()
        };
        assert_eq!(
            names("hd 600"),
            [
                "Sennheiser HD 600 / oratory1990",
                "Sennheiser HD 600 / crinacle on GRAS 43AG-7"
            ]
        );
        assert_eq!(
            names("aero")[0],
            "1MORE Aero (ANC Off) / HypetheSonics on GRAS RA0045"
        );
        assert!(names("   ").is_empty());
        assert_eq!(names("sennheiser").len(), 3);
    }

    #[test]
    fn a_cached_index_is_used_while_fresh() {
        let dir = std::env::temp_dir().join(format!("tidalfast-autoeq-{}", std::process::id()));
        let path = dir.join(INDEX_FILE);
        keep(&path, INDEX);
        assert_eq!(fresh(&path, Some(INDEX_FRESH)).as_deref(), Some(INDEX));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
