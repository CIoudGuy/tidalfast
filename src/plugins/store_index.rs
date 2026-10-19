//! What the Plugin Store and Themes tabs know beyond the stores' own lists:
//! when each plugin was added and last updated, how popular a store is, and
//! the community theme catalogue with a colour preview of each theme.
//!
//! TidaLuna has no theme store (its Themes tab installs a CSS file by URL),
//! and a plugin store (`store.json`) carries no dates. The dates here come
//! from what the stores do expose:
//!
//! * a store published as a GitHub release: the release's asset times
//!   (`<plugin>.json` `updated_at`) and the repository's creation time;
//! * every store: the build hash of each plugin, remembered between runs, so
//!   a plugin that appears or changes after the first look is dated by that
//!   look;
//! * themes: the git commit dates of the theme's CSS file (last commit and
//!   first commit) and the repository's stars.
//!
//! Popularity is the stars of the repository a plugin or theme is published
//! from (public data, nothing is tracked). Plugins of one repository share its
//! stars, as they do on GitHub. Stars tie-break by the latest update, then
//! the name, so the order is the same on every run.
//!
//! Everything is fetched on one background thread, cached on disk with
//! ETags (a `304` costs no GitHub quota), refreshed when the page opens at
//! most every few hours, and read by the UI as one cheap `Arc` snapshot.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long a finished refresh stays fresh.
pub const FRESH_SECS: i64 = 4 * 3600;
/// How long to wait before trying again after a refresh stopped early (rate
/// limit, network).
const RETRY_SECS: i64 = 30 * 60;
/// "New" covers this long.
pub const RECENT_DAYS: i64 = 45;
/// The most the New shelf shows.
pub const SHELF_LEN: usize = 6;
/// The head of a theme's CSS that is read for its manifest and colours.
const CSS_HEAD: usize = 160 * 1024;
/// The largest GitHub API answer read.
const MAX_JSON: usize = 16 * 1024 * 1024;

/// A curated community theme: Neptune/TidaLuna CSS themes that stand on their
/// own (no base file to import). TidaLuna lists none itself.
pub struct CatalogEntry {
    pub name: &'static str,
    pub author: &'static str,
    pub description: &'static str,
    /// `owner/repo`.
    pub repo: &'static str,
    /// The CSS file's path in the repository (default branch).
    pub path: &'static str,
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "Catppuccin Mocha",
        author: "pernydev",
        description: "The Catppuccin palette for the TIDAL player, Mocha flavour.",
        repo: "pernydev/tidal-catppuccin",
        path: "mocha.css",
    },
    CatalogEntry {
        name: "Catppuccin Macchiato",
        author: "pernydev",
        description: "The Catppuccin palette for the TIDAL player, Macchiato flavour.",
        repo: "pernydev/tidal-catppuccin",
        path: "macchiato.css",
    },
    CatalogEntry {
        name: "Catppuccin Frappe",
        author: "pernydev",
        description: "The Catppuccin palette for the TIDAL player, Frappe flavour.",
        repo: "pernydev/tidal-catppuccin",
        path: "frappe.css",
    },
    CatalogEntry {
        name: "Catppuccin Mocha Blue",
        author: "DavidHiFi",
        description: "Catppuccin Mocha with a blue accent and a mauve progress bar.",
        repo: "DavidHiFi/MochaTidal",
        path: "MochaTidal.css",
    },
    CatalogEntry {
        name: "Lunallena",
        author: "Acercandr0",
        description: "A full screen theme inspired by full'moon.",
        repo: "Acercandr0/Lunallena",
        path: "Lunallena.css",
    },
    CatalogEntry {
        name: "Cotton Candy",
        author: "aya-zzz-zzz",
        description: "Pink and blue pastels with a soft glow.",
        repo: "aya-zzz-zzz/cottoncandy",
        path: "cottoncandy_win.css",
    },
    CatalogEntry {
        name: "Cotton Candy Nights",
        author: "aya-zzz-zzz",
        description: "The colourful dark companion of Cotton Candy.",
        repo: "aya-zzz-zzz/cottoncandy",
        path: "cottoncandynights_win.css",
    },
    CatalogEntry {
        name: "Sugarcoat",
        author: "aya-zzz-zzz",
        description: "A light theme that follows the playing cover.",
        repo: "aya-zzz-zzz/tidal-sugarcoat",
        path: "sugarcoat-win.css",
    },
    CatalogEntry {
        name: "Meteor Night",
        author: "masterjabberwook",
        description: "Dark glass floating in the sky during a meteor shower.",
        repo: "masterjabberwook/TidaLuna-Meteor-Night",
        path: "meteor-night.css",
    },
];

