//! Plugin developer support: the real output of TidaLuna's plugin template
//! (`luna-template`, `pnpm build`) runs as a dev install, its errors point
//! at the TypeScript sources, and an API Tidalfast lacks is named in the log.
//!
//! `fixtures/template/` is the template's `dist/` as built; `template-crash/`
//! is the same build after adding a file that throws (`explode.ts`).

use super::*;

const MJS: &str = "luna.example.mjs";
const MAP: &str = "luna.example.mjs.map";
const JSON: &str = "luna.example.json";

fn template_files(crash: bool) -> [(&'static str, &'static [u8]); 4] {
    if crash {
        [
            (
                MJS,
                include_bytes!("fixtures/template-crash/luna.example.mjs"),
            ),
            (
                MAP,
                include_bytes!("fixtures/template-crash/luna.example.mjs.map"),
            ),
            (
                JSON,
                include_bytes!("fixtures/template-crash/luna.example.json"),
            ),
            ("store.json", include_bytes!("fixtures/template/store.json")),
        ]
    } else {
        [
            (MJS, include_bytes!("fixtures/template/luna.example.mjs")),
            (
                MAP,
                include_bytes!("fixtures/template/luna.example.mjs.map"),
            ),
            (JSON, include_bytes!("fixtures/template/luna.example.json")),
            ("store.json", include_bytes!("fixtures/template/store.json")),
        ]
    }
}

/// A copy of the template's `dist/` in a temp folder: (dist, data dir).
fn template_dist(tag: &str, crash: bool) -> (PathBuf, PathBuf) {
    let root = temp_dir(tag);
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    for (name, bytes) in template_files(crash) {
        std::fs::write(dist.join(name), bytes).unwrap();
    }
    (dist, root.join("data"))
}

fn start(data: &Path, host: MockHost) -> Luna {
    Luna::start(config(data), Arc::new(host)).unwrap()
}

