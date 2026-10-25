//! Luna API round 3: the rows `docs/_reference/luna-api-coverage.md` still
//! listed as partial or missing after round 2. Each test that runs a plugin
//! uses the catalogue host of `coverage_tests`.

use std::time::Duration;

use serde_json::{Value, json};

use super::coverage_tests::{run_catalogue, wait_for};
use super::redux::{ActionOutcome, action_outcome, bookkeeping, is_tidal_action};
use super::*;

/// Every action type of TIDAL's desktop app (`action-types.txt`) either does
/// something in Tidalfast or has a reason for doing nothing; the reason is
/// what the plugin's log shows. A plugin's dispatch is never dropped
/// silently.
#[test]
fn every_tidal_action_acts_or_says_why_it_does_not() {
    let snap = PlaybackSnapshot {
        queue: Default::default(),
        track_lists: [(
            "album/3".to_string(),
            std::sync::Arc::new(vec!["1".to_string(), "2".to_string()]),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    // A payload rich enough for every handled action, and the two bare
    // shapes TIDAL's own seek, volume and index actions take.
    let rich = json!({
        "mediaItemIds": [1, 2], "mediaItemId": 1, "itemId": 1, "items": [{ "id": 1 }],
        "index": 1, "fromIndex": 0, "toIndex": 1, "uid": "u", "trackListName": "album/3",
        "playlistUUID": "p", "volume": 5, "searchPhrase": "a", "pathname": "/home",
        "elements": [{ "mediaItemId": 1 }], "currentIndex": 0, "message": "hi",
    });
    let mut acting = 0;
    let mut explained = 0;
    for kind in include_str!("action-types.txt").lines() {
        assert!(is_tidal_action(kind), "{kind}");
        let mut outcomes = [
            action_outcome(kind, &rich, &snap),
            action_outcome(kind, &json!(5), &snap),
            action_outcome(kind, &Value::Null, &snap),
        ];
        // No known action may be `Unknown` for every payload: that is a
        // dispatch with no line in the log.
        assert!(
            outcomes.iter().any(|o| *o != ActionOutcome::Unknown),
            "{kind} does nothing and says nothing"
        );
        match &mut outcomes[0] {
            ActionOutcome::Bookkeeping(why) => {
                assert!(!why.is_empty(), "{kind}");
                assert_eq!(bookkeeping(kind), Some(*why), "{kind}");
                explained += 1;
            }
            _ => acting += 1,
        }
    }
    eprintln!("tidal actions: {acting} act, {explained} explained");
    // The split is the table of the coverage page; a drift here means the
    // page's counts need a look.
    assert!(acting >= 50, "{acting} actions act");
    assert!(explained >= 400, "{explained} actions have a reason");
    // A name TIDAL does not have is not excused.
    assert!(!is_tidal_action("route/NOT_A_THING"));
    assert_eq!(bookkeeping("route/NOT_A_THING"), None);
    assert_eq!(
        action_outcome("route/NOT_A_THING", &rich, &snap),
        ActionOutcome::Unknown
    );
    // New: native fullscreen, minimize and play-by-id act.
    for (kind, payload, want) in [
        (
            "view/ENTER_NATIVE_FULLSCREEN",
            json!({}),
            HostCommand::SetFullscreen(true),
        ),
        (
            "view/LEAVE_NATIVE_FULLSCREEN",
            json!({}),
            HostCommand::SetFullscreen(false),
        ),
        ("view/MINIMIZE", json!({}), HostCommand::Minimize),
        (
            "content/FETCH_AND_PLAY_MEDIA_ITEM",
            json!({ "mediaItemId": 7 }),
            HostCommand::PlayNow(vec!["7".to_string()]),
        ),
    ] {
        assert_eq!(
            action_outcome(kind, &payload, &snap),
            ActionOutcome::Commands(vec![want]),
            "{kind}"
        );
    }
}

/// The plugin's log says why a dispatched action did nothing, at debug for
/// a store plugin, as a warning once for a plugin under development.
#[test]
fn a_dispatch_that_does_nothing_leaves_a_line_in_the_plugins_log() {
    let (r, _host) = run_catalogue(
        r##"
const d = (type, payload) => lib.redux.store.dispatch({ type, payload });
d("route/LOADER_DATA__ALBUM", {});
d("settings/SET_AUTOPLAY", true);
d("made/UP_ACTION", {});
d("playQueue/MOVE_TRACK", {});
d("search/SEARCH_COMMIT", { searchPhrase: "  " });
R.ready = true;
await new Promise((r) => setTimeout(r, 600));
R.done = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            std::thread::sleep(Duration::from_millis(300));
            let s = luna.snapshot();
            let p = s.plugins.iter().find(|p| p.name == "Compat").unwrap();
            let logs: Vec<&str> = p.logs.iter().map(|l| l.message.as_str()).collect();
            let has = |needle: &str| logs.iter().any(|l| l.contains(needle));
            assert!(
                has("Redux action route/LOADER_DATA__ALBUM does nothing in Tidalfast: "),
                "{logs:?}"
            );
            assert!(
                has(
                    "Redux action settings/SET_AUTOPLAY does nothing in Tidalfast: Tidalfast's settings belong to the user"
                ),
                "{logs:?}"
            );
            assert!(
                has("Redux action made/UP_ACTION is not an action type TIDAL has"),
                "{logs:?}"
            );
            assert!(
                has("Redux action playQueue/MOVE_TRACK was not understood"),
                "{logs:?}"
            );
            assert!(
                has("Redux action search/SEARCH_COMMIT: the payload named nothing to act on"),
                "{logs:?}"
            );
        },
    );
    assert_eq!(r["done"], true, "{r}");
}

/// A DNS server on the loopback with canned answers for `example.org`:
/// A 192.0.2.1 (ttl 60), MX, TXT, SRV, PTR for any `*.arpa`, ANY (A and
/// TXT), SOA, CAA; `nope.*` does not exist, every other type is empty.
fn fake_dns() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            let q = &buf[..n];
            let qname_end = 12 + q[12..].iter().position(|&b| b == 0).unwrap() + 1;
            let qname = &q[12..qname_end];
            let qtype = u16::from_be_bytes([q[qname_end], q[qname_end + 1]]);
            let rr = |t: u16, ttl: u32, data: &[u8]| {
                let mut a = vec![0xc0, 12];
                a.extend_from_slice(&t.to_be_bytes());
                a.extend_from_slice(&[0, 1]);
                a.extend_from_slice(&ttl.to_be_bytes());
                a.extend_from_slice(&(data.len() as u16).to_be_bytes());
                a.extend_from_slice(data);
                a
            };
            let mut mx = vec![0, 10];
            mx.extend_from_slice(b"\x04mail\xc0\x0c");
            let mut srv = vec![0, 1, 0, 2, 0x1f, 0x90];
            srv.extend_from_slice(b"\x03srv\xc0\x0c");
            let mut soa = b"\x02ns\xc0\x0c\x04host\xc0\x0c".to_vec();
            for n in [7u32, 8, 9, 10, 11] {
                soa.extend_from_slice(&n.to_be_bytes());
            }
            let caa = [&[0u8, 5][..], b"issue", b"ca.test"].concat();
            let nope = qname.starts_with(b"\x04nope");
            let answers: Vec<Vec<u8>> = match qtype {
                _ if nope => vec![],
                1 => vec![rr(1, 60, &[192, 0, 2, 1])],
                15 => vec![rr(15, 300, &mx)],
                16 => vec![rr(16, 10, b"\x06v=spf1\x01x")],
                33 => vec![rr(33, 10, &srv)],
                12 => vec![rr(12, 10, b"\x04host\x04test\x00")],
                6 => vec![rr(6, 10, &soa)],
                257 => vec![rr(257, 10, &caa)],
                255 => vec![rr(1, 60, &[192, 0, 2, 1]), rr(16, 10, b"\x01t")],
                _ => vec![],
            };
            let mut r = q[..2].to_vec();
            r.extend_from_slice(&[0x81, 0x80 | if nope { 3 } else { 0 }, 0, 1]);
            r.extend_from_slice(&(answers.len() as u16).to_be_bytes());
            r.extend_from_slice(&[0, 0, 0, 0]);
            r.extend_from_slice(qname);
            r.extend_from_slice(&q[qname_end..qname_end + 4]);
            for a in &answers {
                r.extend_from_slice(a);
            }
            let _ = socket.send_to(&r, from);
        }
    });
    port
}