impl CatalogEntry {
    pub fn url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/HEAD/{}",
            self.repo, self.path
        )
    }
}

// ------------------------------------------------------------------- data

/// When a plugin was added and last updated (unix seconds), and how popular
/// its store is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDates {
    pub added: Option<i64>,
    pub updated: Option<i64>,
    /// Stars of the store's repository, the only popularity signal stores
    /// expose.
    pub stars: u32,
}

/// A community theme with the colours read from its CSS.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogTheme {
    pub url: String,
    pub name: String,
    pub author: String,
    /// Where the author's name leads, when the theme's CSS gives a link.
    pub author_url: Option<String>,
    /// The author's profile picture, when the theme's CSS gives one.
    pub author_avatar: Option<String>,
    pub description: String,
    pub repo: String,
    /// Background, surface, accent, second accent, text (those found).
    pub colors: Vec<[u8; 3]>,
    pub dark: bool,
    /// A miniature of the app in the theme, Dark and Light.
    #[serde(skip)]
    pub preview: Option<std::sync::Arc<crate::ui::theme_preview::Previews>>,
    pub added: Option<i64>,
    pub updated: Option<i64>,
    pub stars: u32,
}

/// What the UI reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    /// By plugin install URL.
    pub plugins: HashMap<String, PluginDates>,
    pub themes: Vec<CatalogTheme>,
    /// When the last complete refresh finished.
    pub fetched_at: Option<i64>,
    pub loading: bool,
}

impl Index {
    pub fn dates(&self, plugin_url: &str) -> PluginDates {
        self.plugins.get(plugin_url).copied().unwrap_or_default()
    }
}

