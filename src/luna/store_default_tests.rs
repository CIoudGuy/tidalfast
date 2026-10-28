//! The store list is TidaLuna's, and what an earlier Tidalfast listed instead
//! of it (Inrixia's `latest` build where TidaLuna lists `dev`) moves over:
//! the saved store list, and plugins installed from exactly that address,
//! through the normal update check. Settings, storage, permissions and the
//! enabled state are kept by plugin name, so they stay.

use super::*;

/// TidaLuna 1.18.0-beta's `PluginStoreTab` list, in its order, copied from
/// its `addToStores("...")` calls (the `/store.json` ending is what the tab
/// strips before it saves one).
const TIDALUNA_STORES: &[&str] = &[
    "https://github.com/SuperslowJelly/TIDAL-Clear-Coat/releases/download/latest/store.json",
    "https://github.com/meowarex/TidalLuna-Plugins/releases/download/latest/store.json",
    "https://github.com/wont-stream/lunar/releases/download/dev/store.json",
    "https://github.com/jxnxsdev/luna-plugins/releases/download/latest/store.json",
    "https://github.com/espeon/luna-plugins/releases/download/latest/store.json",
    "https://github.com/Inrixia/luna-plugins/releases/download/dev/store.json",
    "https://github.com/Aztup/luna-plugins/releases/download/latest/store.json",
    "https://github.com/vMohammad24/luna-plugins/releases/download/latest/store.json",
    "https://github.com/MathDesigns/luna-plugins/releases/download/latest/store.json",
    "https://github.com/otomir23/luna-plugins/releases/download/latest/store.json",
    "https://github.com/dantraynor/lunaplugins/releases/download/latest/store.json",
    "https://github.com/Akasiek/tidaluna-plugins/releases/download/latest/store.json",
    "https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest/store.json",
    "https://github.com/Renskursa/tidaluna-plugins/releases/download/latest/store.json",
    "https://github.com/Henr1ES/TidalLunaPlugins/releases/download/latest/store.json",
    "https://github.com/SinnerK0N/tidaluna-plugins/releases/download/latest/store.json",
    "https://github.com/squadgazzz/luna-plugins/releases/download/latest/store.json",
    "https://github.com/minseokk7/luna-plugins/releases/download/latest/store.json",
    "https://github.com/FireWall-code/TidaLuna-Plugins/releases/download/latest/store.json",
    "https://github.com/FlazeIGuess/tidaluna-plugins/releases/download/latest/store.json",
    "https://github.com/seomin0610/luna-plugins/releases/download/latest/store.json",
    "https://github.com/visiuun/tidaluna-plugins/releases/download/latest/store.json",
    "https://github.com/Kisakay/luna-plugins/releases/download/latest/store.json",
    "https://github.com/BaeHyolyn/tidal-presence/releases/latest/download/store.json",
];

const OLD_INRIXIA: &str = "https://github.com/Inrixia/luna-plugins/releases/download/latest";
const NEW_INRIXIA: &str = "https://github.com/Inrixia/luna-plugins/releases/download/dev";

#[test]
fn the_default_stores_are_tidalunas_list_with_its_channels() {
    let want: Vec<&str> = TIDALUNA_STORES
        .iter()
        .map(|s| s.strip_suffix("/store.json").unwrap())
        .collect();
    assert_eq!(DEFAULT_STORES, want.as_slice());
    assert!(
        !DEFAULT_STORES.contains(&OLD_INRIXIA),
        "Inrixia's store is the dev build, as in TidaLuna"
    );
}

