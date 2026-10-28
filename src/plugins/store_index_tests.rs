use super::*;
use serde_json::json;

const DAY: i64 = 86400;

#[test]
fn dates_parse_from_the_release_api_and_http_headers() {
    let t = parse_rfc3339("2026-09-07T22:55:12Z").unwrap();
    assert_eq!(parse_http_date("Mon, 07 Sep 2026 22:55:12 GMT"), Some(t));
    assert_eq!(parse_rfc3339("1970-01-02T00:00:00Z"), Some(DAY));
    assert_eq!(
        parse_rfc3339("2026-09-07T23:55:12+01:00"),
        Some(t),
        "an offset is applied"
    );
    assert_eq!(parse_rfc3339("nonsense"), None);
}

#[test]
fn a_release_gives_each_plugin_its_asset_time() {
    let v = json!({
        "published_at": "2026-09-07T22:55:34Z",
        "assets": [
            {"name": "Avatar.json", "updated_at": "2026-09-01T00:00:00Z"},
            {"name": "Avatar.mjs", "updated_at": "2026-09-07T00:00:00Z"},
            {"name": "store.json", "updated_at": "2026-09-07T22:55:30Z"},
        ]
    });
    let r = parse_release(&v).unwrap();
    assert_eq!(r.published, parse_rfc3339("2026-09-07T22:55:34Z").unwrap());
    assert_eq!(
        r.assets["Avatar.json"],
        parse_rfc3339("2026-09-01T00:00:00Z").unwrap()
    );
    let url = [
        "https://git",
        "hub.com/Inrixia/luna-plugins/releases/download/latest",
    ]
    .concat();
    assert_eq!(
        github_release(&url),
        Some((
            "Inrixia".into(),
            "luna-plugins".into(),
            "releases/tags/latest".into()
        ))
    );
    // GitHub's address of a repository's latest release.
    let latest = [
        "https://git",
        "hub.com/BaeHyolyn/tidal-presence/releases/latest/download",
    ]
    .concat();
    assert_eq!(
        github_release(&latest),
        Some((
            "BaeHyolyn".into(),
            "tidal-presence".into(),
            "releases/latest".into()
        ))
    );
    assert_eq!(github_release("https://example.com/store"), None);
}

#[test]
fn a_repository_and_commit_listing_are_read() {
    let repo = parse_repo(&json!({
        "created_at": "2026-01-02T00:00:00Z",
        "pushed_at": "2026-03-03T00:00:00Z",
        "stargazers_count": 12
    }));
    assert_eq!(repo.stars, 12);
    assert!(repo.pushed > repo.created);
    let commits = json!([{ "commit": { "committer": { "date": "2026-03-03T10:00:00Z" } } }]);
    assert_eq!(commit_date(&commits), parse_rfc3339("2026-03-03T10:00:00Z"));
    assert_eq!(
        last_page(
            "<https://api.example.com/commits?path=a.css&per_page=1&page=2>; rel=\"next\", \
             <https://api.example.com/commits?path=a.css&per_page=1&page=7>; rel=\"last\""
        ),
        Some(7)
    );
}

#[test]
fn a_plugin_is_dated_by_what_was_seen_of_it() {
    let now = 100 * DAY;
    let asset = Some(90 * DAY);
    // First look at the store: not new, last updated when its build was published.
    let mut seen = Seen::default();
    let d = plugin_dates(&mut seen, Some("h1"), asset, None, true, now);
    assert_eq!((d.added, d.updated), (None, Some(90 * DAY)));
    // Same build later: nothing moves.
    let d = plugin_dates(&mut seen, Some("h1"), Some(95 * DAY), None, false, now);
    assert_eq!(d.updated, Some(90 * DAY));
    // A new build: updated at its publication.
    let d = plugin_dates(&mut seen, Some("h2"), Some(99 * DAY), None, false, now);
    assert_eq!(d.updated, Some(99 * DAY));
    // A plugin that appears in a store already looked at is new.
    let mut fresh = Seen::default();
    let d = plugin_dates(&mut fresh, Some("x"), Some(98 * DAY), None, false, now);
    assert_eq!((d.added, d.updated), (Some(98 * DAY), Some(98 * DAY)));
    // A brand new store makes everything in it new.
    let mut other = Seen::default();
    let d = plugin_dates(&mut other, None, asset, Some(80 * DAY), true, now);
    assert_eq!(d.added, Some(80 * DAY));
}