// ------------------------------------------------------------------ cache

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Cached<T> {
    etag: Option<String>,
    data: T,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct ReleaseInfo {
    published: i64,
    /// Asset name to `updated_at`.
    assets: BTreeMap<String, i64>,
}

/// Whether a release asset is one of the files a plugin published.
fn is_plugin_file(name: &str, file: &str) -> bool {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let dotted = stem.replace(' ', ".");
    [stem, dotted.as_str()].iter().any(|stem| {
        name.strip_prefix(*stem)
            .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// When the newest of a plugin's files in the release was uploaded.
fn plugin_built(release: &ReleaseInfo, file: &str) -> Option<i64> {
    release
        .assets
        .iter()
        .filter(|(name, _)| is_plugin_file(name, file))
        .map(|(_, t)| *t)
        .max()
}

/// Orders plugins for the Popular sort: the repository's stars, then the
/// newest build, then the name and URL so equal plugins never swap places.
pub fn popularity_order(
    a: (&PluginDates, &str, &str),
    b: (&PluginDates, &str, &str),
) -> std::cmp::Ordering {
    b.0.stars
        .cmp(&a.0.stars)
        .then(b.0.updated.cmp(&a.0.updated))
        .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        .then_with(|| a.2.cmp(b.2))
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
struct RepoInfo {
    created: i64,
    pushed: i64,
    stars: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Seen {
    hash: Option<String>,
    added: Option<i64>,
    updated: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ThemeCss {
    name: Option<String>,
    author: Option<String>,
    #[serde(default)]
    author_url: Option<String>,
    #[serde(default)]
    author_avatar: Option<String>,
    description: Option<String>,
    colors: Vec<[u8; 3]>,
    dark: bool,
    #[serde(default)]
    preview: Option<crate::ui::theme_preview::Previews>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ThemeDates {
    updated: Option<i64>,
    added: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    fetched_at: Option<i64>,
    /// By API URL.
    releases: HashMap<String, Cached<ReleaseInfo>>,
    repos: HashMap<String, Cached<RepoInfo>>,
    /// By CSS URL.
    css: HashMap<String, Cached<ThemeCss>>,
    /// By `repo/path`: last commit date, and the first commit's.
    theme_dates: HashMap<String, Cached<ThemeDates>>,
    /// By store URL: when this store was first looked at.
    stores_seen: HashMap<String, i64>,
    /// By plugin install URL.
    plugins: HashMap<String, Seen>,
    /// By store URL: the plugin dates last worked out.
    dates: HashMap<String, HashMap<String, PluginDates>>,
}

/// 2: theme previews. 3: a cache from before the download counts were
/// dropped is read as it is (its download totals are ignored).
const CACHE_VERSION: u32 = 3;

fn index_of(cache: &Cache) -> Index {
    let mut index = Index {
        fetched_at: cache.fetched_at,
        ..Index::default()
    };
    for dates in cache.dates.values() {
        index
            .plugins
            .extend(dates.iter().map(|(url, d)| (url.clone(), *d)));
    }
    for entry in CATALOG {
        let url = entry.url();
        let css = cache.css.get(&url).map(|c| c.data.clone());
        let css = css.unwrap_or_default();
        let dates = cache
            .theme_dates
            .get(&format!("{}/{}", entry.repo, entry.path))
            .map(|d| d.data.clone())
            .unwrap_or_default();
        let repo = cache
            .repos
            .get(&repo_api(entry.repo))
            .map(|r| r.data)
            .unwrap_or_default();
        index.themes.push(CatalogTheme {
            url,
            name: css.name.unwrap_or_else(|| entry.name.to_string()),
            author: css.author.unwrap_or_else(|| entry.author.to_string()),
            author_url: css.author_url,
            author_avatar: css.author_avatar,
            description: css
                .description
                .unwrap_or_else(|| entry.description.to_string()),
            repo: entry.repo.to_string(),
            colors: css.colors,
            dark: css.dark,
            preview: css.preview.map(std::sync::Arc::new),
            added: dates.added.or(Some(repo.created).filter(|t| *t > 0)),
            updated: dates.updated.or(Some(repo.pushed).filter(|t| *t > 0)),
            stars: repo.stars,
        });
    }
    index
}

fn repo_api(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}")
}

fn read_cache(path: &Path) -> Cache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or(Cache {
            version: CACHE_VERSION,
            ..Cache::default()
        })
}

/// Atomically: write beside, then rename.
fn write_cache(path: &Path, cache: &Cache) {
    let Ok(text) = serde_json::to_string(cache) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ------------------------------------------------------------------ state

struct Shared {
    index: Arc<Index>,
    loaded: bool,
    loading: bool,
    /// When the last attempt started (unix seconds).
    attempted: i64,
}

fn shared() -> &'static Mutex<Shared> {
    static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
    SHARED.get_or_init(|| {
        Mutex::new(Shared {
            index: Arc::new(Index::default()),
            loaded: false,
            loading: false,
            attempted: 0,
        })
    })
}

fn cache_path(dir: &Path) -> PathBuf {
    dir.join("store-index.json")
}

/// The current index; the first call reads the cache from `dir`.
pub fn snapshot(dir: &Path) -> Arc<Index> {
    let Ok(mut s) = shared().lock() else {
        return Arc::new(Index::default());
    };
    if !s.loaded {
        s.loaded = true;
        let cache = read_cache(&cache_path(dir));
        s.index = Arc::new(index_of(&cache));
    }
    if s.loading && !s.index.loading {
        let mut index = (*s.index).clone();
        index.loading = true;
        s.index = Arc::new(index);
    }
    s.index.clone()
}

/// One plugin of a store, as the Luna runtime lists it.
#[derive(Clone, Debug)]
pub struct Listed {
    pub url: String,
    pub file: String,
    pub hash: Option<String>,
}

/// A store with the plugins it lists.
#[derive(Clone, Debug)]
pub struct ListedStore {
    pub url: String,
    pub plugins: Vec<Listed>,
}

/// Whether the cache is old enough for a refresh, and the last attempt far
/// enough back (a failed one is retried sooner than a finished one expires).
fn refresh_wanted(now: i64, fetched_at: Option<i64>, attempted: i64) -> bool {
    now - fetched_at.unwrap_or(0) >= FRESH_SECS && now - attempted >= RETRY_SECS
}

/// Whether a refresh would start now (cheap: the page asks every frame).
pub fn due(now: i64) -> bool {
    #[cfg(feature = "demo")]
    if std::env::var_os("TIDALFAST_DEMO_LUNA_FIXTURE").is_some() {
        return false;
    }
    shared()
        .lock()
        .is_ok_and(|s| !s.loading && refresh_wanted(now, s.index.fetched_at, s.attempted))
}

/// Starts a background refresh when the cache is older than [`FRESH_SECS`]
/// (or the last attempt stopped early) and none is running. `wake` is called
/// whenever the index changed.
pub fn refresh(
    dir: &Path,
    stores: Vec<ListedStore>,
    now: i64,
    wake: impl Fn() + Send + 'static,
) -> bool {
    #[cfg(feature = "demo")]
    if std::env::var_os("TIDALFAST_DEMO_LUNA_FIXTURE").is_some() {
        return false;
    }
    let path = cache_path(dir);
    {
        let Ok(mut s) = shared().lock() else {
            return false;
        };
        if s.loading {
            return false;
        }
        if !refresh_wanted(now, s.index.fetched_at, s.attempted) {
            return false;
        }
        if !s.loaded {
            s.loaded = true;
            s.index = Arc::new(index_of(&read_cache(&path)));
        }
        if !refresh_wanted(now, s.index.fetched_at, s.attempted) {
            return false;
        }
        s.loading = true;
        s.attempted = now;
    }
    let spawned = std::thread::Builder::new()
        .name("luna-store-index".into())
        .spawn(move || {
            let mut cache = read_cache(&path);
            let complete = fetch_all(&mut cache, &stores, now, &wake);
            if complete {
                cache.fetched_at = Some(now);
            }
            write_cache(&path, &cache);
            if let Ok(mut s) = shared().lock() {
                s.index = Arc::new(index_of(&cache));
                s.loading = false;
            }
            wake();
        });
    if spawned.is_err()
        && let Ok(mut s) = shared().lock()
    {
        s.loading = false;
    }
    true
}

// ------------------------------------------------------------------- fetch

struct Net {
    client: reqwest::blocking::Client,
    /// GitHub's API said stop (rate limit); the rest waits for next time.
    limited: bool,
}

/// What a refresh needs from the network (a fake in the tests).
trait Http {
    /// A conditional GET.
    fn get_json(&mut self, url: &str, etag: Option<&str>) -> Fetch<(Value, Option<String>)>;
    fn get_head(&mut self, url: &str, etag: Option<&str>) -> Fetch<(String, Option<String>)>;
    fn last_modified(&mut self, url: &str) -> Option<i64>;
    /// GitHub's API said stop; the rest waits for next time.
    fn limited(&self) -> bool;
}

impl Net {
    fn new() -> Option<Net> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Tidalfast-Luna/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Net {
            client,
            limited: false,
        })
    }
}

impl Http for Net {
    fn limited(&self) -> bool {
        self.limited
    }

    /// A conditional GET: `None` when unchanged (`304`) or failed.
    fn get_json(&mut self, url: &str, etag: Option<&str>) -> Fetch<(Value, Option<String>)> {
        let api = url.starts_with("https://api.github.com/");
        if api && self.limited {
            return Fetch::Skipped;
        }
        let mut req = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json");
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let Ok(resp) = req.send() else {
            return Fetch::Failed;
        };
        if api
            && resp
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok())
                .is_some_and(|left| left <= 1)
        {
            self.limited = true;
        }
        let status = resp.status();
        if status.as_u16() == 304 {
            return Fetch::Unchanged;
        }
        if status.as_u16() == 403 || status.as_u16() == 429 {
            self.limited = true;
            return Fetch::Skipped;
        }
        if !status.is_success() {
            return Fetch::Failed;
        }
        let tag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let link = resp
            .headers()
            .get("link")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let parsed = crate::luna::read_capped(resp, MAX_JSON, url)
            .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
        match parsed {
            Ok(mut value) => {
                // The `Link` header rides along for paging.
                if let Some(link) = link
                    && let Some(last) = last_page(&link)
                    && value.is_array()
                {
                    value = serde_json::json!({ "items": value, "last": last });
                }
                Fetch::Fresh((value, tag))
            }
            Err(_) => Fetch::Failed,
        }
    }

    fn get_head(&mut self, url: &str, etag: Option<&str>) -> Fetch<(String, Option<String>)> {
        let mut req = self
            .client
            .get(url)
            .header("Range", format!("bytes=0-{}", CSS_HEAD - 1));
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let Ok(resp) = req.send() else {
            return Fetch::Failed;
        };
        if resp.status().as_u16() == 304 {
            return Fetch::Unchanged;
        }
        if !resp.status().is_success() {
            return Fetch::Failed;
        }
        let tag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        // Only the head is wanted, also when a server ignores the Range.
        use std::io::Read;
        let mut head = Vec::new();
        match resp.take(CSS_HEAD as u64).read_to_end(&mut head) {
            Ok(_) => Fetch::Fresh((String::from_utf8_lossy(&head).into_owned(), tag)),
            Err(_) => Fetch::Failed,
        }
    }

    fn last_modified(&mut self, url: &str) -> Option<i64> {
        let resp = self.client.head(url).send().ok()?;
        resp.headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_http_date)
    }
}

enum Fetch<T> {
    Fresh(T),
    Unchanged,
    Skipped,
    Failed,
}

/// Returns whether every part finished (nothing was cut short).
fn fetch_all(cache: &mut Cache, stores: &[ListedStore], now: i64, wake: &dyn Fn()) -> bool {
    let Some(mut net) = Net::new() else {
        return false;
    };
    fetch_all_with(&mut net, cache, stores, now, wake)
}

fn fetch_all_with(
    net: &mut dyn Http,
    cache: &mut Cache,
    stores: &[ListedStore],
    now: i64,
    wake: &dyn Fn(),
) -> bool {
    let mut complete = true;
    // GitHub answers 60 API requests an hour to an address without a
    // sign-in, and a first refresh needs more. What the store shows most
    // goes first: every store's release (dates), then the
    // themes, then the repositories (stars, a new store's badge). Answers
    // that did not change (304) do not count, so later refreshes are cheap.
    for store in stores {
        complete &= fetch_store(net, cache, store, now, Pass::Releases);
    }
    wake();
    for entry in CATALOG {
        complete &= fetch_theme(net, cache, entry);
    }
    wake();
    for store in stores {
        complete &= fetch_store(net, cache, store, now, Pass::Repos);
    }
    complete && !net.limited()
}

fn fetch_theme(net: &mut dyn Http, cache: &mut Cache, entry: &CatalogEntry) -> bool {
    let mut ok = true;
    let url = entry.url();
    let tag = cache.css.get(&url).and_then(|c| c.etag.clone());
    match net.get_head(&crate::luna::themes::fetch_url(&url), tag.as_deref()) {
        Fetch::Fresh((text, tag)) => {
            cache.css.insert(
                url,
                Cached {
                    etag: tag,
                    data: theme_css(&text),
                },
            );
        }
        Fetch::Unchanged => {}
        Fetch::Skipped | Fetch::Failed => ok = false,
    }
    let api = repo_api(entry.repo);
    let tag = cache.repos.get(&api).and_then(|c| c.etag.clone());
    match net.get_json(&api, tag.as_deref()) {
        Fetch::Fresh((value, tag)) => {
            cache.repos.insert(
                api.clone(),
                Cached {
                    etag: tag,
                    data: parse_repo(&value),
                },
            );
        }
        Fetch::Unchanged => {}
        Fetch::Skipped | Fetch::Failed => ok = false,
    }
    // The file's own history: the newest commit, then the oldest.
    let key = format!("{}/{}", entry.repo, entry.path);
    let latest = format!("{api}/commits?path={}&per_page=1", entry.path);
    let tag = cache.theme_dates.get(&key).and_then(|c| c.etag.clone());
    match net.get_json(&latest, tag.as_deref()) {
        Fetch::Fresh((value, tag)) => {
            let updated = commit_date(&value);
            let mut added = updated;
            if let Some(last) = value["last"].as_u64().filter(|n| *n > 1) {
                let oldest = format!("{latest}&page={last}");
                if let Fetch::Fresh((value, _)) = net.get_json(&oldest, None) {
                    added = commit_date(&value).or(added);
                }
            }
            cache.theme_dates.insert(
                key,
                Cached {
                    etag: tag,
                    data: ThemeDates { updated, added },
                },
            );
        }
        Fetch::Unchanged => {}
        Fetch::Skipped | Fetch::Failed => ok = false,
    }
    ok
}

/// Which of a store's two GitHub answers a pass asks for; the other comes
/// from the cache.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Releases,
    Repos,
}

fn fetch_store(
    net: &mut dyn Http,
    cache: &mut Cache,
    store: &ListedStore,
    now: i64,
    pass: Pass,
) -> bool {
    if pass == Pass::Repos && github_release(&store.url).is_none() {
        return true;
    }
    let mut ok = true;
    let mut release = None;
    let mut repo = RepoInfo::default();
    if let Some((owner, name, tag)) = github_release(&store.url) {
        let api = format!("https://api.github.com/repos/{owner}/{name}/releases/tags/{tag}");
        let etag = cache.releases.get(&api).and_then(|c| c.etag.clone());
        match (pass == Pass::Releases).then(|| net.get_json(&api, etag.as_deref())) {
            Some(Fetch::Fresh((value, etag))) => {
                if let Some(info) = parse_release(&value) {
                    cache
                        .releases
                        .insert(api.clone(), Cached { etag, data: info });
                }
            }
            None | Some(Fetch::Unchanged) => {}
            Some(Fetch::Skipped | Fetch::Failed) => ok = false,
        }
        release = cache.releases.get(&api).map(|c| c.data.clone());
        let api = repo_api(&format!("{owner}/{name}"));
        let etag = cache.repos.get(&api).and_then(|c| c.etag.clone());
        match (pass == Pass::Repos).then(|| net.get_json(&api, etag.as_deref())) {
            Some(Fetch::Fresh((value, etag))) => {
                cache.repos.insert(
                    api.clone(),
                    Cached {
                        etag,
                        data: parse_repo(&value),
                    },
                );
            }
            None | Some(Fetch::Unchanged) => {}
            Some(Fetch::Skipped | Fetch::Failed) => ok = false,
        }
        repo = cache.repos.get(&api).map(|c| c.data).unwrap_or_default();
    }
    // Stores that are not GitHub releases: the build time of `store.json`.
    let store_built = if release.is_none() {
        net.last_modified(&format!("{}/store.json", store.url))
    } else {
        None
    };
    let first_look = !cache.stores_seen.contains_key(&store.url);
    let store_new = Some(repo.created).filter(|t| *t > 0 && now - t < RECENT_DAYS * 86400);
    let mut dates = HashMap::new();
    for p in &store.plugins {
        // The newest of the plugin's own files (bundle or manifest): on a
        // rolling "latest" release only those carry the real date.
        let asset = release
            .as_ref()
            .map(|r| plugin_built(r, &p.file).unwrap_or(r.published))
            .or(store_built);
        let seen = cache.plugins.entry(p.url.clone()).or_default();
        let d = plugin_dates(seen, p.hash.as_deref(), asset, store_new, first_look, now);
        dates.insert(
            p.url.clone(),
            PluginDates {
                stars: repo.stars,
                ..d
            },
        );
    }
    if ok && !store.plugins.is_empty() {
        cache.stores_seen.insert(store.url.clone(), now);
    }
    if !dates.is_empty() {
        cache.dates.insert(store.url.clone(), dates);
    }
    ok
}

// ------------------------------------------------------------------ logic

/// Works out a plugin's dates from what is known and remembers it. `asset`
/// is when the store published this build, `store_new` when the store itself
/// appeared (if recently), `first_look` whether this is the first time the
/// store was looked at.
fn plugin_dates(
    seen: &mut Seen,
    hash: Option<&str>,
    asset: Option<i64>,
    store_new: Option<i64>,
    first_look: bool,
    now: i64,
) -> PluginDates {
    let published = asset.unwrap_or(now);
    let known = seen.updated.is_some() || seen.added.is_some() || seen.hash.is_some();
    if !known {
        if first_look {
            // The plugin was there before the first look: dated by when its
            // build was published, new only along with a new store.
            seen.added = store_new;
            seen.updated = Some(published);
        } else {
            // It appeared after the store was first seen.
            seen.added = Some(published);
            seen.updated = Some(published);
        }
        seen.hash = hash.map(str::to_string);
    } else if hash.is_some() && seen.hash.as_deref() != hash {
        seen.hash = hash.map(str::to_string);
        seen.updated = Some(published.max(seen.updated.unwrap_or(0)));
    }
    PluginDates {
        added: seen.added,
        updated: seen.updated,
        stars: 0,
    }
}

/// `https://github.com/<owner>/<repo>/releases/download/<tag>`.
fn github_release(store_url: &str) -> Option<(String, String, String)> {
    let rest = store_url.strip_prefix("https://github.com/")?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    (parts.next()? == "releases" && parts.next()? == "download").then_some(())?;
    let tag = parts.next()?;
    (parts.next().is_none()).then(|| (owner.to_string(), repo.to_string(), tag.to_string()))
}

fn parse_release(v: &Value) -> Option<ReleaseInfo> {
    let published = v["published_at"]
        .as_str()
        .or(v["created_at"].as_str())
        .and_then(parse_rfc3339)?;
    let list = v["assets"].as_array()?;
    let assets = list
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            let t = a["updated_at"]
                .as_str()
                .or(a["created_at"].as_str())
                .and_then(parse_rfc3339)?;
            Some((name.to_string(), t))
        })
        .collect();
    Some(ReleaseInfo { published, assets })
}