#[test]
fn only_an_address_that_is_exactly_a_retired_default_moves() {
    let retired: Vec<(String, String)> = RETIRED_DEFAULT_STORES
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    // The saved store.
    for old in [
        OLD_INRIXIA.to_string(),
        format!("{OLD_INRIXIA}/"),
        format!("{OLD_INRIXIA}/store.json"),
        OLD_INRIXIA.to_uppercase(),
    ] {
        assert_eq!(
            migrated_store(&retired, &old).as_deref(),
            Some(NEW_INRIXIA),
            "{old}"
        );
    }
    // A plugin installed from it.
    assert_eq!(
        migrated_plugin_url(&retired, &format!("{OLD_INRIXIA}/Tidal.Tags")).as_deref(),
        Some("https://github.com/Inrixia/luna-plugins/releases/download/dev/Tidal.Tags")
    );
    assert_eq!(
        migrated_plugin_url(&retired, &format!("{OLD_INRIXIA}/Tidal.Tags/")).as_deref(),
        None,
        "not a store plus a file"
    );
    // The user's own choices stay: other tags, forks, other hosts, other stores.
    for own in [
        NEW_INRIXIA,
        "https://github.com/Inrixia/luna-plugins/releases/download/v2",
        "https://github.com/forker/luna-plugins/releases/download/latest",
        "https://github.com/Inrixia/TidaLuna-Plugins/releases/download/latest",
        "http://github.com/Inrixia/luna-plugins/releases/download/latest",
        "https://github.com/Aztup/luna-plugins/releases/download/latest",
        "https://example.com/Inrixia/luna-plugins/releases/download/latest",
    ] {
        assert_eq!(migrated_store(&retired, own), None, "{own}");
        assert_eq!(
            migrated_plugin_url(&retired, &format!("{own}/Tidal.Tags")),
            None,
            "{own}"
        );
    }
    assert_eq!(
        migrated_plugin_url(
            &retired,
            "https://raw.githubusercontent.com/Inrixia/luna-plugins/latest/Tidal.Tags"
        ),
        None
    );
    assert_eq!(
        migrated_plugin_url(&[], &format!("{OLD_INRIXIA}/Tidal.Tags")),
        None
    );
}

/// Starts on a saved store list that has been given every default store.
fn start_with_stores(dir: &Path, stores: &[&str], defaults: bool) -> (Luna, LunaSnapshot) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("default-stores.json"),
        serde_json::to_string(DEFAULT_STORES).unwrap(),
    )
    .unwrap();
    start_with_list(dir, stores, defaults)
}

fn start_with_list(dir: &Path, stores: &[&str], defaults: bool) -> (Luna, LunaSnapshot) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("stores.json"),
        serde_json::to_string(&stores).unwrap(),
    )
    .unwrap();
    let mut cfg = config(dir);
    cfg.default_stores = defaults;
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| !s.stores.is_empty());
    (luna, s)
}