/// `dns.resolve*` and `reverse` ask a DNS server through the host: records
/// of every type come back in Node's shapes, errors carry c-ares' codes.
#[test]
fn dns_record_types_are_asked_of_a_dns_server_by_the_host() {
    let port = fake_dns();
    let body = r##"
const n = await native("Compat/dnsq.native.ts", `"use strict";
const dns = require("dns");
const code = async (p) => { try { await p; return "resolved"; } catch (e) { return [e.code, e.syscall, e.hostname].join("|"); } };
exports.go = async () => {
	const out = {};
	const server = "127.0.0.1:@PORT@";
	dns.setServers([server]);
	out.servers = JSON.stringify(dns.getServers());
	const p = dns.promises;
	out.a = JSON.stringify(await p.resolve4("example.org", { ttl: true }));
	out.a2 = JSON.stringify(await p.resolve4("example.org"));
	out.mx = JSON.stringify(await p.resolveMx("example.org"));
	out.txt = JSON.stringify(await p.resolveTxt("example.org"));
	out.srv = JSON.stringify(await p.resolveSrv("_x._tcp.example.org"));
	out.soa = JSON.stringify(await p.resolveSoa("example.org"));
	out.caa = JSON.stringify(await p.resolveCaa("example.org"));
	out.ptr = JSON.stringify(await p.reverse("192.0.2.9"));
	out.any = JSON.stringify(await p.resolveAny("example.org"));
	out.viaResolve = JSON.stringify(await p.resolve("example.org", "MX"));
	out.missing = await code(p.resolve4("nope.example.org"));
	out.nodata = await code(p.resolveNs("example.org"));
	out.badType = await code(p.resolve("example.org", "WAT"));
	// The callback Resolver asks the servers it was given.
	const r = new dns.Resolver();
	r.setServers([server]);
	out.cb = await new Promise((res) => r.resolveMx("example.org", (e, a) => res(e ? e.code : a[0].exchange)));
	// Something that is not an address is rejected like Node's.
	try { dns.setServers(["not an address"]); await p.resolveMx("example.org"); out.badServer = "accepted"; } catch (e) { out.badServer = "rejected"; }
	return JSON.stringify(out);
};`);
R.out = JSON.parse(await n("go"));
R.done = true;
"##
    .replace("@PORT@", &port.to_string());
    let (r, _host) = run_catalogue(&body, |_, _| {});
    let o = &r["out"];
    let v = |k: &str| -> Value { serde_json::from_str(o[k].as_str().unwrap()).unwrap() };
    assert_eq!(v("servers"), json!([format!("127.0.0.1:{port}")]), "{r}");
    assert_eq!(
        v("a"),
        json!([{ "address": "192.0.2.1", "ttl": 60 }]),
        "{r}"
    );
    assert_eq!(v("a2"), json!(["192.0.2.1"]), "{r}");
    assert_eq!(
        v("mx"),
        json!([{ "priority": 10, "exchange": "mail.example.org" }]),
        "{r}"
    );
    assert_eq!(v("txt"), json!([["v=spf1", "x"]]), "{r}");
    assert_eq!(
        v("srv"),
        json!([{ "priority": 1, "weight": 2, "port": 8080, "name": "srv._x._tcp.example.org" }]),
        "{r}"
    );
    let soa = v("soa");
    assert_eq!(soa["nsname"], "ns.example.org", "{r}");
    assert_eq!(soa["minttl"], 11, "{r}");
    assert_eq!(
        v("caa"),
        json!([{ "critical": 0, "issue": "ca.test" }]),
        "{r}"
    );
    assert_eq!(v("ptr"), json!(["host.test"]), "{r}");
    assert_eq!(
        v("any"),
        json!([
            { "type": "A", "address": "192.0.2.1", "ttl": 60 },
            { "type": "TXT", "entries": ["t"] }
        ]),
        "{r}"
    );
    assert_eq!(o["viaResolve"], o["mx"], "{r}");
    assert_eq!(o["missing"], "ENOTFOUND|queryA|nope.example.org", "{r}");
    assert_eq!(o["nodata"], "ENODATA|queryNs|example.org", "{r}");
    assert_eq!(o["badType"], "ERR_INVALID_ARG_VALUE||", "{r}");
    assert_eq!(o["cb"], "mail.example.org", "{r}");
    assert_eq!(o["badServer"], "rejected", "{r}");
}

