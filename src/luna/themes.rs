//! Installed themes as one set: each theme once whatever URL it came by,
//! one theme applied at a time, and a readable name for themes whose CSS
//! does not name itself.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

/// Branches a GitHub repository's default branch goes by in raw links.
const DEFAULT_REFS: &[&str] = &["HEAD", "main", "master"];

/// A theme URL in the form it is fetched and stored by: GitHub file pages
/// (`github.com/o/r/blob/ref/path`), `refs/heads/` raw links and jsDelivr's
/// `cdn.jsdelivr.net/gh/o/r@ref/path` become
/// `raw.githubusercontent.com/o/r/ref/path`; a `#fragment` is dropped.
/// Anything else is kept as given.
pub fn canonical_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or(url);
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let host_l = host.to_ascii_lowercase();
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let raw = |owner: &str, repo: &str, reference: &str, file: &[&str]| {
        format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{reference}/{}",
            file.join("/")
        )
    };
    match host_l.as_str() {
        "github.com" | "www.github.com"
            if parts.len() >= 5 && matches!(parts[2], "blob" | "raw") =>
        {
            raw(parts[0], parts[1], parts[3], &parts[4..])
        }
        "raw.githubusercontent.com"
            if parts.len() >= 6 && parts[2] == "refs" && matches!(parts[3], "heads" | "tags") =>
        {
            raw(parts[0], parts[1], parts[4], &parts[5..])
        }
        "cdn.jsdelivr.net" if parts.len() >= 4 && parts[0] == "gh" => {
            let (repo, reference) = parts[2].split_once('@').unwrap_or((parts[2], "HEAD"));
            raw(parts[1], repo, reference, &parts[3..])
        }
        _ => {
            let mut out = format!("{scheme}://{host_l}/{path}");
            if let Some(q) = query {
                out.push('?');
                out.push_str(q);
            }
            out
        }
    }
}

/// What makes two theme URLs the same theme: the canonical URL without
/// its query, the host and a GitHub owner and repository in lower case
/// (GitHub ignores their case), and the default branch however it is
/// named (`HEAD`, `main`, `master`).
pub fn identity(url: &str) -> String {
    let c = canonical_url(url);
    let c = c.split('?').next().unwrap_or(&c);
    let Some(rest) = c
        .strip_prefix("https://")
        .or_else(|| c.strip_prefix("http://"))
    else {
        return c.to_string();
    };
    let mut parts: Vec<String> = rest.split('/').map(str::to_string).collect();
    if parts.first().map(String::as_str) == Some("raw.githubusercontent.com") && parts.len() >= 5 {
        parts[1] = parts[1].to_ascii_lowercase();
        parts[2] = parts[2].to_ascii_lowercase();
        if DEFAULT_REFS.contains(&parts[3].as_str()) {
            parts[3] = "HEAD".into();
        }
    }
    parts.join("/")
}

/// Where a theme is fetched from. Demo builds with
/// `TIDALFAST_DEMO_THEME_MIRROR=<base>/` read `raw.githubusercontent.com`
/// files from that base instead, so off-screen shots of the Themes tab need
/// no network.
pub fn fetch_url(url: &str) -> String {
    #[cfg(feature = "demo")]
    if let Ok(base) = std::env::var("TIDALFAST_DEMO_THEME_MIRROR")
        && let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/")
    {
        return format!("{base}{rest}");
    }
    url.to_string()
}

/// The installed entry that is the same theme as `url`, if any.
pub fn find_installed<'a>(themes: &'a Map<String, Value>, url: &str) -> Option<&'a String> {
    let id = identity(url);
    themes.keys().find(|k| identity(k) == id)
}

fn on(t: &Value) -> bool {
    t["enabled"].as_bool() == Some(true)
}