#[test]
fn a_saved_inrixia_latest_store_becomes_the_dev_store_in_place() {
    let dir = temp_dir("store-move");
    let mine = "https://example.invalid/mine";
    let fork = "https://github.com/Inrixia/luna-plugins/releases/download/v2";
    let owner = PINNED_STORES[0];
    let (luna, s) = start_with_stores(
        &dir,
        &[
            DEFAULT_STORES[0],
            &format!("{OLD_INRIXIA}/store.json"),
            mine,
            fork,
        ],
        true,
    );
    let want = [DEFAULT_STORES[0], NEW_INRIXIA, mine, fork, owner];
    assert_eq!(store_urls(&s), want, "moved where it stood, the rest kept");
    assert_eq!(saved_stores(&dir), want, "and saved");
    luna.shutdown();

    // A list that has both ends with the dev store once.
    let (luna, s) = start_with_stores(&dir, &[OLD_INRIXIA, mine, NEW_INRIXIA], true);
    assert_eq!(store_urls(&s), [NEW_INRIXIA, mine, owner]);
    luna.shutdown();

    // A list that is already TidaLuna's is not touched (nor reordered).
    let (luna, s) = start_with_stores(&dir, &[mine, NEW_INRIXIA, DEFAULT_STORES[0]], true);
    assert_eq!(
        store_urls(&s),
        [mine, NEW_INRIXIA, DEFAULT_STORES[0], owner]
    );
    luna.shutdown();

    // With the defaults off the list is the user's alone.
    let (luna, s) = start_with_stores(&dir, &[OLD_INRIXIA, mine], false);
    assert_eq!(store_urls(&s), [OLD_INRIXIA, mine]);
    assert_eq!(saved_stores(&dir), [OLD_INRIXIA, mine]);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store TidaLuna adds to its list reaches a list saved before, once, as
/// TidaLuna adds it on the next start; a default the user removed stays
/// removed, and a store at GitHub's latest-release address is a store.
#[test]
fn a_new_default_store_is_offered_once_to_a_saved_list() {
    let dir = temp_dir("store-offer");
    let mine = "https://example.invalid/mine";
    let owner = PINNED_STORES[0];
    let new = *DEFAULT_STORES.last().unwrap();
    assert!(new.ends_with("/releases/latest/download"));
    // A list saved by an earlier Tidalfast: TidaLuna 1.17.1-beta's stores
    // less one the user removed, and their own.
    let mut old: Vec<&str> = DEFAULT_STORES[1..23].to_vec();
    old.push(mine);
    let (luna, s) = start_with_list(&dir, &old, true);
    let mut want = old.clone();
    want.push(new);
    want.push(owner);
    assert_eq!(store_urls(&s), want, "the new store added after the user's");
    assert_eq!(saved_stores(&dir), want, "and saved");
    assert!(
        !store_urls(&s).iter().any(|u| u == DEFAULT_STORES[0]),
        "a default the user removed does not come back"
    );
    luna.remove_store(new);
    let s = wait_until(&luna, 10, |s| !store_urls(s).iter().any(|u| u == new));
    assert!(!store_urls(&s).iter().any(|u| u == new));
    luna.shutdown();
    // Removed, it stays removed on the next start.
    let after: Vec<String> = saved_stores(&dir);
    let after: Vec<&str> = after.iter().map(String::as_str).collect();
    let (luna, s) = start_with_list(&dir, &after, true);
    assert!(!store_urls(&s).iter().any(|u| u == new), "offered once");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

struct Moved {
    dir: PathBuf,
    base: String,
}

/// A server with a `latest` and a `dev` release of the stores, and a data
/// folder with plugins installed from `latest` (`retired_stores` points the
/// retired address at the server, as the real one is on the internet).
fn plugins_from_the_old_store(tag: &str) -> Moved {
    let files: Files = Arc::default();
    {
        let mut f = files.lock().unwrap();
        let mut put = |path: &str, v: Value| {
            f.insert(path.into(), v.to_string().into_bytes());
        };
        put(
            "/latest/Fixture.json",
            json!({ "name": "Fixture", "version": "1.0.0", "hash": "h-old" }),
        );
        put(
            "/dev/Fixture.json",
            json!({ "name": "Fixture", "version": "2.0.0", "hash": "h-new" }),
        );
        // Another plugin lives at that name in the dev store.
        put(
            "/dev/Stranger.json",
            json!({ "name": "Impostor", "version": "9.0.0", "hash": "h-x" }),
        );
        f.insert("/latest/Fixture.mjs".into(), fixture(1));
        f.insert("/dev/Fixture.mjs".into(), fixture(2));
        f.insert("/dev/Stranger.mjs".into(), fixture(9));
    }
    let base = serve(files);
    let dir = temp_dir(tag);
    std::fs::create_dir_all(dir.join("settings")).unwrap();
    let record = |name: &str, path: &str, enabled: bool| {
        json!({
            "name": name,
            "url": format!("{base}/{path}"),
            "enabled": enabled,
            "package": { "name": name, "version": "1.0.0", "hash": "h-old" },
        })
    };
    let mut fixture_record = record("Fixture", "latest/Fixture", false);
    fixture_record["programs"] = json!(["/usr/bin/mpv"]);
    fixture_record["account_writes"] = json!(true);
    std::fs::write(
        dir.join("plugins.json"),
        json!([
            fixture_record,
            record("Stranger", "latest/Stranger", false),
            record("Gone", "latest/Gone", false),
        ])
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("settings").join("Fixture.json"),
        json!({ "Fixture": { "imported": 5, "mine": "kept" } }).to_string(),
    )
    .unwrap();
    Moved { dir, base }
}

fn start_against(m: &Moved, defaults: bool) -> Luna {
    let mut cfg = config(&m.dir);
    cfg.default_stores = defaults;
    cfg.retired_stores = vec![(format!("{}/latest", m.base), format!("{}/dev", m.base))];
    Luna::start(cfg, Arc::new(MockHost::default())).unwrap()
}

fn saved_record(dir: &Path, name: &str) -> Value {
    let all = super::storage::read_json(&dir.join("plugins.json")).unwrap();
    all.as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .cloned()
        .unwrap()
}

#[test]
fn a_plugin_from_the_retired_store_updates_from_tidalunas_through_the_update_check() {
    let m = plugins_from_the_old_store("store-plugin-move");
    let luna = start_against(&m, true);
    luna.check_updates();
    let s = wait_until(&luna, 30, |s| {
        plugin(s, "Fixture").is_some_and(|p| p.hash.as_deref() == Some("h-new"))
            && plugin(s, "Stranger")
                .is_some_and(|p| p.logs.iter().any(|l| l.message.contains("another plugin")))
            && plugin(s, "Gone").is_some_and(|p| {
                p.logs
                    .iter()
                    .any(|l| l.message.contains("Update check failed"))
            })
    });
    // Moved: new address, new build, and nothing else of the plugin's changed.
    let p = plugin(&s, "Fixture").unwrap();
    assert_eq!(p.url, format!("{}/dev/Fixture", m.base));
    assert_eq!(p.version.as_deref(), Some("2.0.0"));
    assert!(!p.enabled, "a disabled plugin stays disabled");
    assert_eq!(p.programs, ["/usr/bin/mpv"]);
    assert!(p.account_writes);
    let saved = saved_record(&m.dir, "Fixture");
    assert_eq!(saved["url"], format!("{}/dev/Fixture", m.base));
    assert_eq!(saved["enabled"], false);
    assert_eq!(saved["package"]["hash"], "h-new");
    let settings = super::storage::read_json(&m.dir.join("settings").join("Fixture.json")).unwrap();
    assert_eq!(
        settings["Fixture"],
        json!({ "imported": 5, "mine": "kept" })
    );
    assert_eq!(s.plugins.len(), 3, "no copy was added or removed");
    // The dev store answers with another plugin under this name, and has
    // nothing for the third: both stay where they were installed from.
    assert_eq!(
        plugin(&s, "Stranger").unwrap().url,
        format!("{}/latest/Stranger", m.base)
    );
    assert_eq!(
        plugin(&s, "Gone").unwrap().url,
        format!("{}/latest/Gone", m.base)
    );
    assert_eq!(
        plugin(&s, "Stranger").unwrap().hash.as_deref(),
        Some("h-old")
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&m.dir);
}

#[test]
fn an_enabled_plugin_stays_enabled_and_runs_the_new_build() {
    let m = plugins_from_the_old_store("store-plugin-enabled");
    let all = super::storage::read_json(&m.dir.join("plugins.json")).unwrap();
    let mut all = all.as_array().unwrap().clone();
    all[0]["enabled"] = json!(true);
    std::fs::write(m.dir.join("plugins.json"), json!(all).to_string()).unwrap();
    // The build installed from `latest`, which is what runs until the update.
    let code = super::storage::Storage::new(m.dir.clone()).code_path("Fixture");
    std::fs::create_dir_all(code.parent().unwrap()).unwrap();
    std::fs::write(code, fixture(1)).unwrap();
    let luna = start_against(&m, true);
    luna.check_updates();
    let s = wait_until(&luna, 30, |s| {
        plugin(s, "Fixture").is_some_and(|p| {
            p.hash.as_deref() == Some("h-new") && p.status == PluginStatus::Running
        })
    });
    let p = plugin(&s, "Fixture").unwrap();
    assert!(p.enabled);
    assert_eq!(p.url, format!("{}/dev/Fixture", m.base));
    let settings = super::storage::read_json(&m.dir.join("settings").join("Fixture.json")).unwrap();
    assert_eq!(settings["Fixture"]["imported"], 5);
    assert_eq!(settings["Fixture"]["mine"], "kept");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&m.dir);
}

#[test]
fn without_the_default_stores_nothing_is_moved() {
    let m = plugins_from_the_old_store("store-plugin-stay");
    let luna = start_against(&m, false);
    luna.look_for_updates();
    // `latest` has no newer build, so nothing is found and nothing moves.
    std::thread::sleep(Duration::from_millis(1500));
    let s = wait_until(&luna, 10, |_| true);
    assert_eq!(
        plugin(&s, "Fixture").unwrap().url,
        format!("{}/latest/Fixture", m.base)
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&m.dir);
}