/// What a plugin does to its menu element that the native row can show: its
/// text colour, and hiding it. (Tidal UI's "Luna Settings" entry is cyan.)
#[test]
fn context_menu_entries_take_the_colour_and_hidden_state_of_their_element() {
    let (r, _host) = run_catalogue(
        r##"
const coloured = lib.ContextMenu.addButton(unloads);
coloured.text = "Coloured";
const hidden = lib.ContextMenu.addButton(unloads);
hidden.text = "Hidden";
const plain = lib.ContextMenu.addButton(unloads);
plain.text = "Plain";
lib.ContextMenu.onMediaItem(unloads, async ({ contextMenu }) => {
	(await coloured.show(contextMenu)).style.color = "#31d8ff";
	(await hidden.show(contextMenu)).style.display = "none";
	await plain.show(contextMenu);
	R.done = true;
});
R.ready = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            luna.open_context_menu(ContextTarget::Tracks(vec!["1".into()]));
            wait_until(luna, 10, |s| !s.context_menu.is_empty());
            std::thread::sleep(Duration::from_millis(800));
            let s = luna.snapshot();
            assert_eq!(s.context_menu.len(), 3, "{:?}", s.context_menu);
            let by = |t: &str| s.context_menu.iter().find(|i| i.text == t).unwrap().clone();
            assert_eq!(by("Coloured").color.as_deref(), Some("#31d8ff"));
            assert!(!by("Coloured").hidden);
            assert!(by("Hidden").hidden);
            assert_eq!(by("Plain").color, None);
            assert!(!by("Plain").hidden);
            luna.click_context_menu(by("Plain").id);
        },
    );
    let _ = r;
}

