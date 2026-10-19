//! Luna API round 2: the rows `docs/_reference/luna-api-coverage.md` listed as
//! partial or missing and that a plugin can be given without TIDAL's web app.
//! Each test runs a plugin on the catalogue host of `coverage_tests`.

use std::time::Duration;

use super::coverage_tests::{run_catalogue, wait_for};
use super::dom_anchors;
use super::*;

#[test]
fn messager_dispatches_message_actions_and_error_boxes_ask_the_user() {
    let (r, host) = run_catalogue(
        r##"
const { Messager } = core;
const lnative = luna.core.modules["@luna/lib.native"];
const seen = [];
// As in Luna, the interceptors of message/MESSAGE_* see what Messager sends
// (the sender's own too); returning true cancels the toast.
lib.redux.intercept("message/MESSAGE_INFO", unloads, (m) => { seen.push(["info", m.message, m.category, m.severity].join("|")); return m.message === "hidden"; });
lib.redux.intercept("message/MESSAGE_WARN", unloads, (m) => { seen.push(["warn", m.message, m.severity].join("|")); });
lib.redux.intercept("message/MESSAGE_ERROR", unloads, (m) => { seen.push(["error", m.message, m.severity].join("|")); });
Messager.Info("shown", { a: 1 });
Messager.Info("hidden");
Messager.Warn("careful");
Messager.Error(new Error("boom"));
R.seen = seen.join(";");
// showErrorBox is a modal with an OK button and returns at once.
R.boxReturn = String(lnative.showErrorBox("Oops", "It broke"));
R.ready = true;
await new Promise((r) => setTimeout(r, 100));
R.done = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            let end = std::time::Instant::now() + Duration::from_secs(10);
            let b = loop {
                if let Some(b) = luna.pending_message_box() {
                    break b;
                }
                assert!(
                    std::time::Instant::now() < end,
                    "showErrorBox opened no dialog"
                );
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!((b.title.as_str(), b.message.as_str()), ("Oops", "It broke"));
            assert_eq!(b.buttons, vec!["OK".to_string()]);
            luna.answer_message_box(b.id, Some(0));
        },
    );
    assert_eq!(
        r["seen"],
        "info|shown {\"a\":1}|OTHER|INFO;info|hidden|OTHER|INFO;warn|careful|MESSAGE_WARN;error|boom|ERROR",
        "{r}"
    );
    assert_eq!(r["boxReturn"], "undefined", "{r}");
    let notes = host.notes.lock().unwrap().clone();
    assert_eq!(
        notes,
        vec!["shown {\"a\":1}", "careful", "boom"],
        "an interceptor cancelled one message, the rest reached the toast"
    );
}

/// Luna's `Page.open()` pushes TIDAL's not-found route with the page's name;
/// while the app shows that route the page's root is in `main`, and any other
/// route takes it out.
#[test]
fn plugin_pages_open_as_routes_and_leave_with_the_route() {
    let navigated = |search: &str| LunaEvent::Action {
        kind: "router/NAVIGATED".into(),
        payload: json!({ "pathname": if search.is_empty() { "/" } else { "/not-found" }, "search": search, "hash": "" }),
    };
    let (r, host) = run_catalogue(
        r##"
const ui = luna.core.modules["@luna/ui"];
const React = luna.core.modules.react;
const page = ui.Page.register("MyPage", unloads, React.createElement("p", null, "Hello page"));
R.same = ui.Page.register("MyPage", unloads) === page;
R.rootId = page.rootId;
page.pageStyles.background = "rgb(1, 2, 3)";
page.open();
R.ready = true;
R.done = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            luna.sync_dom(dom_anchors::sample_tree());
            let has = |luna: &Luna| {
                luna.sync(Duration::from_secs(10));
                serde_json::to_string(&luna.dom_overlay().injected)
                    .unwrap()
                    .contains("Hello page")
            };
            let until = |luna: &Luna, want: bool| {
                let end = std::time::Instant::now() + Duration::from_secs(10);
                while has(luna) != want {
                    assert!(std::time::Instant::now() < end, "page present: {}", !want);
                    std::thread::sleep(Duration::from_millis(20));
                }
            };
            // The route is not open yet: the page is not in `main`.
            assert!(!has(luna));
            luna.event(navigated("?MyPage"));
            until(luna, true);
            // Another page's route does not show it; any other route removes it.
            luna.event(navigated("?Other"));
            until(luna, false);
            luna.event(navigated("?MyPage"));
            until(luna, true);
            luna.event(navigated(""));
            until(luna, false);
            // A name that is not a page does nothing (not even for names every object has).
            luna.event(navigated("?constructor"));
            luna.event(navigated("?MyPage"));
            until(luna, true);
        },
    );
    assert_eq!(r["same"], true, "{r}");
    assert_eq!(r["rootId"], "luna-page-MyPage", "{r}");
    let commands = host.commands.lock().unwrap().clone();
    assert!(
        commands
            .iter()
            .any(|c| c == "Navigate(\"/not-found?MyPage\")"),
        "{commands:?}"
    );
}