fn parse_repo(v: &Value) -> RepoInfo {
    RepoInfo {
        created: v["created_at"]
            .as_str()
            .and_then(parse_rfc3339)
            .unwrap_or(0),
        pushed: v["pushed_at"].as_str().and_then(parse_rfc3339).unwrap_or(0),
        stars: v["stargazers_count"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
    }
}

/// The date of the first commit in a commits listing.
fn commit_date(v: &Value) -> Option<i64> {
    let list = v.get("items").unwrap_or(v);
    list.get(0)?["commit"]["committer"]["date"]
        .as_str()
        .or(list.get(0)?["commit"]["author"]["date"].as_str())
        .and_then(parse_rfc3339)
}

/// The `page` of `rel="last"` in a `Link` header.
fn last_page(link: &str) -> Option<u64> {
    link.split(',').find_map(|part| {
        if !part.contains("rel=\"last\"") {
            return None;
        }
        let url = part.split(['<', '>']).nth(1)?;
        let query = url.split_once('?')?.1;
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix("page=")?.parse().ok())
    })
}

// ------------------------------------------------------------------ dates

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// `2026-09-07T22:55:12Z` (an offset or fractional seconds are accepted).
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let clock: String = time.chars().take(8).collect();
    let mut t = clock.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    let mut offset = 0;
    if let Some(pos) = time.rfind(['+', '-'])
        && pos > 0
    {
        let tz = &time[pos + 1..];
        let (th, tm) = tz.split_once(':').unwrap_or((tz, "0"));
        let secs = th.parse::<i64>().ok()? * 3600 + tm.parse::<i64>().ok()? * 60;
        offset = if time.as_bytes()[pos] == b'+' {
            secs
        } else {
            -secs
        };
    }
    Some(days_from_civil(y, m, day) * 86400 + h * 3600 + mi * 60 + sec - offset)
}