struct Item(&'static str, Option<i64>, Option<i64>);
impl Dated for Item {
    fn added(&self) -> Option<i64> {
        self.1
    }
    fn updated(&self) -> Option<i64> {
        self.2
    }
    fn key(&self) -> &str {
        self.0
    }
}

#[test]
fn the_new_shelf_is_ordered_newest_first_and_capped() {
    let now = 1000 * DAY;
    let items = [
        Item("old", Some(now - 400 * DAY), Some(now - 400 * DAY)),
        Item("b-new", Some(now - 3 * DAY), Some(now - 3 * DAY)),
        Item("a-new", Some(now - 3 * DAY), Some(now - DAY)),
        Item("newest", Some(now - DAY), Some(now - DAY)),
        Item("upd-old", None, Some(now - 20 * DAY)),
        Item("upd-new", None, Some(now - 2 * DAY)),
        Item("unknown", None, None),
    ];
    let new: Vec<&str> = new_shelf(&items, now).iter().map(|i| i.key()).collect();
    assert_eq!(new, ["newest", "a-new", "b-new"]);
    let many: Vec<Item> = (0..20)
        .map(|i| {
            Item(
                Box::leak(format!("p{i}").into_boxed_str()),
                Some(now - i * 60),
                None,
            )
        })
        .collect();
    assert_eq!(new_shelf(&many, now).len(), SHELF_LEN);
}

#[test]
fn colours_parse_in_the_forms_themes_use() {
    assert_eq!(parse_color("#1e1e2e"), Some([0x1e, 0x1e, 0x2e]));
    assert_eq!(parse_color("#fff"), Some([255, 255, 255]));
    assert_eq!(parse_color("rgb(10, 20, 30)"), Some([10, 20, 30]));
    assert_eq!(
        parse_color("rgba(10, 20, 30, 0.2)"),
        None,
        "mostly transparent"
    );
    assert_eq!(parse_color("255, 103, 235"), Some([255, 103, 235]));
    assert_eq!(parse_color("blur(18px) saturate(130%)"), None);
    assert_eq!(parse_color("var(--x)"), None);
}

#[test]
fn a_theme_gets_its_manifest_and_a_colour_preview() {
    let css = r#"/*
{ "name": "Mocha", "author": "Me", "description": "Warm" }
*/
:root {
  --ctp-base: #1e1e2e;
  --ctp-mantle: #181825;
  --ctp-text: #cdd6f4;
  --ctp-mauve: #cba6f7;
  --ctp-teal: #94e2d5;
  --glass-blur: blur(18px);
}"#;
    let t = theme_css(css);
    assert_eq!(t.name.as_deref(), Some("Mocha"));
    assert_eq!(t.author.as_deref(), Some("Me"));
    assert!(t.dark);
    assert_eq!(t.colors.first(), Some(&[0x1e, 0x1e, 0x2e]));
    assert_eq!(t.colors.last(), Some(&[0xcd, 0xd6, 0xf4]));
    assert!(t.colors.len() >= 4, "{:?}", t.colors);
    // A light theme is recognised, and one with no colours has no preview.
    let (_, dark) = palette(":root { --bg: #fafafa; --text: #111111; --accent: #e91e63; }");
    assert!(!dark);
    assert!(palette("body { color: red }").0.is_empty());
}

#[test]
fn the_catalogue_has_standalone_themes_with_raw_urls() {
    assert!(CATALOG.len() >= 8);
    for e in CATALOG {
        assert!(e.url().starts_with("https://raw.githubusercontent.com/"));
        assert!(e.path.ends_with(".css"));
        assert!(!e.url().contains(' '), "{}", e.url());
        if let Some(base) = e.base {
            assert!(!base.paths.is_empty() && base.paths.iter().all(|p| p.ends_with(".css")));
            assert_ne!(e.base_url(), Some(e.url()), "{}", e.name);
        }
        // Card text says what it is in plain words.
        for text in [e.name, e.description] {
            assert!(!text.contains('\u{2014}'), "no em dash: {text}");
        }
    }
    let names: std::collections::HashSet<_> = CATALOG.iter().map(|e| e.name).collect();
    assert_eq!(names.len(), CATALOG.len());
    let urls: std::collections::HashSet<_> = CATALOG.iter().map(|e| e.url()).collect();
    assert_eq!(urls.len(), CATALOG.len(), "each file once");
}

/// Some colourways live in folders with spaces in their names
/// (`halloween collection/`): their links are percent-encoded, as raw GitHub
/// links must be, and so is the path in a commits query.
#[test]
fn catalogue_links_percent_encode_spaces_in_paths() {
    assert_eq!(
        raw_url("o/r", "halloween collection/calavera.css"),
        "https://raw.githubusercontent.com/o/r/HEAD/halloween%20collection/calavera.css"
    );
    assert_eq!(
        raw_url("o/r", "colorways/astronaut icecream/blueberry_cobbler.css"),
        "https://raw.githubusercontent.com/o/r/HEAD/colorways/astronaut%20icecream/blueberry_cobbler.css"
    );
    // Paths without anything to encode are as they were.
    assert_eq!(
        raw_url("Acercandr0/Lunallena", "Lunallena_MacOS.css"),
        "https://raw.githubusercontent.com/Acercandr0/Lunallena/HEAD/Lunallena_MacOS.css"
    );
    assert_eq!(encode_path("a+b/ü.css"), "a%2Bb/%C3%BC.css");
    let calavera = CATALOG
        .iter()
        .find(|e| e.name == "Cotton Candy: Calavera")
        .unwrap();
    assert!(
        calavera
            .url()
            .ends_with("/HEAD/halloween%20collection/calavera.css")
    );
    // Any link to it (a raw link with the space typed as `%20`, a GitHub
    // page) finds the catalogue's base.
    let base = catalog_base(
        "https://github.com/aya-zzz-zzz/cotton-candy-colorways/blob/main/halloween%20collection/calavera.css",
    )
    .unwrap();
    assert!(
        base.starts_with(
            "https://raw.githubusercontent.com/aya-zzz-zzz/cotton-candy-colorways/HEAD/base-"
        ),
        "{base}"
    );
    assert_eq!(catalog_base(&CATALOG[0].url()), None, "a standalone theme");
    assert_eq!(catalog_base("https://example.com/x.css"), None);
}

/// A colourway's card reads the colourway's manifest and colours, and its
/// preview comes from the base and colourway together; the base is fetched
/// once for every colourway on it, and a base that fails keeps nothing, so
/// the next refresh tries again.
#[test]
fn a_colourway_card_is_previewed_over_its_base_fetched_once() {
    let based: Vec<&CatalogEntry> = CATALOG
        .iter()
        .filter(|e| e.base.is_some() && e.base_url() == CATALOG[5].base_url())
        .collect();
    assert!(based.len() >= 10, "Cotton Candy's colourways");
    let base_url = based[0].base_url().unwrap();
    let mut net = fake(FakeMode::Up, &[]);
    net.css.insert(
        base_url.clone(),
        "/* {\"name\": \"cotton candy\"} */\n#main { background: var(--cover-Vib1) }\n".into(),
    );
    for e in &based {
        net.css.insert(
            e.url(),
            ":root { --cover-Vib1: #ff88cc; --bg: #fff0f8 }".into(),
        );
    }
    let mut cache = Cache::default();
    let mut bases = HashMap::new();
    for e in &based {
        assert!(fetch_theme(&mut net, &mut cache, e, &mut bases));
    }
    assert_eq!(
        net.asked.iter().filter(|u| **u == base_url).count(),
        1,
        "the base is fetched once"
    );
    let card = &cache.css[&based[1].url()].data;
    assert_eq!(
        card.name, None,
        "the base's manifest is not the colourway's"
    );
    assert!(
        card.colors.contains(&[0xff, 0x88, 0xcc]),
        "{:?}",
        card.colors
    );
    assert!(card.preview.is_some());
    let index = index_of(&cache);
    let shown = index
        .themes
        .iter()
        .find(|t| t.url == based[1].url())
        .unwrap();
    assert_eq!(shown.name, based[1].name);

    // The base fails: the colourway is not cached without it.
    let mut net = fake(FakeMode::Up, &[]);
    net.css
        .insert(based[0].url(), ":root { --cover-Vib1: #ff88cc }".into());
    let mut cache = Cache::default();
    assert!(!fetch_theme(
        &mut net,
        &mut cache,
        based[0],
        &mut HashMap::new()
    ));
    assert!(!cache.css.contains_key(&based[0].url()));
}

#[test]
fn a_cut_off_base_ends_after_its_last_whole_rule() {
    assert_eq!(whole_rules("a { b: c } d { e"), "a { b: c }");
    assert_eq!(
        whole_rules("@media x { a { b: c } } @media y { d { e: f }"),
        "@media x { a { b: c } }"
    );
    assert_eq!(
        whole_rules("@import \"x}\"; a { b: \"}\" } c {"),
        "@import \"x}\"; a { b: \"}\" }"
    );
    assert_eq!(whole_rules("a {} /* } */ b { c"), "a {}");
    assert_eq!(whole_rules("a { b"), "");
}

/// Two catalogue files whose CSS names them alike (Abyss Neptune and its
/// macOS build) show the catalogue's names and descriptions; a name only
/// one file has stays the file's own.
#[test]
fn catalogue_themes_whose_css_shares_a_name_show_the_catalogues() {
    let abyss: Vec<&CatalogEntry> = CATALOG
        .iter()
        .filter(|e| e.repo == "itzzexcel/neptune-projects")
        .collect();
    assert_eq!(abyss.len(), 2);
    let manifest =
        "/* {\"name\": \"Abyss Neptune\", \"description\": \"same\"} */ :root{--bg:#000}";
    let mut cache = Cache::default();
    for e in &abyss {
        cache.css.insert(
            e.url(),
            Cached {
                etag: None,
                data: theme_css(manifest),
            },
        );
    }
    cache.css.insert(
        CATALOG[0].url(),
        Cached {
            etag: None,
            data: theme_css("/* {\"name\": \"Mocha Own\"} */ :root{--bg:#000}"),
        },
    );
    let index = index_of(&cache);
    for e in &abyss {
        let t = index.themes.iter().find(|t| t.url == e.url()).unwrap();
        assert_eq!(t.name, e.name);
        assert_eq!(t.description, e.description);
    }
    assert_eq!(index.themes[0].name, "Mocha Own");
}

#[test]
fn the_index_reads_back_from_the_cache_file() {
    let dir = std::env::temp_dir().join(format!("tidalfast-index-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store-index.json");
    let mut cache = Cache {
        version: CACHE_VERSION,
        fetched_at: Some(5),
        ..Cache::default()
    };
    cache.dates.insert(
        "https://s".into(),
        HashMap::from([(
            "https://s/A".to_string(),
            PluginDates {
                added: Some(1),
                updated: Some(2),
                stars: 3,
            },
        )]),
    );
    let first = CATALOG[0].url();
    cache.css.insert(
        first.clone(),
        Cached {
            etag: Some("e".into()),
            data: theme_css(":root{--bg:#101010;--text:#eeeeee;--accent:#00aaff}"),
        },
    );
    write_cache(&path, &cache);
    let index = index_of(&read_cache(&path));
    assert_eq!(index.fetched_at, Some(5));
    assert_eq!(index.dates("https://s/A").stars, 3);
    assert_eq!(index.dates("https://s/none"), PluginDates::default());
    let t = index.themes.iter().find(|t| t.url == first).unwrap();
    assert!(!t.colors.is_empty());
    assert_eq!(index.themes.len(), CATALOG.len());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn popular_is_ordered_by_stars_then_newest_then_name_then_url() {
    let d = |stars| PluginDates {
        stars,
        ..PluginDates::default()
    };
    let mut rows = vec![
        (d(1), "beta", "https://x/2"),
        (d(1), "Alpha", "https://x/3"),
        (d(9), "zeta", "https://x/1"),
        (d(500), "omega", "https://x/9"),
        (d(1), "alpha", "https://x/0"),
        (d(0), "none", "https://x/4"),
        (
            PluginDates {
                updated: Some(9),
                ..d(1)
            },
            "newest",
            "https://x/5",
        ),
    ];
    let order = |rows: &mut Vec<(PluginDates, &'static str, &'static str)>| {
        rows.sort_by(|a, b| popularity_order((&a.0, a.1, a.2), (&b.0, b.1, b.2)));
        rows.iter().map(|r| r.2).collect::<Vec<_>>()
    };
    let expected = [
        "https://x/9",
        "https://x/1",
        "https://x/5",
        "https://x/0",
        "https://x/3",
        "https://x/2",
        "https://x/4",
    ];
    assert_eq!(order(&mut rows), expected);
    rows.reverse();
    assert_eq!(order(&mut rows), expected, "input order does not matter");
}

#[test]
fn a_refresh_is_wanted_when_the_cache_is_old_and_the_last_try_is_not_recent() {
    let now = 100 * DAY;
    assert!(refresh_wanted(now, None, 0), "nothing cached yet");
    assert!(
        !refresh_wanted(now, Some(now - FRESH_SECS + 1), 0),
        "still fresh"
    );
    assert!(
        refresh_wanted(now, Some(now - FRESH_SECS), 0),
        "just expired"
    );
    // A try that stopped early (rate limit, offline) is retried after half an
    // hour, not at once and not only after the cache expires.
    let old = Some(now - 10 * FRESH_SECS);
    assert!(!refresh_wanted(now, old, now - RETRY_SECS + 1));
    assert!(refresh_wanted(now, old, now - RETRY_SECS));
}

/// A GitHub that answers from fixtures, and records what was asked.
struct Fake {
    release: Value,
    repo: Value,
    /// Everything fails (offline), or the rate limit is spent.
    mode: FakeMode,
    asked: Vec<String>,
    /// API requests answered before GitHub's hourly limit runs out.
    budget: Option<usize>,
    /// Raw files by URL; anything else fails.
    css: HashMap<String, String>,
}

/// Both passes of a refresh for one store.
fn fetch_both(net: &mut Fake, cache: &mut Cache, store: &ListedStore, now: i64) -> bool {
    let releases = fetch_store(net, cache, store, now, Pass::Releases);
    fetch_store(net, cache, store, now, Pass::Repos) && releases
}

#[derive(Clone, Copy, PartialEq)]
enum FakeMode {
    Up,
    Offline,
    RateLimited,
}

impl Http for Fake {
    fn get_json(&mut self, url: &str, _: Option<&str>) -> Fetch<(Value, Option<String>)> {
        self.asked.push(url.to_string());
        if let Some(left) = self.budget.as_mut() {
            if *left == 0 {
                self.mode = FakeMode::RateLimited;
            } else {
                *left -= 1;
            }
        }
        match self.mode {
            FakeMode::Offline => return Fetch::Failed,
            FakeMode::RateLimited => return Fetch::Skipped,
            FakeMode::Up => {}
        }
        if url.contains("/releases/tags/") {
            Fetch::Fresh((self.release.clone(), Some("r".into())))
        } else {
            Fetch::Fresh((self.repo.clone(), Some("p".into())))
        }
    }
    fn get_head(&mut self, url: &str, _: Option<&str>) -> Fetch<(String, Option<String>)> {
        self.asked.push(url.to_string());
        match self.css.get(url) {
            Some(css) => Fetch::Fresh((css.clone(), None)),
            None => Fetch::Failed,
        }
    }
    fn last_modified(&mut self, url: &str) -> Option<i64> {
        self.asked.push(url.to_string());
        None
    }
    fn limited(&self) -> bool {
        self.mode == FakeMode::RateLimited
    }
}

fn github_store(names: &[&str]) -> ListedStore {
    let base = ["https://git", "hub.com/o/r/releases/download/latest"].concat();
    ListedStore {
        url: base.clone(),
        plugins: names
            .iter()
            .map(|n| Listed {
                url: format!("{base}/{n}"),
                file: format!("{n}.mjs"),
                hash: Some("h".into()),
            })
            .collect(),
    }
}

fn fake(mode: FakeMode, counts: &[(&str, u64)]) -> Fake {
    let assets: Vec<Value> = counts
        .iter()
        .map(|(n, c)| {
            json!({"name": format!("{n}.mjs"), "updated_at": "2026-09-30T00:00:00Z", "download_count": c})
        })
        .collect();
    Fake {
        release: json!({"published_at": "2026-09-30T00:00:00Z", "assets": assets}),
        repo: json!({"created_at": "2025-01-01T00:00:00Z", "pushed_at": "2026-09-30T00:00:00Z", "stargazers_count": 7}),
        mode,
        asked: Vec::new(),
        budget: None,
        css: HashMap::new(),
    }
}

/// A first refresh needs more requests than GitHub answers an address
/// without a sign-in in an hour. The releases (dates) come
/// first, so every store shows them even when the limit runs out on the
/// themes or the repositories (7.10.0 asked for the themes first, and most
/// stores showed nothing).
#[test]
fn every_store_gets_its_dates_before_the_rate_limit_runs_out() {
    let now = parse_rfc3339("2026-10-01T00:00:00Z").unwrap();
    let stores: Vec<ListedStore> = (0..25)
        .map(|i| {
            let mut store = github_store(&["p"]);
            store.url = store.url.replace("/o/r/", &format!("/o{i}/r/"));
            for p in &mut store.plugins {
                p.url = format!("{}/p", store.url);
            }
            store
        })
        .collect();
    let mut net = fake(FakeMode::Up, &[("p", 42)]);
    net.budget = Some(stores.len());
    let mut cache = Cache::default();
    assert!(!fetch_all_with(&mut net, &mut cache, &stores, now, &|| {}));
    let index = index_of(&cache);
    for store in &stores {
        assert!(
            index.dates(&store.plugins[0].url).updated.is_some(),
            "{} has no dates",
            store.url
        );
    }
}

#[test]
fn a_store_ranks_its_plugins_without_a_request_per_plugin() {
    let now = parse_rfc3339("2026-10-01T00:00:00Z").unwrap();
    let names: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let counts: Vec<(&str, u64)> = refs
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, i as u64))
        .collect();
    let mut net = fake(FakeMode::Up, &counts);
    let mut cache = Cache::default();
    let store = github_store(&refs);
    assert!(fetch_both(&mut net, &mut cache, &store, now));
    assert_eq!(
        net.asked.len(),
        2,
        "the release and the repository: {:?}",
        net.asked
    );
    let index = index_of(&cache);
    // The repository's stars, shared by every plugin published from it.
    for p in &store.plugins {
        assert_eq!(index.dates(&p.url).stars, 7);
    }
}

#[test]
fn offline_or_rate_limited_keeps_the_last_stars() {
    let t0 = parse_rfc3339("2026-10-01T00:00:00Z").unwrap();
    let store = github_store(&["a", "b"]);
    let mut cache = Cache::default();
    let mut net = fake(FakeMode::Up, &[("a", 10), ("b", 500)]);
    assert!(fetch_both(&mut net, &mut cache, &store, t0));
    let before = index_of(&cache);
    assert_eq!(before.dates(&store.plugins[1].url).stars, 7);

    for mode in [FakeMode::Offline, FakeMode::RateLimited] {
        let t1 = t0 + 5 * 3600;
        let mut down = fake(mode, &[]);
        assert!(
            !fetch_both(&mut down, &mut cache, &store, t1),
            "reported incomplete"
        );
        let after = index_of(&cache);
        assert_eq!(
            after.plugins, before.plugins,
            "the cache still ranks the plugins"
        );
    }
    // The whole refresh is incomplete too, so it is retried soon and the
    // finished-at time of the good cache is not replaced.
    let mut down = fake(FakeMode::Offline, &[]);
    cache.fetched_at = Some(t0);
    let done = fetch_all_with(&mut down, &mut cache, &[store], t0 + 5 * 3600, &|| {});
    assert!(!done);
    assert_eq!(cache.fetched_at, Some(t0));
}

#[test]
fn an_older_cache_file_with_download_counts_still_loads() {
    let old = r#"{"version":1,"fetched_at":5,"releases":{"u":{"etag":null,"data":{"published":1,"assets":{"A.json":2},"downloads":{"A.json":9}}}},
        "repos":{},"css":{},"theme_dates":{},"stores_seen":{},"plugins":{},
        "dates":{"s":{"s/A":{"added":1,"updated":2,"stars":3,"popularity":500,"downloads":40}}},
        "downloads":{"s/A":{"score":1.0,"at":1,"last":2,"built":1,"total":40}}}"#;
    let cache: Cache = serde_json::from_str(old).unwrap();
    let index = index_of(&cache);
    assert_eq!(index.dates("s/A").stars, 3);
    assert_eq!(index.dates("s/A").updated, Some(2));
}