/// `v8.getHeapStatistics` reports QuickJS's own accounting: bytes in use
/// grow with what the plugin holds and stay under the plugins' limit.
#[test]
fn v8_heap_statistics_follow_the_runtimes_memory() {
    let (r, _host) = run_catalogue(
        r##"
const n = await native("Compat/heap.native.ts", `"use strict";
const v8 = require("v8");
exports.go = async () => {
	const before = v8.getHeapStatistics();
	const keep = [];
	for (let i = 0; i < 20000; i++) keep.push({ i, s: "x".repeat(50) });
	const after = v8.getHeapStatistics();
	const spaces = v8.getHeapSpaceStatistics();
	return JSON.stringify({
		used: before.used_heap_size > 0,
		grew: after.used_heap_size > before.used_heap_size + 100000,
		limit: after.heap_size_limit > after.total_heap_size && after.total_available_size === after.heap_size_limit - after.total_heap_size,
		held: keep.length,
		spaces: spaces.length === 1 && spaces[0].space_used_size === spaces[0].space_used_size,
		code: v8.getHeapCodeStatistics().bytecode_and_metadata_size > 0,
	});
};`);
R.out = JSON.parse(await n("go"));
R.done = true;
"##,
        |_, _| {},
    );
    let o = &r["out"];
    assert_eq!(o["used"], true, "{r}");
    assert_eq!(o["grew"], true, "{r}");
    assert_eq!(o["limit"], true, "{r}");
    assert_eq!(o["spaces"], true, "{r}");
    assert_eq!(o["code"], true, "{r}");
}