/// Each theme once: entries that are the same theme by URL ([`identity`])
/// or by identical CSS collapse into one, the enabled one kept (else the
/// first in the list). Whether anything changed.
pub fn dedupe(themes: &mut Map<String, Value>) -> bool {
    let urls: Vec<String> = themes.keys().cloned().collect();
    let mut drop = Vec::new();
    for (n, a) in urls.iter().enumerate() {
        if drop.contains(a) {
            continue;
        }
        for b in &urls[n + 1..] {
            if drop.contains(b) {
                continue;
            }
            // A colourway is its CSS over its base's.
            let same_css = matches!(
                (applied_css(&themes[a]), applied_css(&themes[b])),
                (Some(x), Some(y)) if x == y
            );
            if identity(a) != identity(b) && !same_css {
                continue;
            }
            // Keep the enabled one; the other goes.
            if on(&themes[b]) && !on(&themes[a]) {
                drop.push(a.clone());
                break;
            }
            drop.push(b.clone());
        }
    }
    for u in &drop {
        themes.remove(u);
    }
    !drop.is_empty()
}

/// One theme applies at a time (themes restyle the same elements and
/// overlap). Settings saved before, with several on, keep the one switched
/// on last (`enabledAt`); themes from before that was recorded carry no
/// time, and then the last one in the list (Luna's map, ordered by URL)
/// stays on. Whether anything changed.
pub fn one_on(themes: &mut Map<String, Value>) -> bool {
    let on: Vec<(String, u64)> = themes
        .iter()
        .filter(|(_, t)| on(t))
        .map(|(u, t)| (u.clone(), t["enabledAt"].as_u64().unwrap_or(0)))
        .collect();
    if on.len() < 2 {
        return false;
    }
    // `max_by_key` keeps the last of equal keys: the last in the list.
    let keep = on
        .iter()
        .max_by_key(|(_, at)| *at)
        .map(|(u, _)| u.clone())
        .unwrap_or_default();
    for (url, _) in on.into_iter().filter(|(u, _)| *u != keep) {
        if let Some(t) = themes.get_mut(&url) {
            t["enabled"] = json!(false);
        }
    }
    true
}

/// Saved themes made whole: each once, one on. Whether anything changed.
pub fn migrate(themes: &mut Map<String, Value>) -> bool {
    let a = dedupe(themes);
    let b = one_on(themes);
    a || b
}

// ------------------------------------------------------------- colourways

/// `css` without its comments.
fn without_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// The custom properties a stylesheet declares (`--name: value`).
pub fn declared_vars(css: &str) -> HashSet<String> {
    let css = without_comments(css);
    let mut out = HashSet::new();
    let mut from = 0;
    while let Some(at) = css[from..].find("--") {
        let start = from + at;
        from = start + 2;
        // `a--b` is part of another word, not a property.
        if css[..start].chars().next_back().is_some_and(is_name_char) {
            continue;
        }
        let name: String = css[start..]
            .chars()
            .take_while(|c| is_name_char(*c))
            .collect();
        let after = css[start + name.len()..].trim_start();
        if name.len() > 2 && after.starts_with(':') {
            out.insert(name);
        }
    }
    out
}

/// The custom properties a stylesheet reads through `var(--name)`.
pub fn used_vars(css: &str) -> HashSet<String> {
    let css = without_comments(css);
    let mut out = HashSet::new();
    let mut from = 0;
    while let Some(at) = css[from..].find("var(") {
        let start = from + at + 4;
        from = start;
        let name: String = css[start..]
            .trim_start()
            .chars()
            .take_while(|c| is_name_char(*c))
            .collect();
        if name.starts_with("--") && name.len() > 2 {
            out.insert(name);
        }
    }
    out
}

const COLOURWAY_MAX: usize = 64 * 1024;
const COLOURWAY_MIN_VARS: usize = 8;

/// A stylesheet that could be a colourway: small, and setting a good many
/// custom properties (colours for a base theme to read).
pub fn looks_like_colourway(css: &str) -> bool {
    css.len() <= COLOURWAY_MAX && declared_vars(css).len() >= COLOURWAY_MIN_VARS
}

/// Whether `colourway` is a colourway of `base`: it is small next to the
/// base, most of the custom properties it sets are read by the base, and the
/// base does not set them itself (it takes them from outside). Cotton
/// Candy's base theme reads `--cover-Vib1` and the like without declaring
/// them, and each colourway file declares them; no other theme in the
/// catalogue meets this, and neither does a theme that sets its own colours.
pub fn is_colourway_of(colourway: &str, base: &str) -> bool {
    if !looks_like_colourway(colourway) || colourway.len().saturating_mul(3) > base.len() {
        return false;
    }
    let set = declared_vars(colourway);
    let read = used_vars(base);
    let own = declared_vars(base);
    let used = set.iter().filter(|v| read.contains(*v)).count();
    let declared = set.iter().filter(|v| own.contains(*v)).count();
    used * 10 >= set.len() * 6 && declared * 4 <= set.len()
}