#[test]
fn a_plugins_date_is_its_newest_own_file() {
    let r = ReleaseInfo {
        published: 500,
        assets: BTreeMap::from([
            ("Avatar.json".to_string(), 100),
            ("Avatar.mjs".to_string(), 300),
            ("Avatar2.mjs".to_string(), 900),
            ("store.json".to_string(), 800),
        ]),
    };
    assert_eq!(
        plugin_built(&r, "Avatar.mjs"),
        Some(300),
        "the bundle's, not the release's"
    );
    assert_eq!(plugin_built(&r, "Missing.mjs"), None);
}

/// Live check against GitHub (needs the internet and some of the hourly
/// API budget): every default store's plugins get stars and dates from the
/// real releases and repositories. Run by hand:
/// `cargo test --lib live_store_stars -- --ignored --nocapture`.
#[test]
#[ignore = "talks to GitHub"]
fn live_store_stars() {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Tidalfast-Luna-live-test")
        .build()
        .unwrap();
    let stores: Vec<ListedStore> = crate::luna::DEFAULT_STORES
        .iter()
        .chain(crate::luna::PINNED_STORES)
        .filter_map(|url| {
            let list: Value = client
                .get(format!("{url}/store.json"))
                .send()
                .ok()?
                .json()
                .ok()?;
            let plugins = list["plugins"]
                .as_array()?
                .iter()
                .filter_map(|f| f.as_str())
                .map(|file| Listed {
                    url: format!("{url}/{file}"),
                    file: file.to_string(),
                    hash: None,
                })
                .collect();
            Some(ListedStore {
                url: url.to_string(),
                plugins,
            })
        })
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut cache = Cache::default();
    let complete = fetch_all(&mut cache, &stores, now, &|| {});
    let index = index_of(&cache);
    let (mut counted, mut dated, mut total) = (0, 0, 0);
    for store in &stores {
        let with = store
            .plugins
            .iter()
            .filter(|p| index.dates(&p.url).stars > 0)
            .count();
        println!("{with}/{} starred  {}", store.plugins.len(), store.url);
        counted += with;
        dated += store
            .plugins
            .iter()
            .filter(|p| index.dates(&p.url).updated.is_some())
            .count();
        total += store.plugins.len();
    }
    println!("complete: {complete}; {counted}/{total} starred, {dated}/{total} dated");
    assert!(counted > 0 && dated > 0);
}

/// A theme's author may be `{ name, url, avatarUrl }`: the name, the link
/// and the picture are read (the object used to be ignored).
#[test]
fn a_theme_author_object_gives_its_link_and_picture() {
    let css = r#"/* { "name": "Cover", "author": { "name": "Nick Oates",
        "url": "https://github.com/NickOates", "avatarUrl": "https://github.com/NickOates.png" } } */
:root { --a: #111111; }"#;
    let t = theme_css(css);
    assert_eq!(t.author.as_deref(), Some("Nick Oates"));
    assert_eq!(
        t.author_url.as_deref(),
        Some("https://github.com/NickOates")
    );
    assert_eq!(
        t.author_avatar.as_deref(),
        Some("https://github.com/NickOates.png?size=80")
    );
}