/// A song's format for `withFormat` / `updateFormat` is what TIDAL's
/// playback info says of it (figures, and the codec and bit rate of the
/// manifest), asked by `updateFormat` itself as Luna asks it, not the
/// tier's assumption; the manifest and its addresses never reach the
/// plugin. A single file's bit rate is its size over the song's length
/// (Tidal Tags' "951kbps" column for a song not played yet).
#[test]
fn media_item_formats_use_the_playback_info_a_plugin_asked_for() {
    let (r, _host) = run_catalogue(
        r##"
const m = await lib.MediaItem.fromId(1);
// updateFormat asks TIDAL's playback info (Tidal Tags' auto populate).
R.tier = JSON.stringify(await m.updateFormat("HIGH"));
R.flac = JSON.stringify(await m.updateFormat("LOSSLESS"));
const info = await m.playbackInfo("HIGH");
R.info = JSON.stringify({ q: info.audioQuality, rate: info.sampleRate, hasManifest: "manifest" in info, stream: info.stream });
// A listener is told the figures TIDAL gave.
const heard = [];
m.withFormat(unloads, "HIGH", (f) => heard.push([f.sampleRate, f.codec, f.bitrate].join("/")));
R.heard = heard.join(",");
R.after = JSON.stringify(await m.updateFormat("HIGH", true));
R.done = true;
"##,
        |_, _| {},
    );
    let tier: Value = serde_json::from_str(r["tier"].as_str().unwrap()).unwrap();
    assert_eq!(
        (tier["sampleRate"].clone(), tier["bitrate"].clone()),
        (json!(48000), json!(222222)),
        "{r}"
    );
    let flac: Value = serde_json::from_str(r["flac"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            flac["sampleRate"].clone(),
            flac["bitDepth"].clone(),
            flac["bitrate"].clone()
        ),
        (json!(44100), json!(16), json!(951000)),
        "{r}"
    );
    let info: Value = serde_json::from_str(r["info"].as_str().unwrap()).unwrap();
    assert_eq!(
        info,
        json!({ "q": "HIGH", "rate": 48000, "hasManifest": false,
            "stream": { "mimeType": "audio/mp4", "codec": "aac", "bitrate": 222222, "encrypted": false } }),
        "{r}"
    );
    assert_eq!(r["heard"], "48000/aac/222222", "{r}");
    let after: Value = serde_json::from_str(r["after"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            after["sampleRate"].clone(),
            after["bitrate"].clone(),
            after["codec"].clone()
        ),
        (json!(48000), json!(222222), json!("aac")),
        "{r}"
    );
}