/// A colourway applied over its base: one stylesheet, the base first.
pub fn with_base(base: &str, colourway: &str) -> String {
    format!("{base}\n{colourway}")
}

/// What a saved theme applies: its CSS, under its base's when it is a
/// colourway (`baseCss`).
pub fn applied_css(theme: &Value) -> Option<String> {
    let css = theme["css"].as_str()?;
    Some(match theme["baseCss"].as_str() {
        Some(base) => with_base(base, css),
        None => css.to_string(),
    })
}

/// The installed theme (other than `url`) that `css` is a colourway of:
/// its address and CSS.
pub fn installed_base(
    themes: &Map<String, Value>,
    url: &str,
    css: &str,
) -> Option<(String, String)> {
    if !looks_like_colourway(css) {
        return None;
    }
    themes
        .iter()
        .filter(|(u, _)| identity(u) != identity(url))
        // A colourway carrying a base is not a base itself.
        .filter(|(_, t)| t["baseCss"].is_null())
        .find_map(|(u, t)| {
            let base = t["css"].as_str()?;
            is_colourway_of(css, base).then(|| (u.clone(), base.to_string()))
        })
}

/// The names a file uses for this system (`base-windows.css`, `-win`).
pub fn this_os() -> &'static [&'static str] {
    if cfg!(windows) {
        &["windows", "win"]
    } else if cfg!(target_os = "macos") {
        &["macos", "mac", "darwin"]
    } else {
        &["linux"]
    }
}

const SYSTEMS: &[&str] = &["windows", "win", "linux", "macos", "mac", "darwin"];

/// The words of a file's name (`adaptive/base/windows-adaptive.css` is
/// `windows`, `adaptive`).
fn file_words(p: &str) -> Vec<String> {
    let file = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
    let stem = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s);
    stem.split(['-', '_', '.', ' '])
        .map(str::to_string)
        .collect()
}

/// Of a base theme's builds, the one for this system (`os`, as
/// [`this_os`]): the first whose name names it, else the first naming no
/// system, else the first (Cotton Candy Nights has no macOS build, and its
/// Windows one is used there).
pub fn pick_build<'a>(paths: &[&'a str], os: &[&str]) -> Option<&'a str> {
    let names = |p: &str, list: &[&str]| file_words(p).iter().any(|w| list.contains(&w.as_str()));
    paths
        .iter()
        .find(|p| names(p, os))
        .or_else(|| paths.iter().find(|p| !names(p, SYSTEMS)))
        .or_else(|| paths.first())
        .copied()
}

/// Paths in a repository that are base files for this machine, nearest the
/// colourway's folder first: CSS files with "base" in the name, those naming
/// another system left out (`base-windows.css` is for Windows only). `os`
/// is this system's names (`["windows", "win"]`).
pub fn base_candidates(paths: &[String], colourway_path: &str, os: &[&str]) -> Vec<String> {
    let words = file_words;
    let mut found: Vec<(usize, &String)> = paths
        .iter()
        .filter(|p| p.to_ascii_lowercase().ends_with(".css") && p.as_str() != colourway_path)
        .filter(|p| words(p).iter().any(|w| w == "base"))
        .filter(|p| {
            let w = words(p);
            let mut named = SYSTEMS
                .iter()
                .filter(|s| w.iter().any(|x| x == *s))
                .peekable();
            named.peek().is_none() || named.any(|s| os.contains(s))
        })
        .map(|p| {
            // Folders in common with the colourway.
            let shared = p
                .split('/')
                .zip(colourway_path.split('/'))
                .take_while(|(a, b)| a == b)
                .count();
            (shared, p)
        })
        .collect();
    found.sort_by_key(|(shared, p)| {
        (
            std::cmp::Reverse(*shared),
            p.matches('/').count(),
            (*p).clone(),
        )
    });
    found.into_iter().map(|(_, p)| p.clone()).collect()
}