/// `Mon, 07 Sep 2026 22:55:34 GMT`.
pub fn parse_http_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let [_, day, month, year, clock, ..] = parts.as_slice() else {
        return None;
    };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = months.iter().position(|m| m.eq_ignore_ascii_case(month))? as i64 + 1;
    let mut t = clock.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    Some(
        days_from_civil(year.parse().ok()?, m, day.parse().ok()?) * 86400
            + h * 3600
            + mi * 60
            + sec,
    )
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

// ----------------------------------------------------------------- shelves

/// A date-carrying item the shelves sort.
pub trait Dated {
    fn added(&self) -> Option<i64>;
    fn updated(&self) -> Option<i64>;
    fn key(&self) -> &str;
}

/// The "New" shelf: added within [`RECENT_DAYS`], newest first.
pub fn new_shelf<T: Dated>(items: &[T], now: i64) -> Vec<&T> {
    let mut v: Vec<&T> = items
        .iter()
        .filter(|i| {
            i.added()
                .is_some_and(|t| t <= now + 86400 && now - t < RECENT_DAYS * 86400)
        })
        .collect();
    v.sort_by(|a, b| b.added().cmp(&a.added()).then_with(|| a.key().cmp(b.key())));
    v.truncate(SHELF_LEN);
    v
}