/// The element `ContextMenuButton.show()` returns is Luna's: the span of a
/// `contextmenu-item` row that takes the button's text and click, that the
/// plugin may style and re-word, and whose words the native menu shows.
#[test]
fn context_menu_button_elements_are_the_span_of_a_menu_row() {
    let (r, _host) = run_catalogue(
        r##"
const b = lib.ContextMenu.addButton(unloads);
b.text = "Start";
lib.ContextMenu.onMediaItem(unloads, async ({ contextMenu }) => {
	await b.show(contextMenu);
	const e = b.elem;
	R.shape = [e.tagName, e.parentElement?.tagName, e.parentElement?.parentElement?.getAttribute("data-type"), e.innerText].join("|");
	// Plugins mark progress on their element and re-word it.
	e.classList.add("download-button");
	e.style.setProperty("--progress", "50%");
	e.textContent = "Changed";
	R.styled = e.classList.contains("download-button") && e.style.getPropertyValue("--progress") === "50%";
});
b.onClick((ev) => { R.clicked = ev.target === b.elem; R.done = true; });
R.ready = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            luna.open_context_menu(ContextTarget::Tracks(vec!["1".into()]));
            let s = wait_until(luna, 10, |s| {
                s.context_menu.first().is_some_and(|i| i.text == "Changed")
            });
            luna.click_context_menu(s.context_menu[0].id);
        },
    );
    assert_eq!(r["shape"], "SPAN|BUTTON|contextmenu-item|Start", "{r}");
    assert_eq!(r["styled"], true, "{r}");
    assert_eq!(r["clicked"], true, "{r}");
}