/// The base of a colourway at `url`, found in its repository: the repository's
/// file list (`list`) gives the candidates, each fetched (`get`) and kept only
/// if the colourway really is one of it. Its address and CSS. GitHub raw
/// links only.
pub fn discover_base(
    url: &str,
    css: &str,
    os: &[&str],
    list: &mut dyn FnMut(&str) -> Option<Vec<String>>,
    get: &mut dyn FnMut(&str) -> Option<String>,
) -> Option<(String, String)> {
    if !looks_like_colourway(css) {
        return None;
    }
    let canon = canonical_url(url);
    let rest = canon.strip_prefix("https://raw.githubusercontent.com/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 4 {
        return None;
    }
    let (owner, repo, reference) = (parts[0], parts[1], parts[2]);
    let path = parts[3..].join("/").replace("%20", " ");
    let paths = list(&format!("{owner}/{repo}@{reference}"))?;
    for candidate in base_candidates(&paths, &path, os).into_iter().take(4) {
        let candidate_url = format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{reference}/{}",
            candidate.replace(' ', "%20")
        );
        if let Some(base) = get(&candidate_url)
            && is_colourway_of(css, &base)
        {
            return Some((candidate_url, base));
        }
    }
    None
}

/// A name and author for a theme from its URL, when its CSS names neither:
/// the file's name (or the repository's, for `theme.css`, `style.css` and
/// the like) in words, and the repository's owner.
/// `.../Acercandr0/Lunallena/HEAD/Lunallena.css` is "Lunallena" by
/// Acercandr0; `.../pernydev/tidal-catppuccin/HEAD/frappe.css` is
/// "Catppuccin Frappe" by pernydev.
pub fn name_from_url(url: &str) -> (String, Option<String>) {
    let c = canonical_url(url);
    let c = c.split('?').next().unwrap_or(&c).to_string();
    let parts: Vec<&str> = c.split('/').filter(|p| !p.is_empty()).collect();
    let file = parts.last().copied().unwrap_or("");
    let stem = file
        .rsplit_once('.')
        .map_or(file, |(s, _)| s)
        .to_ascii_lowercase();
    let github = parts.get(1) == Some(&"raw.githubusercontent.com") && parts.len() >= 6;
    let (owner, repo) = if github {
        (
            Some(parts[2].to_string()),
            Some(parts[3].to_ascii_lowercase()),
        )
    } else {
        (None, None)
    };
    let noise = [
        "tidal", "luna", "tidaluna", "theme", "themes", "win", "windows", "css", "dist", "hifi",
    ];
    let words = |s: &str| -> Vec<String> {
        s.split(['-', '_', '.', ' '])
            .filter(|w| !w.is_empty() && !noise.contains(w))
            .map(str::to_string)
            .collect()
    };
    let generic = [
        "theme",
        "style",
        "styles",
        "index",
        "main",
        "userstyle",
        "custom",
    ];
    let mut name = if generic.contains(&stem.as_str()) || words(&stem).is_empty() {
        match repo.as_deref().map(words) {
            Some(w) if !w.is_empty() => w,
            _ => owner.iter().cloned().collect(),
        }
    } else {
        let mut w = words(&stem);
        // A flavour named alone (`frappe.css`) takes its repository's name.
        if let Some(repo) = &repo {
            let r = words(repo);
            if w.len() == 1 && !r.is_empty() && !r.iter().any(|x| w[0].contains(x.as_str())) {
                let mut both = r;
                both.extend(w);
                w = both;
            }
        }
        w
    };
    if name.is_empty() {
        name = vec![file.to_string()];
    }
    let title = name
        .iter()
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    (title, owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_theme_by_any_link_is_one_theme() {
        let raw = "https://raw.githubusercontent.com/pernydev/tidal-catppuccin/HEAD/frappe.css";
        for other in [
            "https://raw.githubusercontent.com/pernydev/tidal-catppuccin/main/frappe.css",
            "https://raw.githubusercontent.com/PernyDev/Tidal-Catppuccin/refs/heads/master/frappe.css",
            "https://github.com/pernydev/tidal-catppuccin/blob/main/frappe.css",
            "https://cdn.jsdelivr.net/gh/pernydev/tidal-catppuccin@main/frappe.css",
            "https://cdn.jsdelivr.net/gh/pernydev/tidal-catppuccin/frappe.css",
            " https://raw.githubusercontent.com/pernydev/tidal-catppuccin/HEAD/frappe.css?v=2#top",
        ] {
            assert_eq!(identity(other), identity(raw), "{other}");
        }
        // A different file, branch or repository is another theme.
        for other in [
            "https://raw.githubusercontent.com/pernydev/tidal-catppuccin/HEAD/mocha.css",
            "https://raw.githubusercontent.com/pernydev/tidal-catppuccin/dev/frappe.css",
            "https://raw.githubusercontent.com/DavidHiFi/MochaTidal/HEAD/MochaTidal.css",
        ] {
            assert_ne!(identity(other), identity(raw), "{other}");
        }
        assert_eq!(
            canonical_url("https://github.com/Acercandr0/Lunallena/blob/main/Lunallena.css"),
            "https://raw.githubusercontent.com/Acercandr0/Lunallena/main/Lunallena.css"
        );
        assert_eq!(
            canonical_url("https://Example.com/a.css?x=1"),
            "https://example.com/a.css?x=1"
        );
    }

    #[test]
    fn duplicates_collapse_keeping_the_enabled_one_and_one_theme_stays_on() {
        let mut m = Map::new();
        let a = "https://raw.githubusercontent.com/o/r/HEAD/a.css";
        let a2 = "https://raw.githubusercontent.com/o/r/main/a.css";
        let b = "https://example.com/b.css";
        let b_copy = "https://mirror.example/b.css";
        let c = "https://example.com/c.css";
        m.insert(a.into(), json!({ "enabled": false, "css": "x" }));
        m.insert(a2.into(), json!({ "enabled": true, "css": "x2" }));
        m.insert(
            b.into(),
            json!({ "enabled": true, "css": "same", "enabledAt": 5 }),
        );
        m.insert(b_copy.into(), json!({ "enabled": false, "css": "same" }));
        m.insert(c.into(), json!({ "enabled": true, "enabledAt": 9 }));
        assert!(migrate(&mut m));
        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        assert_eq!(keys, vec![b, c, a2]);
        // c was switched on last.
        let on: Vec<&str> = m
            .iter()
            .filter(|(_, t)| t["enabled"] == json!(true))
            .map(|(u, _)| u.as_str())
            .collect();
        assert_eq!(on, vec![c]);
        assert!(!migrate(&mut m), "settled");
    }

    #[test]
    fn several_on_without_times_keep_the_last_in_the_list() {
        let mut m = Map::new();
        for u in ["https://e/a.css", "https://e/b.css", "https://e/c.css"] {
            m.insert(u.into(), json!({ "enabled": true }));
        }
        assert!(one_on(&mut m));
        assert_eq!(m["https://e/c.css"]["enabled"], json!(true));
        assert_eq!(m["https://e/a.css"]["enabled"], json!(false));
        assert_eq!(m["https://e/b.css"]["enabled"], json!(false));
    }

    #[test]
    fn unnamed_themes_get_a_tidy_name_and_their_owner() {
        let n = |u: &str| name_from_url(u);
        assert_eq!(
            n("https://raw.githubusercontent.com/Acercandr0/Lunallena/HEAD/Lunallena.css"),
            ("Lunallena".into(), Some("Acercandr0".into()))
        );
        assert_eq!(
            n("https://raw.githubusercontent.com/pernydev/tidal-catppuccin/HEAD/frappe.css").0,
            "Catppuccin Frappe"
        );
        // `theme.css` in a repository named only "Tidaluna-Theme": the owner.
        assert_eq!(
            n("https://raw.githubusercontent.com/Snusehunden/Tidaluna-Theme/HEAD/dist/theme.css").0,
            "Snusehunden"
        );
        assert_eq!(
            n("https://raw.githubusercontent.com/aya-zzz-zzz/cottoncandy/HEAD/cottoncandynights_win.css").0,
            "Cottoncandynights"
        );
        assert_eq!(
            n("https://example.com/themes/rose-pine.css"),
            ("Rose Pine".into(), None)
        );
    }

    /// A base reading ten colours it does not set, padded with rules (or
    /// setting them itself); a colourway setting them.
    fn base_css(declares: bool) -> String {
        let mut css = String::new();
        if declares {
            css.push_str(":root {");
            for n in 0..10 {
                css.push_str(&format!(" --c{n}: #{n}{n}{n};"));
            }
            css.push_str(" }\n");
        }
        for n in 0..400 {
            css.push_str(&format!(
                ".row{n} {{ color: var(--c{}); background: var(--c{}, red); margin: {n}px }}\n",
                n % 10,
                (n + 3) % 10
            ));
        }
        css
    }

    fn colourway_css() -> String {
        let mut css = String::from("/* a colourway --x: y */\n:root {");
        for n in 0..10 {
            css.push_str(&format!(" --c{n}: rgb({n}, 10, 20);"));
        }
        css.push_str(" }\n.extra { color: var(--c1) }");
        css
    }

    #[test]
    fn a_colourway_is_a_small_stylesheet_setting_what_a_larger_base_reads() {
        let colourway = colourway_css();
        let base = base_css(false);
        assert!(is_colourway_of(&colourway, &base));
        // A base that sets the colours itself takes nothing from outside.
        assert!(!is_colourway_of(&colourway, &base_css(true)));
        // Two stylesheets of one size: neither is the other's colourway.
        assert!(!is_colourway_of(&base, &colourway));
        assert!(!is_colourway_of(&base, &base));
        // Few variables, or ones the base never reads.
        assert!(!is_colourway_of(":root { --c1: red; --c2: blue }", &base));
        let other = colourway.replace("--c", "--d");
        assert!(!is_colourway_of(&other, &base));
        // The base alone is not a colourway of anything.
        assert!(!looks_like_colourway(&base));
        assert_eq!(declared_vars(&colourway).len(), 10, "comments are skipped");
        assert_eq!(used_vars(&base).len(), 10);
        assert_eq!(
            with_base("a{}", "b{}"),
            "a{}\nb{}",
            "the base comes first so the colourway wins"
        );
    }

    #[test]
    fn an_installed_base_is_found_for_a_colourway_and_not_for_other_themes() {
        let mut m = Map::new();
        let base_url = "https://raw.githubusercontent.com/o/r/HEAD/base.css";
        m.insert(
            base_url.into(),
            json!({ "enabled": false, "css": base_css(false) }),
        );
        m.insert(
            "https://e/self-contained.css".into(),
            json!({ "enabled": false, "css": base_css(true) }),
        );
        let cw = colourway_css();
        let url = "https://raw.githubusercontent.com/o/r/HEAD/colourways/pink.css";
        let (found, css) = installed_base(&m, url, &cw).unwrap();
        assert_eq!(found, base_url);
        assert_eq!(css, base_css(false));
        // A theme with its own colours is no colourway.
        assert!(installed_base(&m, url, &base_css(true)).is_none());
        // A colourway carrying a base is not a base for the next one.
        m.insert(
            "https://e/other-cw.css".into(),
            json!({ "enabled": false, "css": cw, "baseCss": "x" }),
        );
        let (found, _) = installed_base(&m, "https://e/new.css", &cw).unwrap();
        assert_eq!(found, base_url);
        let saved = json!({ "css": "b{}", "baseCss": "a{}" });
        assert_eq!(applied_css(&saved).as_deref(), Some("a{}\nb{}"));
        assert_eq!(
            applied_css(&json!({ "css": "b{}" })).as_deref(),
            Some("b{}")
        );
        assert_eq!(applied_css(&json!({})), None);
    }

    #[test]
    fn a_repositorys_base_files_for_this_system_come_nearest_first() {
        let paths: Vec<String> = [
            "README.md",
            "base-linux.css",
            "base-macos.css",
            "base-windows.css",
            "adaptive/base/linux-adaptive.css",
            "adaptive/base/base-windows.css",
            "adaptive/colorways/pink-adaptive.css",
            "cottoncandy.css",
            "pride collection/snowcone.css",
        ]
        .map(String::from)
        .to_vec();
        let win = ["windows", "win"];
        assert_eq!(
            base_candidates(&paths, "cottoncandy.css", &win),
            vec!["base-windows.css", "adaptive/base/base-windows.css"],
            "the root's own base first, then the others"
        );
        assert_eq!(
            base_candidates(&paths, "pride collection/snowcone.css", &["linux"]),
            vec!["base-linux.css"]
        );
        // A colourway in a folder prefers the base files beside it.
        assert_eq!(
            base_candidates(&paths, "adaptive/colorways/pink.css", &win)[0],
            "adaptive/base/base-windows.css"
        );
    }

    #[test]
    fn a_base_theme_build_is_picked_for_this_system() {
        let cc = ["base-windows.css", "base-linux.css", "base-macos.css"];
        let win: &[&str] = &["windows", "win"];
        let mac: &[&str] = &["macos", "mac", "darwin"];
        assert_eq!(pick_build(&cc, win), Some("base-windows.css"));
        assert_eq!(pick_build(&cc, &["linux"]), Some("base-linux.css"));
        assert_eq!(pick_build(&cc, mac), Some("base-macos.css"));
        // No macOS build: the first.
        let nights = ["base-windows.css", "base-linux.css"];
        assert_eq!(pick_build(&nights, mac), Some("base-windows.css"));
        // The system's word anywhere in the file's name, not its folders.
        let adaptive = [
            "adaptive/base/windows-adaptive.css",
            "adaptive/base/linux-adaptive.css",
        ];
        assert_eq!(
            pick_build(&adaptive, &["linux"]),
            Some("adaptive/base/linux-adaptive.css")
        );
        // A build for every system is taken when this one has none.
        let mixed = ["base-linux.css", "base.css"];
        assert_eq!(pick_build(&mixed, win), Some("base.css"));
        assert_eq!(pick_build(&[], win), None);
        assert!(!this_os().is_empty());
    }

    #[test]
    fn a_colourway_over_another_base_is_another_theme() {
        let mut m = Map::new();
        m.insert(
            "https://e/light/calavera.css".into(),
            json!({ "enabled": false, "css": "c", "baseCss": "light" }),
        );
        m.insert(
            "https://e/nights/calavera.css".into(),
            json!({ "enabled": true, "css": "c", "baseCss": "dark" }),
        );
        assert!(!dedupe(&mut m), "same colours, other bases");
        m.insert(
            "https://mirror/nights/calavera.css".into(),
            json!({ "enabled": false, "css": "c", "baseCss": "dark" }),
        );
        assert!(dedupe(&mut m));
        assert_eq!(m.len(), 2);
        assert!(
            m.contains_key("https://e/nights/calavera.css"),
            "the one on stays"
        );
    }

    #[test]
    fn a_colourways_base_is_found_in_its_repository_and_checked() {
        let colourway = colourway_css();
        let paths = vec![
            "base-linux.css".to_string(),
            "base-windows.css".to_string(),
            "colourways/pink.css".to_string(),
        ];
        let mut asked = Vec::new();
        let url = "https://raw.githubusercontent.com/o/r/main/colourways/pink.css";
        let found = discover_base(
            url,
            &colourway,
            &["windows", "win"],
            &mut |repo| {
                asked.push(repo.to_string());
                Some(paths.clone())
            },
            &mut |u| {
                assert_eq!(
                    u,
                    "https://raw.githubusercontent.com/o/r/main/base-windows.css"
                );
                Some(base_css(false))
            },
        );
        assert_eq!(
            found,
            Some((
                "https://raw.githubusercontent.com/o/r/main/base-windows.css".into(),
                base_css(false)
            ))
        );
        assert_eq!(asked, vec!["o/r@main"]);
        // The candidate must really be this colourway's base.
        let wrong = discover_base(
            url,
            &colourway,
            &["windows"],
            &mut |_| Some(paths.clone()),
            &mut |_| Some(base_css(true)),
        );
        assert_eq!(wrong, None);
        // Not a colourway, or not on GitHub: nothing is asked.
        let none = |u: &str, css: &str| {
            discover_base(
                u,
                css,
                &["windows"],
                &mut |_| panic!("no listing"),
                &mut |_| panic!("no fetch"),
            )
        };
        assert_eq!(none(url, &base_css(false)), None);
        assert_eq!(none("https://example.com/pink.css", &colourway), None);
    }
}