// ------------------------------------------------------------- theme CSS

/// Name, author, description and colours from the head of a theme's CSS.
fn theme_css(css: &str) -> ThemeCss {
    let mut out = ThemeCss::default();
    if let Some(start) = css.find("/*")
        && let Some(end) = css[start..].find("*/")
    {
        let body = &css[start + 2..start + end];
        if let (Some(a), Some(b)) = (body.find('{'), body.rfind('}'))
            && a < b
            && let Ok(v) = serde_json::from_str::<Value>(&body[a..=b])
        {
            let text = |k: &str| {
                v[k].as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            out.name = text("name");
            let author = crate::luna::author::parse(&v["author"]);
            out.author = author
                .name
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty());
            out.author_url = author.url;
            out.author_avatar = author.avatar;
            out.description = text("description");
        }
    }
    let (colors, dark) = palette(css);
    out.colors = colors;
    out.dark = dark;
    out.preview = Some(crate::ui::theme_preview::resolve_both(css));
    out
}

/// A colour from a CSS value: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`,
/// `rgba()`, or a bare `r, g, b` triple (as cover-colour themes write them).
/// Mostly transparent colours are not colours.
pub fn parse_color(value: &str) -> Option<[u8; 3]> {
    let v = value.trim().trim_end_matches("!important").trim();
    if let Some(hex) = v.strip_prefix('#') {
        let hex = hex.trim();
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|d| d * 17);
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return match hex.len() {
            3 => Some([digit(0)?, digit(1)?, digit(2)?]),
            6 => Some([pair(0)?, pair(2)?, pair(4)?]),
            8 if pair(6)? >= 200 => Some([pair(0)?, pair(2)?, pair(4)?]),
            _ => None,
        };
    }
    let (inner, alpha_ok) =
        if let Some(rest) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
            (rest.strip_suffix(')')?, true)
        } else {
            (v, false)
        };
    let nums: Vec<f32> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim_end_matches('%').parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() < 3 || nums.len() > 4 || (nums.len() == 4 && !alpha_ok) {
        return None;
    }
    if nums.len() == 4 && nums[3] < 0.8 {
        return None;
    }
    if nums[..3].iter().any(|n| !(0.0..=255.0).contains(n)) {
        return None;
    }
    Some([nums[0] as u8, nums[1] as u8, nums[2] as u8])
}