/// Installs plugin `Compat` (`body`) and serves the `others` (name, body)
/// next to it without installing them; returns `Compat`'s results.
fn run_with_others(
    body: &str,
    others: &[(&str, &str)],
    during: impl FnOnce(&Luna, &std::path::Path),
) -> Value {
    use super::compat_tests::{HEADER, results};
    let files: Files = Arc::default();
    for (name, code) in std::iter::once(&("Compat", body)).chain(others.iter()) {
        files.lock().unwrap().insert(
            format!("/{name}.json"),
            json!({ "name": name, "version": "1.0.0", "hash": format!("{name}1") })
                .to_string()
                .into_bytes(),
        );
        files.lock().unwrap().insert(
            format!("/{name}.mjs"),
            format!("{HEADER}{code}").into_bytes(),
        );
    }
    let base = serve(files);
    let dir = temp_dir("api3");
    let host = Arc::new(super::coverage_tests::CatalogueHost::default());
    let luna = Luna::start(config(&dir), host).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    during(&luna, &dir);
    wait_until(&luna, 40, |_| results(&dir)["done"] == json!(true));
    let r = results(&dir);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// `LunaPlugin#install` of a plugin that is not installed asks the user
/// (naming the asking plugin, the plugin and its address); the app installs
/// it itself when they agree, and the plugin's promise resolves when it is
/// loading. A "no" installs nothing and rejects.
#[test]
fn a_plugin_installs_another_only_when_the_user_agrees() {
    let r = run_with_others(
        r##"
const { LunaPlugin } = core;
const mine = LunaPlugin.getByName("Compat").url;
const at = (n) => mine.replace(/Compat$/, n);
const yes = await LunaPlugin.fromStorage({ url: at("Wanted"), package: { name: "Wanted" } });
const no = await LunaPlugin.fromStorage({ url: at("Refused"), package: { name: "Refused" } });
R.before = [yes.installed, no.installed].join(",");
R.ready = true;
// Asked second: the first answer is Install.
await yes.install();
R.installed = yes.installed && LunaPlugin.getByName("Wanted").installed;
R.exports = String(LunaPlugin.getByName("Wanted").enabled);
R.ready2 = true;
try { await no.install(); R.declined = "installed"; } catch (e) { R.declined = e.message; }
R.afterNo = String(no.installed);
R.done = true;
"##,
        &[
            ("Wanted", "R.wantedLoaded = true;"),
            ("Refused", "R.refusedLoaded = true;"),
        ],
        |luna, dir| {
            wait_for(luna, dir, "ready");
            let ask = |luna: &Luna| {
                let end = std::time::Instant::now() + Duration::from_secs(10);
                loop {
                    if let Some(b) = luna.pending_message_box() {
                        return b;
                    }
                    assert!(std::time::Instant::now() < end, "no install question");
                    std::thread::sleep(Duration::from_millis(20));
                }
            };
            let b = ask(luna);
            assert_eq!(b.title, "Install a plugin?");
            assert_eq!(b.message, "Compat wants to install Wanted.");
            assert!(b.detail.contains("/Wanted"), "{b:?}");
            assert_eq!(b.buttons, vec!["Install".to_string(), "Cancel".to_string()]);
            assert_eq!(b.default_id, 1, "Cancel is the default");
            luna.answer_message_box(b.id, Some(0));
            wait_for(luna, dir, "ready2");
            let b = ask(luna);
            assert_eq!(b.message, "Compat wants to install Refused.");
            luna.answer_message_box(b.id, Some(1));
        },
    );
    assert_eq!(r["before"], "false,false", "{r}");
    assert_eq!(r["installed"], true, "{r}");
    assert_eq!(r["exports"], "true", "{r}");
    assert_eq!(r["wantedLoaded"], true, "{r}");
    assert_eq!(r["declined"], "Installing Refused was not allowed", "{r}");
    assert_ne!(r["afterNo"], "true", "{r}");
    assert_eq!(r["refusedLoaded"], Value::Null, "{r}");
}

/// The theme the settings page gives MUI has every field a MUI component
/// reads, with TidaLuna's palette, typography and overrides.
#[test]
fn the_mui_theme_has_the_fields_components_read() {
    let (r, _host) = run_catalogue(
        r##"
const ui = luna.core.modules["@luna/ui"];
const t = ui.lunaMuiTheme;
R.palette = [t.palette.mode, t.palette.primary.main, t.palette.primary.contrastText, t.palette.secondary.main, t.palette.text.primary, t.palette.text.secondary, t.palette.grey[900], t.palette.error.main, t.palette.getContrastText("#000")].join("|");
R.type = [t.typography.fontSize, t.typography.h7.fontSize, t.typography.h9.fontSize, t.typography.h6.fontWeight, t.typography.pxToRem(24), t.typography.subtitle2.color].join("|");
R.layout = [t.spacing(2), t.spacing(1, 2), t.breakpoints.up("md"), t.breakpoints.down("sm"), t.breakpoints.between("sm", "lg"), t.shape.borderRadius, t.shadows.length, t.zIndex.modal].join("|");
R.motion = [t.transitions.create("color", { duration: t.transitions.duration.short }), t.transitions.create(["opacity", "transform"]), t.transitions.easing.easeOut].join("|");
// The overrides Luna's page sets, including the ones that read the theme.
const tab = t.components.MuiTab.styleOverrides.root({ theme: t });
R.over = [t.components.MuiPaper.styleOverrides.root.backgroundColor, t.components.MuiSwitch.styleOverrides.track.borderRadius, tab["&:hover:not(.Mui-selected)"].color, tab["&.Mui-selected"]["& .MuiTab-icon"].color, t.components.MuiTabs.styleOverrides.indicator({ theme: t }).backgroundColor].join("|");
R.apply = JSON.stringify([t.applyStyles("light", { a: 1 }), t.applyStyles("dark", { a: 1 })]);
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(
        r["palette"],
        "light|#eeeeee|rgba(0, 0, 0, 0.87)|#31d8ff|#eeeeee|#a7a7a9|#212121|#d32f2f|#fff",
        "{r}"
    );
    assert_eq!(r["type"], "14|1.2rem|1.05rem|500|1.5rem|#bdbdbd", "{r}");
    assert_eq!(
        r["layout"],
        "16px|8px 16px|@media (min-width:900px)|@media (max-width:599.95px)|@media (min-width:600px) and (max-width:1199.95px)|4|25|1300",
        "{r}"
    );
    assert_eq!(
        r["motion"],
        "color 250ms cubic-bezier(0.4, 0, 0.2, 1) 0ms|opacity 300ms cubic-bezier(0.4, 0, 0.2, 1) 0ms,transform 300ms cubic-bezier(0.4, 0, 0.2, 1) 0ms|cubic-bezier(0.0, 0, 0.2, 1)",
        "{r}"
    );
    assert_eq!(r["over"], "#212121|12|#eeeeee|#31d8ff|#eeeeee", "{r}");
    assert_eq!(r["apply"], "[{\"a\":1},{}]", "{r}");
}

/// QuickJS has no `Intl`, so `Number#toLocaleString` was `toString`:
/// Tidal Tags wrote "1530kb/s" where TIDAL's Chromium writes "1,530kb/s".
/// Thousands are grouped, at most three decimals are kept, and the
/// fraction options are honoured.
#[test]
fn numbers_print_with_chromiums_locale_grouping() {
    let (r, _host) = run_catalogue(
        r##"
R.list = [
	(1530).toLocaleString(),
	Math.floor(951000 / 1e3).toLocaleString(),
	(1234567.891).toLocaleString(),
	(-12345.5).toLocaleString(),
	(0.12345).toLocaleString(),
	(5).toLocaleString(undefined, { minimumFractionDigits: 2 }),
	(1234.5678).toLocaleString("en-US", { maximumFractionDigits: 1 }),
	(1234).toLocaleString("en-US", { useGrouping: false }),
	NaN.toLocaleString(),
].join("|");
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(
        r["list"], "1,530|951|1,234,567.891|-12,345.5|0.123|5.00|1,234.6|1234|NaN",
        "{r}"
    );
}

/// QuickJS ignores the options of a date's `toLocale*String`:
/// DownloadManager's taskbar clock (`{ hour: "2-digit", minute: "2-digit" }`)
/// showed "04:00:32 PM" with the seconds where TIDAL's Chromium (en-US)
/// shows "04:00 PM". The en-US forms of the options plugins use.
#[test]
fn dates_print_with_chromiums_locale_options() {
    let (r, _host) = run_catalogue(
        r##"
const d = new Date(Date.UTC(2026, 9, 21, 16, 0, 32));
const u = { timeZone: "UTC" };
R.list = [
	d.toLocaleTimeString([], { ...u, hour: "2-digit", minute: "2-digit" }),
	d.toLocaleDateString([], { ...u, day: "2-digit", month: "2-digit", year: "numeric" }),
	d.toLocaleTimeString("en-US", u),
	d.toLocaleDateString("en-US", u),
	d.toLocaleString("en-US", u),
	d.toLocaleDateString("en-US", { ...u, month: "long", day: "numeric", year: "numeric" }),
	d.toLocaleDateString("en-US", { ...u, weekday: "short", month: "short", day: "numeric" }),
	d.toLocaleTimeString("en-US", { ...u, hour12: false, hour: "2-digit", minute: "2-digit" }),
	new Date(Date.UTC(2026, 0, 5, 9, 7)).toLocaleTimeString([], { ...u, hour: "numeric", minute: "2-digit" }),
	new Date(NaN).toLocaleDateString(),
].join("|");
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(
        r["list"],
        "04:00 PM|10/21/2026|4:00:32 PM|10/21/2026|10/21/2026, 4:00:32 PM|October 21, 2026|Wed, Oct 21|16:00|9:07 AM|Invalid Date",
        "{r}"
    );
}