#[test]
fn idb_store_serves_idb_keyval_over_the_stores_file() {
    let (r, _host) = run_catalogue(
        r##"
// The calls idb-keyval makes on a UseStore, as its source writes them.
const pr = (r) => new Promise((res, rej) => { r.oncomplete = r.onsuccess = () => res(r.result); r.onabort = r.onerror = () => rej(r.error); });
const kv = {
	get: (k, s) => s("readonly", (st) => pr(st.get(k))),
	set: (k, v, s) => s("readwrite", (st) => { st.put(v, k); return pr(st.transaction); }),
	update: (k, f, s) => s("readwrite", (st) => new Promise((res, rej) => { st.get(k).onsuccess = function () { try { st.put(f(this.result), k); res(pr(st.transaction)); } catch (e) { rej(e); } }; })),
	del: (k, s) => s("readwrite", (st) => { st.delete(k); return pr(st.transaction); }),
	clear: (s) => s("readwrite", (st) => { st.clear(); return pr(st.transaction); }),
	keys: (s) => s("readonly", (st) => pr(st.getAllKeys())),
	entries: (s) => s("readonly", (st) => Promise.all([pr(st.getAllKeys()), pr(st.getAll())]).then(([k, v]) => k.map((x, i) => [x, v[i]]))),
	getMany: (ks, s) => s("readonly", (st) => Promise.all(ks.map((k) => pr(st.get(k))))),
	setMany: (es, s) => s("readwrite", (st) => { es.forEach((e) => st.put(e[1], e[0])); return pr(st.transaction); }),
};
const rs = core.ReactiveStore.getStore("IdbTest");
const s = rs.idbStore;
await kv.set("a", { x: 1 }, s);
await kv.set("b", 2, s);
R.get = JSON.stringify(await kv.get("a", s));
R.missing = String(await kv.get("zzz", s));
await kv.update("b", (v) => v + 1, s);
R.upd = await kv.get("b", s);
// The same file as ReactiveStore's get/set.
R.shared = JSON.stringify([await rs.get("a"), await rs.get("b")]);
await rs.set("c", "from reactive");
R.fromRs = await kv.get("c", s);
// Values JSON cannot hold come back as they went in.
await kv.set("when", new Date(5000), s);
await kv.set("map", new Map([[1, 2]]), s);
await kv.set("u", undefined, s);
await kv.set("n", null, s);
const w = await kv.get("when", s);
R.rich = [w instanceof Date && w.getTime(), (await kv.get("map", s)).get(1), String(await kv.get("u", s)), String(await kv.get("n", s))].join("|");
// Keys of every type, in IndexedDB's order.
await kv.clear(s);
await kv.setMany([["str", 1], [10, 2], [2, 3], [new Date(7), 4], [[1, "a"], 5]], s);
R.order = (await kv.keys(s)).map((k) => (k instanceof Date ? "D" + k.getTime() : Array.isArray(k) ? "A" + k.join(",") : typeof k + ":" + k)).join(" ");
R.entries = JSON.stringify((await kv.entries(s)).map((e) => e[1]));
R.many = JSON.stringify(await kv.getMany([2, "str", "nope"], s));
await kv.del(10, s);
R.afterDel = (await kv.keys(s)).length;
// A cursor walks the keys, and ends with a null result.
const walked = [];
await s("readonly", (st) => { st.openCursor().onsuccess = function () { if (!this.result) return; const k = this.result.key; walked.push((k instanceof Date ? "D" : Array.isArray(k) ? "A" : k) + "=" + this.result.value); this.result.continue(); }; return pr(st.transaction); });
R.cursor = walked.join(",");
// An aborted transaction puts back what it changed.
await kv.clear(s);
await kv.set("keep", 1, s);
await s("readwrite", (st) => new Promise((res) => { st.transaction.onabort = res; st.put(2, "keep").onsuccess = () => { st.put(3, "other"); st.transaction.abort(); }; }));
R.rollback = [await kv.get("keep", s), String(await kv.get("other", s))].join(",");
// Read-only transactions refuse writes; a store holds 16 MB.
R.readonly = await s("readonly", (st) => st.put(1, "x")).then(() => "wrote", (e) => e.name);
R.quota = await kv.set("big", "x".repeat(17000000), s).then(() => "stored", (e) => e.name);
R.afterQuota = String(await kv.get("big", s));
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(r["get"], "{\"x\":1}", "{r}");
    assert_eq!(r["missing"], "undefined", "{r}");
    assert_eq!(r["upd"], 3, "{r}");
    assert_eq!(r["shared"], "[{\"x\":1},3]", "{r}");
    assert_eq!(r["fromRs"], "from reactive", "{r}");
    assert_eq!(r["rich"], "5000|2|undefined|null", "{r}");
    assert_eq!(r["order"], "number:2 number:10 D7 string:str A1,a", "{r}");
    assert_eq!(r["entries"], "[3,2,4,1,5]", "{r}");
    assert_eq!(r["many"], "[3,1,null]", "{r}");
    assert_eq!(r["afterDel"], 4, "{r}");
    assert_eq!(r["cursor"], "2=3,D=4,str=1,A=5", "{r}");
    assert_eq!(r["rollback"], "1,undefined", "{r}");
    assert_eq!(r["readonly"], "ReadOnlyError", "{r}");
    assert_eq!(r["quota"], "QuotaExceededError", "{r}");
    assert_eq!(r["afterQuota"], "undefined", "{r}");
}

#[test]
fn dns_and_v8_answer_like_node() {
    let (r, _host) = run_catalogue(
        r##"
const n = await native("Compat/dnsv8.native.ts", `"use strict";
const dns = require("dns"), v8 = require("v8");
const code = async (p) => { try { await p; return "resolved"; } catch (e) { return e.code + "|" + e.syscall + "|" + e.hostname; } };
exports.go = async () => {
	const out = {};
	// Literals answer without a lookup, in callback style and as promises.
	out.literal = await new Promise((res) => dns.lookup("127.0.0.1", (e, a, f) => res([e, a, f].join(","))));
	out.literal6 = JSON.stringify(await dns.promises.lookup("::1", { all: true }));
	out.empty = JSON.stringify(await dns.promises.lookup(""));
	// A name goes to the resolver; a name that does not exist fails as Node's does.
	const all = await dns.promises.lookup("localhost", { all: true });
	out.localhost = all.length > 0 && all.every((a) => (a.family === 4 || a.family === 6) && typeof a.address === "string");
	out.v4only = (await dns.promises.lookup("localhost", 4)).family;
	out.missing = await code(dns.promises.lookup("no-such-host.invalid"));
	out.resolve4 = JSON.stringify(await dns.promises.resolve4("10.1.2.3", { ttl: true }));
	dns.setDefaultResultOrder("ipv4first");
	out.order = dns.getDefaultResultOrder();
	out.servers = Array.isArray(dns.getServers());
	out.sub = require("dns/promises") === dns.promises;
	out.cbMissing = await new Promise((res) => dns.lookup("no-such-host.invalid", (e) => res(e.code)));
	// v8: structured-clone round trips.
	const a = { n: undefined, nan: NaN, neg0: -0, big: 12345678901234567890n, d: new Date(86400000), re: /a+/gi, m: new Map([[1, { x: 1 }], ["k", [1, 2]]]), s: new Set([1, "a"]), buf: Buffer.from("hi"), u8: new Uint8Array([1, 2, 3]), err: new RangeError("bad"), arr: [1, , 3] };
	a.self = a;
	a.shared = a.m;
	const b = v8.deserialize(v8.serialize(a));
	out.rt = [b.n === undefined && "n" in b, Number.isNaN(b.nan), Object.is(b.neg0, -0), b.big === 12345678901234567890n, b.d.getTime(), String(b.re), b.m.get(1).x, b.m.get("k").join("+"), b.s.has("a"), Buffer.isBuffer(b.buf) && b.buf.toString(), Array.from(b.u8).join(""), b.err instanceof RangeError && b.err.message, b.arr.length + ":" + (1 in b.arr), b.self === b, b.shared === b.m].join("|");
	try { v8.serialize(() => 1); } catch (e) { out.fn = e.name; }
	try { v8.deserialize(Buffer.from("not ours")); out.bad = "read"; } catch (e) { out.bad = "rejected"; }
	out.heap = v8.getHeapStatistics().heap_size_limit > 0;
	try { v8.writeHeapSnapshot(); out.snap = "written"; } catch (e) { out.snap = "refused"; }
	return JSON.stringify(out);
};`);
R.out = JSON.parse(await n("go"));
R.done = true;
"##,
        |_, _| {},
    );
    let o = &r["out"];
    assert_eq!(o["literal"], ",127.0.0.1,4", "{r}");
    assert_eq!(o["literal6"], "[{\"address\":\"::1\",\"family\":6}]", "{r}");
    assert_eq!(o["empty"], "{\"address\":null,\"family\":4}", "{r}");
    assert_eq!(o["localhost"], true, "{r}");
    assert_eq!(o["v4only"], 4, "{r}");
    assert_eq!(
        o["missing"], "ENOTFOUND|getaddrinfo|no-such-host.invalid",
        "{r}"
    );
    assert_eq!(
        o["resolve4"], "[{\"address\":\"10.1.2.3\",\"ttl\":0}]",
        "{r}"
    );
    assert_eq!(o["order"], "ipv4first", "{r}");
    assert_eq!(o["servers"], true, "{r}");
    assert_eq!(o["sub"], true, "{r}");
    assert_eq!(o["cbMissing"], "ENOTFOUND", "{r}");
    assert_eq!(
        o["rt"], "true|true|true|true|86400000|/a+/gi|1|1+2|true|hi|123|bad|3:false|true|true",
        "{r}"
    );
    assert_eq!(o["fn"], "DataCloneError", "{r}");
    assert_eq!(o["bad"], "rejected", "{r}");
    assert_eq!(o["heap"], true, "{r}");
    assert_eq!(o["snap"], "refused", "{r}");
}