fn luminance(c: [u8; 3]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

fn saturation(c: [u8; 3]) -> f32 {
    let max = c.iter().copied().max().unwrap_or(0) as f32;
    let min = c.iter().copied().min().unwrap_or(0) as f32;
    if max == 0.0 { 0.0 } else { (max - min) / max }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (*x as f32 - y as f32).abs())
        .sum()
}

/// Up to five representative colours (background, surface, accent, second
/// accent, text) from a theme's custom properties, and whether the theme is
/// dark.
pub fn palette(css: &str) -> (Vec<[u8; 3]>, bool) {
    let mut found: Vec<(String, [u8; 3])> = Vec::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while let Some(pos) = css[i..].find("--") {
        let start = i + pos;
        i = start + 2;
        if start > 0 && !matches!(bytes[start - 1], b' ' | b'\n' | b'\t' | b'{' | b';' | b'\r') {
            continue;
        }
        let Some(colon) = css[start..].find(':') else {
            break;
        };
        let name = css[start + 2..start + colon].trim().to_lowercase();
        if name.is_empty() || name.len() > 60 || name.contains([' ', '\n', ';', '{', '}']) {
            continue;
        }
        let rest = &css[start + colon + 1..];
        let end = rest.find([';', '}', '\n']).unwrap_or(rest.len());
        let value = rest[..end].trim();
        // Values with a comment after them.
        let value = value.split("/*").next().unwrap_or(value).trim();
        if let Some(c) = parse_color(value) {
            found.push((name, c));
        }
        if found.len() >= 400 {
            break;
        }
    }
    if found.is_empty() {
        return (Vec::new(), true);
    }
    let named = |c: &(String, [u8; 3]), words: &[&str]| words.iter().any(|w| c.0.contains(w));
    let bg = found
        .iter()
        .find(|c| named(c, &["bg", "background", "base", "crust", "night", "sky"]))
        .or_else(|| {
            found.iter().min_by(|a, b| {
                luminance(a.1)
                    .partial_cmp(&luminance(b.1))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .map(|c| c.1)
        .unwrap_or([0, 0, 0]);
    let dark = luminance(bg) < 0.5;
    let text = found
        .iter()
        .find(|c| named(c, &["text", "fg", "foreground", "contrast"]) && distance(c.1, bg) > 150.0)
        .or_else(|| {
            found.iter().max_by(|a, b| {
                distance(a.1, bg)
                    .partial_cmp(&distance(b.1, bg))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .map(|c| c.1)
        .unwrap_or([255, 255, 255]);
    let accents = {
        let mut v: Vec<&(String, [u8; 3])> = found
            .iter()
            .filter(|c| saturation(c.1) > 0.25 && (0.12..0.95).contains(&luminance(c.1)))
            .collect();
        v.sort_by(|a, b| {
            let score = |c: &(String, [u8; 3])| {
                saturation(c.1)
                    + if named(c, &["accent", "primary", "brand", "highlight"]) {
                        1.0
                    } else {
                        0.0
                    }
            };
            score(b)
                .partial_cmp(&score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v
    };
    let mut out = vec![bg];
    let surface = found
        .iter()
        .find(|c| named(c, &["surface", "mantle", "panel", "card"]) && distance(c.1, bg) > 12.0)
        .map(|c| c.1)
        .unwrap_or_else(|| {
            // A step toward the text colour.
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * 0.08) as u8;
            [
                mix(bg[0], text[0]),
                mix(bg[1], text[1]),
                mix(bg[2], text[2]),
            ]
        });
    out.push(surface);
    for c in accents {
        if out.len() >= 4 {
            break;
        }
        if out.iter().all(|o| distance(*o, c.1) > 60.0) {
            out.push(c.1);
        }
    }
    if out.len() < 3 {
        out.push(text);
    }
    out.push(text);
    (out, dark)
}

#[cfg(test)]
#[path = "store_index_tests.rs"]
mod tests;