fn log_text(p: &PluginInfo) -> String {
    p.logs
        .iter()
        .map(|l| format!("{:?} {}", l.level, l.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wait until the plugin's log has `needle`; the text of the log.
fn wait_for_log(luna: &Luna, name: &str, needle: &str) -> String {
    let s = wait_until(luna, 20, |s| {
        plugin(s, name).is_some_and(|p| log_text(p).contains(needle))
    });
    log_text(plugin(&s, name).unwrap())
}

#[test]
fn the_template_build_runs_as_a_dev_install_with_its_settings() {
    let (dist, data) = template_dist("tpl", false);
    let host = MockHost::default();
    host.snap.lock().unwrap().profile_name = Some("Ada".into());
    let luna = start(&data, host);
    luna.install_dev(dist.to_str().unwrap());
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "@luna/example").is_some_and(|p| p.status == PluginStatus::Running)
    });
    let p = plugin(&s, "@luna/example").unwrap();
    assert!(p.dev.is_some());
    assert!(p.has_settings, "{:?}", p.status);
    // The Plugins tab opens: the template's Settings component is drawn
    // like a store plugin's.
    luna.set_settings_open(true);
    wait_until(&luna, 10, |s| {
        plugin(s, "@luna/example").is_some_and(|p| p.settings.is_some())
    });
    let log = log_text(p);
    // The template greets `user.meta.profileName`.
    assert!(
        log.contains("Info [ExamplePlugin] Hello Ada from the Example plugin!"),
        "{log}"
    );
    assert!(log.contains("Source map loaded (2 source files)"), "{log}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dist.parent().unwrap());
}

#[test]
fn a_rebuild_reloads_the_plugin_without_restarting() {
    let (dist, data) = template_dist("rebuild", false);
    let luna = start(&data, MockHost::default());
    luna.install_dev(dist.to_str().unwrap());
    wait_until(&luna, 20, |s| {
        plugin(s, "@luna/example").is_some_and(|p| p.status == PluginStatus::Running)
    });
    let first = plugin(&luna.snapshot(), "@luna/example")
        .unwrap()
        .hash
        .clone();
    std::thread::sleep(Duration::from_millis(50));
    // `pnpm watch` writes the map, then the code, then the package.
    let rebuilt =
        include_str!("fixtures/template/luna.example.mjs").replace("Hello ", "Rebuilt hello ");
    std::fs::write(dist.join(MJS), rebuilt).unwrap();
    wait_for_log(&luna, "@luna/example", "Rebuilt hello");
    assert_ne!(
        plugin(&luna.snapshot(), "@luna/example").unwrap().hash,
        first
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dist.parent().unwrap());
}

#[test]
fn errors_point_at_the_typescript_sources() {
    let (dist, data) = template_dist("crash", true);
    let luna = start(&data, MockHost::default());
    luna.install_dev(dist.to_str().unwrap());
    let log = wait_for_log(&luna, "@luna/example", "Example plugin crashed on purpose");
    assert!(
        log.contains("plugins/Example/src/explode.ts:2:"),
        "the throw is at explode.ts line 2: {log}"
    );
    assert!(
        log.contains("plugins/Example/src/index.ts:"),
        "the caller is in index.ts: {log}"
    );
    assert!(
        !log.contains("plugin:%40luna%2Fexample@"),
        "no frame is left pointing into the built file: {log}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dist.parent().unwrap());
}

#[test]
fn the_source_map_is_kept_across_a_restart_and_dropped_with_the_plugin() {
    let (dist, data) = template_dist("restart", true);
    let luna = start(&data, MockHost::default());
    luna.install_dev(dist.to_str().unwrap());
    wait_for_log(&luna, "@luna/example", "plugins/Example/src/explode.ts");
    luna.shutdown();
    // A new run loads the saved code again: its errors are mapped too.
    let luna = start(&data, MockHost::default());
    let log = wait_for_log(&luna, "@luna/example", "Example plugin crashed on purpose");
    assert!(log.contains("plugins/Example/src/explode.ts:2:"), "{log}");
    luna.uninstall("@luna/example");
    wait_until(&luna, 10, |s| plugin(s, "@luna/example").is_none());
    let code = data.join("code");
    let left: Vec<_> = std::fs::read_dir(&code)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "uninstall leaves no code or map: {left:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dist.parent().unwrap());
}

#[test]
fn a_build_without_a_map_stops_using_the_old_one() {
    let (dist, data) = template_dist("nomap", true);
    let luna = start(&data, MockHost::default());
    luna.install_dev(dist.to_str().unwrap());
    wait_for_log(&luna, "@luna/example", "plugins/Example/src/explode.ts");
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_file(dist.join(MAP)).unwrap();
    let rebuilt = include_str!("fixtures/template-crash/luna.example.mjs")
        .replace("crashed", "crashed again")
        .replace("//# sourceMappingURL=luna.example.mjs.map", "");
    std::fs::write(dist.join(MJS), rebuilt).unwrap();
    let log = wait_for_log(&luna, "@luna/example", "crashed again on purpose");
    let after = log.split("crashed again on purpose").nth(1).unwrap_or("");
    assert!(
        after.contains("plugin:%40luna%2Fexample@") && !after.contains("explode.ts"),
        "an old map must not name sources for a different build: {after}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dist.parent().unwrap());
}

#[test]
fn a_dev_server_install_reads_the_map_over_http() {
    let files: Files = Arc::default();
    for (name, bytes) in template_files(true) {
        files
            .lock()
            .unwrap()
            .insert(format!("/{name}"), bytes.to_vec());
    }
    let base = serve(files);
    let dir = temp_dir("server");
    let luna = start(&dir, MockHost::default());
    // `pnpm watch` serves ./dist on port 3000; this is the same shape.
    luna.install_dev(&format!("{base}/luna.example"));
    let log = wait_for_log(&luna, "@luna/example", "Example plugin crashed on purpose");
    assert!(log.contains("plugins/Example/src/explode.ts:2:"), "{log}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

const PROBE: &str = r#"
var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
core.findModuleProperty((k) => k === "nothingHere");
lib.redux.actions["some/UNKNOWN_ACTION"]({ a: 1 });
lib.redux.actions["some/UNKNOWN_ACTION"]({ a: 2 });
lib.notARealThing.go();
Promise.reject(new Error("left unhandled"));
"#;

#[test]
fn unsupported_apis_are_named_in_the_log() {
    let dir = temp_dir("probe");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Probe.mjs"), PROBE).unwrap();
    let luna = start(&dir.join("data"), MockHost::default());
    luna.install_dev(dir.to_str().unwrap());
    let log = wait_for_log(&luna, "Probe", "left unhandled");
    // A TIDAL web module that does not exist here.
    assert!(
        log.contains("Warn [@luna/core] findModuleProperty No module matches selector"),
        "{log}"
    );
    // A Redux action only TIDAL's own app handles: named, once.
    let hits = log
        .matches("Redux action some/UNKNOWN_ACTION is not an action type TIDAL has")
        .count();
    assert_eq!(hits, 1, "{log}");
    assert!(
        log.contains("Warn Redux action some/UNKNOWN_ACTION"),
        "{log}"
    );
    // An export of @luna/lib the shim lacks.
    assert!(
        log.contains("Warn @luna/lib.notARealThing is not available in Tidalfast's Luna"),
        "{log}"
    );
    // A promise nobody handled is an error for a plugin under development.
    assert!(
        log.contains("Error Unhandled promise rejection: left unhandled"),
        "{log}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

const REJECTIONS: &str = r#"
export const unloads = new Set();
async function f() { throw new Error("awaited at once"); }
try { await f(); } catch {}
const p = Promise.reject(new Error("caught right after"));
p.catch(() => {});
setTimeout(() => {
	Promise.reject(new Error("left unhandled"));
}, 50);
"#;

/// A rejection handled before the microtask queue is drained is not an
/// unhandled one (as in a browser): NoBuffer logged "Unhandled promise
/// rejection" beside the error its own handler had caught.
#[test]
fn a_rejection_handled_in_the_same_turn_is_not_reported() {
    let dir = temp_dir("rejections");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Probe.mjs"), REJECTIONS).unwrap();
    let luna = start(&dir.join("data"), MockHost::default());
    luna.install_dev(dir.to_str().unwrap());
    let log = wait_for_log(&luna, "Probe", "left unhandled");
    assert!(
        log.contains("Unhandled promise rejection: left unhandled"),
        "{log}"
    );
    assert!(!log.contains("awaited at once"), "{log}");
    assert!(!log.contains("caught right after"), "{log}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
