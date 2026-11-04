//! DOM bridge tests: the host tree reconciled into the plugins' document,
//! mutation records, injected elements, events from the UI, scrolling,
//! animation frames and plugin CSS, each with a small inline plugin; plus
//! ignored network tests running real DOM plugins unmodified.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::dom::{DomEvent, DomOverlay, DomTarget, HostTree, Position};
use super::dom_anchors::{self, keys};
use super::*;

const HEADER: &str = r#"var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Dom.results", {});
"#;

fn dom_results(dir: &Path) -> Value {
    super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", super::storage::safe_name("Dom.results"))),
    )
    .and_then(|v| v.get("Dom.results").cloned())
    .unwrap_or(Value::Null)
}

/// Installs `body` as plugin `DomTest` and waits until it runs.
pub(super) fn start(body: &str) -> (Luna, Arc<MockHost>, PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/DomTest.json".into(),
        json!({ "name": "DomTest", "version": "1.0.0", "hash": "d1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/DomTest.mjs".into(),
        format!("{HEADER}{body}").into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("dom");
    let host = Arc::new(MockHost::default());
    host.snap.lock().unwrap().volume = 50.0;
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&format!("{base}/DomTest.mjs"));
    wait_until(&luna, 30, |s| {
        plugin(s, "DomTest").is_some_and(|p| p.status == PluginStatus::Running)
    });
    (luna, host, dir)
}

fn overlay_until(
    luna: &Luna,
    secs: u64,
    mut cond: impl FnMut(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let o = luna.dom_overlay();
        if cond(&o) {
            return o;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; overlay: {}\nlogs: {:#?}",
                serde_json::to_string_pretty(&*o).unwrap(),
                s.plugins
                    .iter()
                    .map(|p| (&p.name, &p.logs))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn results_until(
    luna: &Luna,
    dir: &Path,
    secs: u64,
    mut cond: impl FnMut(&Value) -> bool,
) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let out = dom_results(dir);
        if cond(&out) {
            return out;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; results: {out}\nruntime log: {:#?}\nplugin logs: {:#?}",
                s.runtime_log.iter().map(|l| &l.message).collect::<Vec<_>>(),
                s.plugins
                    .iter()
                    .map(|p| (
                        &p.name,
                        p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tree() -> HostTree {
    dom_anchors::sample_tree()
}

fn node<'a>(t: &'a mut HostTree, key: &str) -> &'a mut super::dom::HostNode {
    t.nodes.iter_mut().find(|n| n.key == key).unwrap()
}

pub(super) fn finish(luna: Luna, dir: PathBuf) {
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn host_tree_reconciles_with_mutation_records() {
    let (luna, _host, dir) = start(
        r##"
const recs = [];
new MutationObserver((list) => {
	for (const r of list) recs.push([r.type, r.attributeName ?? "", r.addedNodes.length, r.removedNodes.length, r.target.getAttribute?.("data-test") ?? r.target.parentNode?.getAttribute?.("data-test") ?? ""]);
	R.recs = recs.slice(-200);
	R.rows = document.querySelectorAll('[data-test="tracklist-row"][data-track-id]').length;
	R.slider = !!document.querySelector('div[class^="_sliderContainer"]');
	R.shell = document.querySelector("#wimp > div[data-market-theme] > div")?.className ?? null;
	R.title = document.querySelector('[data-test="footer-track-title"]')?.textContent ?? null;
	R.mine = !!document.querySelector("#mine");
	R.order = [...document.querySelector('[data-test="track-info"]')?.parentElement?.children ?? []].map((e) => e.getAttribute("data-test") ?? e.id ?? "");
}).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class"], characterData: true });
lib.redux.intercept("page/SET_PAGE_ID", unloads, (p) => (R.page = [p?.pageId ?? null, document.querySelectorAll('div[class^="_sliderContainer"]').length]));
lib.observe(unloads, "._utilityButtons_4d7aaf9", (el) => {
	// Called again when TIDAL's markup around it changes, as in Luna.
	if (el.querySelector("#mine")) return;
	const d = document.createElement("div");
	d.id = "mine";
	el.prepend(d);
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| {
        r["rows"] == json!(3) && r["mine"] == json!(true)
    });
    assert_eq!(r["slider"], json!(true));
    // TIDAL's class list exactly: themes match it with `[class="..."]`.
    assert_eq!(
        r["shell"],
        json!("has-footer-player is-desktop-app is-now-playing")
    );
    assert_eq!(r["title"], json!("Song One"));
    // The first page render is announced, with the document in place.
    let r = results_until(&luna, &dir, 10, |r| r["page"].is_array());
    assert_eq!(r["page"], json!(["album", 1]));
    // Changes: a class, a title text, a removed row.
    let mut t = tree();
    node(&mut t, keys::SHELL)
        .attrs
        .iter_mut()
        .find(|(k, _)| k == "class")
        .unwrap()
        .1
        .push_str(" is-fullscreen");
    let title = keys::TITLE_SPAN;
    node(&mut t, title).text = Some("Song Changed".into());
    t.nodes
        .retain(|n| !n.key.starts_with(&keys::row("album:55", 2)));
    luna.sync_dom(t.clone());
    let r = results_until(&luna, &dir, 10, |r| {
        r["rows"] == json!(2) && r["title"] == json!("Song Changed")
    });
    let recs = r["recs"].as_array().unwrap();
    let has = |ty: &str, attr: &str| recs.iter().any(|x| x[0] == ty && x[1] == attr);
    assert!(has("attributes", "class"), "{recs:?}");
    assert!(has("characterData", ""), "{recs:?}");
    assert!(
        recs.iter()
            .any(|x| x[0] == "childList" && x[3].as_u64() > Some(0)),
        "{recs:?}"
    );
    // The plugin's node survived the resync, at its anchor.
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::UTILITY_BUTTONS, Position::Prepend).count() == 1
    });
    assert_eq!(
        o.at(keys::UTILITY_BUTTONS, Position::Prepend)
            .next()
            .unwrap()
            .attrs,
        vec![("id".to_string(), "mine".to_string())]
    );
    // Re-sending the same tree records nothing new.
    let before = dom_results(&dir)["recs"].as_array().unwrap().len();
    luna.sync_dom(t);
    luna.sync(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    luna.sync(Duration::from_secs(5));
    assert_eq!(dom_results(&dir)["recs"].as_array().unwrap().len(), before);
    finish(luna, dir);
}

#[test]
fn injected_elements_report_their_anchor_and_position() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const slider = document.querySelector('div[class*="sliderContainer"]');
	const more = document.querySelector('div[class*="utilityContainer"]');
	const util = document.querySelector('div[class*="utilityButtons"]');
	if (!slider || !more || !util || document.querySelector(".t-before")) return;
	const mk = (cls, text) => { const d = document.createElement("button"); d.className = cls; d.textContent = text; return d; };
	slider.before(mk("t-before", "B"));
	slider.after(mk("t-after", "A"));
	more.prepend(mk("t-prepend", "P"));
	util.append(mk("t-append", "E"));
	const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
	svg.setAttribute("viewBox", "0 0 24 24");
	svg.innerHTML = '<path d="M0 0h24v24H0z"/>';
	util.lastChild.appendChild(svg);
	const fixed = mk("t-fixed", "F");
	Object.assign(fixed.style, { position: "fixed", bottom: "100px", right: "20px", backgroundColor: "red" });
	fixed.addEventListener("click", () => (R.clicked = (R.clicked ?? 0) + 1));
	document.body.appendChild(fixed);
	slider.querySelector("input")?.insertAdjacentHTML("afterend", '<span class="t-html">x<b>y</b></span>');
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.injected.len() >= 5);
    let class_at = |key: &str, pos: Position| -> Vec<String> {
        o.at(key, pos)
            .filter_map(|v| {
                v.attrs
                    .iter()
                    .find(|(k, _)| k == "class")
                    .map(|(_, c)| c.clone())
            })
            .collect()
    };
    let slider_prev = {
        // "before" the slider is "after" its previous host sibling, or a
        // prepend into the parent when it has none.
        let mut v = class_at(keys::UTILITY_BUTTONS, Position::Prepend);
        for n in &tree().nodes {
            if n.parent.as_deref() == Some(keys::UTILITY_BUTTONS) {
                v.extend(class_at(&n.key, Position::After));
            }
        }
        v
    };
    assert!(
        slider_prev.contains(&"t-before".into()),
        "{:#?}",
        o.injected
    );
    // After the last host child reads as an append to the parent.
    assert!(
        class_at(keys::SLIDER, Position::After).contains(&"t-after".into())
            || class_at(keys::UTILITY_BUTTONS, Position::Append).first() == Some(&"t-after".into())
    );
    assert_eq!(
        class_at(keys::UTILITY, Position::Prepend),
        vec!["t-prepend".to_string()]
    );
    assert!(class_at(keys::UTILITY_BUTTONS, Position::Append).contains(&"t-append".into()));
    let fixed = o
        .at("", Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-fixed"))
        .expect("fixed button under body");
    assert_eq!(fixed.style.position.as_deref(), Some("fixed"));
    assert_eq!(fixed.style.background_color.as_deref(), Some("red"));
    assert!(fixed.listens.contains(&"click".to_string()));
    let append = o
        .at(keys::UTILITY_BUTTONS, Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-append"))
        .unwrap();
    let svg = append.children.iter().find(|c| c.tag == "svg").unwrap();
    assert!(
        svg.markup
            .as_deref()
            .unwrap()
            .contains("xmlns=\"http://www.w3.org/2000/svg\"")
    );
    assert!(
        svg.markup
            .as_deref()
            .unwrap()
            .contains("<path d=\"M0 0h24v24H0z\"></path>")
    );
    // A click on the injected button reaches its listener.
    luna.dom_event(DomEvent {
        target: DomTarget::Node(fixed.id),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    results_until(&luna, &dir, 10, |r| r["clicked"] == json!(1));
    finish(luna, dir);
}

#[test]
fn wheel_on_the_volume_slider_sets_the_volume() {
    let (luna, host, dir) = start(
        r##"
const wheel = (e) => {
	if (!e.deltaY) return;
	const { playbackControls } = lib.redux.store.getState();
	const v = Math.min(100, Math.max(0, playbackControls.volume + (e.deltaY > 0 ? -10 : 10) / (e.shiftKey ? 10 : 1)));
	R.seen = { phase: e.eventPhase, bubbles: e.bubbles, trusted: e.isTrusted, target: e.target.getAttribute("class")?.split(" ")[0] };
	lib.redux.actions["playbackControls/SET_VOLUME"]({ volume: v });
};
let el = null;
const hook = () => {
	if (el) return;
	el = document.querySelectorAll('div[class^="_sliderContainer"]')[0] ?? null;
	el?.addEventListener("wheel", wheel);
};
new MutationObserver(hook).observe(document.body, { childList: true, subtree: true });
unloads.add(() => el?.removeEventListener("wheel", wheel));
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host(keys::SLIDER)
            .is_some_and(|h| h.listens.contains(&"wheel".to_string()))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -100.0 }),
    });
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetVolume(60.0))
    });
    let r = dom_results(&dir);
    assert_eq!(r["seen"]["trusted"], json!(true));
    assert_eq!(r["seen"]["bubbles"], json!(true));
    assert!(
        r["seen"]["target"]
            .as_str()
            .unwrap()
            .starts_with("_sliderContainer_")
    );
    // Input on a host the plugin does not listen to does nothing.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::UTILITY_BUTTONS.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -100.0 }),
    });
    luna.sync(Duration::from_secs(5));
    assert_eq!(
        host.commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, HostCommand::SetVolume(_)))
            .count(),
        1
    );
    finish(luna, dir);
}

#[test]
fn capture_bubble_stop_and_once() {
    let (luna, _host, dir) = start(
        r##"
const log = [];
const put = (s) => { log.push(s); R.log = log.slice(); };
document.addEventListener("click", () => put("doc-capture"), true);
document.addEventListener("click", () => put("doc-bubble"));
window.addEventListener("click", () => put("win-bubble"), { once: true });
const hook = () => {
	const b = document.querySelector('[data-test="footer-favorite-button"]');
	if (!b || b.__hooked) return;
	b.__hooked = true;
	b.addEventListener("click", (e) => { put("target"); if (log.filter((x) => x === "target").length > 1) e.stopPropagation(); });
	b.parentElement.addEventListener("click", () => put("parent"));
};
new MutationObserver(hook).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host(keys::FAVORITE)
            .is_some_and(|h| h.listens.contains(&"click".to_string()))
    });
    let click = || DomEvent {
        target: DomTarget::Host(keys::FAVORITE.into()),
        kind: "click".into(),
        data: json!({}),
    };
    luna.dom_event(click());
    let r = results_until(&luna, &dir, 10, |r| {
        r["log"].as_array().is_some_and(|l| l.len() >= 5)
    });
    assert_eq!(
        r["log"],
        json!([
            "doc-capture",
            "target",
            "parent",
            "doc-bubble",
            "win-bubble"
        ])
    );
    luna.dom_event(click());
    let r = results_until(&luna, &dir, 10, |r| {
        r["log"].as_array().is_some_and(|l| l.len() >= 7)
    });
    assert_eq!(
        r["log"].as_array().unwrap()[5..],
        [json!("doc-capture"), json!("target")]
    );
    finish(luna, dir);
}

#[test]
fn scroll_into_view_and_scroll_to_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-track-id="1235"]', (row) => row.scrollIntoView({ block: "center" }));
window.addEventListener("keydown", (e) => {
	if (e.key !== "End") return;
	const main = document.querySelector("main");
	R.main = [main.scrollHeight, main.clientHeight, main.scrollTop];
	main.scrollTo({ top: main.scrollHeight, behavior: "smooth" });
});
"##,
    );
    let mut t = tree();
    node(&mut t, keys::MAIN).scroll = Some([0.0, 3000.0]);
    luna.sync_dom(t);
    let o = overlay_until(&luna, 10, |o| o.scroll_to.is_some());
    assert_eq!(o.scroll_to.as_ref().unwrap().0, keys::row("album:55", 1));
    let seq = o.scroll_to.as_ref().unwrap().1;
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "End" }),
    });
    let o = overlay_until(&luna, 10, |o| o.scroll_top.is_some());
    let (key, top, seq2) = o.scroll_top.clone().unwrap();
    assert_eq!(key, keys::MAIN);
    assert!(top > 2000.0, "{top}");
    assert!(seq2 > seq);
    assert_eq!(dom_results(&dir)["main"][0].as_f64(), Some(3000.0));
    finish(luna, dir);
}

#[test]
fn animation_frames_run_at_most_sixty_a_second_and_stop() {
    let (luna, _host, dir) = start(
        r##"
const stamps = [];
const frame = (t) => {
	stamps.push(t);
	if (stamps.length < 6) requestAnimationFrame(frame);
	else R.stamps = stamps;
};
requestAnimationFrame(frame);
const id = requestAnimationFrame(() => (R.cancelled = false));
cancelAnimationFrame(id);
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["stamps"].is_array());
    let s: Vec<f64> = r["stamps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(s.len(), 6);
    assert!(s.windows(2).all(|w| w[1] >= w[0]));
    assert!(s[5] - s[0] >= 70.0, "{s:?}");
    assert!(r.get("cancelled").is_none());
    finish(luna, dir);
}

/// element-hider: a right-click anywhere opens its menu; hovering "Hide
/// This Element" puts a class on the element right-clicked, whose rule
/// rings it (`outline`, `outline-offset`, a `box-shadow` glow). The ring
/// reaches the UI on that app element.
#[test]
fn a_class_a_plugin_puts_on_an_app_element_outlines_it() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `.t-target { outline: 2px solid #ff6b6b; outline-offset: 2px; box-shadow: 0 0 10px rgba(255,107,107,.6); }`);
let target = null;
document.addEventListener("contextmenu", (e) => {
	target = e.target;
	const menu = document.createElement("div");
	menu.style.cssText = "position: fixed; left: 10px; top: 10px;";
	const hide = document.createElement("button");
	hide.textContent = "Hide This Element";
	hide.addEventListener("mouseenter", () => target.classList.add("t-target"));
	menu.appendChild(hide);
	document.body.appendChild(menu);
}, true);
"##,
    );
    luna.sync_dom(tree());
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::FAVORITE.into()),
        kind: "contextmenu".into(),
        data: json!({ "clientX": 5, "clientY": 5, "button": 2 }),
    });
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).next().is_some());
    let button = o.at("", Position::Append).next().unwrap().children[0].id;
    luna.dom_event(DomEvent {
        target: DomTarget::Node(button),
        kind: "mouseenter".into(),
        data: json!({}),
    });
    let o = overlay_until(&luna, 10, |o| {
        o.host(keys::FAVORITE)
            .is_some_and(|h| h.style.outline.is_some())
    });
    let ring = &o.host(keys::FAVORITE).unwrap().style;
    assert_eq!(
        ring.outline.as_deref(),
        Some("2px solid rgb(255, 107, 107)")
    );
    assert_eq!(ring.outline_offset.as_deref(), Some("2px"));
    assert!(ring.box_shadow.is_some());
    finish(luna, dir);
}

/// radiant-lyrics puts each word in a `display: inline-block` span and a
/// `" "` text node between two words. The UI was never told of white space,
/// so the words ran together ("Iwon'tleaveyou"). A space between siblings
/// reaches it; white space at either end of a box does not.
#[test]
fn white_space_between_words_reaches_the_ui() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "._utilityButtons_4d7aaf9", (el) => {
	const line = document.createElement("div");
	line.className = "t-line";
	line.appendChild(document.createTextNode("  "));
	for (const w of ["I", "won't", "leave"]) {
		const s = document.createElement("span");
		s.style.display = "inline-block";
		s.textContent = w;
		line.appendChild(s);
		line.appendChild(document.createTextNode(" "));
	}
	el.prepend(line);
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| !o.injected.is_empty());
    let line = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|v| v.attrs.iter().any(|(k, c)| k == "class" && c == "t-line"))
        .expect("the line");
    let parts: Vec<&str> = line
        .children
        .iter()
        .map(|c| {
            if c.tag == "#text" {
                c.text.as_deref().unwrap_or("")
            } else {
                c.children[0].text.as_deref().unwrap_or("")
            }
        })
        .collect();
    assert_eq!(parts, ["I", " ", "won't", " ", "leave"]);
    finish(luna, dir);
}

/// TIDAL 2.46.0's track rows, player bar and header as a plugin reads them
/// (captured): a row's index column first, with its number and its Play
/// button, its "…", add to playlist and heart buttons with their words and
/// the heart's state, an explicit song's badge and an album row's track
/// number; the player bar's heart and its shuffle switch; the header's Back
/// and Next and its search field's label; the tab bar's links. Translators
/// (korean-translation) rewrite exactly these words: the index column, the
/// add and heart buttons, the badge and the tab bar's links were missing,
/// the "…" read "More", the hearts "favorites" and the arrows "Forward".
#[test]
fn rows_player_bar_and_header_carry_tidals_buttons_and_words() {
    let (luna, _host, dir) = start(
        r##"
const q = (s) => document.querySelector(s);
const qa = (s) => [...document.querySelectorAll(s)];
const a = (el, n) => (el ? el.getAttribute(n) : null);
const look = () => {
	const rows = qa('[data-test="tracklist-row"]');
	if (rows.length < 3) return false;
	const first = rows[0];
	const play = first.querySelector('[data-test="mt-index-col"] [data-test="play-button"]');
	const heart = q('[data-test="footer-favorite-button"]');
	const shuffle = q('[data-test="shuffle"]');
	R.found = {
		index: rows.map((r) => r.querySelector('[data-test="mt-index-col"] > span')?.textContent ?? null),
		firstCell: first.firstElementChild?.className ?? null,
		play: [a(play, "aria-label"), a(play, "title")],
		menu: rows.map((r) => a(r.querySelector('[data-test="context-menu-button"]'), "aria-label")),
		add: rows.map((r) => a(r.querySelector('[data-test="add-to-playlist-button"]'), "title")),
		hearts: rows.map((r) => {
			const h = r.querySelector('[data-test="add-to-favorites-button"]');
			return [a(h, "aria-label"), a(h, "aria-checked"), a(h, "role")];
		}),
		explicit: rows.map((r) => a(r.querySelector('[data-test="explicit-badge"]'), "title")),
		number: a(first, "data-test-track-number"),
		footerHeart: [a(heart, "aria-label"), a(heart, "role"), a(heart, "aria-checked")],
		shuffle: [a(shuffle, "role"), a(shuffle, "aria-checked")],
		arrows: qa('[data-test="navigation-arrows"] button').map((b) => a(b, "aria-label")),
		search: a(q('[data-test="search-popover-search-field"]'), "aria-label"),
		tabs: qa('[data-test="tab-bar"] a').map((l) => a(l, "aria-label")),
	};
	return true;
};
const timer = setInterval(() => {
	if (look()) clearInterval(timer);
}, 50);
unloads.add(() => clearInterval(timer));
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(f["index"], json!(["1", "2", "3"]), "{f}");
    assert_eq!(f["firstCell"], json!("_indexColumn_3ac64ab"), "{f}");
    assert_eq!(f["play"], json!(["Play", "Play"]), "{f}");
    assert_eq!(
        f["menu"],
        json!(["Show options", "Show options", "Show options"]),
        "{f}"
    );
    assert_eq!(
        f["add"],
        json!(["Add to playlist", "Add to playlist", "Add to playlist"]),
        "{f}"
    );
    let add = json!(["Add to My Collection", "false", "switch"]);
    let remove = json!(["Remove from My Collection", "true", "switch"]);
    assert_eq!(f["hearts"], json!([add, remove, add]), "{f}");
    assert_eq!(f["explicit"], json!([null, null, "Explicit"]), "{f}");
    assert_eq!(f["number"], json!("1"), "{f}");
    assert_eq!(
        f["footerHeart"],
        json!(["Add to My Collection", "switch", "false"]),
        "{f}"
    );
    assert_eq!(f["shuffle"], json!(["switch", "false"]), "{f}");
    assert_eq!(f["arrows"], json!(["Back", "Next"]), "{f}");
    assert_eq!(f["search"], json!("Search"), "{f}");
    assert_eq!(f["tabs"], json!(["Home", "Upload", "Search"]), "{f}");
    finish(luna, dir);
}

/// `getComputedStyle` reports what Chromium computes in TIDAL 2.46.0's page:
/// an element that sets no font has TIDAL's (14 px, weight 500, its family:
/// `html` and `body` set them), a size in `rem`, `em` or `%` is in pixels
/// against the root's or the parent's, a weight keyword is its number, a
/// z-index stays within a 32-bit integer and an image's address is quoted.
/// The checker found every plugin element at the browser's 16 px and 400
/// (oneko's cat, StarryNight's sky, star-ratings' stars, LuckyLP's button).
#[test]
fn computed_style_reports_tidals_fonts_and_chromiums_values() {
    let (luna, _host, dir) = start(
        r##"
const add = (parent, css) => {
	const el = document.createElement("div");
	el.style.cssText = css;
	parent.appendChild(el);
	return el;
};
const plain = add(document.body, "");
const small = add(document.body, "font-size: 0.8rem");
const big = add(document.body, "font-size: 2em");
const inner = add(big, "font-size: 50%");
const deep = add(inner, "");
const bold = add(document.body, "font-weight: bold");
const top = add(document.body, "position: absolute; z-index: " + Number.MAX_SAFE_INTEGER);
const cat = add(document.body, "background-image: url(https://example.test/cat.gif)");
const cs = (el) => getComputedStyle(el);
R.found = {
	plain: [cs(plain).fontSize, cs(plain).fontWeight, cs(plain).fontFamily],
	small: cs(small).fontSize,
	big: cs(big).fontSize,
	inner: cs(inner).fontSize,
	deep: cs(deep).fontSize,
	bold: cs(bold).fontWeight,
	z: cs(top).zIndex,
	image: cs(cat).backgroundImage,
};
"##,
    );
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["plain"],
        json!([
            "14px",
            "500",
            "\"Square Sans Text VF\", \"Square Sans Text\", Helvetica, Arial, sans-serif"
        ]),
        "{f}"
    );
    assert_eq!(f["small"], json!("11.2px"), "{f}");
    assert_eq!(f["big"], json!("28px"), "{f}");
    assert_eq!(f["inner"], json!("14px"), "{f}");
    assert_eq!(f["deep"], json!("14px"), "{f}");
    assert_eq!(f["bold"], json!("700"), "{f}");
    assert_eq!(f["z"], json!("2147483647"), "{f}");
    assert_eq!(
        f["image"],
        json!("url(\"https://example.test/cat.gif\")"),
        "{f}"
    );
    finish(luna, dir);
}

/// `querySelectorAll` and `querySelector` with descendant and child
/// combinators match from the root down; what they find, and its order, is
/// what matching each element from itself up finds, from the document, an
/// element (whose own ancestors count), a detached subtree, a fragment and
/// a template, over TIDAL's page and plugin markup with CSS-module classes,
/// SVG's camelCase tags, `:scope`, `:is`, `:not`, `:has` and positions.
#[test]
fn queries_from_the_root_down_find_what_matching_upwards_finds() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const run = () => {
	if (R.done || !document.querySelector('[data-test="footer-player"]')) return;
	const box = document.createElement("section");
	box.className = "plug _item_1234abc";
	box.innerHTML = `
		<div class="row a" data-x="1"><span class="t">one</span><a href="/track/1"><img src="x.png"></a>
			<div class="row b"><span class="t">two</span><p><span>three</span></p></div></div>
		<ul><li class="row">1</li><li>2</li><li class="row last"><span class="t">4</span></li></ul>
		<svg viewBox="0 0 2 2"><defs><linearGradient id="g1"><stop></stop></linearGradient></defs><rect></rect></svg>
		<div class="_wrapper_9999999"><div class="_artworkWrapper_abcdef1"><img><video></video></div></div>`;
	const tpl = document.createElement("template");
	const inTpl = document.createElement("div");
	inTpl.className = "row";
	inTpl.innerHTML = `<span class="t">in template</span>`;
	tpl.content.appendChild(inTpl);
	box.querySelector("ul").before(tpl);
	document.querySelector("main")?.appendChild(box);
	const detached = document.createElement("div");
	detached.innerHTML = `<div class="row"><span class="t">d</span><div><span>e</span></div></div>`;
	const frag = document.createDocumentFragment();
	frag.append(detached.firstChild.cloneNode(true), document.createElement("span"));
	const roots = { document, body: document.body, box, row: box.querySelector(".row.b"), detached, frag, tpl };
	const sels = [
		"*", "span", "div span", "div > span", ".row span", ".row > span", ".row .t", "section .row > .t",
		"body div div span", "main section", "html > body", "#wimp div > div", "div *", "* > *",
		'footer a[href*="/track/"]', 'a[href^="/track"] img', "[data-test] span", "[data-test=footer-player] button",
		"[class*=_artworkWrapper_] :is(img, video)", "._item_5555555 .row", "._wrapper_1111111 img",
		"svg linearGradient", "svg > defs stop", "defs > linearGradient > stop", "rect",
		":scope > div", ":scope span", ":scope > .row > span", "div:not(.row) span", ".row:has(p) span",
		"li:nth-child(2n+1)", "li:first-child, li.last span", "span, div span, .t", "p span, .row > a > img",
		".row.a .row.b span", "div div div div", "ul > li > span.t", "section > div > span", "#nothing", ".none span",
		"button, a", "[aria-label] span, nav a", "div:is(.a, .b) > span", "main *",
	];
	const bad = [];
	let checked = 0;
	for (const [name, root] of Object.entries(roots)) {
		for (const sel of sels) {
			const scope = root instanceof Element ? root : null;
			const list = D.sel.compile(sel);
			const want = D.descendants(root).filter((e) => D.sel.matches(e, list, scope));
			const got = [...root.querySelectorAll(sel)];
			const one = root.querySelector(sel);
			checked++;
			if (got.length !== want.length || got.some((e, i) => e !== want[i]) || one !== (want[0] ?? null)) bad.push(`${name} ${sel}: ${got.length} vs ${want.length}`);
		}
	}
	R.found = box.querySelectorAll("span").length;
	R.bad = bad;
	R.checked = checked;
	R.done = true;
};
new MutationObserver(run).observe(document.body, { subtree: true, childList: true });
run();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 20, |r| r["done"] == json!(true));
    assert_eq!(r["bad"], json!([]), "{r}");
    assert_eq!(r["checked"], json!(7 * 44), "{r}");
    assert_eq!(r["found"], json!(5), "{r}");
    finish(luna, dir);
}

/// Every app element's `listens` in the overlay is the sorted set of event
/// types listened for on it and on every node an event on it reaches, as
/// listeners come and go (a second listener of a type, an `on*` handler,
/// one taken away) and as plugins put nodes in and move app elements, over
/// overlays built one after another (an entry kept from the build before
/// must still be right).
#[test]
fn an_app_elements_listened_types_follow_listeners_and_moves() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const typesOn = (n) => [...(n._ls?.keys() ?? []), ...Object.entries(n._on ?? {}).filter(([, v]) => v).map(([k]) => k)];
const up = (n) => (n.parentNode ?? (n === document ? null : n instanceof ShadowRoot ? n.host : null));
// The window's own types are in the document's entry (with those of the
// document, `html` and `body`, which every connected element reaches).
const want = (el, ov) => {
	const set = new Set();
	let n = el;
	for (; n; n = up(n)) {
		for (const t of typesOn(n)) set.add(t);
		if (n === document) for (const t of ov.hosts[""]?.listens ?? []) set.add(t);
	}
	return [...set].sort();
};
const bad = [];
const counts = [];
const check = (step, type) => {
	const ov = D.build();
	let n = 0;
	for (const [key, el] of D.hosts) {
		const got = ov.hosts[key]?.listens ?? [];
		const w = want(el, ov);
		if (got.join() !== w.join()) bad.push(`${step} ${key}: ${got} vs ${w}`);
		if (type && got.includes(type)) n++;
	}
	counts.push(n);
};
const run = () => {
	const footer = document.querySelector('[data-test="footer-player"]');
	if (R.done || !footer) return;
	R.done = true;
	check("start");
	check("again");
	const a = () => {};
	const b = () => {};
	footer.addEventListener("pointerdown", a);
	check("pointerdown", "pointerdown");
	footer.addEventListener("pointerdown", b);
	check("second", "pointerdown");
	const box = document.createElement("div");
	box.addEventListener("keyup", () => {});
	document.querySelector("main").appendChild(box);
	check("box", "keyup");
	const moved = footer.querySelector("[data-test]") ?? footer.firstElementChild;
	box.appendChild(moved);
	check("moved", "keyup");
	footer.removeEventListener("pointerdown", a);
	check("one left", "pointerdown");
	footer.removeEventListener("pointerdown", b);
	check("none left", "pointerdown");
	document.body.onclick = () => {};
	check("onclick", "click");
	document.body.onclick = null;
	check("no onclick", "click");
	R.bad = bad;
	R.counts = counts;
};
new MutationObserver(run).observe(document.body, { subtree: true, childList: true });
run();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 20, |r| !r["counts"].is_null());
    assert_eq!(r["bad"], json!([]), "{r}");
    let counts: Vec<u64> = r["counts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_u64().unwrap())
        .collect();
    // pointerdown reaches the footer's elements, then none; keyup only the
    // moved one and what is inside it; click every element, then none.
    assert!(counts[2] > 1 && counts[3] == counts[2], "{r}");
    assert!(counts[4] == 0 && counts[5] >= 1, "{r}");
    assert!(counts[6] == counts[2] - counts[5] && counts[7] == 0, "{r}");
    assert!(counts[8] > counts[2] && counts[9] == 0, "{r}");
    finish(luna, dir);
}

/// The same attributes in a new tree change nothing, and only what the
/// host changed is applied: what a plugin set on an app element (a class
/// added, one of TIDAL's taken away, an attribute of its own) stays through
/// trees that repeat it, a host attribute's change, a class change and a
/// removed attribute, with one record for each change.
#[test]
fn a_tree_with_the_same_attributes_leaves_what_plugins_set() {
    let (luna, _host, dir) = start(
        r##"
const recs = [];
let shell = null;
const look = () => {
	shell ??= document.querySelector("#wimp > div[data-market-theme] > div");
	if (!shell) return;
	if (!R.ready) {
		shell.classList.add("plug");
		shell.classList.remove("is-desktop-app");
		shell.setAttribute("aria-label", "mine");
		new MutationObserver((list) => {
			for (const r of list) if (r.target === shell) recs.push(r.attributeName);
		}).observe(shell, { attributes: true });
		R.ready = true;
	}
	R.state = { cls: shell.className, label: shell.getAttribute("aria-label"), title: shell.getAttribute("title"), recs: recs.slice(), same: shell === document.querySelector("#wimp > div[data-market-theme] > div") };
};
const id = setInterval(look, 30);
unloads.add(() => clearInterval(id));
"##,
    );
    fn shell(t: &mut HostTree) -> &mut Vec<(std::borrow::Cow<'static, str>, String)> {
        &mut node(t, keys::SHELL).attrs
    }
    let t0 = tree();
    luna.sync_dom(t0.clone());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let state = |want: &dyn Fn(&Value) -> bool| {
        results_until(&luna, &dir, 10, |r| want(&r["state"]))["state"].clone()
    };
    let s = state(&|s| s["label"] == json!("mine"));
    assert_eq!(
        s["cls"],
        json!("has-footer-player is-now-playing plug"),
        "{s}"
    );
    // The same tree, again and again: nothing changes, nothing is recorded.
    for _ in 0..3 {
        luna.sync_dom(t0.clone());
        luna.sync(Duration::from_secs(5));
    }
    std::thread::sleep(Duration::from_millis(150));
    let s = state(&|_| true);
    assert_eq!(
        s,
        json!({ "cls": "has-footer-player is-now-playing plug", "label": "mine", "title": null, "recs": [], "same": true })
    );
    // A host attribute changes: only it.
    let mut t1 = t0.clone();
    shell(&mut t1).push(("title".into(), "x".into()));
    luna.sync_dom(t1.clone());
    let s = state(&|s| s["title"] == json!("x"));
    assert_eq!(
        s,
        json!({ "cls": "has-footer-player is-now-playing plug", "label": "mine", "title": "x", "recs": ["title"], "same": true })
    );
    // TIDAL's classes change: the plugin's stay added and taken away.
    let mut t2 = t1.clone();
    shell(&mut t2)
        .iter_mut()
        .find(|(k, _)| k == "class")
        .unwrap()
        .1
        .push_str(" is-fullscreen");
    luna.sync_dom(t2.clone());
    let s = state(&|s| s["recs"].as_array().is_some_and(|r| r.len() == 2));
    assert_eq!(
        s["cls"],
        json!("has-footer-player is-now-playing is-fullscreen plug"),
        "{s}"
    );
    // A host attribute goes.
    let mut t3 = t2.clone();
    shell(&mut t3).retain(|(k, _)| k != "title");
    luna.sync_dom(t3);
    let s = state(&|s| s["title"].is_null());
    assert_eq!(
        s,
        json!({ "cls": "has-footer-player is-now-playing is-fullscreen plug", "label": "mine", "title": null, "recs": ["title", "class", "title"], "same": true })
    );
    finish(luna, dir);
}

/// The first tree goes whole, as it serialises; after it, a node as it was
/// goes as its key and box, and a node with any change whole.
#[test]
fn a_tree_goes_whole_first_and_then_as_its_changes() {
    let t0 = tree();
    assert_eq!(
        serde_json::to_value(t0.wire(None)).unwrap(),
        serde_json::to_value(&t0).unwrap()
    );
    let mut t1 = t0.clone();
    node(&mut t1, keys::TITLE_SPAN).text = Some("Song Changed".into());
    let wire = serde_json::to_value(t1.wire(Some(&t0))).unwrap();
    let whole = serde_json::to_value(&t1).unwrap();
    for (w, n) in wire["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .zip(whole["nodes"].as_array().unwrap())
    {
        if n["key"] == json!(keys::TITLE_SPAN) {
            assert_eq!(w, n);
        } else {
            let mut short = json!({ "key": n["key"], "same": 1 });
            for k in ["rect", "scroll"] {
                if !n[k].is_null() {
                    short[k] = n[k].clone();
                }
            }
            assert_eq!(w, &short);
        }
    }
    let mut rest = wire.clone();
    let mut whole_rest = whole.clone();
    rest.as_object_mut().unwrap().remove("nodes");
    whole_rest.as_object_mut().unwrap().remove("nodes");
    assert_eq!(rest, whole_rest);
}

/// Trees after the first come as changes (`HostTree::wire`): through
/// changed attributes and words, boxes alone moving, an element gone and
/// back with another tag, a changed value and an element moved to another
/// parent, every app element in the document is what the newest tree says,
/// as when each tree came whole.
#[test]
fn trees_sent_as_changes_build_what_whole_trees_build() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const dump = () => {
	const out = [];
	for (const [key, el] of D.hosts) {
		const attrs = [...el._hostAttrs.entries()];
		const shown = attrs.every(([k, v]) => k === "class" || el.getAttribute(k) === v);
		out.push([key, el.localName, attrs, el._hostTextVal ?? null, el._hostParent?._hostKey ?? null, el._rect?.map((x) => x.toFixed(2)) ?? null, el._hostValue ?? null, shown && el.parentNode === el._hostParent]);
	}
	R.dump = out.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
	R.syncs = D.syncCount();
};
const id = setInterval(dump, 20);
unloads.add(() => clearInterval(id));
"##,
    );
    let expect = |t: &HostTree| -> Value {
        let mut rows: Vec<Value> = t
            .nodes
            .iter()
            .map(|n| {
                json!([
                    n.key,
                    n.tag.to_lowercase(),
                    n.attrs
                        .iter()
                        .map(|(k, v)| json!([k.to_lowercase(), v]))
                        .collect::<Vec<_>>(),
                    n.text,
                    n.parent,
                    n.rect.map(|r| r.map(|x| format!("{x:.2}"))),
                    n.value,
                    true
                ])
            })
            .collect();
        rows.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
        Value::Array(rows)
    };
    let mut trees = vec![tree()];
    let mut t = tree();
    node(&mut t, keys::SHELL)
        .attrs
        .push(("title".into(), "x".into()));
    node(&mut t, keys::TITLE_SPAN).text = Some("Song Changed".into());
    trees.push(t.clone());
    for n in &mut t.nodes {
        if let Some(r) = &mut n.rect {
            r[1] += 7.0;
        }
    }
    trees.push(t.clone());
    let gone = t
        .nodes
        .iter()
        .rposition(|n| t.nodes.iter().all(|c| c.parent.as_deref() != Some(&n.key)))
        .unwrap();
    let leaf = t.nodes.remove(gone);
    trees.push(t.clone());
    let mut back = leaf.clone();
    back.tag = if back.tag == "span" {
        "div".into()
    } else {
        "span".into()
    };
    back.attrs.push(("data-back".into(), "1".into()));
    t.nodes.insert(gone, back);
    if let Some(n) = t.nodes.iter_mut().find(|n| n.tag == "input") {
        n.value = Some("41".into());
    }
    trees.push(t.clone());
    let leaf_key = t.nodes[gone].key.clone();
    let new_parent = t.nodes[..gone]
        .iter()
        .rev()
        .find(|n| Some(&n.key) != t.nodes[gone].parent.as_ref())
        .unwrap()
        .key
        .clone();
    node(&mut t, &leaf_key).parent = Some(new_parent);
    trees.push(t.clone());
    trees.push(t.clone());
    for (i, t) in trees.iter().enumerate() {
        luna.sync_dom(t.clone());
        let r = results_until(&luna, &dir, 10, |r| {
            r["syncs"].as_u64() >= Some(i as u64 + 1)
        });
        let want = expect(t);
        let got = &r["dump"];
        assert_eq!(
            got.as_array().map(Vec::len),
            want.as_array().map(Vec::len),
            "tree {i}"
        );
        for (g, w) in got.as_array().unwrap().iter().zip(want.as_array().unwrap()) {
            assert_eq!(g, w, "tree {i}");
        }
    }
    finish(luna, dir);
}

/// Markup with a `<template>` parses as in Chromium: the template's
/// markup is its content, and what follows it goes on in its parent (any
/// closing tag after a template's opening one threw a TypeError).
#[test]
fn markup_with_a_template_parses() {
    let (luna, _host, dir) = start(
        r##"
const box = document.createElement("div");
try {
	box.innerHTML = `<template id="t"><div class="row"><span>in</span></div></template><p>after</p><template></template>`;
	const t = box.querySelector("template");
	R.found = [box.children.length, t?.content?.childNodes?.length ?? -1, t?.content?.firstChild?.className ?? null, box.querySelector("p")?.textContent ?? null, box.innerHTML];
} catch (err) {
	R.found = String(err);
}
"##,
    );
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    assert_eq!(
        r["found"],
        json!([
            3,
            1,
            "row",
            "after",
            "<template id=\"t\"><div class=\"row\"><span>in</span></div></template><p>after</p><template></template>"
        ]),
        "{r}"
    );
    finish(luna, dir);
}

/// A computed opacity is the number Chromium reports: `.75` reads `0.75`,
/// a percentage its fraction, and a value past 1 or below 0 is clamped.
/// The declared text came back as written (true-taste's `.75`).
#[test]
fn computed_opacity_is_chromiums_number() {
    let (luna, _host, dir) = start(
        r##"
const css = document.createElement("style");
css.textContent = ".fade { opacity: .75; }";
document.head.appendChild(css);
const add = (cls, style) => {
	const el = document.createElement("div");
	el.className = cls;
	el.style.cssText = style;
	document.body.appendChild(el);
	return getComputedStyle(el).opacity;
};
R.found = [add("fade", ""), add("", "opacity: 40%"), add("", "opacity: 1.5"), add("", "opacity: -2"), add("", "")];
"##,
    );
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    assert_eq!(r["found"], json!(["0.75", "0.4", "1", "0", "1"]), "{r}");
    finish(luna, dir);
}

/// TIDAL's market buttons compute their type and colour from its sheet:
/// a size class sets the type (small 14 px at 600, medium 16 px), a class
/// that sets the button's own tokens wins whatever the class order (a
/// plugin's copy of the now-playing header's full screen button reads
/// 55 % white at 600, as TidalLyricsTranslate's in TIDAL), and Play's
/// primary pill is TIDAL's dark grey, a secondary one white.
#[test]
fn market_buttons_compute_tidals_type_and_colour() {
    let (luna, _host, dir) = start(
        r##"
const cs = (el) => getComputedStyle(el);
lib.observe(unloads, '[data-test="footer-track-title"]', () => {
	if (R.found) return;
	const btn = (cls) => {
		const b = document.createElement("button");
		b.className = cls;
		document.body.appendChild(b);
		return b;
	};
	const look = (b) => [cs(b).color, cs(b).fontWeight, cs(b).fontSize];
	R.found = {
		icon: look(btn("_marketButton_1d16h_1 _tertiary_1d16h_8 _small_1d16h_3 _iconOnly_1d16h_1 _iconButton_fe68160 _fullscreenButton_109e50d")),
		iconReordered: look(btn("_iconButton_fe68160 _small_1d16h_3 _iconOnly_1d16h_1 _tertiary_1d16h_8 _marketButton_1d16h_1")),
		play: look(btn("_marketButton_1d16h_1 _primary_1d16h_6 _medium_1d16h_4")),
		shuffle: look(btn("_marketButton_1d16h_1 _secondary_1d16h_7 _medium_1d16h_4")),
	};
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["icon"],
        json!(["rgba(255, 255, 255, 0.55)", "600", "14px"]),
        "{r}"
    );
    assert_eq!(f["iconReordered"], f["icon"], "{r}");
    assert_eq!(f["play"], json!(["rgb(16, 16, 16)", "600", "16px"]), "{r}");
    assert_eq!(
        f["shuffle"],
        json!(["rgb(255, 255, 255)", "600", "16px"]),
        "{r}"
    );
    finish(luna, dir);
}

/// The now-playing header's tabs are TIDAL's toggle buttons, which re-set
/// their market button's tokens (`._toggleButton_934359e`): medium type
/// (500, not the small size's 600) in the tertiary grey, black on white
/// while pressed (`._toggleButtonActive_b595428`). What a plugin puts in a
/// tab (radiant-lyrics' menu trigger in the Lyrics tab) inherits that.
#[test]
fn the_now_playing_tabs_compute_tidals_toggle_look() {
    let (luna, _host, dir) = start(
        r##"
const cs = (el) => getComputedStyle(el);
const go = () => {
	const lyrics = document.querySelector('[data-test="toggle-lyrics"]');
	const credits = document.querySelector('[data-test="toggle-credits"]');
	if (!lyrics || !credits || R.found) return;
	const mine = document.createElement("div");
	lyrics.appendChild(mine);
	const look = (b) => [cs(b).color, cs(b).fontWeight, cs(b).fontSize];
	R.found = {
		pressed: [...look(lyrics), cs(lyrics).backgroundColor],
		other: look(credits),
		child: cs(mine).fontWeight,
	};
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    if let Some(np) = frame.now_playing.as_mut() {
        np.tabs = vec![
            dom_anchors::NowPlayingTab {
                name: "lyrics",
                label: "Lyrics".into(),
                pressed: true,
                rect: Some([600.0, 20.0, 80.0, 30.0]),
            },
            dom_anchors::NowPlayingTab {
                name: "credits",
                label: "Credits".into(),
                pressed: false,
                rect: Some([690.0, 20.0, 80.0, 30.0]),
            },
        ];
    }
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["pressed"],
        json!(["rgba(0, 0, 0, 0.9)", "500", "14px", "rgb(255, 255, 255)"]),
        "{r}"
    );
    assert_eq!(
        f["other"],
        json!(["rgba(255, 255, 255, 0.55)", "500", "14px"]),
        "{r}"
    );
    assert_eq!(f["child"], json!("500"), "{r}");
    finish(luna, dir);
}

/// `display` as Chromium computes it on TIDAL's page: a flex or grid item,
/// an absolutely positioned or floated box is blockified (SongReleaseDate's
/// `inline-flex` date in the player bar's title, a flex box in TIDAL's
/// sheet, reads `flex`; tidal-genres' badge in `._trackContent_` too), a
/// `display: contents` box passes its parent's on, and the tags start from
/// Chromium's sheet and TIDAL's normalize (`video { inline-block }`, an
/// `img` and an `svg` inline, `li` a list item, `[hidden]` and an `audio`
/// without controls not shown). Text inside TIDAL's `._smallText_` is its
/// 12 px, and what is inside the closed now-playing view is hidden
/// (StarryNight's sky). A corner radius computes to pixels, written as briefly as it
/// reads, and TIDAL's radius variables resolve.
#[test]
fn computed_display_and_lengths_follow_chromium_and_tidals_sheet() {
    let (luna, _host, dir) = start(
        r##"
const add = (parent, tag, css) => {
	const el = document.createElement(tag);
	if (css) el.style.cssText = css;
	parent.appendChild(el);
	return el;
};
const cs = (el) => getComputedStyle(el);
lib.observe(unloads, '[data-test="footer-track-title"]', (title) => {
	if (R.found) return;
	const flex = add(document.body, "div", "display: flex");
	const item = add(flex, "span", "display: inline-flex");
	const plainItem = add(flex, "span", "");
	const contents = add(flex, "div", "display: contents");
	const deepItem = add(contents, "span", "display: inline-block");
	const abs = add(document.body, "span", "position: absolute");
	const floated = add(document.body, "span", "float: left");
	const outside = add(document.body, "span", "display: inline-flex");
	const li = add(add(document.body, "ul", ""), "li", "");
	const hidden = add(document.body, "div", "");
	hidden.hidden = true;
	const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
	document.body.appendChild(svg);
	const date = add(title, "span", "display: inline-flex");
	const bar = document.querySelector('[class*="_smallText_"]');
	const small = add(bar, "span", "");
	const closed = add(document.body, "section", "");
	closed.className = "_nowPlayingContainer_88d37df _nowPlayingHidden_7e74506";
	const sky = add(closed, "div", "");
	R.found = {
		sky: cs(sky).visibility,
		items: [item, plainItem, deepItem, abs, floated, outside].map((e) => cs(e).display),
		tags: ["video", "img", "button", "audio"].map((t) => cs(add(document.body, t, "")).display).concat([cs(svg).display, cs(li).display, cs(hidden).display]),
		date: cs(date).display,
		small: cs(small).fontSize,
		radii: ["border-radius: 0 0 12px 12px", "border-radius: 0.5em 0.5em", "border-radius: var(--wave-border-radius--extra-small)"].map((c) => cs(add(document.body, "div", c)).borderRadius),
	};
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["items"],
        json!(["flex", "block", "block", "block", "block", "inline-flex"]),
        "{f}"
    );
    assert_eq!(
        f["tags"],
        json!([
            "inline-block",
            "inline",
            "inline-block",
            "none",
            "inline",
            "list-item",
            "none"
        ]),
        "{f}"
    );
    assert_eq!(f["date"], json!("flex"), "{f}");
    assert_eq!(f["small"], json!("12px"), "{f}");
    assert_eq!(
        f["sky"],
        json!("hidden"),
        "inside the closed now-playing view: {f}"
    );
    assert_eq!(
        f["radii"],
        json!(["0px 0px 12px 12px", "7px", "4px"]),
        "{f}"
    );
    finish(luna, dir);
}

/// Chromium's prefixed aliases are the standard property: radiant-lyrics
/// sets `backdrop-filter` and `-webkit-backdrop-filter` on the player bar,
/// which in TIDAL is one property, written once in the style attribute.
/// A `-moz-` property is not Chromium's at all: it is dropped.
#[test]
fn prefixed_style_aliases_are_the_standard_property() {
    let (luna, _host, dir) = start(
        r##"
const el = document.createElement("div");
document.body.appendChild(el);
el.style.setProperty("backdrop-filter", "blur(15px)");
el.style.setProperty("-webkit-backdrop-filter", "blur(15px)");
el.style.webkitTransform = "rotate(1deg)";
el.style.setProperty("-moz-transform", "scale(2)");
R.found = {
	style: el.getAttribute("style"),
	backdrop: [el.style.getPropertyValue("backdrop-filter"), el.style.getPropertyValue("-webkit-backdrop-filter")],
	transform: [el.style.transform, el.style.webkitTransform],
	moz: el.style.getPropertyValue("-moz-transform"),
	length: el.style.length,
};
"##,
    );
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["style"],
        json!("backdrop-filter: blur(15px); transform: rotate(1deg);"),
        "{f}"
    );
    assert_eq!(f["backdrop"], json!(["blur(15px)", "blur(15px)"]), "{f}");
    assert_eq!(
        f["transform"],
        json!(["rotate(1deg)", "rotate(1deg)"]),
        "{f}"
    );
    assert_eq!(f["moz"], json!(""), "{f}");
    assert_eq!(f["length"], json!(2), "{f}");
    finish(luna, dir);
}

/// A declared colour reads back as Chromium serializes it, not as it was
/// written: a plugin that sets `#3fe` on an element (radiant-lyrics' progress
/// colour) finds `rgb(51, 255, 238)` in its style and style attribute, as in
/// TIDAL, wherever the colour stands in the value. Custom properties, values
/// with `var()`, names and addresses keep their text, and the style
/// attribute keeps the text it was set to until the style object changes it.
#[test]
fn declared_colours_read_back_as_chromium_serializes_them() {
    let (luna, _host, dir) = start(
        r##"
const el = document.createElement("div");
document.body.appendChild(el);
el.style.setProperty("background-color", "#3fe", "important");
el.style.color = "hsl(120, 50%, 50%)";
el.style.boxShadow = "0 0 4px #00000080";
el.style.backgroundImage = "linear-gradient(#FFF, rgba(0,0,0,.5))";
el.style.borderTopColor = "rgba(10, 20, 30, 1)";
el.style.fill = "url(#grad)";
el.style.outlineColor = "red";
el.style.setProperty("--accent", "#3fe");
el.style.caretColor = "var(--accent, #fff)";
const set = document.createElement("div");
set.setAttribute("style", "color:#fff");
const parsed = document.createElement("div");
parsed.innerHTML = '<span style="color:#000">x</span>';
const sheet = document.createElement("style");
sheet.textContent = ".dom-test-colour { color: #abc; }";
document.head.appendChild(sheet);
R.found = {
	attr: el.getAttribute("style"),
	read: [el.style.backgroundColor, el.style.color, el.style.boxShadow, el.style.backgroundImage, el.style.borderTopColor, el.style.fill, el.style.outlineColor, el.style.getPropertyValue("--accent"), el.style.caretColor],
	set: [set.getAttribute("style"), set.style.color],
	parsed: [parsed.firstChild.getAttribute("style"), parsed.firstChild.style.color, parsed.innerHTML],
	rule: sheet.sheet.cssRules[0].style.color,
};
"##,
    );
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    let f = &r["found"];
    assert_eq!(
        f["attr"],
        json!(
            "background-color: rgb(51, 255, 238) !important; color: rgb(64, 191, 64); \
             box-shadow: 0 0 4px rgba(0, 0, 0, 0.5); background-image: linear-gradient(rgb(255, \
             255, 255), rgba(0, 0, 0, 0.5)); border-top-color: rgb(10, 20, 30); fill: url(#grad); \
             outline-color: red; --accent: #3fe; caret-color: var(--accent, #fff);"
        ),
        "{f}"
    );
    assert_eq!(
        f["read"],
        json!([
            "rgb(51, 255, 238)",
            "rgb(64, 191, 64)",
            "0 0 4px rgba(0, 0, 0, 0.5)",
            "linear-gradient(rgb(255, 255, 255), rgba(0, 0, 0, 0.5))",
            "rgb(10, 20, 30)",
            "url(#grad)",
            "red",
            "#3fe",
            "var(--accent, #fff)"
        ]),
        "{f}"
    );
    assert_eq!(f["set"], json!(["color:#fff", "rgb(255, 255, 255)"]), "{f}");
    assert_eq!(
        f["parsed"],
        json!([
            "color:#000",
            "rgb(0, 0, 0)",
            "<span style=\"color:#000\">x</span>"
        ]),
        "{f}"
    );
    assert_eq!(f["rule"], json!("rgb(170, 187, 204)"), "{f}");
    finish(luna, dir);
}

/// CSS hides what is inside a hidden element: `display: none` on the
/// sidebar (element-hider's "Hide This Element" on a box round the rows)
/// takes its rows and what a plugin put in it out of the page, and an
/// inherited `visibility: hidden` hides the player bar's buttons except one
/// that sets `visible` again. The overlay said only the element the rule
/// named was hidden, so every native part inside it went on drawing.
#[test]
fn what_is_inside_a_hidden_element_is_hidden() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
[data-test="main-layout-sidebar-wrapper"] { display: none; }
[data-test="footer-player"] { visibility: hidden; }
[data-test="next"] { visibility: visible; }
`);
lib.observe(unloads, "#sidebar-content", (el) => {
	const p = document.createElement("span");
	p.className = "t-inside";
	p.textContent = "inside";
	el.append(p);
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        !o.injected.is_empty() && o.hidden(&keys::sidebar_item("sidebar-music"))
    });
    for key in [
        keys::SIDEBAR_NAV.to_string(),
        keys::sidebar_item("sidebar-music"),
        keys::sidebar_item_label("sidebar-music"),
    ] {
        let h = o
            .host(&key)
            .unwrap_or_else(|| panic!("{key} is in the overlay"));
        assert_eq!(h.style.display.as_deref(), Some("none"), "{key}");
    }
    let inside = o
        .injected
        .iter()
        .flat_map(|i| &i.nodes)
        .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == "t-inside"))
        .expect("the plugin's element");
    assert!(inside.style.hidden(), "the plugin's element goes with it");
    assert!(o.hidden(keys::PLAY), "visibility is inherited");
    assert!(o.hidden(keys::SHUFFLE));
    assert!(!o.hidden(keys::NEXT), "a child that sets visible shows");
    assert!(o.host(keys::NEXT).is_none_or(|h| h.style.display.is_none()));
    finish(luna, dir);
}

#[test]
fn plugin_css_restyles_hosts_and_sets_variables() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
/* comment { not: a rule } */
:root { --wave-color-solid-accent-fill: #ff0000; --x: var(--missing, 4px); }
[role="row"]:has([data-test="table-cell-title"][data-id="1235"]) { background: rgba(1, 2, 3, 0.5) !important; }
[data-track-id="1235"] { background: blue; }
div[class^="_bar"] { display: none; }
#footerPlayer { background: 0 0; }
main#main { background: center / cover no-repeat rgb(1, 2, 3); }
@media (max-width: 100px) { div[class^="_bar"] { display: block !important; } }
@media (prefers-color-scheme: dark) { .t-pill { color: var(--wave-color-solid-accent-fill); } }
@keyframes spin { from { color: red } }
.t-pill:hover { color: green; }
._utilityButtons_4d7aaf9 > :nth-child(1 of .t-pill) { border-radius: 9px; }
`);
const sheet = new CSSStyleSheet();
sheet.replaceSync('[data-test="footer-favorite-button"] { opacity: .5 }');
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
lib.observe(unloads, "._utilityButtons_4d7aaf9", (el) => {
	const p = document.createElement("div");
	p.className = "t-pill";
	p.style.padding = "3px 6px";
	p.textContent = "00:01:00";
	el.prepend(p);
	R.computed = getComputedStyle(p).getPropertyValue("color");
	R.rootVar = getComputedStyle(document.documentElement).getPropertyValue("--wave-color-solid-accent-fill");
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| !o.injected.is_empty() && o.hidden(keys::BAR));
    assert_eq!(o.var("--wave-color-solid-accent-fill"), Some("#ff0000"));
    assert_eq!(o.var("--x"), Some("4px"));
    let row = o.host(&keys::row("album:55", 1)).unwrap();
    assert_eq!(
        row.style.background_color.as_deref(),
        Some("rgba(1, 2, 3, 0.5)")
    );
    assert!(
        o.host(&keys::row("album:55", 0))
            .is_none_or(|h| h.style.background_color.is_none())
    );
    assert_eq!(
        o.host(keys::FAVORITE).unwrap().style.opacity.as_deref(),
        Some(".5")
    );
    // The `background` shorthand without an image gives its colour, or
    // transparent (radiant-lyrics' `background: 0 0` on the now-playing view).
    assert_eq!(
        o.host(keys::FOOTER)
            .unwrap()
            .style
            .background_color
            .as_deref(),
        Some("transparent")
    );
    assert_eq!(
        o.host(keys::MAIN)
            .unwrap()
            .style
            .background_color
            .as_deref(),
        Some("rgb(1, 2, 3)")
    );
    let pill = o
        .at(keys::UTILITY_BUTTONS, Position::Prepend)
        .next()
        .unwrap();
    assert_eq!(pill.style.color.as_deref(), Some("#ff0000"));
    assert_eq!(pill.style.padding.as_deref(), Some("3px 6px"));
    assert_eq!(pill.style.border_radius.as_deref(), Some("9px"));
    assert_eq!(pill.children[0].text.as_deref(), Some("00:01:00"));
    let r = results_until(&luna, &dir, 10, |r| r["computed"].is_string());
    assert_eq!(r["computed"], json!("#ff0000"));
    assert_eq!(r["rootVar"], json!("#ff0000"));
    assert!(o.css_bytes > 100);
    finish(luna, dir);
}

/// Keyframes read `var()` against the element they animate, as in a
/// browser: each line of a list slides in from its own distance, which
/// the plugin sets on the line (`--delta`). They were resolved against the
/// root's variables only, where `--delta` is not defined, so the lines did
/// not move (radiant-lyrics' lines snapped to their new place instead of
/// sliding up when the sung line changed).
#[test]
fn keyframes_read_the_animated_elements_own_variables() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.slide { animation: slide .4s ease both; }
@keyframes slide { from { translate: 0 var(--delta); } to { translate: 0 0; } }
`);
const box = document.createElement("div");
box.style.position = "fixed";
for (const d of ["30px", "60px", "60px"]) {
	const line = document.createElement("div");
	line.className = "slide";
	line.style.setProperty("--delta", d);
	box.append(line);
}
document.body.append(box);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).next().is_some());
    let lines = &o.at("", Position::Append).next().unwrap().children;
    let names: Vec<&str> = lines
        .iter()
        .map(|l| {
            l.style
                .animation
                .as_deref()
                .and_then(|a| a.split(' ').next())
                .expect("animated")
        })
        .collect();
    assert_eq!(names.len(), 3);
    assert_ne!(
        names[0], names[1],
        "different distances, different keyframes"
    );
    assert_eq!(names[1], names[2], "the same distance shares them");
    let from = |name: &str| {
        o.keyframes[name][0]
            .1
            .transform
            .clone()
            .unwrap_or_default()
            .replace(' ', "")
    };
    assert!(from(names[0]).contains("30px"), "{}", from(names[0]));
    assert!(from(names[1]).contains("60px"), "{}", from(names[1]));
    finish(luna, dir);
}

#[test]
fn hover_rules_follow_the_pointer_and_animations_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.t-btn { opacity: 0; }
div[role="row"]:hover .t-btn { opacity: 1; }
.t-spin { animation: t-rot 4s linear infinite; translate: -50% -50%; }
@keyframes t-rot { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
`);
lib.observe(unloads, '[data-test="tracklist-row"]', (row) => {
	// The actions' wrapper, as quick-queue finds it: the title has a
	// `_flexWrapper_` of its own before it (TIDAL 2.46.0).
	const flex = row.querySelector('div[class*="_contextColumn_"] > div[class*="_flexWrapper_"]');
	if (!flex || flex.querySelector(".t-btn")) return;
	const b = document.createElement("button");
	b.className = "t-btn";
	b.textContent = "+";
	flex.prepend(b);
});
const s = document.createElement("div");
s.className = "t-spin";
s.style.position = "fixed";
s.style.zIndex = "-1";
document.body.append(s);
window.addEventListener("mousemove", (e) => (R.move = [e.clientX, e.clientY]));
"##,
    );
    luna.sync_dom(tree());
    let row = keys::row("album:55", 1);
    let actions = format!("{row}:actions");
    let o = overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend).next().is_some()
    });
    assert!(o.hover);
    let btn = o.at(&actions, Position::Prepend).next().unwrap();
    assert_eq!(btn.style.opacity.as_deref(), Some("0"));
    let spin = o.at("", Position::Append).next().unwrap();
    assert_eq!(
        spin.style.animation.as_deref(),
        Some("t-rot 4 0 infinite normal linear running none")
    );
    assert!(
        spin.style
            .transform
            .as_deref()
            .is_some_and(|t| t.contains("translate(-50%,-50%)"))
    );
    let frames = &o.keyframes["t-rot"];
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1].1.transform.as_deref(), Some("rotate(360deg)"));
    // The pointer over the row: its button shows.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(row.clone()),
        kind: "hover".into(),
        data: Value::Null,
    });
    let o = overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend)
            .next()
            .is_some_and(|b| b.style.opacity.as_deref() == Some("1"))
    });
    assert!(o.hover);
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "hover".into(),
        data: Value::Null,
    });
    overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend)
            .next()
            .is_some_and(|b| b.style.opacity.as_deref() == Some("0"))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "mousemove".into(),
        data: json!({ "clientX": 12, "clientY": 34 }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["move"].is_array());
    assert_eq!(r["move"], json!([12, 34]));
    finish(luna, dir);
}

#[test]
fn f11_requests_fullscreen() {
    let (luna, host, dir) = start(
        r##"
window.addEventListener("keydown", (e) => {
	if (e.key !== "F11") return;
	e.preventDefault();
	if (document.fullscreenElement) document.exitFullscreen();
	else document.documentElement.requestFullscreen();
});
document.addEventListener("fullscreenchange", () => (R.changes = (R.changes ?? 0) + 1));
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host("")
            .is_some_and(|h| h.listens.contains(&"keydown".to_string()))
    });
    let f11 = || DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "F11", "code": "F11" }),
    };
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(true))
    });
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(false))
    });
    results_until(&luna, &dir, 10, |r| r["changes"] == json!(2));
    finish(luna, dir);
}

/// OneClickLyrics disables its button while the song has no words: in a
/// browser a disabled button, and what is inside it, takes no clicks, so
/// it does not open now playing. Its clicks used to run the listener.
#[test]
fn a_disabled_button_takes_no_clicks() {
    let (luna, _host, dir) = start(
        r##"
R.clicks = 0;
const b = document.createElement("button");
b.id = "off";
b.disabled = true;
b.innerHTML = "<div><svg viewBox='0 0 4 4'></svg></div>";
b.addEventListener("click", () => R.clicks++);
document.body.append(b);
b.click();
R.programmatic = R.clicks;
R.tick = 0;
setInterval(() => { R.now = R.clicks; R.tick++; }, 30);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["tick"].as_u64() > Some(0));
    assert_eq!(r["programmatic"], json!(0), "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .any(|n| n.tag == "button")
    });
    let button = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|n| n.tag == "button")
        .expect("the button");
    let svg = button.children[0].children[0].id;
    for target in [button.id, svg] {
        luna.dom_event(DomEvent {
            target: DomTarget::Node(target),
            kind: "click".into(),
            data: json!({}),
        });
    }
    let seen = results_until(&luna, &dir, 10, |_| true)["tick"]
        .as_u64()
        .unwrap_or(0);
    let r = results_until(&luna, &dir, 10, |r| r["tick"].as_u64() > Some(seen + 10));
    assert_eq!(r["now"], json!(0), "{r}");
    finish(luna, dir);
}

/// OneClickLyrics clicks `#footerPlayer` (opening now playing) and then
/// another of TIDAL's buttons in the same turn: both clicks reach their
/// widgets, in that order. Only the newer one used to.
#[test]
fn two_clicks_on_native_widgets_in_one_turn_both_arrive() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const bar = document.querySelector("#footerPlayer");
	const lyrics = document.querySelector('[data-test="play-queue-button"]');
	if (!bar || !lyrics || R.done) return;
	R.done = true;
	bar.click();
	lyrics.click();
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let o = overlay_until(&luna, 10, |o| o.clicks.len() >= 2);
    let clicked: Vec<&str> = o.clicks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(clicked, [keys::FOOTER, keys::QUEUE]);
    assert!(o.clicks[0].1 < o.clicks[1].1);
    finish(luna, dir);
}

/// AltPlay takes the transport over: it clicks TIDAL's pause button to stop
/// TIDAL (a click on the button is the app's pause), and lays its own round
/// element over the button's `parentElement`, which is a box of the button's
/// size, not the row of all the transport buttons.
#[test]
fn a_plugin_can_click_the_play_button_and_lay_an_element_over_its_box() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const button = document.querySelector('#footerPlayer button[data-test="play"], #footerPlayer button[data-test="pause"]');
	if (!button || R.done) return;
	R.done = true;
	R.parent = button.parentElement.getAttribute("class");
	const over = document.createElement("div");
	over.className = "over";
	Object.assign(over.style, { position: "absolute", inset: "0" });
	button.parentElement.appendChild(over);
	button.click();
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert!(
        r["parent"].as_str().unwrap().contains("_playButtonInner_"),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        !o.clicks.is_empty() && o.at(keys::PLAY_INNER, Position::Append).count() == 1
    });
    assert_eq!(o.clicks.last().unwrap().0, keys::PLAY);
    finish(luna, dir);
}

/// element-hider's right-click menu measures itself (a fixed element with a
/// `min-width`), then sits at `min(clientX, innerWidth - width - 10)`. A fresh
/// fixed element reported the whole window as its size, which put the menu in
/// the corner of the page.
#[test]
fn a_fixed_element_measures_its_declared_size_not_the_window() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (R.done || !document.body) return;
	R.done = true;
	const s = document.createElement("style");
	s.textContent = ".menu{position:fixed;min-width:180px;padding:8px 0}";
	document.head.appendChild(s);
	const m = document.createElement("div");
	m.className = "menu";
	document.body.appendChild(m);
	const r = m.getBoundingClientRect();
	R.menu = [r.width, r.height];	const f = document.createElement("div");
	f.style.cssText = "position:fixed;width:100%;height:100vh";
	document.body.appendChild(f);
	const fr = f.getBoundingClientRect();
	R.full = [fr.width, fr.height, window.innerWidth, window.innerHeight];
	const plain = document.createElement("div");
	document.body.appendChild(plain);
	R.plain = [plain.getBoundingClientRect().width, window.innerWidth];
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["menu"][0], json!(180), "{r}");
    assert_eq!(r["full"][0], r["full"][2], "{r}");
    assert_eq!(r["full"][1], r["full"][3], "{r}");
    assert_eq!(r["plain"][0], r["plain"][1], "{r}");
    finish(luna, dir);
}

/// radiant-lyrics appends its backdrop (`position: fixed`, `100vw` x
/// `100vh`, a canvas filling it) to `main` and sizes the canvas from it.
/// It measured `main`'s box, which changes when the now-playing view opens:
/// the canvas was made again at the new size and drew streaks. A fixed box
/// is the window's size wherever it sits, as in a browser.
#[test]
fn a_fixed_element_inside_an_app_element_measures_against_the_window() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "main", (main) => {
	if (R.done) return;
	const c = document.createElement("div");
	Object.assign(c.style, { position: "fixed", left: "0", top: "0", width: "100vw", height: "100vh", zIndex: "-3" });
	const k = document.createElement("canvas");
	Object.assign(k.style, { position: "absolute", left: "0", top: "0", width: "100%", height: "100%" });
	c.appendChild(k);
	main.appendChild(c);
	const r = c.getBoundingClientRect();
	const kr = k.getBoundingClientRect();
	const m = main.getBoundingClientRect();
	R.box = [r.width, r.height, kr.width, kr.height, window.innerWidth, window.innerHeight, m.width, m.height];
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let b = &r["box"];
    assert_ne!(
        (&b[6], &b[7]),
        (&b[4], &b[5]),
        "main is not the window: {r}"
    );
    assert_eq!((&b[0], &b[1]), (&b[4], &b[5]), "the fixed box: {r}");
    assert_eq!((&b[2], &b[3]), (&b[4], &b[5]), "the canvas in it: {r}");
    finish(luna, dir);
}

#[test]
fn selectors_cover_what_plugins_use() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (!document.querySelector('[data-test="footer-player"]') || R.done) return;
	const q = (s) => { try { return document.querySelectorAll(s).length; } catch (e) { return "ERR " + e.message; } };
	R.counts = {
		has: q('[role="row"]:has(> * [data-test="table-cell-title"])'),
		scope: document.querySelector('[data-type="media-table"]').querySelectorAll(":scope > div > [role=row], :scope > div > div > div > [role=row]").length,
		is: q(':is(#footerPlayer, #sidebar) a'),
		where: q(':where([data-test="tracklist-row"]):not([data-index="0"])'),
		nth: q('[data-test="tracklist-row"]:nth-child(2n+1)'),
		nthOfType: q('[data-test="tracklist-row"]:nth-of-type(2)'),
		dash: q('[lang|=en]'),
		tilde: q('[class~="enable-scrollbar-styles"]'),
		pseudo: q('[data-test="tracklist-row"]::before'),
		hover: q('[data-test="tracklist-row"]:hover'),
		escaped: q('.' + CSS.escape("is-now-playing")),
		bad: q('div[[['),
	};
	R.closest = document.querySelector('[data-test="footer-artist-name"]').closest("#footerPlayer")?.id ?? null;
	R.walker = (() => { const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT); let n = 0; while (w.nextNode()) n++; return n; })();
	R.html = (() => { const d = document.createElement("div"); d.innerHTML = '<p class="a">x &amp; <b>y</b></p><br><img src="/i.png">'; return [d.children.length, d.querySelector("b").textContent, d.innerHTML]; })();
	R.dataset = (() => { const d = document.createElement("div"); d.dataset.trackId = "5"; return d.getAttribute("data-track-id"); })();
	R.rect = document.querySelector('div[class*="sliderContainer"]').getBoundingClientRect().width;
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let c = &r["counts"];
    assert_eq!(c["has"], json!(3), "{c}");
    // The header row in its box, and the rows inside TIDAL's list
    // (`div > div[k] > div > row`).
    assert_eq!(c["scope"], json!(4), "{c}");
    assert!(c["is"].as_u64().unwrap() >= 2, "{c}");
    assert_eq!(c["where"], json!(2), "{c}");
    assert_eq!(c["pseudo"], json!(0));
    assert_eq!(c["hover"], json!(0));
    assert_eq!(c["dash"], json!(1));
    assert_eq!(c["tilde"], json!(1));
    assert_eq!(c["escaped"], json!(1));
    assert!(c["bad"].as_str().unwrap().starts_with("ERR"));
    assert_eq!(r["closest"], json!("footerPlayer"));
    assert!(r["walker"].as_u64().unwrap() > 5);
    assert_eq!(r["html"][0], json!(3));
    assert_eq!(r["html"][1], json!("y"));
    assert_eq!(
        r["html"][2],
        json!("<p class=\"a\">x &amp; <b>y</b></p><br><img src=\"/i.png\">")
    );
    assert_eq!(r["dataset"], json!("5"));
    assert_eq!(r["rect"].as_f64(), Some(110.0));
    finish(luna, dir);
}

// ------------------------------------------------------------------ real plugins

/// Real DOM plugins from their release URLs, unmodified, against the
/// sample anchor tree. Needs the network.
#[test]
#[ignore]
fn real_dom_plugins_work_end_to_end() {
    let dir = temp_dir("dom-real");
    let host = Arc::new(MockHost::default());
    {
        let mut s = host.snap.lock().unwrap();
        s.volume = 50.0;
        s.queue = ["1234", "1235", "1236"]
            .iter()
            .map(|id| QueueEntry {
                id: (*id).into(),
                uid: format!("u{id}"),
                kind: "track".into(),
            })
            .collect::<Vec<_>>()
            .into();
        s.current_index = Some(0);
        s.duration = Some(200.0);
    }
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(10);
    let luna = Luna::start(cfg, host.clone()).unwrap();
    let urls = [
        (
            "VolumeScroll",
            "https://github.com/Inrixia/luna-plugins/releases/download/latest/VolumeScroll",
        ),
        (
            "NativeFullscreen",
            "https://github.com/Inrixia/luna-plugins/releases/download/latest/NativeFullscreen",
        ),
        (
            "@jxnxsdev/total-play-time",
            "https://github.com/jxnxsdev/luna-plugins/releases/download/latest/jxnxsdev.total-play-time",
        ),
        (
            "@squadgazzz/scroll-to-playing",
            "https://github.com/squadgazzz/luna-plugins/releases/download/latest/squadgazzz.scroll-to-playing",
        ),
    ];
    for (_, u) in urls {
        luna.install(u);
    }
    let s = wait_until(&luna, 120, |s| {
        urls.iter().all(|(n, _)| {
            plugin(s, n).is_some_and(|p| {
                !matches!(p.status, PluginStatus::Loading | PluginStatus::Installing)
            })
        })
    });
    for (n, _) in urls {
        let p = plugin(&s, n).unwrap_or_else(|| {
            panic!(
                "{n} missing: {:?}",
                s.plugins.iter().map(|p| &p.name).collect::<Vec<_>>()
            )
        });
        eprintln!(
            "{n}: {:?}\n  logs: {:?}",
            p.status,
            p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
        assert_eq!(p.status, PluginStatus::Running, "{n}");
    }
    let mut t = tree();
    node(&mut t, keys::MAIN).scroll = Some([0.0, 3000.0]);
    // Row 2 is below the visible part of the page.
    for n in t.nodes.iter_mut() {
        if n.key.starts_with(&keys::row("album:55", 2))
            && let Some(r) = n.rect.as_mut()
        {
            r[1] = 720.0;
        }
    }
    luna.sync_dom(t.clone());
    // VolumeScroll: the wheel on the volume slider changes the volume.
    overlay_until(&luna, 20, |o| {
        o.host(keys::SLIDER)
            .is_some_and(|h| h.listens.contains(&"wheel".to_string()))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -120.0 }),
    });
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetVolume(60.0))
    });
    eprintln!("VolumeScroll: SetVolume(60) on wheel up");
    // total-play-time: its pill in the more container, with a time.
    let o = overlay_until(&luna, 20, |o| {
        o.at(keys::UTILITY_BUTTONS, Position::Prepend).any(|v| {
            v.attrs
                .iter()
                .any(|(k, val)| k == "id" && val == "totalPlayTimeParent")
                && v.children.first().is_some_and(|c| {
                    c.children
                        .first()
                        .and_then(|t| t.text.as_deref())
                        .is_some_and(|t| t.contains(':'))
                })
        })
    });
    let pill = o
        .at(keys::UTILITY_BUTTONS, Position::Prepend)
        .next()
        .unwrap();
    eprintln!(
        "total-play-time pill: {}",
        serde_json::to_string(pill).unwrap()
    );
    // scroll-to-playing: the playing row is highlighted, and on a track
    // change to a row out of view, the list scrolls to it.
    let playing = keys::row("album:55", 0);
    let o = overlay_until(&luna, 20, |o| {
        o.host(&playing)
            .is_some_and(|h| h.style.background_color.is_some())
    });
    eprintln!(
        "scroll-to-playing highlight: {:?}",
        o.host(&playing).unwrap().style
    );
    host.snap.lock().unwrap().current_index = Some(2);
    luna.event(LunaEvent::TrackChanged { id: "1236".into() });
    let o = overlay_until(&luna, 20, |o| {
        o.host(&keys::row("album:55", 2))
            .is_some_and(|h| h.style.background_color.is_some())
            && o.scroll_top.is_some()
    });
    eprintln!(
        "scroll-to-playing scroll: {:?} {:?}",
        o.scroll_top, o.scroll_to
    );
    assert_eq!(o.scroll_to.as_ref().unwrap().0, keys::row("album:55", 2));
    // NativeFullscreen: F11 toggles fullscreen.
    overlay_until(&luna, 10, |o| {
        o.host("")
            .is_some_and(|h| h.listens.contains(&"keydown".to_string()))
    });
    let f11 = || DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "F11", "code": "F11" }),
    };
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(true))
    });
    let o = overlay_until(&luna, 10, |o| o.hidden(keys::BAR));
    eprintln!(
        "NativeFullscreen: fullscreen on, top bar hidden={}",
        o.hidden(keys::BAR)
    );
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(false))
    });
    overlay_until(&luna, 10, |o| !o.hidden(keys::BAR));
    let s = wait_until(&luna, 5, |_| true);
    for (n, _) in urls {
        let p = plugin(&s, n).unwrap();
        assert_eq!(p.status, PluginStatus::Running, "{n}: {:?}", p.logs);
        assert!(p.uses_dom, "{n}");
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugins_render_react_use_sprites_click_hosts_and_receive_drops() {
    let (luna, _host, dir) = start(
        r##"
const React = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const holder = document.createElement("div");
holder.id = "holder";
lib.observe(unloads, '[data-test="track-info"]', (ti) => ti.append(holder));
// LuckyLP: a button container after the page's h1.
lib.observe(unloads, "div[data-track--page-id='album']", (page) => {
	const box = document.createElement("div");
	box.className = "luckyBtnContainer";
	box.innerHTML = '<button type="button"><svg viewBox="0 0 20 20"><use href="#player__shuffle"></use></svg></button>';
	page.querySelector("h1")?.insertAdjacentElement("afterend", box);
});
function Counter() {
	const [n, set] = React.useState(0);
	return React.createElement("button", { className: "cnt", onClick: () => set(n + 1) }, "n=", n);
}
createRoot(holder).render(
	React.createElement("div", { style: { color: "red" } }, React.createElement(Counter, null),
		React.createElement("svg", { viewBox: "0 0 24 24", strokeWidth: 2 }, React.createElement("use", { href: "#player__shuffle" }))),
);
document.addEventListener("drop", async (e) => {
	R.drop = [e.dataTransfer.files.length, e.dataTransfer.files[0].name, await e.dataTransfer.files[0].text()];
});
setTimeout(async () => {
	R.html = holder.innerHTML;
	document.querySelector(".cnt").click();
	document.querySelector('[data-test="lyrics-sync-button"]').click();
	R.menu = (await lib.ContextMenu.getCurrent())?.getAttribute("data-test") ?? null;
	setTimeout(() => (R.after = holder.querySelector(".cnt")?.textContent ?? null), 100);
}, 400);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 15, |r| r["after"].is_string());
    assert!(r["html"].as_str().unwrap().contains("n=0"), "{r}");
    assert_eq!(r["after"], json!("n=1"));
    assert_eq!(r["menu"], json!("contextmenu"));
    let o = overlay_until(&luna, 10, |o| {
        !o.clicks.is_empty() && o.at(keys::TRACK_INFO, Position::Append).count() > 0
    });
    assert_eq!(o.clicks.last().unwrap().0, keys::LYRICS_SYNC);
    // After the page's h1, as LuckyLP inserts its button.
    assert!(
        o.at(keys::PAGE_TITLE, Position::Append).count()
            + o.at(keys::PAGE_TITLE, Position::After).count()
            > 0,
        "{:#?}",
        o.injected
            .iter()
            .map(|i| (&i.anchor, i.position))
            .collect::<Vec<_>>()
    );
    let holder = o.at(keys::TRACK_INFO, Position::Append).next().unwrap();
    let svg = holder.children[0]
        .children
        .iter()
        .find(|c| c.tag == "svg")
        .expect("svg");
    let markup = svg.markup.as_deref().unwrap();
    // TIDAL's shuffle in the symbol's own 24-unit box.
    assert!(markup.contains("M18 14l4 4-4 4"), "{markup}");
    assert!(
        markup.contains(r#"<svg viewBox="0 0 24 24" width="100%" height="100%">"#),
        "{markup}"
    );
    assert!(markup.contains("stroke-width"), "{markup}");
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::PAGE.into()),
        kind: "drop".into(),
        data: json!({ "files": [{ "name": "a.txt", "type": "text/plain", "b64": "aGk=" }] }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["drop"].is_array());
    assert_eq!(r["drop"], json!([1, "a.txt", "hi"]));
    finish(luna, dir);
}

#[test]
fn a_page_change_renders_a_new_page_element() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "div[data-track--page-id='mycollection_albums']", (page) => {
	R.seen = 1;
	R.h1 = !!page.querySelector("h1");
});
"##,
    );
    luna.sync_dom(tree());
    let mut t = tree();
    node(&mut t, keys::PAGE)
        .attrs
        .iter_mut()
        .find(|(k, _)| k == "data-track--page-id")
        .unwrap()
        .1 = "mycollection_albums".into();
    luna.sync_dom(t);
    let r = results_until(&luna, &dir, 10, |r| r["seen"] == json!(1));
    assert_eq!(r["h1"], json!(true));
    finish(luna, dir);
}

/// betterFullscreen: `lib.observe` reports `.is-fullscreen.is-now-playing`
/// (as Luna does: the mutated parent and the matches inside it) and the
/// plugin swaps TIDAL's `_fullscreen_` container for its own React root.
#[test]
fn fullscreen_now_playing_can_be_replaced_by_a_plugin_view() {
    let (luna, _host, dir) = start(
        r##"
const react = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const swap = () => {
	const t = document.querySelector(".is-fullscreen.is-now-playing");
	R.seen = !!t;
	const e = t?.querySelector('[class^="_fullscreen_"]');
	if (!e || e.querySelector(".bf-player")) return;
	const r = document.createElement("div");
	r.className = e.className;
	e.parentNode.replaceChild(r, e);
	createRoot(r).render(react.createElement("div", { className: "bf-player", style: { position: "absolute", inset: 0 } }, "Lyrics here"));
	R.swapped = true;
};
lib.observe(unloads, ".is-fullscreen.is-now-playing", swap);
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    frame.shell.fullscreen = true;
    // The fullscreen view's first render: the shell arrives with both
    // classes, and the plugin finds it inside the mutated `#wimp`.
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["swapped"] == json!(true));
    assert_eq!(r["seen"], json!(true));
    let o = overlay_until(&luna, 10, |o| {
        serde_json::to_string(&o.injected)
            .unwrap()
            .contains("Lyrics here")
    });
    assert!(
        o.hidden(keys::NP_FULLSCREEN),
        "TIDAL's container is replaced"
    );
    // In its place in TIDAL's root, the plugin's box covers the window as
    // TIDAL's section does there (fixed, shown under `.is-fullscreen`).
    let swapped = o
        .injected
        .iter()
        .find(|i| {
            serde_json::to_string(&i.nodes)
                .unwrap()
                .contains("Lyrics here")
        })
        .unwrap();
    assert!(
        dom_anchors::in_root(&swapped.anchor, swapped.position),
        "{} {:?}",
        swapped.anchor,
        swapped.position
    );
    let style = &swapped.nodes[0].style;
    assert_eq!(style.position.as_deref(), Some("fixed"));
    assert_eq!(style.opacity.as_deref(), Some("1"));
    finish(luna, dir);
}

/// TIDAL 2.46.0 keeps its docked now-playing view (`section#nowPlaying.
/// _nowPlayingContainer_`) as a child of its root, hidden while closed,
/// and takes it out while the view is fullscreen, where `section.
/// _fullscreen_ > section#nowPlaying` holds the view. That removal is a
/// change to the root's own children, which Luna's `observe` reports with
/// the root, so an observer waiting for the root's classes (betterFullscreen's
/// `.is-fullscreen.is-now-playing`) runs as the view goes fullscreen,
/// whichever of the two came first.
#[test]
fn an_observer_waiting_for_the_roots_fullscreen_classes_runs_when_they_come() {
    let (luna, _host, dir) = start(
        r##"
R.calls = 0;
lib.observe(unloads, ".is-fullscreen.is-now-playing", (el) => {
	R.calls++;
	R.root = el.matches("#wimp > div[data-market-theme] > div");
	R.inside = !!el.querySelector('[class^="_fullscreen_"] > section[data-test="new-now-playing"] > header[class^="_header_"]');
	R.docked = !!el.querySelector(':scope > [class*="_nowPlayingContainer_"]');
});
"##,
    );
    let calls = |luna: &Luna, dir: &Path, n: u64| {
        results_until(luna, dir, 10, |r| r["calls"].as_u64() >= Some(n))
    };
    let settle = |luna: &Luna| {
        for _ in 0..5 {
            luna.sync(Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let fullscreen = |frame: &mut dom_anchors::Frame, on: bool| frame.shell.fullscreen = on;
    // The view open, then fullscreen.
    let mut frame = dom_anchors::sample_frame();
    let view = frame.now_playing.clone();
    luna.sync_dom(dom_anchors::build(&frame));
    settle(&luna);
    assert_eq!(dom_results(&dir)["calls"], json!(0));
    fullscreen(&mut frame, true);
    luna.sync_dom(dom_anchors::build(&frame));
    let r = calls(&luna, &dir, 1);
    assert_eq!(r["root"], json!(true));
    assert_eq!(r["inside"], json!(true), "the view is in `_fullscreen_`");
    assert_eq!(r["docked"], json!(false), "the docked view is out");
    // Back out, the view closed, then fullscreen first and the view after.
    fullscreen(&mut frame, false);
    luna.sync_dom(dom_anchors::build(&frame));
    frame.now_playing = None;
    luna.sync_dom(dom_anchors::build(&frame));
    fullscreen(&mut frame, true);
    luna.sync_dom(dom_anchors::build(&frame));
    settle(&luna);
    assert_eq!(dom_results(&dir)["calls"], json!(1));
    frame.now_playing = view;
    fullscreen(&mut frame, true);
    luna.sync_dom(dom_anchors::build(&frame));
    let r = calls(&luna, &dir, 2);
    assert_eq!(r["inside"], json!(true));
    finish(luna, dir);
}

/// TIDAL 2.46.0's root variables plugins build on: its stacking levels and
/// the profile gradient. betterFullscreen's player lost its whole
/// background (`radial-gradient(...), var(--user-profile-linear-gradient)`
/// is invalid while the variable is missing) and its backdrop fell back to
/// `z-index: -1` under the player, where TIDAL lays it at 10.
#[test]
fn tidals_root_variables_reach_plugin_styles() {
    let (luna, _host, dir) = start(
        r##"
const s = document.createElement("style");
s.textContent = ".p { position: fixed; inset: 0; background: radial-gradient(rgba(20, 20, 30, 0.9), #000), var(--user-profile-linear-gradient); } .b { position: absolute; inset: 0; z-index: var(--z-index-low, -1); } .c { position: relative; z-index: var(--z-index-high); }";
document.head.appendChild(s);
const p = document.createElement("div");
p.className = "p";
const b = document.createElement("div");
b.className = "b";
const c = document.createElement("div");
c.className = "c";
p.append(b, c);
document.body.appendChild(p);
R.low = getComputedStyle(document.documentElement).getPropertyValue("--z-index-low").trim();
R.z = [getComputedStyle(b).zIndex, getComputedStyle(c).zIndex];
R.bg = getComputedStyle(p).backgroundImage;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["bg"].is_string());
    assert_eq!(r["low"], json!("10"));
    assert_eq!(r["z"], json!(["10", "1000"]));
    assert!(r["bg"].as_str().unwrap().contains("160deg"), "{r}");
    finish(luna, dir);
}

/// TIDAL 2.46.0 marks its layout with the view's state, and plugins wait
/// for those classes with an attribute observer or match them exactly as
/// themes do: the player bar is `_player_8c80ae6 nowPlayingVisible
/// _notFullscreen_60c5a14` over the docked view and carries the playback
/// state, the docked view drops `_nowPlayingHidden_`, the sidebar wrapper,
/// the header and the page box take their `_nowPlayingVisible_`, and the
/// main container its `_isFullscreen_`.
#[test]
fn observers_waiting_for_tidals_state_classes_see_them_change() {
    let (luna, _host, dir) = start(
        r##"
R.seen = {};
const watch = (name, sel) => {
	const el = document.querySelector(sel);
	if (!el) return void (R.missing = (R.missing ?? []).concat(name));
	new MutationObserver(() => {
		R.seen[name] = (R.seen[name] ?? []).concat(el.className + (el.dataset.testPlaybackState ? "|" + el.dataset.testPlaybackState : ""));
	}).observe(el, { attributes: true, attributeFilter: ["class", "data-test-playback-state"] });
};
const ready = () => {
	if (R.ready) return;
	R.ready = true;
	watch("footer", "#footerPlayer");
	watch("docked", '[class*="_nowPlayingContainer_"]');
	watch("sidebar", '[data-test="main-layout-sidebar-wrapper"]');
	watch("header", "#mainHeader");
	watch("main", "#main");
	watch("container", '[class*="_mainContainer_"]');
};
lib.observe(unloads, "#footerPlayer", ready);
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    let view = frame.now_playing.take();
    if let Some(p) = frame.player.as_mut() {
        p.now_playing_visible = false;
        p.playing = false;
    }
    luna.sync_dom(dom_anchors::build(&frame));
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    // The view opens and the song plays.
    frame.now_playing = view;
    if let Some(p) = frame.player.as_mut() {
        p.now_playing_visible = true;
        p.playing = true;
    }
    luna.sync_dom(dom_anchors::build(&frame));
    // Each state is seen before the next comes (an observer reads the
    // element when it runs, after every change queued by then).
    results_until(&luna, &dir, 10, |r| {
        r["seen"]["footer"].as_array().is_some_and(|a| {
            a.iter()
                .any(|c| c.as_str().is_some_and(|c| c.ends_with("|PLAYING")))
        })
    });
    // Fullscreen.
    frame.shell.fullscreen = true;
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"]["container"].as_array().is_some_and(|a| {
            a.iter().any(|c| {
                c.as_str()
                    .is_some_and(|c| c.contains("_isFullscreen_75c1ba4"))
            })
        })
    });
    assert_eq!(r["missing"], Value::Null, "{r}");
    let saw = |name: &str, want: &str| {
        assert!(
            r["seen"][name]
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c.as_str() == Some(want))),
            "{name} never became {want:?}: {}",
            r["seen"][name]
        );
    };
    saw(
        "footer",
        "_player_8c80ae6 nowPlayingVisible _notFullscreen_60c5a14|PLAYING",
    );
    let has = |name: &str, class: &str| {
        assert!(
            r["seen"][name].as_array().is_some_and(|a| a
                .iter()
                .any(|c| c.as_str().is_some_and(|c| c.split(' ').any(|w| w == class)))),
            "{name} never had {class}: {}",
            r["seen"][name]
        );
    };
    has("sidebar", "_nowPlayingVisible_03bf437");
    has("header", "_nowPlayingVisible_03bf437");
    has("main", "_nowPlayingVisible_b1bf3e8");
    // The docked view was shown (no `_nowPlayingHidden_`) before it left.
    assert!(
        r["seen"]["docked"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c
                .as_str()
                .is_some_and(|c| !c.contains("_nowPlayingHidden_")))),
        "{}",
        r["seen"]["docked"]
    );
    finish(luna, dir);
}

/// Tidal Tags (dev): the player bar's quality badge has TIDAL's
/// `span > span` label with the stream's figures, as TIDAL's reads; the
/// plugin writes the stream's format into it and colours the badge, and
/// both reach the native pill.
#[test]
fn quality_badge_label_takes_plugin_text_and_colour() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[class*="_qualityBadgeContainer"]', (t) => {
	const n = t.querySelector("span > span");
	if (!n || R.native !== undefined) return;
	R.native = n.textContent;
	t.style.color = "#ffd432";
	n.textContent = "24bit 96kHz FLAC";
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["native"].is_string());
    assert_eq!(r["native"], json!("16-bit 44.1kHz"));
    let o = overlay_until(&luna, 10, |o| {
        o.host(keys::QUALITY_LABEL)
            .is_some_and(|h| h.text.as_deref() == Some("24bit 96kHz FLAC"))
    });
    assert_eq!(
        o.host(keys::QUALITY).unwrap().style.color.as_deref(),
        Some("rgb(255, 212, 50)")
    );
    finish(luna, dir);
}

/// A plugin reading the quality badge before the song's stream says reads
/// TIDAL 2.46.0's words for a badge with no bit depth or sample rate yet:
/// "LOW" for a song expected lossless.
#[test]
fn a_badge_waiting_for_its_stream_reads_tidals_words() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[class*="_qualityBadgeContainer"]', (t) => {
	if (R.words !== undefined) return;
	R.words = t.querySelector("span > span")?.textContent ?? null;
	R.quality = t.querySelector("a")?.getAttribute("data-test-quality-badge-streaming-quality") ?? null;
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    let player = frame.player.as_mut().unwrap();
    player.quality = None;
    player.expected_quality = Some("LOSSLESS".into());
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["words"].is_string());
    assert_eq!(
        (&r["words"], &r["quality"]),
        (&json!("LOW"), &json!("LOSSLESS"))
    );
    finish(luna, dir);
}

/// @minseokk7/korean-translation: walks the document's text nodes and
/// replaces the ones a dictionary knows (`node.textContent = ...` on a text
/// node). The text of a host element's own text node reaches the overlay as
/// the element's text.
#[test]
fn a_plugin_replacing_a_text_node_of_an_anchor_reaches_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const dict = { Home: "홈", Album: "앨범" };
lib.observe(unloads, '[data-test="sidebar-music"]', () => {
	if (R.hits !== undefined) return;
	const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
	let n; let hits = 0;
	while ((n = w.nextNode())) {
		const t = n.textContent.trim();
		if (dict[t]) { n.textContent = n.textContent.replace(t, dict[t]); hits++; }
	}
	R.hits = hits;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["hits"].is_number());
    let o = overlay_until(&luna, 10, |o| {
        o.host(&keys::sidebar_item_label("sidebar-music"))
            .is_some_and(|h| h.text.as_deref() == Some("홈"))
    });
    assert!(r["hits"].as_u64().unwrap() >= 1, "{r}");
    assert_eq!(
        o.host(keys::PAGE_TITLE).and_then(|h| h.text.as_deref()),
        Some("앨범")
    );
    finish(luna, dir);
}

/// Ids and page names TIDAL's own DOM carries and themes name: Inrixia's
/// example colours `[id^="titleCell"]`, `[id^="albumText"]`,
/// `[id^="timeColumn"]`, `[id^="contributionText"]` and
/// `[id^="headerColumn"]`; Catppuccin and the cotton candy family style
/// `#playbackControlBar button`; Solo Luna scopes rules with
/// `[data-test-page-name="my-collection"]`. The cotton candy family's
/// `#nowPlayingHeader` (TIDAL 2.43.2's, with `[data-test=header-container]`)
/// is in no page of TIDAL 2.46.0, nor here.
#[test]
fn ids_and_page_names_of_tidals_dom_reach_the_anchors() {
    let (luna, _host, dir) = start(
        r##"
const sels = [
	'[id^="titleCell"]', '[id^="albumText"]', '[id^="timeColumn"]', '[id^="contributionText"]',
	'[id^="headerColumn"]', '#playbackControlBar button', '[data-test=play-controls] button',
	'[data-test-page-name="album"]', '[data-test-page-name="my-collection"]',
	'#nowPlayingHeader', '[data-test=header-container]',
];
setInterval(() => { R.q = sels.map((s) => document.querySelectorAll(s).length); }, 40);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| {
        r["q"].as_array().is_some_and(|q| q[0].as_u64() > Some(0))
    });
    let q: Vec<u64> = r["q"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap())
        .collect();
    assert!(q[..7].iter().all(|n| *n > 0), "{q:?}");
    assert_eq!((q[7], q[8]), (1, 0), "an album page, not the collection");
    assert_eq!(
        (q[9], q[10]),
        (0, 0),
        "TIDAL 2.43.2's now-playing header ids"
    );
    let mut frame = dom_anchors::sample_frame();
    frame.page.as_mut().unwrap().page_id = Some("mycollection_playlists".into());
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| {
        r["q"].as_array().is_some_and(|q| q[8].as_u64() == Some(1))
    });
    assert_eq!(r["q"][7], json!(0), "{r}");
    finish(luna, dir);
}

/// TidalLyricsTranslate puts its Translate button after the now-playing
/// view's full screen button, in `div._actionButtons_` (here: before the
/// close chevron, beside it there). TIDAL 2.46.0's
/// header there is a plain `header._header_` in
/// `section#nowPlaying[data-test=new-now-playing]`, so the nearest element
/// above the button with an id or a `data-test` is the section; Tidalfast's
/// header still carried TIDAL 2.43.2's `#nowPlayingHeader[data-test=
/// header-container]`.
#[test]
fn the_now_playing_header_has_no_id_of_its_own_as_in_tidal_2_46() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="new-now-playing-close"]', (close) => {
	if (R.done) return;
	const b = document.createElement("button");
	b.className = close.className;
	b.textContent = "Translate";
	close.insertAdjacentElement("beforebegin", b);
	const header = b.closest("header");
	R.header = header ? [header.id, header.getAttribute("data-test"), header.className] : null;
	R.near = b.parentElement.closest("[id], [data-test]")?.getAttribute("data-test") ?? null;
	R.done = true;
});
"##,
    );
    luna.sync_dom(dom_anchors::build(&dom_anchors::sample_frame()));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["header"][0], json!(""), "{r}");
    assert_eq!(r["header"][1], json!(null), "{r}");
    assert!(
        r["header"][2].as_str().unwrap().starts_with("_header_"),
        "{r}"
    );
    assert_eq!(r["near"], json!("new-now-playing"), "{r}");
    finish(luna, dir);
}

/// A disabled button in the player is at half strength, as TIDAL 2.46.0's
/// own rule (`:is(._player_8c80ae6 button):disabled { opacity: .5 }`) draws
/// it: OneClickLyrics' button for a song without words, drawn and read.
#[test]
fn a_disabled_button_in_the_player_is_at_half_strength() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "[class*='_sliderContainer']", (slider) => {
	if (R.done) return;
	const off = document.createElement("button");
	off.id = "off";
	off.disabled = true;
	slider.before(off);
	const on = document.createElement("button");
	on.id = "on";
	slider.before(on);
	R.opacity = [getComputedStyle(off).opacity, getComputedStyle(on).opacity];
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["opacity"], json!(["0.5", "1"]), "{r}");
    let by_id = |o: &DomOverlay, id: &str| {
        o.injected
            .iter()
            .flat_map(|i| &i.nodes)
            .find(|n| n.attrs.iter().any(|(k, v)| k == "id" && v == id))
            .map(|n| n.style.opacity.clone())
    };
    let o = overlay_until(&luna, 10, |o| by_id(o, "off").is_some());
    assert_eq!(by_id(&o, "off"), Some(Some("0.5".into())));
    assert_eq!(by_id(&o, "on"), Some(None));
    finish(luna, dir);
}

/// TIDAL 2.46.0's markup where plugins read or rewrite it: a card's picture
/// is `img._image_145331a` and a mix card's `img[data-test=mix-image]`, a
/// shortcut tile's picture `img._image_145331a` (uwu-ifier replaces each and
/// sets its `object-fit`); a track list's heading row is
/// `div._tableHeader_ebaf366._smallText_a636ed6` (Tidal Tags' headings there
/// read 12 px); the header's Play and Shuffle pills have no `aria-label`
/// and the collection filter field is named Search (korean-translation
/// translates what it finds).
#[test]
fn pictures_headings_pills_and_the_filter_carry_tidals_markup() {
    let (luna, _host, dir) = start(
        r##"
setInterval(() => {
	const q = (s) => document.querySelector(s);
	R.found = {
		album: q('[data-test^="album-card-"] img')?.className ?? null,
		mix: q('[data-test^="mix-card-"] img')?.getAttribute("data-test") ?? null,
		tile: q("#playlist-shortcut > img")?.className ?? null,
		heading: q('[data-test="media-table"] > div > [role="row"]')?.className ?? null,
		play: q('[data-test="play-all"]')?.getAttribute("aria-label") ?? "none",
		shuffle: q('[data-test="shuffle-all"]')?.getAttribute("aria-label") ?? "none",
		filter: q('[data-test="playlist-filter-input"]')?.getAttribute("aria-label") ?? null,
	};
}, 40);
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        let card = |kind, id: &str, x: f32| dom_anchors::GridCard {
            rect: Some([x, 300.0, 173.0, 240.0]),
            cover: Some([x, 300.0, 173.0, 173.0]),
            shelf: Some(0),
            item: Some(dom_anchors::CardItem {
                kind,
                id: id.into(),
                ..dom_anchors::CardItem::default()
            }),
            ..dom_anchors::GridCard::default()
        };
        page.shelves = vec![Some([240.0, 300.0, 900.0, 240.0])];
        page.shelf_modules = vec!["NEW_ALBUM_SUGGESTIONS".into()];
        page.cards = vec![
            card(dom_anchors::CardKind::Album, "alb1", 240.0),
            card(dom_anchors::CardKind::Mix, "mix1", 430.0),
        ];
        page.tiles = vec![dom_anchors::Tile {
            rect: Some([240.0, 100.0, 300.0, 56.0]),
            image: Some([240.0, 100.0, 56.0, 56.0]),
            title: "Mine".into(),
            ..dom_anchors::Tile::default()
        }];
        page.filter = Some(dom_anchors::FilterInput {
            rect: Some([900.0, 200.0, 200.0, 32.0]),
            value: String::new(),
        });
    }
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| {
        r["found"]["album"].is_string() && r["found"]["filter"].is_string()
    });
    let f = &r["found"];
    assert_eq!(f["album"], json!("_image_145331a"), "{f}");
    assert_eq!(f["mix"], json!("mix-image"), "{f}");
    assert_eq!(f["tile"], json!("_image_145331a"), "{f}");
    assert_eq!(
        f["heading"],
        json!("_tableHeader_ebaf366 _smallText_a636ed6"),
        "{f}"
    );
    assert_eq!(
        (&f["play"], &f["shuffle"]),
        (&json!("none"), &json!("none")),
        "{f}"
    );
    assert_eq!(f["filter"], json!("Search"), "{f}");
    finish(luna, dir);
}

/// An image's and a source's `srcset` and `sizes` reflect their attributes,
/// as in Chromium: uwu-ifier sets `img.srcset = ""` and `img.sizes = ""` on
/// every picture it replaces, which TIDAL's page sees as attribute changes.
/// Here they were plain properties: no attribute, no mutation record.
#[test]
fn an_images_srcset_and_sizes_reflect_their_attributes() {
    let (luna, _host, dir) = start(
        r##"
const img = document.createElement("img");
img.setAttribute("srcset", "a.jpg 160w, b.jpg 320w");
document.body.append(img);
const seen = [];
new MutationObserver((rs) => {
	for (const r of rs) seen.push([r.attributeName, r.oldValue]);
	R.seen = seen;
}).observe(img, { attributes: true, attributeOldValue: true });
R.before = img.srcset;
img.srcset = "";
img.sizes = "";
R.after = [img.getAttribute("srcset"), img.getAttribute("sizes"), img.srcset, img.sizes];
const s = document.createElement("source");
s.srcset = "c.jpg 2x";
R.source = s.getAttribute("srcset");
R.div = "srcset" in document.createElement("div");
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"].as_array().is_some_and(|s| s.len() >= 2)
    });
    assert_eq!(r["before"], json!("a.jpg 160w, b.jpg 320w"), "{r}");
    assert_eq!(r["after"], json!(["", "", "", ""]), "{r}");
    assert_eq!(
        r["seen"],
        json!([["srcset", "a.jpg 160w, b.jpg 320w"], ["sizes", null]]),
        "{r}"
    );
    assert_eq!(r["source"], json!("c.jpg 2x"), "{r}");
    assert_eq!(r["div"], json!(false), "{r}");
    finish(luna, dir);
}

/// custom-fonts: its settings look up `._title_a453dad`, which in TIDAL is
/// the app's name in the title bar, and rewrite it ("TIDALuna" and its
/// Comic Sans tag). It used to be the Plugins page's heading, which TIDAL's
/// is not; the page's heading is left alone.
#[test]
fn the_title_bars_name_can_be_rewritten() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "._title_a453dad", (e) => {
	if (R.before !== undefined) return;
	R.before = e.textContent;
	e.innerHTML = 'TIDA<b><span style="color: #32f4ff;">Luna</span></b>';
	e.innerHTML += ' <span style="color:#1E90FF">Comic Sans Edition</span>';
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    frame.page = Some(dom_anchors::Page {
        page_id: Some("settings".into()),
        title: Some("Plugins".into()),
        ..dom_anchors::Page::default()
    });
    frame.lists.clear();
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["before"].is_string());
    assert_eq!(r["before"], json!("Tidalfast"));
    let o = overlay_until(&luna, 10, |o| {
        serde_json::to_string(&o.injected)
            .unwrap()
            .contains("Comic Sans Edition")
    });
    let text = o.host(keys::BAR_TITLE).unwrap().text.clone();
    assert!(text.unwrap_or_default().starts_with("TIDA"));
    assert!(o.at(keys::BAR_TITLE, Position::Append).count() > 0);
    // The rewritten words are the title's own text, drawn once: not also
    // an element beside it.
    assert!(
        o.at(keys::BAR_TITLE, Position::Append)
            .all(|n| n.tag != "#text"),
        "{:?}",
        o.at(keys::BAR_TITLE, Position::Append)
            .map(|n| (&n.tag, &n.text))
            .collect::<Vec<_>>()
    );
    assert!(o.host(keys::PAGE_TITLE).is_none_or(|h| h.text.is_none()));
    finish(luna, dir);
}

/// quickExclusiveMode registers its `observe` after an `await`, when the
/// player bar is already there: the existing match is reported once, as
/// TIDAL's UI mounting after the plugins would.
#[test]
fn observing_after_the_ui_is_there_reports_existing_matches_once() {
    let (luna, _host, dir) = start(
        r##"
const t = setInterval(() => {
	if (!document.querySelector('[class*="_utilityButtons_"]')) return;
	clearInterval(t);
	R.calls = 0;
	lib.observe(unloads, '[class*="_utilityButtons_"]', (e) => {
		R.calls += 1;
		if (e.querySelector(".qem")) return;
		const b = document.createElement("button");
		b.className = "qem";
		b.textContent = "Exclusive";
		e.appendChild(b);
	});
}, 20);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::UTILITY_BUTTONS, Position::Append)
            .next()
            .is_some()
    });
    assert_eq!(o.at(keys::UTILITY_BUTTONS, Position::Append).count(), 1);
    let r = results_until(&luna, &dir, 10, |r| {
        r["calls"].as_u64().is_some_and(|n| n >= 1)
    });
    assert_eq!(r["calls"], json!(1), "{r}");
    finish(luna, dir);
}

/// M9: the overlay carries at most 10000 plugin nodes and 4 MB of plugin
/// and theme CSS, whatever a plugin adds; the rest is left out with a log
/// line, and a sheet over the cap is not even parsed (a 5 MB one took
/// seconds).
#[test]
fn the_overlay_is_capped() {
    let (luna, _host, dir) = start(
        r#"
const small = document.createElement("style");
small.textContent = ".capped-a { color: red; }";
document.head.appendChild(small);
const big = document.createElement("style");
big.textContent = ".capped-b { color: blue; }\n".repeat(200000);
document.head.appendChild(big);
for (let i = 0; i < 12000; i++) {
	const d = document.createElement("div");
	d.className = "capped-a";
	d.textContent = "row " + i;
	document.body.appendChild(d);
}
R.done = true;
"#,
    );
    luna.sync_dom(tree());
    fn count(n: &[super::dom::VNode]) -> usize {
        n.iter().map(|v| 1 + count(&v.children)).sum()
    }
    let o = overlay_until(&luna, 20, |o| {
        o.injected.iter().map(|i| count(&i.nodes)).sum::<usize>() > 1000
    });
    let nodes: usize = o.injected.iter().map(|i| count(&i.nodes)).sum();
    assert!((1000..=10000).contains(&nodes), "{nodes}");
    assert!(o.css_bytes > 0 && o.css_bytes <= 4 << 20, "{}", o.css_bytes);
    let s = luna.snapshot();
    let logs: Vec<&LogLine> = s
        .runtime_log
        .iter()
        .chain(s.plugins.iter().flat_map(|p| p.logs.iter()))
        .collect();
    let logged = |what: &str| logs.iter().any(|l| l.message.contains(what));
    assert!(logged("more than 10000 elements"), "{logs:?}");
    assert!(logged("capped at 4 MB"), "{logs:?}");
    finish(luna, dir);
}

/// L10: a plugin writes to the clipboard only right after the user did
/// something (here a click it listens for), never on its own.
#[test]
fn clipboard_writes_need_a_user_action() {
    let (luna, _host, dir) = start(
        r#"
await navigator.clipboard.writeText("unasked").then(() => (R.unasked = "written"), (e) => (R.unasked = e.message));
document.addEventListener("click", () => {
	navigator.clipboard.writeText("asked").then(() => (R.asked = "written"), (e) => (R.asked = e.message));
	R.copy = document.execCommand("copy");
});
R.ready = true;
"#,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    assert!(
        r["unasked"].as_str().unwrap().contains("NotAllowedError"),
        "{r}"
    );
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["asked"].is_string());
    assert_eq!(r["asked"], "written", "{r}");
    finish(luna, dir);
}

/// What makes star-ratings, quick-queue and qr-share work: an element's
/// own box arrives with its events (star-ratings reads which half of a
/// star was clicked), custom properties set on an ancestor colour its SVG
/// (`--star-on` per widget), and an `<img>` without a CSS size reports
/// its natural size once it loaded (qr-share's QR code).
#[test]
fn plugin_elements_get_their_box_svg_variables_and_natural_image_size() {
    let (luna, _host, dir) = start(
        r##"
const c = document.createElement("canvas");
c.width = 30;
c.height = 20;
c.getContext("2d").fillRect(0, 0, 30, 20);
const box = document.createElement("div");
box.style.position = "fixed";
box.style.top = "0";
box.style.left = "0";
const img = document.createElement("img");
img.src = c.toDataURL("image/png");
img.style.maxWidth = "90%";
const stars = document.createElement("span");
stars.style.setProperty("--star-on", "red");
stars.innerHTML = '<svg id="star" viewBox="0 0 32 32"><path fill="var(--star-on, blue)" d="M0 0h32v32H0z"/><path fill="var(--nobody, green)" d="M0 0h1v1H0z"/><path fill="var(--wave-color-solid-accent-fill)" d="M0 0h1v1H0z"/></svg>';
stars.addEventListener("click", (e) => {
	const r = e.target.getBoundingClientRect();
	R.clicked = [e.target.id, r.left, r.top, r.width, r.height];
}, true);
box.append(img, stars);
document.body.append(box);
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 20, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .flat_map(|n| n.children.iter())
            .any(|n| n.tag == "img" && n.natural == Some([30.0, 20.0]))
    });
    let svg = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .flat_map(|n| n.children.iter())
        .flat_map(|n| n.children.iter())
        .find(|n| n.tag == "svg")
        .expect("the svg")
        .clone();
    let markup = svg.markup.clone().unwrap_or_default();
    assert!(markup.contains(r#"fill="red""#), "{markup}");
    // Names nobody defines keep their fallback or stay for the UI's theme.
    assert!(markup.contains("var(--nobody, green)"), "{markup}");
    assert!(
        markup.contains("var(--wave-color-solid-accent-fill)"),
        "{markup}"
    );
    luna.dom_event(DomEvent {
        target: DomTarget::Node(svg.id),
        kind: "click".into(),
        data: json!({ "clientX": 15, "clientY": 25, "rect": [10, 20, 30, 40] }),
    });
    let r = results_until(&luna, &dir, 10, |r| !r["clicked"].is_null());
    assert_eq!(r["clicked"], json!(["star", 10, 20, 30, 40]));
    finish(luna, dir);
}

/// AltPlay sizes the quality badge from `offsetWidth` of the elements it
/// put in it: until an element has a box of its own that is the anchor's,
/// which made the badge grow on every pass. The UI tells each element its
/// drawn width (the `measure` event); the plugin then reads that.
#[test]
fn a_plugin_element_reads_the_width_the_ui_measured() {
    let (luna, _host, dir) = start(
        r##"
const mark = document.createElement("span");
mark.textContent = "mark";
document.body.append(mark);
setInterval(() => { R.width = mark.offsetWidth; }, 40);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 20, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .any(|n| n.tag == "span")
    });
    let id = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|n| n.tag == "span")
        .expect("the span")
        .id;
    let before = results_until(&luna, &dir, 10, |r| r["width"].is_number());
    assert_ne!(before["width"].as_f64(), Some(21.0), "{before}");
    luna.dom_event(DomEvent {
        target: DomTarget::Node(id),
        kind: "measure".into(),
        data: json!({ "rect": [5, 6, 21, 18] }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["width"].as_f64() == Some(21.0));
    assert_eq!(r["width"].as_f64(), Some(21.0));
    finish(luna, dir);
}

/// Plugins that rewrite their elements all the time (tidal-genres
/// re-renders its badges on every mutation it observes, star-ratings
/// repaints its stars every 1.5 s) made every overlay build match every
/// element of the page again (~100 ms each, a third of a core on an album
/// page). Inserting and removing nodes, attributes no selector names,
/// changes to elements outside the document and the same text written
/// again keep the other elements' cascades; the same text is no mutation.
#[test]
fn plugin_rewrites_keep_the_page_cascade() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".rw span { color: rgb(255, 0, 0); } .rw .b { color: rgb(0, 0, 255); }";
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
for (let i = 0; i < 50; i++) box.append(document.createElement("span"));
document.body.append(box);
const label = document.createElement("b");
label.textContent = "3.5";
box.append(label);
let records = 0;
new MutationObserver((m) => { records += m.length; }).observe(box, { childList: true, subtree: true, characterData: true });
getComputedStyle(box.firstChild).color;
const before = D.cascadeResets;
for (let i = 0; i < 10; i++) {
	const s = document.createElement("span");
	s.setAttribute("title", "x" + i);
	s.className = i % 2 ? "b" : "";
	box.append(s);
	getComputedStyle(s).color;
	s.remove();
	box.firstChild.setAttribute("width", String(i));
	getComputedStyle(box.firstChild).color;
}
label.textContent = "3.5";
const late = document.createElement("span");
late.className = "b";
box.append(late);
R.color = getComputedStyle(late).color;
R.first = getComputedStyle(box.firstChild).color;
R.resets = D.cascadeResets - before;
await Promise.resolve();
setTimeout(() => { R.records = records; R.done = true; }, 50);
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["color"], json!("rgb(0, 0, 255)"), "{r}");
    assert_eq!(r["first"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    // 10 appends + 10 removals + the last append; the same text again is none.
    assert_eq!(r["records"], json!(21), "{r}");
    finish(luna, dir);
}

/// Lunallena names the style attribute in a selector
/// (`[class*="_sidebarWrapper_"][style="width: 240px;"]`), and TIDAL's
/// progress bar writes `style="--progress: .4123"` every second: that one
/// change made every overlay build match every element again (~40 ms a
/// second, 6% of a core at rest). A style change can only change what
/// matches the element itself and what is inside it, the siblings after it
/// for a selector that reads the style and a sibling combinator, and the
/// ancestors for one that reads the style inside `:has`: `:has` and position
/// selectors that do not read the style (Catppuccin Mocha Blue's, Snusehunden's)
/// change nothing, so only that subtree is matched again.
#[test]
fn a_style_change_keeps_the_cascade_of_the_rest_of_the_page() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '.rw [style*="mark"] { color: rgb(255, 0, 0); } .rw [style*="mark"] > i { color: rgb(0, 0, 255); } body:has(.rw span) .rw { font-weight: bold; } .rw span:nth-child(odd) { margin-top: 1px; }';
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
const items = [];
for (let i = 0; i < 40; i++) {
	const s = document.createElement("span");
	s.append(document.createElement("i"));
	box.append(s);
	items.push(s);
}
document.body.append(box);
getComputedStyle(items[0]).color;
getComputedStyle(items[7]).color;
const before = D.cascadeResets;
const runs = D.cascadeRuns;
for (let i = 0; i < 10; i++) {
	items[5].setAttribute("style", "--progress: " + i);
	getComputedStyle(items[7]).color;
}
R.quiet = D.cascadeRuns - runs;
items[5].setAttribute("style", "mark: 1");
R.span = getComputedStyle(items[5]).color;
R.kid = getComputedStyle(items[5].firstChild).color;
R.other = getComputedStyle(items[7]).color;
items[5].setAttribute("style", "--progress: 1");
R.span2 = getComputedStyle(items[5]).color;
R.kid2 = getComputedStyle(items[5].firstChild).color;
R.resets = D.cascadeResets - before;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["span"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["kid"], json!("rgb(0, 0, 255)"), "{r}");
    assert_ne!(r["other"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["span2"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["kid2"], json!("rgb(0, 0, 255)"), "{r}");
    assert_eq!(r["quiet"], json!(0), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// The host's tree with the progress bar's inline style set to `style`.
fn with_progress(t: &mut HostTree, style: &str) {
    let bar = node(t, keys::PROGRESS_BAR);
    bar.attrs.retain(|(k, _)| k != "style");
    bar.attrs.push(("style".into(), style.into()));
}

/// TIDAL's progress bar sets `--progress` every second. While no rule reads
/// that variable the overlay is not built again for it (every build came out
/// the same, and took most of the runtime's time at rest), and a script
/// still reads the new value.
#[test]
fn a_variable_no_rule_reads_builds_no_overlay() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '[data-test="progress-bar"] { color: rgb(1, 2, 3); }';
document.head.append(st);
let base = null;
const check = () => {
	const bar = document.querySelector('[data-test="progress-bar"]');
	if (!bar || R.done) return;
	const v = bar.getAttribute("style") ?? "";
	if (base === null) {
		// The overlay with the sheet and the bar is out before counting.
		setTimeout(() => {
			base = D.overlayBuilds;
			R.ready = true;
		}, 300);
		return;
	}
	if (!v.includes("0.5000")) return;
	setTimeout(() => {
		R.builds = D.overlayBuilds - base;
		R.value = getComputedStyle(bar).getPropertyValue("--progress");
		R.done = true;
	}, 300);
};
new MutationObserver(check).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["style"] });
"##,
    );
    let mut t = tree();
    // No pictures: an `<img>` that finishes loading builds an overlay of
    // its own.
    for n in &mut t.nodes {
        n.attrs.retain(|(k, _)| k != "src");
    }
    with_progress(&mut t, "--progress: 0.0000");
    luna.sync_dom(t.clone());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    for v in ["0.1000", "0.2000", "0.3000", "0.4000", "0.5000"] {
        with_progress(&mut t, &format!("--progress: {v}"));
        luna.sync_dom(t.clone());
        std::thread::sleep(Duration::from_millis(60));
    }
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["builds"], json!(0), "{r}");
    assert_eq!(r["value"], json!("0.5000"), "{r}");
    finish(luna, dir);
}

/// A rule that reads the variable gets each new value at once, and so does
/// one a plugin adds after values went by unread.
#[test]
fn a_variable_a_rule_reads_reaches_the_overlay_at_once() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = '[data-test="progress-bar"] { color: rgb(1, 2, 3); }';
document.head.append(st);
new MutationObserver(() => {
	const bar = document.querySelector('[data-test="progress-bar"]');
	if (bar && (bar.getAttribute("style") ?? "").includes("0.3000") && !R.added) {
		R.added = true;
		const reads = document.createElement("style");
		reads.textContent = '[data-test="progress-bar"] { opacity: var(--progress); }';
		document.head.append(reads);
	}
}).observe(document.body, { subtree: true, attributes: true, attributeFilter: ["style"] });
"##,
    );
    let opacity = |o: &DomOverlay| {
        o.hosts
            .get(keys::PROGRESS_BAR)
            .and_then(|h| h.style.opacity.clone())
    };
    let mut t = tree();
    for v in ["0.1000", "0.2000", "0.3000"] {
        with_progress(&mut t, &format!("--progress: {v}"));
        luna.sync_dom(t.clone());
        std::thread::sleep(Duration::from_millis(60));
    }
    overlay_until(&luna, 10, |o| opacity(o).as_deref() == Some("0.3000"));
    for v in ["0.4000", "0.7000"] {
        with_progress(&mut t, &format!("--progress: {v}"));
        luna.sync_dom(t.clone());
        overlay_until(&luna, 10, |o| opacity(o).as_deref() == Some(v));
    }
    finish(luna, dir);
}

/// The overlay's theme version stays while only a plugin's own element
/// changes (the UI keeps the looks it resolved from the theme), and moves
/// when an app element's look changes.
#[test]
fn the_theme_version_moves_with_the_apps_look_not_a_plugins_element() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = '[data-test="footer-player"] { color: rgb(1, 2, 3); }';
document.head.append(st);
const tick = document.createElement("div");
tick.textContent = "0";
document.body.append(tick);
let n = 0;
const id = setInterval(() => {
	tick.textContent = String(++n);
	if (n === 5) document.querySelector('[data-test="footer-player"]')?.classList.add("lit");
	if (n >= 8) clearInterval(id);
}, 120);
unloads.add(() => clearInterval(id));
"##,
    );
    luna.sync_dom(tree());
    // The counter's words in an overlay, with its theme version.
    let count = |o: &DomOverlay| {
        o.injected
            .iter()
            .flat_map(|i| &i.nodes)
            .flat_map(|n| &n.children)
            .find_map(|c| c.text.as_deref()?.parse::<u32>().ok())
    };
    let mut seen: Vec<(u32, u64)> = Vec::new();
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if let Some(n) = count(&o)
            && seen.last().is_none_or(|(m, _)| *m != n)
        {
            seen.push((n, o.theme_version));
        }
        if seen.last().is_some_and(|(n, _)| *n >= 8) {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    // From the second tick: the first can come before the app's look has
    // had its first pass on a slow machine.
    let before: Vec<u64> = seen
        .iter()
        .filter(|(n, _)| (2..5).contains(n))
        .map(|s| s.1)
        .collect();
    let after: Vec<u64> = seen.iter().filter(|(n, _)| *n >= 5).map(|s| s.1).collect();
    assert!(before.len() >= 2 && !after.is_empty(), "{seen:?}");
    assert!(before.windows(2).all(|w| w[0] == w[1]), "{seen:?}");
    assert!(after.iter().all(|t| *t > before[0]), "{seen:?}");
    finish(luna, dir);
}

/// An overlay build makes again only the app elements' entries whose inputs
/// changed: with nothing changed none, a class a plugin adds that element's,
/// a listener on the page every element's under it.
#[test]
fn an_overlay_build_makes_again_only_the_entries_that_changed() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const go = () => {
	const bar = document.querySelector('[data-test="progress-bar"]');
	if (!bar || R.done) return;
	R.done = true;
	D.build();
	let n = D.hostBuilds;
	D.build();
	R.same = D.hostBuilds - n;
	bar.classList.add("lit");
	n = D.hostBuilds;
	const lit = D.build().hosts["progress-bar"]?.classes ?? [];
	R.classed = [D.hostBuilds - n > 0 && D.hostBuilds - n < 10, lit];
	n = D.hostBuilds;
	const before = Object.keys(D.build().hosts).length;
	document.body.addEventListener("click", () => {});
	const all = D.build();
	R.listened = [D.hostBuilds - n > before, (all.hosts["progress-bar"]?.listens ?? []).includes("click")];
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["listened"].is_array());
    assert_eq!(r["same"], json!(0), "{r}");
    assert_eq!(r["classed"], json!([true, ["lit"]]), "{r}");
    assert_eq!(r["listened"], json!([true, true]), "{r}");
    finish(luna, dir);
}

/// What the speed work keeps from one overlay to the next (an app element's
/// entry while what it is made of is the same, the looks the UI resolved
/// from a theme while only a plugin's own element changes) never shows a
/// state that is gone. A class, the pointer, the window's size and an
/// inline custom property a rule reads each reach the look drawn on the
/// first frame after the runtime has them, while a plugin's own counter
/// ticks beside the bar (whose overlays keep the looks).
#[test]
fn every_change_of_an_app_element_reaches_its_drawn_look_at_once() {
    use crate::ui::plugin_dom as pd;
    use egui::Color32;
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `
[data-test="footer-player"] { background-color: var(--fill, rgb(10, 20, 30)); }
@media (max-width: 1000px) { [data-test="footer-player"] { background-color: rgb(100, 110, 120); } }
[data-test="footer-player"].lit { background-color: rgb(40, 50, 60); }
[data-test="footer-player"]:hover { background-color: rgb(70, 80, 90); }
`;
document.head.append(st);
const tick = document.createElement("div");
tick.textContent = "0";
document.body.append(tick);
let n = 0;
const id = setInterval(() => (tick.textContent = String(++n)), 40);
unloads.add(() => clearInterval(id));
"##,
    );
    let ctx = egui::Context::default();
    // One UI frame as the app draws it: the newest overlay taken, then the
    // bar's look and the counter the frame's overlay holds.
    let frame = |t: &HostTree| {
        let [w, h] = t.viewport;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mut out = (None, None);
        let mut output = ctx.run_ui(input, |ui| {
            pd::begin_frame(ui.ctx(), Some(&luna), rect, &crate::model::Page::Home);
            out = (
                pd::anchor_look(keys::FOOTER),
                pd::overlay().map(|o| o.version),
            );
        });
        output.textures_delta.clear();
        out
    };
    let fill_of = |o: &DomOverlay| {
        o.hosts
            .get(keys::FOOTER)
            .and_then(|h| h.style.background_color.as_deref())
            .map(|c| c.replace(' ', ""))
    };
    let mut t = tree();
    t.viewport = [1280.0, 800.0];
    luna.sync_dom(t.clone());
    // Frames kept the looks while only the counter changed.
    let mut kept = 0;
    let mut last: Option<(u64, Arc<pd::AnchorLook>)> = None;
    let mut step = |name: &str, t: &HostTree, want: [u8; 3]| {
        let css = format!("rgb({},{},{})", want[0], want[1], want[2]);
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            luna.sync(Duration::from_secs(5));
            let has = fill_of(&luna.dom_overlay());
            let (look, version) = frame(t);
            if let (Some((v, before)), Some(look), Some(version)) = (&last, &look, version)
                && *v != version
                && Arc::ptr_eq(before, look)
            {
                kept += 1;
            }
            if let (Some(look), Some(version)) = (&look, version) {
                last = Some((version, Arc::clone(look)));
            }
            let fill = Color32::from_rgb(want[0], want[1], want[2]);
            if has.as_deref() == Some(css.as_str()) {
                assert_eq!(
                    look.and_then(|l| l.fill),
                    Some(fill),
                    "{name}: the runtime has {css}, the frame after drew another look"
                );
                // It stays while the counter ticks (each tick a new overlay
                // that keeps the looks).
                let end = Instant::now() + Duration::from_millis(300);
                while Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(15));
                    let (look, version) = frame(t);
                    assert_eq!(look.as_ref().and_then(|l| l.fill), Some(fill), "{name}");
                    if let (Some((v, before)), Some(look), Some(version)) = (&last, &look, version)
                        && *v != version
                        && Arc::ptr_eq(before, look)
                    {
                        kept += 1;
                    }
                    if let (Some(look), Some(version)) = (look, version) {
                        last = Some((version, look));
                    }
                }
                return;
            }
            assert!(
                Instant::now() < end,
                "{name}: {css} never reached the overlay ({has:?})"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    step("start", &t, [10, 20, 30]);
    let class = |t: &mut HostTree, lit: bool| {
        let bar = node(t, keys::FOOTER);
        for (k, v) in bar.attrs.iter_mut() {
            if k == "class" {
                *v = v
                    .split_whitespace()
                    .filter(|c| *c != "lit")
                    .collect::<Vec<_>>()
                    .join(" ");
                if lit {
                    v.push_str(" lit");
                }
            }
        }
    };
    class(&mut t, true);
    luna.sync_dom(t.clone());
    step("a class the app sets", &t, [40, 50, 60]);
    class(&mut t, false);
    luna.sync_dom(t.clone());
    step("the class taken away", &t, [10, 20, 30]);
    let hover = |key: &str| DomEvent {
        target: DomTarget::Host(key.into()),
        kind: "hover".into(),
        data: Value::Null,
    };
    luna.dom_event(hover(keys::FOOTER));
    step("the pointer over the bar", &t, [70, 80, 90]);
    luna.dom_event(hover(""));
    step("the pointer gone", &t, [10, 20, 30]);
    t.viewport = [900.0, 800.0];
    luna.sync_dom(t.clone());
    step("a narrow window", &t, [100, 110, 120]);
    t.viewport = [1280.0, 800.0];
    luna.sync_dom(t.clone());
    step("the window wide again", &t, [10, 20, 30]);
    for fill in [[130, 140, 150], [150, 160, 170]] {
        let bar = node(&mut t, keys::FOOTER);
        bar.attrs.retain(|(k, _)| k != "style");
        bar.attrs.push((
            "style".into(),
            format!("--fill: rgb({}, {}, {})", fill[0], fill[1], fill[2]),
        ));
        luna.sync_dom(t.clone());
        step("an inline custom property a rule reads", &t, fill);
    }
    assert!(kept > 0, "no frame kept the looks while the counter ticked");
    // Luna off: the window's state starts over.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        pd::begin_frame(ui.ctx(), None, ui.max_rect(), &crate::model::Page::Home);
    });
    output.textures_delta.clear();
    finish(luna, dir);
}

/// A theme's `vw` and `vh` are the window's: the frame drawn at a new window
/// size has the theme's lengths at that size, in the anchor's look and in
/// the layers painted on its box. A resize changes nothing in the overlay
/// (its rules say `20vh` at any size), so the runtime publishes none: what
/// the UI resolved for the old size must not stay (Lunallena's
/// `padding-left: 20vh` on the card shelves).
#[test]
fn lengths_in_vw_and_vh_follow_the_window_size_at_once() {
    use crate::ui::plugin_dom as pd;
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `
[data-test="footer-player"] { background-color: rgb(10, 20, 30); padding-left: 20vh; border-radius: 1vw; border: 2px solid rgb(200, 0, 0); }
`;
document.head.append(st);
"##,
    );
    let ctx = egui::Context::default();
    // One UI frame at window size `[w, h]`: the bar's look, and the corners
    // of the border its layers paint.
    let frame = |[w, h]: [f32; 2]| {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mut look = None;
        let mut output = ctx.run_ui(input, |ui| {
            pd::begin_frame(ui.ctx(), Some(&luna), rect, &crate::model::Page::Home);
            look = pd::anchor_look(keys::FOOTER);
            let bar = egui::Rect::from_min_size(egui::pos2(0.0, h - 90.0), egui::vec2(w, 90.0));
            pd::paint_host_layers(ui, keys::FOOTER, bar);
        });
        output.textures_delta.clear();
        let border = output.shapes.iter().find_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.stroke.width > 0.0 => Some(r.corner_radius.nw),
            _ => None,
        });
        (
            look.as_ref().and_then(|l| l.padding.map(|p| p[3])),
            look.as_ref().and_then(|l| l.corners.map(|c| c.nw)),
            border,
            pd::overlay().map(|o| o.version),
        )
    };
    let mut t = tree();
    t.viewport = [1280.0, 800.0];
    luna.sync_dom(t.clone());
    overlay_until(&luna, 15, |o| {
        o.hosts
            .get(keys::FOOTER)
            .is_some_and(|h| h.style.padding_left.is_some())
    });
    let (padding, corners, border, version) = frame(t.viewport);
    assert_eq!(
        (padding, corners, border),
        (Some(160.0), Some(13), Some(13)),
        "at 1280 x 800"
    );
    // The window shrinks: the UI tells the runtime, whose overlay stays as
    // it was.
    t.viewport = [1000.0, 400.0];
    luna.sync_dom(t.clone());
    luna.sync(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    luna.sync(Duration::from_secs(5));
    let (padding, corners, border, now) = frame(t.viewport);
    assert_eq!(now, version, "a resize alone publishes no new overlay");
    assert_eq!(
        (padding, corners, border),
        (Some(80.0), Some(10), Some(10)),
        "at 1000 x 400: the lengths are the new window's"
    );
    // And back.
    t.viewport = [1280.0, 800.0];
    luna.sync_dom(t.clone());
    let (padding, corners, border, _) = frame(t.viewport);
    assert_eq!(
        (padding, corners, border),
        (Some(160.0), Some(13), Some(13))
    );
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        pd::begin_frame(ui.ctx(), None, ui.max_rect(), &crate::model::Page::Home);
    });
    output.textures_delta.clear();
    finish(luna, dir);
}

/// A theme with `::before` rules, root variables and keyframes costs nothing
/// per overlay build while none of them change (the page's clock rewrites the
/// tree every second, and Cotton Candy declares hundreds of variables), and
/// a changed variable is worked out again.
#[test]
fn an_overlay_build_keeps_the_pseudo_boxes_variables_and_keyframes_it_has() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
const sheet = (c) => ':root { --a: ' + c + '; --b: var(--a); } @keyframes k { from { opacity: 0 } to { opacity: 1 } } [data-test="footer-player"]::before { background-color: var(--b); } [data-test="footer-player"] { animation: k 2s linear infinite; }';
st.textContent = sheet("#ff0000");
document.head.append(st);
const go = () => {
	if (!document.querySelector('[data-test="footer-player"]') || R.done) return;
	const first = D.computeOverlayStyles();
	const base = [D.pseudoRuns, D.rootResolves];
	let last;
	for (let i = 0; i < 5; i++) last = D.computeOverlayStyles();
	R.quiet = [D.pseudoRuns - base[0], D.rootResolves - base[1]];
	R.first = [first.vars["--b"], first.pseudo.size, Object.keys(first.keyframes)];
	R.same = last.vars === first.vars && last.keyframes.k === first.keyframes.k;
	st.textContent = sheet("#00ff00");
	const changed = D.computeOverlayStyles();
	R.changed = [changed.vars["--b"], D.rootResolves - base[1], [...changed.pseudo.values()][0]?.before?.["background-color"] ?? null];
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["first"], json!(["#ff0000", 1, ["k"]]), "{r}");
    assert_eq!(r["quiet"], json!([0, 0]), "{r}");
    assert_eq!(r["same"], json!(true), "{r}");
    assert_eq!(r["changed"][0], json!("#00ff00"), "{r}");
    assert_eq!(r["changed"][1], json!(1), "{r}");
    finish(luna, dir);
}

/// What a style change can reach besides the element and what is inside
/// it: the siblings after it (`[style] + b`) and the ancestors that have it
/// (`:has(> [style])`) are matched again, nothing else is.
#[test]
fn a_style_change_rematches_the_siblings_and_ancestors_it_can_reach() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '.rw [style*="mark"] + b { color: rgb(255, 0, 0); } .rw p:has(> [style*="mark"]) { color: rgb(0, 128, 0); }';
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
const rows = [];
for (let i = 0; i < 30; i++) {
	const p = document.createElement("p");
	const a = document.createElement("span");
	const b = document.createElement("b");
	p.append(a, b);
	box.append(p);
	rows.push([p, a, b]);
}
document.body.append(box);
for (const [p, a, b] of rows) { getComputedStyle(p).color; getComputedStyle(a).color; getComputedStyle(b).color; }
const runs = D.cascadeRuns;
rows[3][1].setAttribute("style", "mark: 1");
R.sibling = getComputedStyle(rows[3][2]).color;
R.parent = getComputedStyle(rows[3][0]).color;
R.otherSibling = getComputedStyle(rows[4][2]).color;
R.otherParent = getComputedStyle(rows[4][0]).color;
R.reran = D.cascadeRuns - runs;
rows[3][1].setAttribute("style", "--x: 1");
R.sibling2 = getComputedStyle(rows[3][2]).color;
R.parent2 = getComputedStyle(rows[3][0]).color;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["sibling"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["parent"], json!("rgb(0, 128, 0)"), "{r}");
    assert_ne!(r["otherSibling"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["otherParent"], json!("rgb(0, 128, 0)"), "{r}");
    assert_ne!(r["sibling2"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["parent2"], json!("rgb(0, 128, 0)"), "{r}");
    assert!(
        r["reran"].as_u64().unwrap() < 20,
        "only the reach of the change is matched again, not 90 elements: {r}"
    );
    finish(luna, dir);
}

fn find_vnode<'a>(
    nodes: &'a [super::dom::VNode],
    pred: &dyn Fn(&super::dom::VNode) -> bool,
) -> Option<&'a super::dom::VNode> {
    nodes.iter().find_map(|n| {
        if pred(n) {
            Some(n)
        } else {
            find_vnode(&n.children, pred)
        }
    })
}

fn overlay_vnode(
    o: &DomOverlay,
    pred: &dyn Fn(&super::dom::VNode) -> bool,
) -> Option<super::dom::VNode> {
    o.injected
        .iter()
        .find_map(|i| find_vnode(&i.nodes, pred))
        .cloned()
}

/// multipleplaylists: a checkbox the user ticks stays ticked (the overlay
/// carries its new state, so the drawn box does not untick itself on the
/// next frame), and a click on the text of its `<label>` ticks it, as in a
/// browser; `:checked` finds both. No host tree is sent: nothing else
/// changes the page (the tree's pictures loading did, a moment after the
/// start), so the overlay that shows a tick is the tick's own.
#[test]
fn ticked_checkboxes_stay_ticked_and_labels_toggle_them() {
    let (luna, _host, dir) = start(
        r#"
const list = document.createElement("div");
list.id = "pl";
document.body.appendChild(list);
list.innerHTML = '<label style="display: flex"><input type="checkbox" data-playlist-id="a"><div><div>Mix A</div></div></label>';
list.innerHTML += '<label style="display: flex"><input type="checkbox" data-playlist-id="b"><div><div>Mix B</div></div></label>';
const picked = () => Array.from(document.querySelectorAll('#pl input[type="checkbox"]:checked')).map((i) => i.dataset.playlistId);
let clicks = 0;
document.addEventListener("click", () => clicks++);
document.addEventListener("change", () => {
	R.checked = picked();
	R.clicks = clicks;
});
R.ready = true;
"#,
    );
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let is_box = |id: &'static str| {
        move |n: &super::dom::VNode| {
            n.tag == "input"
                && n.attrs
                    .iter()
                    .any(|(k, v)| k == "data-playlist-id" && v == id)
        }
    };
    let o = overlay_until(&luna, 10, |o| overlay_vnode(o, &is_box("b")).is_some());
    let a = overlay_vnode(&o, &is_box("a")).unwrap();
    // The UI ticks box a.
    luna.dom_event(DomEvent {
        target: DomTarget::Node(a.id),
        kind: "change".into(),
        data: json!({ "value": "true", "checked": true }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["checked"].is_array());
    assert_eq!(r["checked"], json!(["a"]), "{r}");
    overlay_until(&luna, 10, |o| {
        overlay_vnode(o, &is_box("a")).and_then(|n| n.value) == Some("true".into())
    });
    // A click on the words "Mix B" ticks box b.
    let text_b = overlay_vnode(&o, &|n| {
        n.tag == "div"
            && (n.text.as_deref() == Some("Mix B")
                || n.children
                    .iter()
                    .any(|c| c.tag == "#text" && c.text.as_deref() == Some("Mix B")))
    })
    .unwrap();
    luna.dom_event(DomEvent {
        target: DomTarget::Node(text_b.id),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    let r = results_until(&luna, &dir, 10, |r| {
        r["checked"].as_array().is_some_and(|c| c.len() == 2)
    });
    assert_eq!(r["checked"], json!(["a", "b"]), "{r}");
    // The click on the text and the one the label passes to its box.
    assert_eq!(r["clicks"], json!(2), "{r}");
    overlay_until(&luna, 10, |o| {
        overlay_vnode(o, &is_box("b")).and_then(|n| n.value) == Some("true".into())
    });
    finish(luna, dir);
}

/// tidal-genres observes every track row again after each change on the
/// page: as in a browser, observing a target already observed is nothing
/// new (no second first report), and observing never makes the page's
/// styles be worked out again (each time was a full recascade).
#[test]
fn observing_an_observed_target_again_changes_nothing() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const row = document.createElement("div");
row.style.height = "20px";
document.body.append(row);
getComputedStyle(row).color;
let calls = 0;
const io = new IntersectionObserver((entries) => { calls += entries.length; });
io.observe(row);
setTimeout(() => {
	R.first = calls;
	getComputedStyle(row).color;
	const before = D.cascadeResets;
	io.observe(row);
	io.observe(row);
	getComputedStyle(row).color;
	R.resets = D.cascadeResets - before;
	setTimeout(() => {
		R.after = calls;
		R.done = true;
	}, 300);
}, 300);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["first"], json!(1), "{r}");
    assert_eq!(r["after"], json!(1), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// tidal-genres and star-ratings change classes on their own badges many
/// times a second: with no selector that looks at siblings or children
/// (`+`, `~`, `:has`, `:nth-child`...), only the changed element and what
/// is inside it are matched again, not the whole page (a full recascade of
/// a 117-song playlist took ~200 ms each time). With a `:has` rule the
/// ancestors it may match are matched again too, and with a sibling rule
/// the changed element's siblings (no whole recascade either).
#[test]
fn class_changes_rematch_only_the_changed_subtree() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".rm span { color: rgb(255, 0, 0); } .rm.on span { color: rgb(0, 0, 255); } .rm .hot { color: rgb(0, 255, 0); }";
document.head.append(st);
const page = document.createElement("div");
for (let i = 0; i < 200; i++) page.append(document.createElement("p"));
document.body.append(page);
const box = document.createElement("div");
box.className = "rm";
for (let i = 0; i < 5; i++) box.append(document.createElement("span"));
document.body.append(box);
getComputedStyle(box.firstChild).color;
getComputedStyle(page.firstChild).color;
const before = D.cascadeResets;
const seen = [];
for (let i = 0; i < 6; i++) {
	box.className = i % 2 ? "rm on" : "rm";
	box.lastChild.className = i % 3 ? "hot" : "";
	seen.push(getComputedStyle(box.firstChild).color, getComputedStyle(box.lastChild).color);
}
R.seen = seen;
R.resets = D.cascadeResets - before;
const has = document.createElement("style");
has.textContent = ".rm:has(.hot) span { font-weight: 700; }";
document.head.append(has);
box.lastChild.className = "hot";
R.bold = getComputedStyle(box.firstChild).fontWeight;
const before2 = D.cascadeResets;
box.lastChild.className = "";
R.weight = getComputedStyle(box.firstChild).fontWeight;
box.lastChild.className = "hot";
R.bold2 = getComputedStyle(box.firstChild).fontWeight;
box.lastChild.remove();
R.weight2 = getComputedStyle(box.firstChild).fontWeight;
R.resets2 = D.cascadeResets - before2;
const sib = document.createElement("style");
sib.textContent = ".rm span + span { font-style: italic; }";
document.head.append(sib);
getComputedStyle(box.firstChild).color;
const before3 = D.cascadeResets;
box.lastChild.className = "x";
getComputedStyle(box.firstChild).color;
R.resets3 = D.cascadeResets - before3;
R.italic = [...box.children].map((e) => getComputedStyle(e).fontStyle);
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let (red, blue, green) = ("rgb(255, 0, 0)", "rgb(0, 0, 255)", "rgb(0, 255, 0)");
    assert_eq!(
        r["seen"],
        // `.rm.on span` is more specific than `.rm .hot`.
        json!([
            red, red, blue, blue, red, green, blue, blue, red, green, blue, blue
        ]),
        "{r}"
    );
    assert_eq!(r["resets"], json!(0), "{r}");
    // Without the rule, the weight TIDAL's `body` gives everything (500).
    assert_eq!(r["bold"], json!("700"), "{r}");
    assert_eq!(r["weight"], json!("500"), "{r}");
    assert_eq!(r["bold2"], json!("700"), "{r}");
    assert_eq!(r["weight2"], json!("500"), "{r}");
    // `:has` rules: the ancestors they may match are matched again, not
    // the page.
    assert_eq!(r["resets2"], json!(0), "{r}");
    // A sibling rule: the siblings are matched again, not the page.
    assert_eq!(r["resets3"], json!(0), "{r}");
    assert_eq!(
        r["italic"],
        json!(["", "italic", "italic", "italic"]),
        "{r}"
    );
    finish(luna, dir);
}

/// copy-lyrics selects by dragging: a press and a release on text selected
/// the text between them for `getSelection()` (it was a stub that never held
/// anything), and its `textarea.select()` + `execCommand("copy")` trick reads
/// the textarea. A click that did not travel selects nothing.
#[test]
fn a_drag_over_text_selects_it_for_get_selection() {
    let (luna, _host, dir) = start(
        r##"
const seen = [];
const find = (t) => [...document.querySelectorAll("*")].find((e) => e.children.length === 0 && e.textContent === t);
document.addEventListener("mouseup", () => {
	const sel = window.getSelection();
	const r = sel.rangeCount > 0 ? sel.getRangeAt(0) : null;
	seen.push({ text: String(sel), count: sel.rangeCount, hits: r ? ['[data-test="footer-track-title"]', '[data-test="footer-artist-name"]'].map((q) => { const e = document.querySelector(q); return e ? r.intersectsNode(e) : null; }) : null });
	R.seen = seen;
});
document.addEventListener("mousedown", () => {
	R.down = (R.down ?? 0) + 1;
	if (R.part === undefined) {
		const range = document.createRange();
		range.selectNodeContents(find("Song One"));
		range.setStart(find("Song One").firstChild, 5);
		R.part = range.toString();
	}
});
const ta = document.createElement("textarea");
ta.value = "copied text";
document.body.appendChild(ta);
ta.select();
R.control = [String(window.getSelection()), window.getSelection().type];
window.getSelection().removeAllRanges();
R.cleared = String(window.getSelection());
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    assert_eq!(r["control"], json!(["copied text", "Range"]), "{r}");
    assert_eq!(r["cleared"], json!(""), "{r}");
    let event = |target: &str, kind: &str, dragged: bool| {
        luna.dom_event(DomEvent {
            target: DomTarget::Host(target.into()),
            kind: kind.into(),
            data: json!({ "clientX": 5, "clientY": 5, "button": 0, "dragged": dragged }),
        });
    };
    // Title to artist, dragged: both, from the first text to the last.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::ARTIST, "mouseup", true);
    // A click on the title that did not travel: nothing selected.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::TITLE_SPAN, "mouseup", false);
    // A drag inside the title: the title.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::TITLE_SPAN, "mouseup", true);
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"].as_array().is_some_and(|a| a.len() == 3)
    });
    let seen = r["seen"].as_array().unwrap();
    let text = seen[0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Song One") && text.ends_with("Artist"),
        "{r}"
    );
    assert_eq!(seen[0]["hits"], json!([true, true]), "{r}");
    assert_eq!(seen[1]["count"], json!(0), "{r}");
    assert_eq!(seen[1]["text"], json!(""), "{r}");
    assert_eq!(seen[2]["text"], json!("Song One"), "{r}");
    assert_eq!(seen[2]["hits"], json!([true, false]), "{r}");
    assert_eq!(r["down"], json!(3), "{r}");
    assert_eq!(r["part"], json!("One"), "{r}");
    finish(luna, dir);
}

/// lyrics-everywhere's CatJam bubble: its tail is two CSS triangles, the
/// element's own `::before` and `::after` (a `content` of `""`, no size, a
/// 10 px transparent border with a coloured top and no bottom). They come
/// out as the element's first and last child with their style, the four
/// border widths included; a pseudo-element with `content: none` or
/// `display: none` (the plugin's other mode) is not there at all.
#[test]
fn a_plugin_elements_pseudo_boxes_are_its_first_and_last_children() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `
#bubble { position: fixed; bottom: 100px; left: 20px; border: 3px solid #111; }
#bubble::after { content: ""; position: absolute; bottom: -10px; left: 20px; width: 0; height: 0;
  border: 10px solid transparent; border-top-color: #111; border-bottom: 0; }
#bubble::before { content: "\\"q\\""; position: absolute; color: #fff; }
#plain::before, #plain::after { display: none; content: none; }
#plain::after { color: red; }
`;
document.head.appendChild(st);
const b = document.createElement("div");
b.id = "bubble";
b.textContent = "line";
document.body.appendChild(b);
const p = document.createElement("div");
p.id = "plain";
p.textContent = "pill";
document.body.appendChild(p);
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .flat_map(|i| &i.nodes)
            .any(|n| n.children.len() == 3)
    });
    let nodes: Vec<&super::dom::VNode> = o.injected.iter().flat_map(|i| &i.nodes).collect();
    let by_id = |id: &str| {
        *nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "id" && v == id))
            .unwrap_or_else(|| panic!("no #{id}"))
    };
    let bubble = by_id("bubble");
    let tags: Vec<&str> = bubble.children.iter().map(|c| c.tag.as_str()).collect();
    assert_eq!(tags, ["::before", "#text", "::after"], "{bubble:?}");
    assert_eq!(
        bubble.children[0]
            .children
            .first()
            .and_then(|t| t.text.as_deref()),
        Some("\"q\"")
    );
    let after = &bubble.children[2].style;
    assert_eq!(after.position.as_deref(), Some("absolute"));
    assert_eq!(after.bottom.as_deref(), Some("-10px"));
    assert_eq!(after.border_width.as_deref(), Some("10px 10px 0px 10px"));
    assert_eq!(
        after.border_color.as_deref(),
        Some("rgb(17, 17, 17) transparent transparent transparent")
    );
    assert!(bubble.style.border_width.is_none(), "one width all round");
    let plain = by_id("plain");
    assert_eq!(plain.children.len(), 1, "{plain:?}");
    finish(luna, dir);
}

/// The `font` shorthand reaches the overlay: `font: inherit` (a button
/// reset, koreansearch's result rows) is said outright when no plugin
/// style above gives a value, so the UI keeps the page's font rather than
/// the browser's control font; a full shorthand gives its parts.
#[test]
fn the_font_shorthand_and_an_outright_inherit_reach_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `#reset { font: inherit; } #full { font: italic bold 12px/1.5 Georgia, serif; }`;
document.head.appendChild(st);
for (const id of ["reset", "full"]) {
	const b = document.createElement("button");
	b.id = id;
	b.textContent = id;
	document.body.appendChild(b);
}
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.injected.iter().flat_map(|i| &i.nodes).count() >= 2
    });
    let nodes: Vec<&super::dom::VNode> = o.injected.iter().flat_map(|i| &i.nodes).collect();
    let by_id = |id: &str| {
        *nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "id" && v == id))
            .unwrap_or_else(|| panic!("no #{id}"))
    };
    let reset = &by_id("reset").style;
    assert_eq!(reset.font_family.as_deref(), Some("inherit"));
    assert_eq!(reset.font_size.as_deref(), Some("inherit"));
    let full = &by_id("full").style;
    assert_eq!(full.font_style.as_deref(), Some("italic"));
    assert_eq!(full.font_weight.as_deref(), Some("bold"));
    assert_eq!(full.font_size.as_deref(), Some("12px"));
    assert_eq!(full.font_family.as_deref(), Some("Georgia, serif"));
    finish(luna, dir);
}

/// The hosts whose text can be selected are those whose nearest `user-select`
/// up the tree (the element's own, else its parent's, `auto` looking
/// further) is not `none`. It is worked out once for each element a build
/// sees, not by every host over its whole chain; the set is the one every
/// host's own walk gives, under rules at the page, a panel and a host.
#[test]
fn the_selectable_hosts_are_those_whose_nearest_user_select_allows_it() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
body { user-select: none; }
[data-test="footer-player"] { user-select: auto; }
[data-test="footer-track-title"] { user-select: text; }
[data-test="footer-artist-name"] { -webkit-user-select: all; }
[data-test="tracklist-row"] { user-select: none; }
[data-test="tracklist-row"] [data-test="track-info"] { user-select: contain; }
`;
document.head.append(st);
const poll = () => {
	if (!D.hosts || D.hosts.size < 10) return setTimeout(poll, 20);
	const said = (el) => {
		for (let n = el; n instanceof Element; n = n.parentNode) {
			const d = D.cascadeOf(n, true);
			const v = String(d?.get("user-select") ?? d?.get("-webkit-user-select") ?? "auto").replace(/!important/i, "").trim().toLowerCase();
			if (v === "auto") continue;
			return v !== "none";
		}
		return false;
	};
	const got = D.computeOverlayStyles().selectable;
	let yes = 0;
	let no = 0;
	const wrong = [];
	for (const [key, el] of D.hosts) {
		if (!el.isConnected) continue;
		const want = said(el);
		if (want) yes++;
		else no++;
		if (got.has(el) !== want) wrong.push(key);
	}
	R.yes = yes;
	R.no = no;
	R.wrong = wrong;
	R.done = true;
};
poll();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["wrong"], json!([]), "{r}");
    assert!(r["yes"].as_u64().unwrap() > 0, "{r}");
    assert!(r["no"].as_u64().unwrap() > 0, "{r}");
    finish(luna, dir);
}

/// copy-lyrics' drag, as a browser selects it: the UI says where in the
/// lines' text the press and the release were (`textPoint`, UTF-16
/// offsets), and the selection holds exactly the characters between them,
/// whichever way the drag went. The host its rule makes selectable is
/// marked, with its `::selection` colours, so the UI can draw the
/// highlight.
#[test]
fn a_drag_selects_the_characters_between_the_measured_points() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = '[data-test="footer-track-title"] { user-select: text; } ::selection { background: #48003c; color: #fff; }';
document.head.appendChild(st);
const seen = [];
document.addEventListener("mouseup", () => {
	seen.push(String(window.getSelection()));
	R.seen = seen;
});
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.hosts.get(keys::TITLE_SPAN).is_some_and(|h| h.selectable)
    });
    let title = o.hosts.get(keys::TITLE_SPAN).unwrap();
    assert_eq!(
        title
            .selection
            .as_ref()
            .and_then(|s| s.background_color.as_deref()),
        Some("rgb(72, 0, 60)"),
        "{title:?}"
    );
    assert!(
        !o.hosts.get(keys::ARTIST).is_some_and(|h| h.selectable),
        "a host no rule makes selectable is not"
    );
    let event = |kind: &str, offset: usize| {
        luna.dom_event(DomEvent {
            target: DomTarget::Host(keys::TITLE_SPAN.into()),
            kind: kind.into(),
            data: json!({
                "clientX": 5, "clientY": 5, "button": 0, "dragged": kind == "mouseup",
                "textPoint": { "key": keys::TITLE_SPAN, "offset": offset },
            }),
        });
    };
    // "Song One": from after "So" to before "ne", then the same backwards.
    event("mousedown", 2);
    event("mouseup", 6);
    event("mousedown", 6);
    event("mouseup", 2);
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"].as_array().is_some_and(|a| a.len() == 2)
    });
    assert_eq!(r["seen"], json!(["ng O", "ng O"]), "{r}");
    finish(luna, dir);
}

/// uwu-ifier rewrites the document's text once at load and in nodes added
/// later; in TIDAL the page is there when a plugin loads. With the app's
/// first host tree awaited, a plugin enabled at start sees the page's text
/// at load (`wait_for_dom`); without it the document is still empty.
#[test]
fn a_plugin_enabled_at_start_finds_the_page_in_the_document_when_it_loads() {
    let (luna, _host, dir) = start(
        r#"const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
let n = 0;
while (w.nextNode()) n++;
R.texts = n;
R.runs = (R.runs ?? 0) + 1;
"#,
    );
    luna.shutdown();
    assert_eq!(dom_results(&dir)["texts"], json!(0));
    // Restarted with the wait, and the app's tree arriving after the plugin
    // would have started.
    let mut cfg = config(&dir);
    cfg.wait_for_dom = Duration::from_secs(20);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 30, |r| r["runs"] == json!(2));
    assert!(r["texts"].as_u64().unwrap() > 0, "{r}");
    finish(luna, dir);
}

/// betterFullscreen's "Loading..." view is a `div` whose only child is a
/// `div` (a single child, not an array): both took the same tree path, so
/// the child was the parent's own cached element and inserting it threw
/// `HierarchyRequestError` on every first mount. A host element's single
/// child, text or element, is its own node.
#[test]
fn a_react_div_with_one_div_child_renders_both_elements() {
    let (luna, _host, dir) = start(
        r##"
const react = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const r = document.createElement("div");
document.body.appendChild(r);
const view = (text) => react.createElement("div", { className: "outer" }, react.createElement("div", { className: "inner" }, text));
const root = createRoot(r);
root.render(view("Loading..."));
const first = r.querySelector(".inner");
R.html = r.innerHTML;
root.render(view("Loaded"));
R.same = r.querySelector(".inner") === first;
R.html2 = r.innerHTML;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(
        r["html"],
        json!(r#"<div class="outer"><div class="inner">Loading...</div></div>"#)
    );
    assert_eq!(
        r["html2"],
        json!(r#"<div class="outer"><div class="inner">Loaded</div></div>"#)
    );
    assert_eq!(r["same"], json!(true), "the node is kept between renders");
    finish(luna, dir);
}

/// What the UI needs to run CSS motion as a browser does: the fill mode and
/// a timing function with commas and spaces kept in the animation list, a
/// keyframe's own easing, each side's border colour (a spinner's bright
/// arc), `transition-property: none`, `prefers-reduced-motion` following
/// the desktop, and, for an element inserted at `opacity: 0` and given
/// `.visible` a frame later (3d-cover's canvas), the style its fade starts
/// from.
#[test]
fn motion_reaches_the_ui_as_a_browser_runs_it() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.m-spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(255, 255, 255, 0.25); border-top-color: rgba(255, 255, 255, 0.9);
  animation: m-rot 0.8s cubic-bezier(0.4, 0, 0.2, 1) 0.1s 3 alternate both; }
.m-lin { animation: m-rot 1s linear(0, 0.25 75%, 1) infinite; }
.m-off { transition: opacity 1s; transition-property: none; }
.m-fade { opacity: 0; transition: opacity 0.12s ease; }
.m-fade.visible { opacity: 1; }
@keyframes m-rot { from { transform: rotate(0deg); animation-timing-function: steps(4, jump-end); } to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .m-spin { animation: none; } }
`);
for (const c of ["m-spin", "m-lin", "m-off"]) {
	const el = document.createElement("div");
	el.className = c;
	document.body.append(el);
}
R.reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
window.addEventListener("click", () => {
	const f = document.createElement("canvas");
	f.className = "m-fade";
	document.body.append(f);
	getComputedStyle(document.body);
	requestAnimationFrame(() => f.classList.add("visible"));
	R.fade = true;
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 3);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    let spin = by("m-spin");
    assert_eq!(
        spin.style.animation.as_deref(),
        Some("m-rot 0.8 0.1 3 alternate cubic-bezier(0.4,0,0.2,1) running both")
    );
    assert_eq!(
        spin.style.border_color.as_deref(),
        Some(
            "rgba(255, 255, 255, 0.9) rgba(255, 255, 255, 0.25) rgba(255, 255, 255, 0.25) rgba(255, 255, 255, 0.25)"
        )
    );
    assert_eq!(
        by("m-lin").style.animation.as_deref(),
        Some("m-rot 1 0 infinite normal linear(0,0.25_75%,1) running none")
    );
    assert_eq!(by("m-off").style.transition, None);
    let frames = &o.keyframes["m-rot"];
    assert_eq!(
        frames[0]
            .1
            .custom
            .as_ref()
            .and_then(|c| c.get("animation-timing-function"))
            .map(String::as_str),
        Some("steps(4,jump-end)")
    );
    assert_eq!(dom_results(&dir)["reduce"], json!(false));
    // Inserted at opacity 0, visible a frame later: the first overlay that
    // shows it carries where its fade starts.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "click".into(),
        data: Value::Null,
    });
    // (An overlay built between the two shows it at 0 itself.)
    let mut shown_at_zero = false;
    let o = overlay_until(&luna, 10, |o| {
        let canvas = o.at("", Position::Append).find(|n| n.tag == "canvas");
        shown_at_zero |= canvas.is_some_and(|n| n.style.opacity.as_deref() == Some("0"));
        canvas.is_some_and(|n| n.style.opacity.as_deref() == Some("1"))
    });
    let fade = o
        .at("", Position::Append)
        .find(|n| n.tag == "canvas")
        .unwrap();
    if !shown_at_zero {
        let from = fade.from.as_ref().expect("the style of its first pass");
        assert_eq!(from.opacity.as_deref(), Some("0"));
        assert!(
            from.transition
                .as_deref()
                .is_some_and(|t| t.starts_with("opacity 0.12"))
        );
    }
    // Less motion on the desktop: the reduce rules apply.
    let mut t = tree();
    t.reduced_motion = true;
    luna.sync_dom(t);
    overlay_until(&luna, 10, |o| {
        o.at("", Position::Append).any(|n| {
            n.attrs.iter().any(|(k, v)| k == "class" && v == "m-spin")
                && n.style.animation.is_none()
        })
    });
    finish(luna, dir);
}

/// Tidal Tags' format columns clone TIDAL's time heading and write their
/// title into its `firstChild.firstChild` (TIDAL's heading holds the label
/// one level down). Tidalfast's heading had the text on the span itself,
/// so the clone threw "cannot set property 'textContent' of null" and no
/// BITRATE / SAMPLE RATE / DEPTH heading appeared. The clones go before
/// the time heading with their `min-width`, which sizes the columns.
#[test]
fn tidal_tags_clones_the_time_heading_and_titles_it() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, 'div[aria-label="Tracklist"]', (e) => {
	if (R.done) return;
	const time = e.querySelector('span[class^="_timeColumn"][role="columnheader"]');
	let before = time;
	for (const [title, min] of [["Depth", "40px"], ["Sample Rate", "110px"], ["Bitrate", "100px"]]) {
		const r = time.cloneNode(true);
		(r.firstChild?.childNodes?.length ?? -1) > 1 && r.firstChild?.lastChild?.remove();
		r.setAttribute("data-test", title);
		r.firstChild.firstChild.textContent = title;
		r.style.setProperty("min-width", min);
		time.parentElement.insertBefore(r, before);
		before = r;
	}
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["done"], json!(true), "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .any(|i| i.anchor.ends_with(":artist-col") && i.nodes.len() == 3)
    });
    let heads = o
        .injected
        .iter()
        .find(|i| i.anchor.ends_with(":artist-col"))
        .unwrap();
    let text = |n: &super::dom::VNode| -> String {
        fn all(n: &super::dom::VNode, out: &mut String) {
            out.push_str(n.text.as_deref().unwrap_or(""));
            n.children.iter().for_each(|c| all(c, out));
        }
        let mut out = String::new();
        all(n, &mut out);
        out
    };
    let got: Vec<(String, Option<String>)> = heads
        .nodes
        .iter()
        .map(|n| (text(n), n.style.min_width.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("Bitrate".to_string(), Some("100px".to_string())),
            ("Sample Rate".to_string(), Some("110px".to_string())),
            ("Depth".to_string(), Some("40px".to_string())),
        ]
    );
    finish(luna, dir);
}

/// LuckyLP colours its shuffle with TIDAL's `--wave-color-opacity-*`
/// variables, read back with `getComputedStyle(document.documentElement)`,
/// as `fill` on its button, after a `margin-left: 16px`. Tidalfast had no
/// such variables (the button stayed `#000000`, black on black), dropped
/// `margin-left` and `fill`, and drew TIDAL's 24-unit sprite symbol unscaled
/// in the button's 20-unit svg.
#[test]
fn lucky_lp_gets_tidals_variables_its_margin_and_the_sprites_paint() {
    let (luna, _host, dir) = start(
        r##"
const css = getComputedStyle(document.documentElement);
R.thick = css.getPropertyValue("--wave-color-opacity-contrast-fill-thick").trim();
R.ultra = css.getPropertyValue("--wave-color-opacity-contrast-fill-ultra-thick").trim();
new lib.StyleTag("Lucky", unloads, ".luckyBtnContainer>button{margin-left:16px;display:flex;fill:#000000}"
	.replace("#000000", R.thick || "#000000"));
lib.observe(unloads, "div[data-track--page-id='album']", (page) => {
	const box = document.createElement("div");
	box.className = "luckyBtnContainer";
	box.innerHTML = '<button type="button"><svg viewBox="0 0 20 20"><use href="#player__shuffle"></use></svg></button>';
	page.querySelector("h1")?.insertAdjacentElement("afterend", box);
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(
        (r["thick"].clone(), r["ultra"].clone()),
        (json!("#fff9"), json!("#fffc")),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::PAGE_TITLE, Position::After)
            .any(|n| n.children.first().is_some_and(|b| b.style.fill.is_some()))
    });
    let button = &o
        .at(keys::PAGE_TITLE, Position::After)
        .next()
        .unwrap()
        .children[0];
    assert_eq!(button.style.margin.as_deref(), Some("0 0 0 16px"));
    assert_eq!(
        button.style.fill.as_deref(),
        Some("rgba(255, 255, 255, 0.6)")
    );
    let markup = button.children[0].markup.as_deref().unwrap();
    assert!(
        markup.contains(r#"<svg viewBox="0 0 24 24" width="100%" height="100%">"#)
            && markup.contains(r#"stroke="__luna-paint__""#),
        "{markup}"
    );
    // TIDAL's own variables are not a theme's: they never recolour the app.
    assert_eq!(o.var("--wave-color-opacity-contrast-fill-thick"), None);
    finish(luna, dir);
}

/// AltPlay's sidebar link: it goes after the parent of TIDAL's Collection
/// menu button (`div._collectionWrapper_`) and copies the Music link's
/// classes, so it looks like TIDAL's navigation rows (TIDAL's stylesheet
/// gives `_navigationItem_` its 208 x 40 row, 55 % white). With its page
/// open it moves `_selected_` from Music to itself. Tidalfast had no
/// wrapper, so the link went after the whole navigation, where nothing
/// draws; nothing gave the copied class TIDAL's look; and a class taken
/// from a TIDAL element was not reported.
#[test]
fn altplay_sidebar_link_follows_the_collection_wrapper_and_takes_tidals_look() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="sidebar-music"]', (music) => {
	if (R.done) return;
	const wrap = document.querySelector('button[data-test="sidebar-collection-menu"]')?.parentElement;
	R.wrapper = wrap?.className ?? null;
	const a = document.createElement("a");
	a.id = "altplay-sidebar-link";
	a.className = music.className.split(/\s+/).filter((c) => c && !c.includes("_selected")).join(" ");
	a.textContent = "AltPlay";
	wrap.insertAdjacentElement("afterend", a);
	const selected = [...music.classList].find((c) => c.startsWith("_selected_"));
	if (selected) {
		music.classList.remove(selected);
		a.classList.add(selected);
	}
	R.done = true;
});
"##,
    );
    // Music is the open page; Collection follows it.
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items[0].selected = true;
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        ..dom_anchors::SidebarItem::default()
    });
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert!(
        r["wrapper"]
            .as_str()
            .is_some_and(|c| c.starts_with("_collectionWrapper_")),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::FIXED_NAV, Position::Append).next().is_some()
            || o.at(keys::COLLECTION_WRAPPER, Position::After)
                .next()
                .is_some()
    });
    let link = o
        .at(keys::FIXED_NAV, Position::Append)
        .chain(o.at(keys::COLLECTION_WRAPPER, Position::After))
        .next()
        .unwrap();
    assert_eq!(
        link.style.color.as_deref(),
        Some("rgb(255, 255, 255)"),
        "selected: white"
    );
    assert_eq!(link.style.padding.as_deref(), Some("8px"));
    assert_eq!(link.style.border_radius.as_deref(), Some("12px"));
    // TIDAL's look is for plugin elements: Tidalfast draws its own rows.
    let music = o.host(&keys::sidebar_item("sidebar-music"));
    assert!(music.is_none_or(|h| h.style.padding.is_none()), "{music:?}");
    assert!(
        music.is_some_and(|h| h.removed.iter().any(|c| c.starts_with("_selected_"))),
        "{music:?}"
    );
    finish(luna, dir);
}

/// HorizontalVolumeSlider's bar is all in its range input's
/// `::-webkit-slider-runnable-track` and `::-webkit-slider-thumb` rules: a
/// 4-pixel track filled white up to `--volume-percentage` (set in the
/// input's inline style) and 30 % white after it, a 3 x 12 white thumb 4
/// pixels above the track's top. Pseudo-element rules never reached a
/// plugin element, so the bar was drawn as egui's own slider.
#[test]
fn a_range_inputs_track_and_thumb_rules_reach_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const style = document.createElement("style");
style.textContent = `
.horizontal-volume-slider::-webkit-slider-runnable-track { height: 4px; border-radius: 2px;
  background: linear-gradient(to right, white var(--volume-percentage), rgba(255,255,255,0.3) var(--volume-percentage)); }
.horizontal-volume-slider::-webkit-slider-thumb { -webkit-appearance: none; appearance: none; width: 3px; height: 12px;
  background: white; border-radius: 1px; cursor: pointer; margin-top: -4px; }`;
document.head.appendChild(style);
lib.observe(unloads, "[class*='_sliderContainer']", (box) => {
	if (R.done) return;
	const range = document.createElement("input");
	range.type = "range";
	range.className = "horizontal-volume-slider";
	range.style.width = "86px";
	range.style.height = "24px";
	range.style.setProperty("--volume-percentage", "40%");
	range.value = "40";
	box.insertAdjacentElement("afterend", range);
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::SLIDER, Position::After)
            .any(|n| n.track.is_some())
    });
    let range = o
        .at(keys::SLIDER, Position::After)
        .find(|n| n.track.is_some())
        .unwrap();
    let track = range.track.as_ref().unwrap();
    assert_eq!(track.height.as_deref(), Some("4px"));
    let fill = track.background_image.as_deref().unwrap_or("");
    assert!(
        fill.contains("white 40%") && fill.contains("0.3) 40%"),
        "the variable resolves at the input: {fill}"
    );
    let thumb = range.thumb.as_ref().unwrap();
    assert_eq!(thumb.width.as_deref(), Some("3px"));
    assert_eq!(thumb.height.as_deref(), Some("12px"));
    assert_eq!(thumb.margin_top.as_deref(), Some("-4px"));
    // The parts are the input's, not its own box's.
    assert!(range.style.margin_top.is_none(), "{:?}", range.style);
    finish(luna, dir);
}

/// LangRomanizer puts its Romanize Lyrics button after the first
/// `[data-test^="request-fullscreen"], [data-test^="toggle-lyrics"]` and
/// copies that element's classes and its span's. In TIDAL that is the
/// now-playing header's Lyrics tab, so the button sits after Lyrics and
/// looks like the tabs. Tidalfast had no tabs in the header, so the button
/// went into the player bar after its lyrics button, unstyled.
#[test]
fn a_button_after_the_now_playing_lyrics_tab_looks_like_the_tabs() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="new-now-playing-close"]', () => {
	if (R.done) return;
	const f = document.querySelector('[data-test^="request-fullscreen"], [data-test^="toggle-lyrics"]');
	const n = f.querySelector("span");
	const t = document.createElement("button");
	const q = document.createElement("span");
	t.id = "romanize-button";
	t.className = "romanize-button " + f.className;
	q.textContent = "Romanize Lyrics";
	q.className = n.className;
	t.appendChild(q);
	f.parentElement.insertBefore(t, f.nextSibling);
	R.anchor = f.closest("header") ? "header" : "player bar";
	R.done = true;
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    if let Some(np) = frame.now_playing.as_mut() {
        for (name, label) in [
            ("similar-tracks", "Similar tracks"),
            ("credits", "Credits"),
            ("lyrics", "Lyrics"),
        ] {
            np.tabs.push(dom_anchors::NowPlayingTab {
                name,
                label: label.into(),
                ..dom_anchors::NowPlayingTab::default()
            });
        }
    }
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["anchor"], json!("header"), "{r}");
    // Lyrics is the last tab: what follows it is the tab box's Append.
    let tab = keys::now_playing_tab("lyrics");
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::NP_CONTENT_BUTTONS, Position::Append)
            .next()
            .is_some()
    });
    let button = o
        .at(keys::NP_CONTENT_BUTTONS, Position::Append)
        .next()
        .unwrap();
    assert_eq!(button.style.height.as_deref(), Some("30px"));
    assert_eq!(button.style.padding.as_deref(), Some("0 11px"));
    assert_eq!(button.style.font_weight.as_deref(), Some("600"));
    assert_eq!(button.style.color.as_deref(), Some("rgb(180, 180, 184)"));
    // Tidalfast's own tabs keep their native look.
    assert!(o.host(&tab).is_none_or(|h| h.style.padding.is_none()));
    finish(luna, dir);
}

/// DownloadManager's window nav: `border: none; border-left: 3px solid
/// transparent` on every item, `border-left-color` on the open one. The
/// side's own shorthand gives its colour, so the closed items' bar is
/// see-through (it was drawn in the text's black) and the open one's blue.
#[test]
fn a_sides_shorthand_gives_that_sides_border_colour() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.dm-nav { border: none; border-left: 3px solid transparent; }
.dm-nav.on { border-left-color: #0078d7; }
`);
for (const c of ["dm-nav", "dm-nav on"]) {
	const el = document.createElement("button");
	el.className = c;
	document.body.append(el);
}
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 2);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    for (cls, left) in [("dm-nav", "transparent"), ("dm-nav on", "rgb(0, 120, 215)")] {
        let style = &by(cls).style;
        assert_eq!(
            style.border_width.as_deref(),
            Some("0px 0px 0px 3px"),
            "{cls}"
        );
        assert_eq!(
            style.border_color.as_deref(),
            Some(format!("currentcolor currentcolor currentcolor {left}").as_str()),
            "{cls}"
        );
    }
    finish(luna, dir);
}

/// The UI's `transitionend` reaches a listener as a `TransitionEvent` with
/// its `propertyName` and `elapsedTime` (radiant-lyrics checks
/// `propertyName === "min-width"` before it hangs its menu).
#[test]
fn a_transition_event_carries_its_property() {
    let (luna, _host, dir) = start(
        r##"
const el = document.createElement("div");
el.className = "t-tab";
el.addEventListener("transitionend", (e) => {
	R.seen = { cls: e instanceof TransitionEvent, name: e.propertyName, elapsed: e.elapsedTime, bubbles: e.bubbles, cancelable: e.cancelable };
});
document.body.append(el);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 1);
    let tab = o
        .at("", Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-tab"))
        .expect("the tab");
    assert!(tab.listens.contains(&"transitionend".to_string()));
    luna.dom_event(DomEvent {
        target: DomTarget::Node(tab.id),
        kind: "transitionend".into(),
        data: json!({ "propertyName": "min-width", "elapsedTime": 0.12, "pseudoElement": "" }),
    });
    let r = results_until(&luna, &dir, 10, |r| !r["seen"].is_null());
    assert_eq!(
        r["seen"],
        json!({ "cls": true, "name": "min-width", "elapsed": 0.12, "bubbles": true, "cancelable": false })
    );
    finish(luna, dir);
}

/// DownloadManager's About body (`overflow-y: auto`) reaches the UI, which
/// scrolls it; a list's `list-style` gives its marker type.
#[test]
fn overflow_y_and_list_style_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.dm-body { overflow-y: auto; }
.dm-list { list-style: none outside; }
.dm-sq { list-style-type: square; }
`);
for (const [tag, c] of [["div", "dm-body"], ["ul", "dm-list"], ["ol", "dm-sq"]]) {
	const el = document.createElement(tag);
	el.className = c;
	document.body.append(el);
}
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 3);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    assert_eq!(by("dm-body").style.overflow_y.as_deref(), Some("auto"));
    assert_eq!(by("dm-list").style.list_style_type.as_deref(), Some("none"));
    assert_eq!(by("dm-sq").style.list_style_type.as_deref(), Some("square"));
    finish(luna, dir);
}

/// A plugin that waits on a timer after an `await` (`await sleep(5)` in a
/// loop, polling a box until it changes) set that timer in a promise job
/// that ran after the time to the next timer was worked out: the runtime
/// slept until something else woke it (the next host tree, up to a
/// second later), and the loop ran a step per wake.
#[test]
fn timers_set_after_an_await_keep_their_time() {
    let (luna, _host, dir) = start(
        r#"
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
(async () => {
	const t0 = Date.now();
	for (let i = 0; i < 20; i++) await sleep(5);
	R.took = Date.now() - t0;
})();
"#,
    );
    // Nothing else wakes the runtime meanwhile.
    std::thread::sleep(Duration::from_millis(1500));
    let r = results_until(&luna, &dir, 10, |r| !r["took"].is_null());
    let took = r["took"].as_u64().unwrap();
    assert!(took < 1000, "twenty 5 ms sleeps took {took} ms");
    finish(luna, dir);
}

/// A plugin that changes an element's style and reads its box at once,
/// before the UI has sent the boxes laid out with the change, reads the new
/// box, as a browser lays out on such a read: radiant-lyrics widens its
/// Lyrics tab and, 200 ms after the click, hangs its menu under it. The
/// read waits on the UI's frame that draws the change, however long that
/// frame takes: on a slow machine it came after the fixed 64 ms the read
/// used to wait, and the read saw the old box. The test drives the frames
/// and the time the wait goes by.
#[test]
fn a_box_read_after_a_change_waits_for_its_layout() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	R.before = el.getBoundingClientRect().width;
	el.style.minWidth = "900px";
	R.after = el.getBoundingClientRect().width;
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    let o = draw_until_footer_width(&luna, &mut t, "900px", 0);
    until_layout_waiting(&luna, |w| w == Some(t0));
    // The UI starts the frame that draws it, a slow one: its tree comes
    // well after the 64 ms the read used to wait, and the read sees that
    // time go by before it comes.
    luna.drawing_boxes(o.version);
    luna.set_layout_clock(t0 + Duration::from_millis(400));
    until_layout_waiting(&luna, |w| w != Some(t0));
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 900.0, 90.0]);
    t.laid_out = o.version;
    luna.sync_dom(t);
    let r = results_until(&luna, &dir, 10, |r| !r["after"].is_null());
    assert_eq!(r["before"].as_f64(), Some(800.0));
    assert_eq!(r["after"].as_f64(), Some(900.0));
    finish(luna, dir);
}

/// A plugin element inside another plugin element has its own box: a read
/// of it asks the UI where it drew the element (`DomOverlay::measure`) and
/// waits for the tree that says (`HostTree::node_rects`), as a browser lays
/// out on a read. It used to get the box of the nearest element the UI had
/// told the runtime about (the outermost one, or the anchor round it), so a
/// lyrics plugin that scrolls its sung line to a place in the view
/// (radiant-lyrics) measured every line at its list's top and scrolled to
/// the start of the song.
#[test]
fn a_nested_plugin_element_reads_its_own_box() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	const list = document.createElement("div");
	for (let i = 0; i < 3; i++) {
		const line = document.createElement("div");
		line.className = "line";
		line.textContent = "line " + i;
		list.appendChild(line);
	}
	el.appendChild(list);
	setTimeout(() => {
		R.top = list.children[2].getBoundingClientRect().top;
		R.again = list.children[2].getBoundingClientRect().top;
	}, 100);
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    // The UI draws each overlay as it comes, and the lines where the
    // overlay asks.
    // Read straight from the published overlay: a round trip to the runtime
    // would queue behind the read's wait.
    let end = Instant::now() + Duration::from_secs(10);
    let mut drawn = 0;
    let asked = loop {
        let o = luna.dom_overlay();
        if o.version > drawn {
            drawn = o.version;
            luna.drawing_boxes(drawn);
            t.node_rects = o
                .measure
                .iter()
                .map(|id| (*id, [10.0, 745.0, 300.0, 20.0]))
                .collect();
            t.laid_out = drawn;
            luna.sync_dom(t.clone());
            if !o.measure.is_empty() {
                break o.measure.clone();
            }
        }
        assert!(Instant::now() < end, "no box was asked for");
        std::thread::sleep(Duration::from_millis(1));
    };
    let r = results_until(&luna, &dir, 10, |r| !r["again"].is_null());
    assert_eq!(asked.len(), 1, "the line read is measured: {asked:?}");
    assert_eq!(r["top"].as_f64(), Some(745.0));
    assert_eq!(r["again"].as_f64(), Some(745.0));
    finish(luna, dir);
}

/// A menu a plugin appends to the page at the pointer, with no height of
/// its own (`position: fixed`, padding and its rows), measures its rows'
/// height, and so keeps inside the window near its bottom edge
/// (element-hider: `top = min(clientY, innerHeight - height - 10)`). It
/// measured only the size its style declares, 0 tall, so the menu opened
/// on a player bar button ran off the window's bottom. Its place stays its
/// style's: moved, it reads where it was moved to at once.
#[test]
fn a_floating_menu_measures_its_rows_and_stays_in_the_window() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	if (!document.querySelector('[data-test="footer-player"]')) return setTimeout(go, 20);
	const s = document.createElement("style");
	s.textContent = ".menu{position:fixed;min-width:180px;padding:8px 0;border:1px solid #444}.menu button{display:flex;width:100%;padding:8px 16px}";
	document.head.appendChild(s);
	const m = document.createElement("div");
	m.className = "menu";
	for (const t of ["Hide This Element", "Unhide All Elements (0)"]) {
		const b = document.createElement("button");
		b.textContent = t;
		m.appendChild(b);
	}
	document.body.appendChild(m);
	const r = m.getBoundingClientRect();
	R.size = [r.width, r.height];
	const top = Math.min(window.innerHeight - 20, window.innerHeight - r.height - 10);
	m.style.left = "600px";
	m.style.top = top + "px";
	const moved = m.getBoundingClientRect();
	R.moved = [moved.left, moved.top, moved.width, moved.height];
	R.top = top;
	R.inner = window.innerHeight;
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    // The UI draws each overlay as it comes, and the menu where it lays it
    // out: at the window's corner until the plugin places it, 182 x 88.
    let end = Instant::now() + Duration::from_secs(10);
    let mut drawn = 0;
    loop {
        let o = luna.dom_overlay();
        if o.version > drawn {
            drawn = o.version;
            luna.drawing_boxes(drawn);
            t.node_rects = o
                .measure
                .iter()
                .map(|id| (*id, [0.0, 0.0, 182.0, 88.0]))
                .collect();
            t.laid_out = drawn;
            luna.sync_dom(t.clone());
            if !o.measure.is_empty() {
                break;
            }
        }
        assert!(Instant::now() < end, "the menu's box was not asked for");
        std::thread::sleep(Duration::from_millis(1));
    }
    let r = results_until(&luna, &dir, 10, |r| !r["inner"].is_null());
    let inner = r["inner"].as_f64().unwrap();
    let numbers = |v: &Value| -> Vec<f64> {
        v.as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    assert_eq!(numbers(&r["size"]), [182.0, 88.0], "{r}");
    assert_eq!(r["top"].as_f64(), Some(inner - 88.0 - 10.0), "{r}");
    assert_eq!(
        numbers(&r["moved"]),
        [600.0, inner - 88.0 - 10.0, 182.0, 88.0],
        "{r}"
    );
    finish(luna, dir);
}

/// Play the UI until the footer's `min-width` is `width`: draw each overlay
/// after version `drawn` published before it at once, and send the tree
/// laid out with it (`t`). Returns that overlay, not drawn. Read straight
/// from the published overlay, as the UI reads it: a round trip to the
/// runtime would queue behind a read's wait.
fn draw_until_footer_width(
    luna: &Luna,
    t: &mut HostTree,
    width: &str,
    mut drawn: u64,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let o = luna.dom_overlay();
        if o.hosts
            .get(keys::FOOTER)
            .is_some_and(|h| h.style.min_width.as_deref() == Some(width))
        {
            return o;
        }
        if o.version > drawn {
            drawn = o.version;
            luna.drawing_boxes(drawn);
            t.laid_out = drawn;
            luna.sync_dom(t.clone());
        }
        assert!(Instant::now() < end, "the change never reached the overlay");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Wait until what [`Luna::layout_waiting`] says meets `cond`.
fn until_layout_waiting(luna: &Luna, mut cond: impl FnMut(Option<Instant>) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !cond(luna.layout_waiting()) {
        assert!(Instant::now() < end, "the read never got there");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A layout read answers with boxes; the page it was laid out from (TIDAL
/// left the album for Home) reaches the document after the running task,
/// as Chromium never runs TIDAL's render inside a read. A walk that reads
/// each of its elements' boxes (a recorder's snapshot, a plugin's measure
/// loop) found a badge in a row's title wrapper cut from its row: the
/// plugin child outlived its row as a root of its own.
#[test]
fn a_layout_read_does_not_take_the_page_apart_under_the_running_task() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const titles = [...document.querySelectorAll('[data-test="table-cell-title"]')];
	if (titles.length < 2) return setTimeout(go, 20);
	for (const t of titles) {
		const b = document.createElement("span");
		b.className = "probe-badge";
		b.textContent = "Electronic";
		t.after(b);
	}
	document.querySelector('[data-test="footer-player"]').style.minWidth = "900px";
	const during = [];
	for (const b of document.querySelectorAll(".probe-badge")) {
		b.getBoundingClientRect();
		const wrap = b.parentElement;
		during.push(!!wrap?.closest('[data-test="tracklist-row"]') && b.isConnected);
	}
	R.badges = titles.length;
	R.during = during;
	setTimeout(() => {
		R.after = document.querySelectorAll(".probe-badge").length;
		R.rows = document.querySelectorAll('[data-test="tracklist-row"]').length;
	}, 300);
};
setTimeout(go, 300);
"#,
    );
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    assert!(t.nodes.iter().any(|n| n.key.starts_with("row:")));
    luna.sync_dom(t.clone());
    // The change is drawn: the tree laid out with it has left the page.
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let o = luna.dom_overlay();
        if o.hosts
            .get(keys::FOOTER)
            .is_some_and(|h| h.style.min_width.as_deref() == Some("900px"))
        {
            luna.drawing_boxes(o.version);
            let mut home = t.clone();
            home.nodes.retain(|n| !n.key.starts_with("row:"));
            home.laid_out = o.version;
            luna.sync_dom(home);
            break;
        }
        assert!(Instant::now() < end, "the change never reached the overlay");
        std::thread::sleep(Duration::from_millis(1));
    }
    let r = results_until(&luna, &dir, 10, |r| !r["after"].is_null());
    let badges = r["badges"].as_u64().unwrap();
    assert!(badges >= 2, "{r}");
    assert_eq!(
        r["during"],
        json!(vec![true; badges as usize]),
        "every badge in its row for the whole task: {r}"
    );
    // After the task the page is Home: the rows went with their badges.
    assert_eq!(r["after"], json!(0), "{r}");
    assert_eq!(r["rows"], json!(0), "{r}");
    finish(luna, dir);
}

/// What went from the page leaves it whole, as React unmounts it: a row
/// TIDAL took away is taken out of its list in one record, with its cells
/// and a plugin's badge in its title cell still inside it. Nothing of it is
/// taken apart piece by piece, so a plugin holding the row finds its cells,
/// a badge still finds its row, and an observer hears of the row alone.
#[test]
fn a_subtree_that_went_leaves_whole() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const titles = [...document.querySelectorAll('[data-test="table-cell-title"]')];
	if (titles.length < 2) return setTimeout(go, 20);
	const badges = titles.map((t) => {
		const b = document.createElement("span");
		b.className = "probe-badge";
		t.after(b);
		return b;
	});
	const rows = badges.map((b) => b.closest('[data-test="tracklist-row"]'));
	const recs = [];
	new MutationObserver((list) => {
		for (const r of list) for (const n of r.removedNodes) recs.push([r.target.isConnected, !!(n.matches?.('[data-test="tracklist-row"]') || n.querySelector?.('[data-test="tracklist-row"]'))]);
		R.recs = recs;
		R.connected = badges.map((b) => b.isConnected);
		R.inRow = badges.map((b, i) => !!rows[i] && b.closest('[data-test="tracklist-row"]') === rows[i]);
		R.cells = rows.map((r) => !!r?.querySelector('[data-test="table-cell-title"]'));
	}).observe(document.body, { childList: true, subtree: true });
	R.badges = badges.length;
};
setTimeout(go, 300);
"#,
    );
    let t = tree();
    luna.sync_dom(t.clone());
    let r = results_until(&luna, &dir, 10, |r| !r["badges"].is_null());
    let badges = r["badges"].as_u64().unwrap() as usize;
    assert!(badges >= 2, "{r}");
    // TIDAL left the album: its rows went.
    let mut home = t;
    home.nodes.retain(|n| !n.key.starts_with("row:"));
    luna.sync_dom(home);
    let r = results_until(&luna, &dir, 10, |r| {
        r["connected"] == json!(vec![false; badges])
    });
    assert_eq!(r["inRow"], json!(vec![true; badges]), "{r}");
    assert_eq!(r["cells"], json!(vec![true; badges]), "{r}");
    let recs = r["recs"].as_array().unwrap();
    assert!(
        recs.iter().all(|x| x[0] == json!(true)),
        "only what left the page is a removal: {r}"
    );
    // Each record is a row leaving with what is in it.
    assert!(recs.len() >= badges, "{r}");
    assert!(recs.iter().all(|x| x[1] == json!(true)), "{r}");
    finish(luna, dir);
}

/// The now-playing lyrics are TIDAL 2.46.0's boxes: the scroll area
/// `[data-test=now-playing-lyrics]`, its viewport, the content box
/// `div._content_fef19e8` and in it the lines' `div[data-test=lyrics-text]`.
/// Lyrics plugins that draw their own lines find that box from a line (its
/// parent's parent, then the box's first `div`) or, before there are lines,
/// from the scroll area (its first `div`, then the child named `_content_`),
/// and put their lines in the lines' box.
#[test]
fn lyrics_plugins_find_tidals_lyrics_boxes() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const line = document.querySelector('span[data-test="lyrics-line"]');
	if (!line) return setTimeout(go, 20);
	const host = document.querySelector('[data-test="now-playing-lyrics"]');
	const box = line.parentElement?.parentElement ?? null;
	const inner = box?.querySelector(":scope > div") ?? null;
	const first = [...host.children].find((a) => a.tagName === "DIV" && ![...a.classList].some((c) => c.startsWith("os-scrollbar")));
	const up = [];
	for (let n = line.parentElement; n && n !== host; n = n.parentElement) up.push(n.getAttribute("data-test") ?? (n.className || n.localName));
	R.found = {
		up,
		box: [...(box?.classList ?? [])].some((c) => c.startsWith("_content_")),
		inner: inner?.getAttribute("data-test") ?? null,
		viaHost: first?.querySelector(':scope > [class*="_content_"]') === box,
	};
	const mine = document.createElement("div");
	mine.className = "probe-lines";
	inner?.appendChild(mine);
};
setTimeout(go, 300);
"#,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| !r["found"].is_null());
    assert_eq!(
        r["found"],
        json!({
            "up": ["lyrics-text", "_content_fef19e8", "div"],
            "box": true,
            "inner": "lyrics-text",
            "viaHost": true,
        }),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::LYRICS_LINES, Position::Append).count() == 1
    });
    assert_eq!(
        o.at(keys::LYRICS_LINES, Position::Append)
            .next()
            .unwrap()
            .attrs,
        vec![("class".to_string(), "probe-lines".to_string())]
    );
    finish(luna, dir);
}

/// The lyrics' lines box and lines are marked up as TIDAL 2.46.0 renders
/// them (its bundle): the box is market text in the `display-20` type,
/// white, with `_hasCues_` for timed lyrics, so what a plugin puts in it
/// inherits the lyrics' 48 px bold (radiant-lyrics' word-by-word box reads
/// that in TIDAL); only the sung line has `data-current`, the others their
/// `data-distance` from it, and timed lines are buttons.
#[test]
fn the_lyrics_lines_box_has_tidals_type_and_lines() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const box = document.querySelector('[data-test="lyrics-text"]');
	if (!box || !box.querySelector('span[data-test="lyrics-line"]')) return setTimeout(go, 20);
	const mine = document.createElement("div");
	mine.className = "probe-words";
	box.appendChild(mine);
	const cs = getComputedStyle(mine);
	const lines = [...box.querySelectorAll('span[data-test="lyrics-line"]')];
	R.found = {
		font: [cs.fontSize, cs.fontWeight, cs.color],
		cues: box.matches('[class^="_marketText_"]._display20_1lyag_21._text_4a89397._hasCues_15d8b83'),
		sung: box.querySelectorAll("span[data-current]").length,
		lines: lines.map((l) => [l.getAttribute("data-current"), l.getAttribute("data-distance"), l.getAttribute("role"), l.getAttribute("tabindex")]),
	};
};
setTimeout(go, 300);
"#,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| !r["found"].is_null());
    assert_eq!(
        r["found"],
        json!({
            "font": ["48px", "700", "rgb(255, 255, 255)"],
            "cues": true,
            "sung": 1,
            "lines": [["true", null, "button", "0"], [null, "1", "button", "0"]],
        }),
        "{r}"
    );
    finish(luna, dir);
}

/// A read never stalls the runtime for a window that does not lay the
/// change out: it gives up when the UI did not start drawing it in time,
/// or, drawing it, did not send its tree, and reads the boxes it has.
#[test]
fn a_box_read_gives_up_on_a_window_that_does_not_lay_out() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	el.style.minWidth = "900px";
	R.undrawn = el.getBoundingClientRect().width;
	el.style.minWidth = "1000px";
	R.unsent = el.getBoundingClientRect().width;
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    // The UI never starts a frame with the first change.
    let first = draw_until_footer_width(&luna, &mut t, "900px", 0);
    let mut now = t0 + super::layout::PICKUP_CAP;
    luna.set_layout_clock(now);
    // It starts one with the second, and never sends its tree.
    let o = draw_until_footer_width(&luna, &mut t, "1000px", first.version);
    luna.drawing_boxes(o.version);
    now += super::layout::CARRY;
    luna.set_layout_clock(now);
    let r = results_until(&luna, &dir, 10, |r| !r["unsent"].is_null());
    assert_eq!(r["undrawn"].as_f64(), Some(800.0));
    assert_eq!(r["unsent"].as_f64(), Some(800.0));
    finish(luna, dir);
}

/// A plugin that rewrites every text node at load and in nodes added later
/// (the shape of a text-rewriting plugin): the navigation rows' words
/// reach the overlay whether the sidebar is in the first host tree, or
/// mounts later.
#[test]
fn text_rewritten_in_navigation_rows_reaches_the_overlay_whenever_they_mount() {
    let body = r##"
const seen = new WeakSet();
const walk = (root) => {
	const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
	let n;
	while ((n = w.nextNode())) {
		if (seen.has(n)) continue;
		const t = n.textContent;
		if (!t.trim()) continue;
		n.textContent = "~" + t;
		seen.add(n);
	}
};
walk(document.body);
new MutationObserver((list) => {
	for (const r of list) r.addedNodes.forEach((a) => walk(a));
}).observe(document.body, { childList: true, subtree: true });
R.started = true;
"##;
    for late in [false, true] {
        let (luna, _host, dir) = start(body);
        results_until(&luna, &dir, 10, |r| r["started"] == json!(true));
        let mut full = tree();
        if late {
            let mut early = full.clone();
            early.nodes.retain(|n| !n.key.starts_with("sidebar"));
            luna.sync_dom(early);
            std::thread::sleep(Duration::from_millis(300));
        }
        full.nodes.iter_mut().for_each(|_| {});
        luna.sync_dom(std::mem::take(&mut full));
        let o = overlay_until(&luna, 10, |o| {
            o.host(&keys::sidebar_item_label("sidebar-music"))
                .is_some_and(|h| h.text.as_deref().is_some_and(|t| t.starts_with('~')))
        });
        assert!(
            o.host(&keys::sidebar_item_label("sidebar-music")).is_some(),
            "late {late}"
        );
        finish(luna, dir);
    }
}

/// The same rewrite made once at load, with the page there when the plugin
/// starts (`wait_for_dom`): the navigation rows' words are rewritten.
#[test]
fn text_rewritten_at_load_in_the_first_host_tree_reaches_the_overlay() {
    let body = r##"
const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
let n; let count = 0;
while ((n = w.nextNode())) {
	const t = n.textContent;
	if (!t.trim()) continue;
	n.textContent = "~" + t;
	count++;
}
R.rewritten = count;
"##;
    let (luna, _host, dir) = start(body);
    luna.shutdown();
    let mut cfg = config(&dir);
    cfg.wait_for_dom = Duration::from_secs(20);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 30, |r| {
        r["rewritten"].as_u64().unwrap_or(0) > 0
    });
    assert!(r["rewritten"].as_u64().unwrap() > 5, "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.host(&keys::sidebar_item_label("sidebar-music"))
            .is_some_and(|h| h.text.as_deref().is_some_and(|t| t.starts_with('~')))
    });
    assert!(o.host(&keys::sidebar_item_label("sidebar-music")).is_some());
    finish(luna, dir);
}

/// TIDAL 2.46.0's account button holds the account picture as an image
/// (`button[data-test=header-profile-menu-button] > img._artworkImage_
/// 5bde5a4._profilePicture_1786ea0`), so rules for elements whose class
/// starts `_profilePicture_` (Avatar's background picture and hidden `svg`)
/// reach nothing in the header and the picture stays TIDAL's, while a rule
/// for TIDAL's class reaches the picture. Tidalfast's button was
/// `button._profilePicture_ > svg`: such rules painted their picture over
/// the account picture, which TIDAL never shows.
#[test]
fn rules_for_the_account_picture_match_as_in_tidal() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
[class^="_profilePicture_"] { background-image: url("https://pictures.test/a.png"); background-size: cover; }
[class^="_profilePicture_"] svg { display: none; }
._profilePicture_1786ea0 { opacity: .5; }
`);
"##,
    );
    let t = tree();
    let picture = t
        .nodes
        .iter()
        .find(|n| n.parent.as_deref() == Some(keys::PROFILE))
        .map(|n| (n.key.clone(), n.tag.clone()));
    luna.sync_dom(t);
    let o = overlay_until(&luna, 10, |o| {
        o.hosts
            .values()
            .any(|h| h.style.opacity.is_some() || h.style.background_image.is_some() || h.hidden())
    });
    let (img, tag) = picture.expect("the account button holds its picture");
    assert_eq!(tag, "img", "the picture is TIDAL's image");
    let button = o.host(keys::PROFILE);
    assert!(
        button.is_none_or(|h| h.style.background_image.is_none()),
        "no picture painted on the button: {button:?}"
    );
    let shown = o.host(&img);
    assert!(
        shown.is_some_and(|h| !h.hidden() && h.style.opacity.as_deref() == Some(".5")),
        "the picture shows, with the rule for TIDAL's class: {shown:?}"
    );
    finish(luna, dir);
}

/// TIDAL 2.46.0's player bar has no `._moreContainer_`, no full screen and
/// no lyrics button: a plugin that waits for the first (total-play-time)
/// finds nothing and adds nothing, as in TIDAL, and one that puts its
/// button beside the first of `toggle-lyrics`, `new-now-playing-expand`
/// and `request-fullscreen` (TidalLyricsTranslate) reaches the now-playing
/// header's buttons. The bar's utility buttons are TIDAL's: the queue, the
/// sound output, the volume and compact mode, then the quality badge's box.
#[test]
fn the_player_bar_holds_only_what_tidal_2_46_has() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (!document.querySelector("#footerPlayer") || R.done) return;
	R.more = !!document.querySelector('[class*="_moreContainer_"]');
	R.fullscreen = !!document.querySelector('[data-test="request-fullscreen"]');
	R.barLyrics = !!document.querySelector('#footerPlayer [data-test="toggle-lyrics"]');
	const f = document.querySelector('[data-test="toggle-lyrics"], [data-test="new-now-playing-expand"], [data-test="request-fullscreen"]');
	R.translate = f ? String(f.parentElement.className) : null;
	R.buttons = [...document.querySelector('[class*="_utilityButtons_"]').children].map((e) => e.getAttribute("data-test") ?? e.className);
	R.badgeBox = String(document.querySelector('[class*="_qualityBadgeContainer_"]')?.parentElement?.className);
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    // The view's header as Tidalfast draws it: full screen and close.
    let mut frame = dom_anchors::sample_frame();
    if let Some(np) = frame.now_playing.as_mut() {
        np.expand_rect = Some([1100.0, 20.0, 30.0, 30.0]);
    }
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["more"], json!(false), "{r}");
    assert_eq!(r["fullscreen"], json!(false), "{r}");
    assert_eq!(r["barLyrics"], json!(false), "{r}");
    assert!(
        r["translate"]
            .as_str()
            .is_some_and(|c| c.starts_with("_actionButtons_")),
        "{r}"
    );
    let buttons: Vec<&str> = r["buttons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap_or(""))
        .collect();
    assert_eq!(buttons[0], "play-queue-button", "{r}");
    assert_eq!(buttons[1], "select-device", "{r}");
    assert!(buttons[2].starts_with("_sliderContainer_"), "{r}");
    assert_eq!(buttons[3], "enter-compact-mode", "{r}");
    assert!(
        r["badgeBox"]
            .as_str()
            .is_some_and(|c| c.starts_with("_utilityContainer_")),
        "{r}"
    );
    finish(luna, dir);
}

/// element-hider ties TIDAL's context menu to the right-click before it:
/// its capture `contextmenu` listener sets a flag, and its observer adds
/// its rows when `[data-test="contextmenu"]` appears with the flag set. In
/// Chromium the event comes first and TIDAL's menu after; the menu the app
/// drew in answer to the right-click reached the document before the event,
/// so the plugin fell back to its own menu.
#[test]
fn a_right_click_reaches_listeners_before_the_menu_it_opens() {
    let (luna, _host, dir) = start(
        r##"
let armed = false;
document.addEventListener("contextmenu", () => {
	armed = true;
	R.heard = (R.heard ?? 0) + 1;
}, true);
new MutationObserver((records) => {
	for (const r of records)
		for (const n of r.addedNodes) {
			if (!(n instanceof Element)) continue;
			const menu = n.matches('[data-test="contextmenu"]') ? n : n.querySelector('[data-test="contextmenu"]');
			if (menu) R.menuSeen = armed ? "after the event" : "before the event";
		}
}).observe(document.body, { childList: true, subtree: true });
// A plugin busy on the thread while the input and the menu arrive.
document.addEventListener("click", () => {
	const until = Date.now() + 400;
	while (Date.now() < until);
	R.busy = true;
});
R.ready = true;
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    let menu = frame.context_menu.take();
    luna.sync_dom(dom_anchors::build(&frame));
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    luna.sync(Duration::from_secs(10));
    // The thread is held by a listener while the right-click goes out and
    // then the frame that draws the menu.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "click".into(),
        data: json!({}),
    });
    std::thread::sleep(Duration::from_millis(100));
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::row("album:55", 1)),
        kind: "contextmenu".into(),
        data: json!({ "button": 2, "clientX": 400, "clientY": 300 }),
    });
    frame.context_menu = menu;
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["menuSeen"].is_string());
    assert_eq!(r["busy"], json!(true), "{r}");
    assert_eq!(r["heard"], json!(1), "{r}");
    assert_eq!(r["menuSeen"], json!("after the event"), "{r}");
    finish(luna, dir);
}

/// Input that keeps arriving (a scrolling list sends a tree, `scroll` and
/// `hover` every frame) does not hold the plugins' timers back until it
/// stops: a timer that is due runs between the events, as a browser runs
/// its timers between input tasks.
#[test]
fn a_due_timer_runs_while_input_keeps_arriving() {
    const EVENTS: u64 = 300;
    let (luna, _host, dir) = start(
        r##"
let heard = 0;
// Each event holds the thread a little: the queue outruns the listener.
document.addEventListener("keydown", () => {
	heard++;
	const until = Date.now() + 4;
	while (Date.now() < until);
});
document.addEventListener("keyup", () => {
	setTimeout(() => {
		R.timerAfter = heard;
	}, 0);
});
R.ready = true;
"##,
    );
    luna.sync_dom(dom_anchors::build(&dom_anchors::sample_frame()));
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    luna.sync(Duration::from_secs(10));
    let event = |kind: &str| DomEvent {
        target: DomTarget::Host(String::new()),
        kind: kind.into(),
        data: json!({}),
    };
    // The timer is set by the first event; the rest are queued behind it
    // at once, faster than the listener takes them.
    luna.dom_event(event("keyup"));
    for _ in 0..EVENTS {
        luna.dom_event(event("keydown"));
    }
    let r = results_until(&luna, &dir, 30, |r| r["timerAfter"].is_number());
    let after = r["timerAfter"].as_u64().unwrap_or(EVENTS);
    assert!(
        after < EVENTS / 2,
        "the timer waited for {after} of {EVENTS} queued events: {r}"
    );
    finish(luna, dir);
}

/// Input that queues up while a plugin holds the thread (a click, then the
/// right-click) reaches the listeners in order, and the menu the
/// right-click opens comes after its `contextmenu`, not after the click
/// queued before it.
#[test]
fn a_right_click_queued_behind_other_input_comes_before_its_menu() {
    let (luna, _host, dir) = start(
        r##"
let armed = false;
document.addEventListener("contextmenu", () => {
	armed = true;
	R.heard = (R.heard ?? 0) + 1;
}, true);
new MutationObserver((records) => {
	for (const r of records)
		for (const n of r.addedNodes) {
			if (!(n instanceof Element)) continue;
			const menu = n.matches('[data-test="contextmenu"]') ? n : n.querySelector('[data-test="contextmenu"]');
			if (menu) R.menuSeen = armed ? "after the event" : "before the event";
		}
}).observe(document.body, { childList: true, subtree: true });
// A plugin busy on the thread while the click, the right-click and the
// menu arrive behind it.
document.addEventListener("mousedown", () => {
	const until = Date.now() + 400;
	while (Date.now() < until);
	R.busy = true;
});
document.addEventListener("click", () => {
	R.clicked = !armed;
});
R.ready = true;
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    let menu = frame.context_menu.take();
    luna.sync_dom(dom_anchors::build(&frame));
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    luna.sync(Duration::from_secs(10));
    let event = |kind: &str| DomEvent {
        target: DomTarget::Host(String::new()),
        kind: kind.into(),
        data: json!({}),
    };
    luna.dom_event(event("mousedown"));
    std::thread::sleep(Duration::from_millis(100));
    luna.dom_event(event("click"));
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::row("album:55", 1)),
        kind: "contextmenu".into(),
        data: json!({ "button": 2, "clientX": 400, "clientY": 300 }),
    });
    frame.context_menu = menu;
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["menuSeen"].is_string());
    assert_eq!(r["busy"], json!(true), "{r}");
    assert_eq!(r["clicked"], json!(true), "the click came first: {r}");
    assert_eq!(r["heard"], json!(1), "{r}");
    assert_eq!(r["menuSeen"], json!("after the event"), "{r}");
    finish(luna, dir);
}

/// LuckyLP observes `div[data-track--page-id='mycollection_albums']` and
/// puts its shuffle after the `h1` in it. In TIDAL 2.46.0 that page is
/// `[data-test=page-title-container]` and the `h1` sits in its `._header_`
/// (beside the "..."), so the shuffle joins the header; here it went into
/// the page itself.
#[test]
fn a_button_after_the_albums_title_joins_its_header() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "div[data-track--page-id='mycollection_albums']", (e) => {
	if (document.querySelector(".lucky")) return;
	const h = e.querySelector("h1");
	if (!h) return;
	const d = document.createElement("div");
	d.className = "lucky";
	h.insertAdjacentElement("afterend", d);
	R.page = e.getAttribute("data-test");
	R.parent = String(d.parentElement.className);
	R.title = h.textContent;
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.page_id = Some("mycollection_albums".into());
        page.title = Some("Albums".into());
    }
    frame.lists.clear();
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["parent"].is_string());
    assert_eq!(r["page"], json!("page-title-container"), "{r}");
    assert_eq!(r["parent"], json!("_header_d2e600f"), "{r}");
    assert_eq!(r["title"], json!("Albums"), "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::PAGE_TITLE_HEADER, Position::Append).count() == 1
    });
    assert!(o.at(keys::PAGE, Position::Append).next().is_none());
    finish(luna, dir);
}

/// The open queue's rows are TIDAL 2.46.0's: a plugin finds each
/// `div[data-test=playqueue-item]`, reads and rewrites its title's `p`, and
/// sets its cover's `img.src` (uwu-ifier); those changes reach the overlay
/// at the rows' parts. The rows were not in the document.
#[test]
fn plugins_read_and_rewrite_the_queue_rows() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const rows = document.querySelectorAll('[data-test="playqueue-scroll"] [data-test="playqueue-item"]');
	if (!rows.length || R.done) return;
	R.titles = [...rows].map((r) => r.querySelector('[data-test="playqueue-item-title"] p')?.textContent ?? null);
	R.priorities = [...rows].map((r) => r.getAttribute("data-test-priority"));
	R.artist = rows[1].querySelector('[data-test="grid-item-detail-text-title-artist"]')?.textContent ?? null;
	R.playing = !!rows[0].querySelector('[data-test="nowplaying-icon"]');
	rows[1].querySelector('[data-test="playqueue-item-title"] p').textContent = "Wewwitten";
	rows[1].querySelector('[data-test="cell-cover"] img').src = "https://example.test/neko.png";
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([900.0, 40.0, 360.0, 600.0]), false));
    let row = |id: &str, y: f32, title: &str, priority: &'static str| dom_anchors::QueueRow {
        id: id.into(),
        priority,
        now_playing: id == "current",
        track_id: "77".into(),
        title: title.into(),
        artists: vec![dom_anchors::Link {
            id: "5".into(),
            name: "Artist Five".into(),
        }],
        cover_url: Some("https://resources.tidal.com/images/a/80x80.jpg".into()),
        removable: id != "current",
        rect: Some([900.0, y, 360.0, 56.0]),
        cover_rect: Some([902.0, y + 8.0, 40.0, 40.0]),
        title_rect: Some([954.0, y + 10.0, 260.0, 20.0]),
        artist_rect: Some([954.0, y + 30.0, 260.0, 20.0]),
        action_rect: Some([1230.0, y + 14.0, 28.0, 28.0]),
    };
    // Noted out of order: the document has them as the screen does.
    frame.queue_rows = vec![
        row("next:0", 160.0, "Next Song", "priority_keep"),
        row("current", 100.0, "Playing Song", "priority_none"),
    ];
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["titles"], json!(["Playing Song", "Next Song"]), "{r}");
    assert_eq!(
        r["priorities"],
        json!(["priority_none", "priority_keep"]),
        "{r}"
    );
    assert_eq!(r["artist"], json!("Artist Five"), "{r}");
    assert_eq!(r["playing"], json!(true), "{r}");
    let row = keys::queue_item("next:0");
    let o = overlay_until(&luna, 10, |o| {
        o.host(&format!("{row}:title"))
            .is_some_and(|h| h.text.is_some())
    });
    assert_eq!(
        o.host(&format!("{row}:title")).unwrap().text.as_deref(),
        Some("Wewwitten")
    );
    assert!(
        o.host(&format!("{row}:img")).is_some_and(|h| h
            .attrs
            .iter()
            .any(|(n, v)| n == "src" && v.contains("neko"))),
        "{:?}",
        o.host(&format!("{row}:img"))
    );
    finish(luna, dir);
}

// ---------------------------------------------------------------- page scans (plugin round 8)

/// Reading a colour or an image from `getComputedStyle` does not lay the
/// page out, as in a browser: the box is read when a size is. A script
/// scanning the whole page (uwu-ifier reads `backgroundImage` of every
/// element for every added node) made the page lay out on each call, and
/// after a style change each read waited for the UI's next layout.
#[test]
fn reading_a_style_does_not_lay_out_the_page() {
    let (luna, _host, dir) = start(
        r##"
const proto = Element.prototype;
const real = proto.getBoundingClientRect;
let boxes = 0;
proto.getBoundingClientRect = function () { boxes++; return real.call(this); };
const box = document.createElement("div");
box.className = "scan-box";
box.style.backgroundImage = 'url("https://example.com/a.png")';
for (let i = 0; i < 20; i++) box.append(document.createElement("span"));
document.body.append(box);
let images = 0;
for (const el of document.body.querySelectorAll("*")) {
	const b = getComputedStyle(el).backgroundImage;
	if (b && b !== "none") images++;
}
R.images = images;
R.scanBoxes = boxes;
boxes = 0;
const s = getComputedStyle(box);
R.idleBoxes = boxes;
R.width = s.width;
R.widthBoxes = boxes > 0;
R.rectWidth = `${real.call(box).width}px`;
proto.getBoundingClientRect = real;
R.done = true;
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["images"], json!(1), "{r}");
    assert_eq!(r["scanBoxes"], json!(0), "{r}");
    assert_eq!(r["idleBoxes"], json!(0), "{r}");
    assert_eq!(r["widthBoxes"], json!(true), "{r}");
    assert_eq!(r["width"], r["rectWidth"], "{r}");
    finish(luna, dir);
}

/// `getComputedStyle` over the whole page works each element out once:
/// a second scan with nothing changed resolves none, a class change
/// resolves the subtree it can change, and every value follows the change
/// (inherited colours, variables set by an ancestor's class, an inline
/// style, a moved element, a new style sheet). A script writing to the
/// declaration it got changes nothing else.
#[test]
fn a_page_scan_resolves_each_element_once() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ":root { --tint: rgb(1, 2, 3); } .scan { color: var(--tint); } .scan.alt { --tint: rgb(4, 5, 6); } .scan i { font-weight: 700; }";
document.head.append(st);
const top = document.createElement("div");
top.className = "scan";
let at = top;
const chain = [];
for (let i = 0; i < 12; i++) {
	const d = document.createElement("div");
	at.append(d);
	chain.push(d);
	for (let j = 0; j < 3; j++) d.append(document.createElement("i"));
	at = d;
}
document.body.append(top);
const all = () => [top, ...top.querySelectorAll("*")];
const scan = () => all().map((el) => { const s = getComputedStyle(el); return `${s.color}/${s.fontWeight}`; });
const leafOf = () => chain.at(-1).lastChild;
scan();
let n = D.styleResolves;
const a = scan();
R.again = D.styleResolves - n;
R.leaf = getComputedStyle(leafOf()).color + "/" + getComputedStyle(leafOf()).fontWeight;
R.elements = all().length;
top.classList.add("alt");
n = D.styleResolves;
scan();
R.afterClass = D.styleResolves - n;
R.leafAlt = getComputedStyle(leafOf()).color;
chain[8].style.color = "rgb(7, 8, 9)";
n = D.styleResolves;
scan();
R.afterInline = D.styleResolves - n;
R.leafInline = getComputedStyle(leafOf()).color;
R.aboveInline = getComputedStyle(chain[7]).color;
const moved = leafOf();
chain[2].append(moved);
R.moved = getComputedStyle(moved).color;
const st2 = document.createElement("style");
st2.textContent = ".scan i { font-weight: 300; }";
document.head.append(st2);
R.sheet = getComputedStyle(moved).fontWeight;
const cs = getComputedStyle(chain[0]);
cs.setProperty("color", "rgb(9, 9, 9)");
R.written = cs.color;
R.fresh = getComputedStyle(chain[0]).color;
R.done = true;
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let elements = r["elements"].as_u64().unwrap();
    assert_eq!(r["again"], json!(0), "{r}");
    assert_eq!(r["leaf"], json!("rgb(1, 2, 3)/700"), "{r}");
    // The class goes on `top`: it and what is inside it, and at most the
    // page round it when a rule looks at siblings.
    let after = r["afterClass"].as_u64().unwrap();
    assert!(after >= elements && after <= elements + 3, "{r}");
    assert_eq!(r["leafAlt"], json!("rgb(4, 5, 6)"), "{r}");
    // An inline style on the ninth: it and the four levels below it.
    let inline = r["afterInline"].as_u64().unwrap();
    assert!((1..=elements / 2).contains(&inline), "{r}");
    assert_eq!(r["leafInline"], json!("rgb(7, 8, 9)"), "{r}");
    assert_eq!(r["aboveInline"], json!("rgb(4, 5, 6)"), "{r}");
    assert_eq!(r["moved"], json!("rgb(4, 5, 6)"), "{r}");
    assert_eq!(r["sheet"], json!("300"), "{r}");
    assert_eq!(r["written"], json!("rgb(9, 9, 9)"), "{r}");
    assert_eq!(r["fresh"], json!("rgb(4, 5, 6)"), "{r}");
    finish(luna, dir);
}

/// A new part of the page arrives as React commits it: put together
/// while detached and inserted as one node, so an observer on the page
/// gets one record with the new subtree's root, the subtree inside it
/// (TIDAL's new page is one record; Tidalfast's was one per element, and
/// a plugin scanning the page for every added node scanned it hundreds of
/// times).
#[test]
fn a_new_part_of_the_page_arrives_as_one_record() {
    let (luna, _host, dir) = start(
        r##"
let on = false;
const recs = [];
new MutationObserver((list) => {
	if (!on) return;
	for (const r of list) {
		if (r.type !== "childList" || !r.addedNodes.length) continue;
		for (const n of r.addedNodes) recs.push([n.getAttribute?.("data-test") ?? n.nodeName, n.querySelectorAll?.('[data-test="x-cell"]').length ?? 0]);
	}
	R.recs = recs;
}).observe(document.body, { childList: true, subtree: true });
const wait = setInterval(() => {
	if (!document.querySelector('[data-test="footer-track-title"]')) return;
	clearInterval(wait);
	on = true;
	R.ready = true;
}, 20);
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let mut t = tree();
    let el = |key: &str, parent: &str, tag: &'static str, test: &str| super::dom::HostNode {
        key: key.into(),
        parent: Some(parent.into()),
        tag: tag.into(),
        attrs: vec![("data-test".into(), test.into())],
        text: None,
        rect: None,
        value: None,
        scroll: None,
    };
    t.nodes
        .push(el("x-panel", keys::PAGE, "section", "x-panel"));
    for i in 0..3 {
        t.nodes
            .push(el(&format!("x-row{i}"), "x-panel", "div", "x-row"));
    }
    for i in 0..3 {
        let mut cell = el(
            &format!("x-cell{i}"),
            &format!("x-row{i}"),
            "span",
            "x-cell",
        );
        cell.text = Some(format!("Cell {i}"));
        t.nodes.push(cell);
    }
    luna.sync_dom(t);
    let r = results_until(&luna, &dir, 10, |r| r["recs"].is_array());
    std::thread::sleep(Duration::from_millis(100));
    luna.sync(Duration::from_secs(5));
    let r = dom_results(&dir)
        .get("recs")
        .cloned()
        .unwrap_or(r["recs"].clone());
    assert_eq!(r, json!([["x-panel", 3]]));
    finish(luna, dir);
}

// ---------------------------------------------------------------- mutation observer delivery (plugin round 8)

/// Installs `first` and then `second` (each as written, with `R` its
/// results), waiting for each to run, with a short watchdog.
fn start_two(first: (&str, &str), second: (&str, &str)) -> (Luna, PathBuf) {
    let files: Files = Arc::default();
    for (name, body) in [first, second] {
        files.lock().unwrap().insert(
            format!("/{name}.json"),
            json!({ "name": name, "version": "1.0.0", "hash": name })
                .to_string()
                .into_bytes(),
        );
        files.lock().unwrap().insert(
            format!("/{name}.mjs"),
            format!("{HEADER}{body}").into_bytes(),
        );
    }
    let base = serve(files);
    let dir = temp_dir("dom2");
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(1);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    for (name, _) in [first, second] {
        luna.install(&format!("{base}/{name}.mjs"));
        wait_until(&luna, 30, |s| {
            plugin(s, name).is_some_and(|p| p.status == PluginStatus::Running)
        });
    }
    (luna, dir)
}

/// Observer callbacks run one after another, never one inside another,
/// whatever a callback does (lays the page out, clicks, changes the page),
/// in the DOM standard's order (Chromium's): the records a callback causes
/// reach an observer whose turn is still to come in the same round, and
/// the observers already called in the next one.
#[test]
fn observer_callbacks_never_run_inside_each_other() {
    let (luna, _host, dir) = start(
        r##"
let depth = 0;
const log = [];
const host = () => document.querySelector('[data-test="footer-track-title"]');
const box = document.createElement("div");
box.className = "nest-box";
document.body.append(box);
const button = document.createElement("button");
button.addEventListener("click", () => box.append(document.createElement("u")));
box.append(button);
const watch = (name, act) => new MutationObserver((list) => {
	if (depth > 0) log.push(`nested ${name}`);
	depth++;
	try {
		log.push(`${name} ${list.length}`);
		act?.();
	} finally {
		depth--;
	}
	R.log = log;
}).observe(box, { childList: true, subtree: true, attributes: true });
let rounds = 0;
watch("a", () => {
	if (rounds++ > 0) return;
	const t = host();
	if (t) {
		t.style.color = "rgb(1, 2, 3)";
		getComputedStyle(t).width;
		t.getBoundingClientRect();
	}
	button.click();
	box.append(document.createElement("i"));
	box.setAttribute("data-x", "1");
});
watch("b");
const wait = setInterval(() => {
	if (!host()) return;
	clearInterval(wait);
	box.append(document.createElement("s"));
	setTimeout(() => (R.done = true), 300);
}, 20);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    // Round one: the first append to `a`; to `b` with the three changes
    // `a` made (its click's append, its append and its attribute). Round
    // two: those three to `a`.
    assert_eq!(r["log"], json!(["a 1", "b 4", "a 3"]), "{r}");
    finish(luna, dir);
}

/// A plugin whose observer callback never returns is stopped by the
/// watchdog, and the other observers still get the records they were
/// waiting for with it (they waited until something else changed the
/// page).
#[test]
fn a_stopped_callback_leaves_the_other_observers_their_records() {
    let (luna, dir) = start_two(
        (
            "Hog",
            r##"
new MutationObserver((list) => {
	for (const r of list) for (const n of r.addedNodes) if (n.dataset?.hog !== undefined) for (;;) {}
}).observe(document.body, { childList: true, subtree: true });
"##,
        ),
        (
            "Watch",
            r##"
new MutationObserver((list) => {
	for (const r of list) for (const n of r.addedNodes) if (n.dataset?.hog !== undefined) R.seen = true;
}).observe(document.body, { childList: true, subtree: true });
setTimeout(() => {
	const d = document.createElement("div");
	d.dataset.hog = "";
	document.body.append(d);
}, 50);
"##,
        ),
    );
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Hog").is_some_and(|p| matches!(p.status, PluginStatus::Failed(_)))
    });
    assert!(
        matches!(&plugin(&s, "Hog").unwrap().status, PluginStatus::Failed(e) if e.contains("without yielding"))
    );
    let r = results_until(&luna, &dir, 5, |r| r["seen"] == json!(true));
    assert_eq!(r["seen"], json!(true));
    assert_eq!(
        plugin(&luna.snapshot(), "Watch").unwrap().status,
        PluginStatus::Running
    );
    finish(luna, dir);
}

/// A plugin stopped by the watchdog in its observer callback gets no
/// more callbacks while its unloads run (each ran away again, holding up
/// every plugin for the watchdog's time once more).
#[test]
fn a_stopped_plugins_observers_get_no_more_records() {
    let (luna, dir) = start_two(
        (
            "Hog",
            r##"
unloads.add(() => new Promise((ok) => setTimeout(ok, 2000)));
new MutationObserver((list) => {
	console.log("hog called");
	for (const r of list) for (const n of r.addedNodes) if (n.dataset?.hog !== undefined) for (;;) {}
}).observe(document.body, { childList: true, subtree: true });
"##,
        ),
        (
            "Watch",
            r##"
let n = 0;
new MutationObserver((list) => {
	n += list.length;
	R.records = n;
}).observe(document.body, { childList: true, subtree: true });
setTimeout(() => {
	const d = document.createElement("div");
	d.dataset.hog = "";
	document.body.append(d);
	setTimeout(() => document.body.append(document.createElement("p")), 1300);
}, 50);
"##,
        ),
    );
    let r = results_until(&luna, &dir, 20, |r| r["records"] == json!(2));
    assert_eq!(r["records"], json!(2));
    std::thread::sleep(Duration::from_millis(2500));
    luna.sync(Duration::from_secs(5));
    let s = luna.snapshot();
    let hog = plugin(&s, "Hog").unwrap();
    assert!(
        matches!(&hog.status, PluginStatus::Failed(_)),
        "{:?}",
        hog.status
    );
    let calls = hog
        .logs
        .iter()
        .filter(|l| l.message.contains("hog called"))
        .count();
    assert_eq!(calls, 1, "{:#?}", hog.logs);
    finish(luna, dir);
}

// ---------------------------------------------------------------- right-click verdicts (plugin round 8)

/// A right-click's `contextmenu` says whether a listener cancelled it:
/// `preventDefault()` anywhere, or `stopPropagation()` before it bubbled to
/// TIDAL's root (`#wimp`, where React runs TIDAL's menu handlers). A
/// listener that only watches (element-hider's document capture) cancels
/// nothing.
#[test]
fn a_right_click_says_whether_a_listener_cancelled_it() {
    let (luna, _host, dir) = start(
        r##"
const wait = setInterval(() => {
	const title = document.querySelector('[data-test="footer-track-title"]');
	const play = document.querySelector('[data-test="play"], [data-test="pause"]');
	const next = document.querySelector('[data-test="next"]');
	if (!title || !play || !next) return;
	clearInterval(wait);
	document.addEventListener("contextmenu", () => (R.watched = (R.watched ?? 0) + 1), true);
	title.addEventListener("contextmenu", (e) => e.preventDefault());
	play.addEventListener("contextmenu", (e) => e.stopPropagation());
	R.ready = true;
}, 20);
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let click = |key: &str| {
        luna.dom_event_verdict(
            DomEvent {
                target: DomTarget::Host(key.into()),
                kind: "contextmenu".into(),
                data: json!({ "clientX": 10, "clientY": 10, "button": 2 }),
            },
            Duration::from_secs(5),
        )
    };
    assert_eq!(click(keys::TITLE_SPAN), Some(true), "preventDefault");
    assert_eq!(click(keys::PLAY), Some(true), "stopped before the root");
    assert_eq!(click(keys::NEXT), Some(false), "only watched");
    let r = results_until(&luna, &dir, 10, |r| r["watched"] == json!(3));
    assert_eq!(r["watched"], json!(3));
    finish(luna, dir);
}

/// Cotton Candy adaptive names hundreds of TIDAL's elements by their whole
/// class attribute (`[class="_wrapper_512d5f4 x"]`), and each such rule was
/// tried on every element that has a class: the page's first style pass
/// under the theme ran for seconds. A rule on the class attribute that
/// needs a class (`=`, `~=`, a `^=` with a whole first class) is tried
/// only on elements of that class, `:root` only on the root and
/// `:is(button)` only on buttons; what they match is what they matched.
#[test]
fn a_rule_on_the_class_attribute_is_tried_only_where_its_class_is() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
let css = "";
for (let i = 0; i < 300; i++) css += `[class="_zz${i}_0abcdef y"] { color: rgb(9, 9, 9); }\n`;
for (let i = 0; i < 100; i++) css += `:root.zz${i}, :is(.zz${i}) { color: rgb(9, 9, 9); }\n`;
css += '[class="_zzModule_5bb6028 x"] { color: rgb(1, 0, 0); }\n';
css += '[class~="tok"] { color: rgb(2, 0, 0); }\n';
css += '[class^="lead "] { color: rgb(3, 0, 0); }\n';
css += '[class="CI ONE" i] { color: rgb(4, 0, 0); }\n';
css += ':root { color: rgb(5, 0, 0); }\n';
css += ':is(section):not(.no) { color: rgb(6, 0, 0); }\n';
css += ':where(.wq) { color: rgb(7, 0, 0); }\n';
const st = document.createElement("style");
st.textContent = css;
document.head.append(st);
const box = document.createElement("div");
const make = (tag, cls) => {
	const e = document.createElement(tag);
	if (cls !== null) e.setAttribute("class", cls);
	box.append(e);
	return e;
};
const els = {
	fuzzy: make("div", "_zzModule_1234567 x"),
	fuzzyMore: make("div", "_zzModule_1234567 x z"),
	tok: make("div", "a tok b"),
	lead: make("div", "lead b"),
	leader: make("div", "leader b"),
	ci: make("div", "ci one"),
	section: make("section", null),
	sectionNo: make("section", "no"),
	wq: make("p", "wq"),
};
const plain = [];
for (let i = 0; i < 50; i++) plain.push(make("div", "plain k"));
document.body.append(box);
const tried = D.candidatesTried ?? 0;
for (const e of plain) getComputedStyle(e).color;
R.perElement = ((D.candidatesTried ?? 0) - tried) / plain.length;
R.colors = Object.fromEntries(Object.entries(els).map(([k, e]) => [k, getComputedStyle(e).color]));
R.root = getComputedStyle(document.documentElement).color;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["colors"]["fuzzy"], json!("rgb(1, 0, 0)"), "{r}");
    assert_ne!(r["colors"]["fuzzyMore"], json!("rgb(1, 0, 0)"), "{r}");
    assert_eq!(r["colors"]["tok"], json!("rgb(2, 0, 0)"), "{r}");
    assert_eq!(r["colors"]["lead"], json!("rgb(3, 0, 0)"), "{r}");
    assert_ne!(r["colors"]["leader"], json!("rgb(3, 0, 0)"), "{r}");
    assert_eq!(r["colors"]["ci"], json!("rgb(4, 0, 0)"), "{r}");
    assert_eq!(r["colors"]["section"], json!("rgb(6, 0, 0)"), "{r}");
    assert_ne!(r["colors"]["sectionNo"], json!("rgb(6, 0, 0)"), "{r}");
    assert_eq!(r["colors"]["wq"], json!("rgb(7, 0, 0)"), "{r}");
    assert_eq!(r["root"], json!("rgb(5, 0, 0)"), "{r}");
    assert!(
        r["perElement"].as_f64().unwrap() < 20.0,
        "each element still tries the class attribute's rules: {r}"
    );
    finish(luna, dir);
}

/// A rule that names an ancestor (`#nowPlaying ._item_235a32d:hover ... *`,
/// the cotton candy family's) walked up every element it was tried on, to
/// the root, though no element of the page had that ancestor. As in
/// Chromium, each element keeps a filter of what its ancestors are (tags,
/// ids, classes, attribute names), and a rule whose ancestors cannot all
/// be there is not walked; one whose can still matches.
#[test]
fn a_rule_whose_ancestors_are_not_there_is_not_walked() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
let css = "";
for (let i = 0; i < 200; i++) css += `#absent${i} ._gone_0abcdef span { color: rgb(9, 9, 9); }\n`;
css += '.box > .mid span { color: rgb(1, 0, 0); }\n';
css += '[data-zone] .mid i { color: rgb(2, 0, 0); }\n';
css += '.first + .mid b { color: rgb(3, 0, 0); }\n';
css += '[class="box q"] u { color: rgb(4, 0, 0); }\n';
const st = document.createElement("style");
st.textContent = css;
document.head.append(st);
const box = document.createElement("div");
box.setAttribute("class", "box q");
box.setAttribute("data-zone", "1");
const first = document.createElement("div");
first.className = "first";
const mid = document.createElement("div");
mid.className = "mid";
box.append(first, mid);
const spans = [];
for (let i = 0; i < 40; i++) {
	const s = document.createElement("span");
	mid.append(s);
	spans.push(s);
}
const i = document.createElement("i");
const b = document.createElement("b");
const u = document.createElement("u");
mid.append(i, b, u);
document.body.append(box);
const walks = D.selectorWalks ?? 0;
R.span = getComputedStyle(spans[3]).color;
for (const s of spans) getComputedStyle(s).color;
R.perSpan = ((D.selectorWalks ?? 0) - walks) / spans.length;
R.i = getComputedStyle(i).color;
R.b = getComputedStyle(b).color;
R.u = getComputedStyle(u).color;
// An ancestor that gains the class a rule needs: the filter follows it.
const late = document.createElement("span");
first.append(late);
R.lateBefore = getComputedStyle(late).color;
first.classList.add("mid");
R.lateAfter = getComputedStyle(late).color;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["span"], json!("rgb(1, 0, 0)"), "{r}");
    assert_eq!(r["i"], json!("rgb(2, 0, 0)"), "{r}");
    assert_eq!(r["b"], json!("rgb(3, 0, 0)"), "{r}");
    assert_eq!(r["u"], json!("rgb(4, 0, 0)"), "{r}");
    assert_ne!(r["lateBefore"], json!("rgb(1, 0, 0)"), "{r}");
    assert_eq!(r["lateAfter"], json!("rgb(1, 0, 0)"), "{r}");
    assert!(
        r["perSpan"].as_f64().unwrap() < 10.0,
        "each span still walks the absent ancestors' rules: {r}"
    );
    finish(luna, dir);
}

/// Every picture of a page that came in (each cover) made the whole page
/// match every rule again, the page's first pass under a large theme over
/// and over. A loaded picture changes what is drawn, never what a selector
/// matches: the cascades stay and only the overlay is built again.
#[test]
fn a_picture_that_loads_keeps_every_cascade() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".pics span { color: rgb(1, 2, 3); }";
document.head.append(st);
const box = document.createElement("div");
box.className = "pics";
for (let i = 0; i < 20; i++) box.append(document.createElement("span"));
document.body.append(box);
const img = document.createElement("img");
const loaded = new Promise((resolve) => {
	img.addEventListener("load", () => resolve("load"));
	img.addEventListener("error", () => resolve("error"));
});
box.append(img);
await new Promise((r) => setTimeout(r, 100));
getComputedStyle(box.firstChild).color;
const resets = D.cascadeResets ?? 0;
const runs = D.cascadeRuns ?? 0;
img.src = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
R.event = await loaded;
await new Promise((r) => setTimeout(r, 100));
R.color = getComputedStyle(box.firstChild).color;
R.resets = (D.cascadeResets ?? 0) - resets;
R.runs = (D.cascadeRuns ?? 0) - runs;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["event"], json!("load"), "{r}");
    assert_eq!(r["color"], json!("rgb(1, 2, 3)"), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    assert_eq!(r["runs"], json!(0), "{r}");
    finish(luna, dir);
}

/// A theme's `:hover` rules matched the whole page again on every move of
/// the pointer (each move a full restyle: on a large theme the hovered row
/// lit late, and the checker's pointer leaving a menu row was undone only
/// seconds later). As Chromium does, a move restyles only the elements
/// whose `:hover` state changed and that a `:hover` compound can match,
/// what is inside them, and the siblings after them where a rule reads
/// those; the result is the one a whole restyle gives.
#[test]
fn a_hover_change_restyles_only_what_it_reaches() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '[data-test="tracklist-row"] { border-radius: 3px; } [data-test="tracklist-row"]:hover { background-color: rgb(1, 2, 3); } [data-test="tracklist-row"]:hover * { color: rgb(7, 8, 9); } ._indexColumn_3ac64ab:hover ~ [data-test="table-row-title"] { color: rgb(4, 5, 6); }';
document.head.append(st);
setInterval(() => {
	R.runs = D.cascadeRuns ?? 0;
	R.total = document.body.querySelectorAll("*").length;
}, 10);
"##,
    );
    luna.sync_dom(tree());
    let list = "album:55";
    let row = |i: usize| keys::row(list, i);
    let bg = |o: &DomOverlay, i: usize| {
        o.hosts
            .get(&row(i))
            .and_then(|h| h.style.background_color.clone())
    };
    let color = |o: &DomOverlay, i: usize, cell: &str| {
        o.hosts
            .get(&keys::row_cell(list, i, cell))
            .and_then(|h| h.style.color.clone())
    };
    let cell = |i: usize| keys::row_cell(list, i, "index-col");
    overlay_until(&luna, 10, |o| {
        o.hosts
            .get(&row(1))
            .is_some_and(|h| h.style.border_radius.is_some())
    });
    let runs = || {
        std::thread::sleep(Duration::from_millis(200));
        let r = results_until(&luna, &dir, 10, |r| r["runs"].is_number());
        (r["runs"].as_u64().unwrap(), r["total"].as_u64().unwrap())
    };
    let hover = |key: String| {
        luna.dom_event(DomEvent {
            target: DomTarget::Host(key),
            kind: "hover".into(),
            data: Value::Null,
        })
    };
    let (start, total) = runs();
    // The pointer on a row's number: the row, what is in it, and the
    // title after the number.
    hover(cell(1));
    let o = overlay_until(&luna, 10, |o| bg(o, 1).as_deref() == Some("rgb(1, 2, 3)"));
    assert_eq!(bg(&o, 0), None);
    assert_eq!(bg(&o, 2), None);
    assert_eq!(color(&o, 1, "title").as_deref(), Some("rgb(4, 5, 6)"));
    assert_eq!(color(&o, 1, "title-text").as_deref(), Some("rgb(7, 8, 9)"));
    assert_eq!(color(&o, 2, "title"), None);
    let (over, _) = runs();
    hover(cell(2));
    let o = overlay_until(&luna, 10, |o| bg(o, 2).as_deref() == Some("rgb(1, 2, 3)"));
    assert_eq!(bg(&o, 1), None);
    assert_eq!(color(&o, 1, "title"), None);
    assert_eq!(color(&o, 1, "title-text"), None);
    assert_eq!(color(&o, 2, "title").as_deref(), Some("rgb(4, 5, 6)"));
    assert_eq!(color(&o, 2, "title-text").as_deref(), Some("rgb(7, 8, 9)"));
    let (moved, _) = runs();
    // The pointer gone: everything as before.
    hover(String::new());
    let o = overlay_until(&luna, 10, |o| bg(o, 2).is_none());
    assert_eq!(color(&o, 2, "title"), None);
    assert_eq!(color(&o, 2, "title-text"), None);
    let (gone, _) = runs();
    for (what, n) in [
        ("onto a row", over - start),
        ("to the next row", moved - over),
        ("off the rows", gone - moved),
    ] {
        assert!(
            n * 4 < total,
            "the pointer moving {what} restyled {n} of the page's {total} elements"
        );
    }
    finish(luna, dir);
}

/// The player bar's cover is TIDAL's image cell: `figure[data-test=current-
/// media-imagery] > div.tidal-ui__geometry.css-4d6l5a._cellImageShowVideo
/// Cover_d698c20 > div.css-16luxag > div.css-1tk08rl > img` (TIDAL 2.46.0
/// and 2.47.0), the picture in an absolutely placed box in a relative one.
/// A plugin that lays its own layer over the picture in the picture's
/// parent (radiant-lyrics' animated cover) finds that box, positioned, as
/// in TIDAL, not the figure.
#[test]
fn the_player_covers_picture_sits_in_tidals_image_cell() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (R.found) return;
	const img = document.querySelector('[data-test="footer-player"] [data-test="current-media-imagery"] img');
	if (!img) return;
	const up = [];
	for (let n = img.parentElement; n && n.getAttribute("data-test") !== "current-media-imagery"; n = n.parentElement) up.push([n.getAttribute("class"), getComputedStyle(n).position]);
	R.found = up;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(dom_anchors::build(&dom_anchors::sample_frame()));
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    assert_eq!(
        r["found"],
        json!([
            ["css-1tk08rl", "absolute"],
            ["css-16luxag", "relative"],
            [
                "tidal-ui__geometry css-4d6l5a _cellImageShowVideoCover_d698c20",
                "static"
            ]
        ]),
        "{r}"
    );
    finish(luna, dir);
}

/// The now-playing cover's tilting box (`div._tilt_8fda471._artworkTilt_
/// 1c6d5cc`) is positioned in TIDAL (`position: relative`, 2.47.0): a
/// plugin that lays its layer over the cover there (radiant-lyrics'
/// animated cover, TidalEQ's visualizer) finds it so and leaves it as it
/// is, as in TIDAL, instead of setting its position itself.
#[test]
fn the_now_playing_covers_tilting_box_is_positioned() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (R.found) return;
	const tilt = document.querySelector('[data-test="new-now-playing"] [class*="_artworkTilt_"]');
	if (!tilt) return;
	R.found = [tilt.getAttribute("class"), getComputedStyle(tilt).position];
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(dom_anchors::build(&dom_anchors::sample_frame()));
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    assert_eq!(
        r["found"],
        json!(["_tilt_8fda471 _artworkTilt_1c6d5cc", "relative"]),
        "{r}"
    );
    finish(luna, dir);
}

/// A rule that reads siblings or position (a plugin's `:nth-child(odd)`
/// rows, a theme's `.a + .b`) made every node that came, went or changed
/// class restyle the whole page: with eighteen plugins about 0.6 s an
/// overlay build, so a scrolling list updated about once a second. As
/// Chromium invalidates siblings, only the children of the node whose
/// children changed that such a compound can match are matched again (with
/// what is inside them where a later step reaches it), and what they
/// match is a whole restyle's.
#[test]
fn a_sibling_rule_restyles_only_the_siblings_it_reaches() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".list > div:nth-child(odd) { background-color: rgb(1, 2, 3); } .list > .sel + div { color: rgb(4, 5, 6); } .list span { margin-left: 1px; }";
document.head.append(st);
const list = document.createElement("div");
list.className = "list";
const row = () => {
	const r = document.createElement("div");
	for (let i = 0; i < 5; i++) r.append(document.createElement("span"));
	return r;
};
for (let i = 0; i < 60; i++) list.append(row());
document.body.append(list);
const all = () => [list, ...list.querySelectorAll("*")];
const look = () => all().map((e) => getComputedStyle(e).backgroundColor + "/" + getComputedStyle(e).color);
look();
R.total = all().length;
let runs = D.cascadeRuns ?? 0;
list.prepend(row());
const after = look();
R.prependRuns = (D.cascadeRuns ?? 0) - runs;
R.first = after[1];
R.second = after[7];
runs = D.cascadeRuns ?? 0;
list.children[10].classList.add("sel");
const marked = look();
R.classRuns = (D.cascadeRuns ?? 0) - runs;
R.next = marked[1 + 11 * 6];
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    // Row 0 is new and odd; row 1 was row 0.
    let part = |k: &str, i: usize| r[k].as_str().unwrap().split('/').nth(i).map(str::to_owned);
    assert_eq!(part("first", 0).as_deref(), Some("rgb(1, 2, 3)"), "{r}");
    assert_eq!(
        part("second", 0).as_deref(),
        Some("rgba(0, 0, 0, 0)"),
        "{r}"
    );
    assert_eq!(part("next", 1).as_deref(), Some("rgb(4, 5, 6)"), "{r}");
    let total = r["total"].as_u64().unwrap();
    for what in ["prependRuns", "classRuns"] {
        let n = r[what].as_u64().unwrap();
        assert!(
            n * 3 < total,
            "{what}: {n} of the list's {total} elements restyled: {r}"
        );
    }
    finish(luna, dir);
}

/// A `:has` rule whose subject is a bare tag after a context
/// (radiant-lyrics' `[data-test=now-playing-lyrics] > div:has(...)`) made
/// every row a list scroll brought in restyle the whole page: each `div`
/// above the row matched the tag alone, and the list's own subtree went
/// with it. An ancestor the rule's left side does not reach keeps its
/// cascade; one it reaches still follows what is inside it.
#[test]
fn a_has_rule_forgets_only_the_ancestors_its_context_reaches() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".lyr > div:has(.on) { color: rgb(1, 2, 3); } .lyr > div:has(.on) span { margin-left: 2px; }";
document.head.append(st);
const lyr = document.createElement("div");
lyr.className = "lyr";
const inner = document.createElement("div");
inner.append(document.createElement("span"));
lyr.append(inner);
const list = document.createElement("div");
list.className = "list";
const row = () => {
	const r = document.createElement("div");
	for (let i = 0; i < 5; i++) r.append(document.createElement("span"));
	return r;
};
for (let i = 0; i < 60; i++) list.append(row());
document.body.append(lyr, list);
const all = () => [lyr, ...lyr.querySelectorAll("*"), list, ...list.querySelectorAll("*")];
const look = () => all().map((e) => getComputedStyle(e).color + "/" + getComputedStyle(e).marginLeft);
look();
R.total = all().length;
let runs = D.cascadeRuns ?? 0;
list.prepend(row());
look();
R.prependRuns = (D.cascadeRuns ?? 0) - runs;
runs = D.cascadeRuns ?? 0;
list.children[3].querySelector("span").append(document.createElement("i"));
look();
R.deepRuns = (D.cascadeRuns ?? 0) - runs;
const marker = document.createElement("b");
marker.className = "on";
inner.append(marker);
R.before = getComputedStyle(inner).color;
R.after = look()[1];
R.spanAfter = getComputedStyle(lyr.querySelector("span")).marginLeft;
marker.remove();
R.gone = getComputedStyle(inner).color + "/" + getComputedStyle(lyr.querySelector("span")).marginLeft;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let total = r["total"].as_u64().unwrap();
    for what in ["prependRuns", "deepRuns"] {
        let n = r[what].as_u64().unwrap();
        assert!(
            n * 3 < total,
            "{what}: {n} of the page's {total} elements restyled: {r}"
        );
    }
    // What the rule reaches still follows its `:has`.
    assert_eq!(r["before"], json!("rgb(1, 2, 3)"), "{r}");
    assert_eq!(r["spanAfter"], json!("2px"), "{r}");
    assert!(!r["gone"].as_str().unwrap().ends_with("/2px"), "{r}");
    assert!(
        !r["gone"].as_str().unwrap().starts_with("rgb(1, 2, 3)"),
        "{r}"
    );
    finish(luna, dir);
}

/// Whatever a script does to the nodes (inserts, removes, moves, class
/// changes, text that empties or fills a node), the cascades kept after a
/// sibling-aware restyle are the ones a fresh match gives, under rules on
/// every side of `+` and `~`, every position pseudo-class, `:empty`,
/// `:nth-child(... of ...)`, `:not(...)`, `:is(...)` and `:has(...)`.
#[test]
fn kept_cascades_equal_fresh_ones_after_any_change() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
.t > *:nth-child(2n+1) { color: rgb(1, 0, 0); }
.t > .a + .b { background-color: rgb(2, 0, 0); }
.t .a ~ .c span { border-top-color: rgb(3, 0, 0); }
.t li:first-child { margin-left: 1px; }
.t li:last-child > i { margin-right: 2px; }
.t div:only-child { padding-top: 3px; }
.t :nth-last-child(2) { padding-left: 4px; }
.t b:first-of-type { opacity: 0.5; }
.t u:nth-of-type(2) { width: 5px; }
.t i:last-of-type, .t i:only-of-type { min-height: 1px; }
.t .e:empty { height: 6px; }
.t .e:empty + * { min-width: 7px; }
.t :not(.a + .b) > span { max-width: 8px; }
.t :nth-child(2 of .c) { top: 9px; }
.t .a:not(:last-child) .x { left: 10px; }
.t :is(.b ~ .a) i { right: 11px; }
.t li:nth-child(3) .y, .t .c + li .y { bottom: 12px; }
.t .y:nth-last-child(odd)::before { content: "y"; color: rgb(5, 0, 0); }
.t .h:has(> .a) { z-index: 3; }
.t .h:has(.c) span:nth-child(2) { flex-grow: 2; }
.t .h:has(> .b:last-child) { flex-shrink: 3; }
.t .x:has(.y):first-child i { order: 4; }
.t > div:has(.c) i { order: 5; }
.t .h > li:has(> .a) b { order: 6; }
.t .h div:has(.y) .x { order: 7; }
.t li:has(.e) { order: 8; }
.t .x { color: rgb(9, 9, 9); }
`;
document.head.append(st);
let seed = 7;
const rnd = (n) => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed % n;
};
const TAGS = ["div", "li", "b", "u", "span", "i"];
const CLS = ["a", "b", "c", "e", "x", "y", "h"];
const make = () => {
	const e = document.createElement(TAGS[rnd(TAGS.length)]);
	for (const c of CLS) if (rnd(4) === 0) e.classList.add(c);
	return e;
};
const root = document.createElement("div");
root.className = "t";
document.body.append(root);
const els = () => [root, ...root.querySelectorAll("*")];
for (let i = 0; i < 60; i++) {
	const all = els();
	all[rnd(all.length)].append(make());
}
const show = (m) => (m ? JSON.stringify([...m]) : "null");
let checks = 0;
let differ = 0;
let resets = D.cascadeResets ?? 0;
const check = (step) => {
	for (const e of els()) {
		checks++;
		const kept = show(D.cascadeOf(e, false));
		const fresh = show(D.cascadeOf(e, true));
		if (kept !== fresh) {
			differ++;
			R.first ??= `step ${step}: <${e.localName} class="${e.className}"> kept ${kept}, fresh ${fresh}`;
		}
	}
};
check(-1);
resets = D.cascadeResets ?? 0;
for (let step = 0; step < 400; step++) {
	const all = els();
	const pick = () => all[rnd(all.length)];
	const what = rnd(8);
	const e = pick();
	if (what === 0) e.append(make());
	else if (what === 1) {
		const p = pick();
		p.insertBefore(make(), p.childNodes[rnd(p.childNodes.length + 1)] ?? null);
	} else if (what === 2 && e !== root) e.remove();
	else if (what === 3 && e !== root) {
		const p = pick();
		if (!e.contains(p)) p.insertBefore(e, p.firstChild);
	} else if (what === 4) e.classList.toggle(CLS[rnd(CLS.length)]);
	else if (what === 5 && !e.firstElementChild) e.textContent = rnd(2) ? "t" : "";
	else if (what === 6) e.append(document.createTextNode(rnd(2) ? "t" : ""));
	else if (what === 7 && e !== root) e.innerHTML = rnd(2) ? "" : "<i class='a'></i><span class='x'></span>";
	if (els().length > 120) for (const k of [...root.children].slice(0, 2)) k.remove();
	check(step);
	if (step % 10 === 9) await new Promise((r) => setTimeout(r, 0));
}
R.checks = checks;
R.differ = differ;
R.resets = (D.cascadeResets ?? 0) - resets;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 60, |r| r["done"] == json!(true));
    assert!(r["checks"].as_u64().unwrap() > 10_000, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    // The kept cascades were really kept, not thrown away at each change.
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// A node put into or taken out of an element a `:has(...)` compound may
/// match leaves the cascades inside that element as they were when the
/// compound's answer for it stays the same (a rating or tags added to each
/// row a scroll brings in matched every row again under scroll-to-playing's
/// `[aria-rowindex]:has(a[href*="/track/..."])`), and follows the answer
/// when it turns; whatever a script does, the cascades kept are the ones a
/// fresh match gives.
#[test]
fn a_change_inside_a_has_subject_keeps_its_cascades_while_the_answer_stays() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
[aria-rowindex]:has(a[href*="/track/7"]) span { color: rgb(1, 2, 3); }
[aria-rowindex]:has(a[href*="/track/7"]) { background-color: rgb(4, 5, 6); }
.t li:has(> .e) i { margin-left: 1px; }
.t div:has(.y):first-child b { opacity: 0.5; }
.t .h:has(.c.x) .x { padding-top: 2px; }
.t :not(:has(.a)) > u { width: 3px; }
.t span { color: rgb(7, 7, 7); }
`;
document.head.append(st);
// A list of rows, one of them playing.
const list = document.createElement("div");
list.className = "t";
document.body.append(list);
for (let i = 0; i < 30; i++) {
	const row = document.createElement("div");
	row.setAttribute("aria-rowindex", String(i));
	row.innerHTML = `<div><a href="/track/${i}">t</a></div><div><span>a</span><span>b</span></div><div><span>c</span></div>`;
	list.append(row);
}
const rows = [...list.children];
const color = (e) => getComputedStyle(e).color;
R.before = color(rows[7].querySelector("span"));
for (const e of list.querySelectorAll("*")) color(e);
// A plugin's rating in each row: only what it adds is matched.
let runs = D.cascadeRuns ?? 0;
for (const row of rows) {
	const s = document.createElement("span");
	s.className = "rating";
	row.lastElementChild.append(s);
	color(s);
}
for (const row of rows) for (const s of row.querySelectorAll("span")) color(s);
R.ratingRuns = (D.cascadeRuns ?? 0) - runs;
// The playing row's link changes: its row is matched again.
rows[7].querySelector("a").setAttribute("href", "/track/70");
R.stillPlaying = color(rows[7].querySelector("span"));
rows[7].querySelector("a").setAttribute("href", "/track/x");
R.after = color(rows[7].querySelector("span"));
const link = document.createElement("a");
link.setAttribute("href", "/track/7");
rows[3].firstElementChild.append(link);
R.added = color(rows[3].querySelector("span"));
link.remove();
R.removed = color(rows[3].querySelector("span"));
// Anything else a script does: what is kept is what a fresh match gives.
let seed = 5;
const rnd = (n) => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed % n;
};
const TAGS = ["div", "li", "b", "u", "span", "i", "a"];
const CLS = ["a", "b", "c", "e", "x", "y", "h"];
const make = () => {
	const e = document.createElement(TAGS[rnd(TAGS.length)]);
	for (const c of CLS) if (rnd(4) === 0) e.classList.add(c);
	if (e.localName === "a") e.setAttribute("href", `/track/${rnd(2) ? 7 : 8}`);
	return e;
};
const els = () => [list, ...list.querySelectorAll("*")];
const show = (m) => (m ? JSON.stringify([...m]) : "null");
let checks = 0;
let differ = 0;
for (let step = 0; step < 200; step++) {
	const all = els();
	const pick = () => all[rnd(all.length)];
	const what = rnd(5);
	const e = pick();
	if (what === 0) e.append(make());
	else if (what === 1) {
		const p = pick();
		p.insertBefore(make(), p.childNodes[rnd(p.childNodes.length + 1)] ?? null);
	} else if (what === 2 && e !== list) e.remove();
	else if (what === 3 && e !== list) {
		const p = pick();
		if (!e.contains(p)) p.insertBefore(e, p.firstChild);
	} else if (what === 4) e.classList.toggle(CLS[rnd(CLS.length)]);
	if (els().length > 400) for (const k of [...list.children].slice(0, 3)) k.remove();
	for (const x of els()) {
		checks++;
		const kept = show(D.cascadeOf(x, false));
		const fresh = show(D.cascadeOf(x, true));
		if (kept !== fresh) {
			differ++;
			R.first ??= `step ${step}: <${x.localName} class="${x.className}"> kept ${kept}, fresh ${fresh}`;
		}
	}
	if (step % 10 === 9) await new Promise((r) => setTimeout(r, 0));
}
R.checks = checks;
R.differ = differ;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 60, |r| r["done"] == json!(true));
    assert_eq!(r["before"], json!("rgb(1, 2, 3)"), "{r}");
    // Thirty ratings, each matched once; not thirty rows of elements.
    assert!(r["ratingRuns"].as_u64().unwrap() <= 40, "{r}");
    assert_eq!(r["stillPlaying"], json!("rgb(1, 2, 3)"), "{r}");
    assert_eq!(r["after"], json!("rgb(7, 7, 7)"), "{r}");
    assert_eq!(r["added"], json!("rgb(1, 2, 3)"), "{r}");
    assert_eq!(r["removed"], json!("rgb(7, 7, 7)"), "{r}");
    assert!(r["checks"].as_u64().unwrap() > 10_000, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    finish(luna, dir);
}

/// Each `:has(...)` compound is asked once of an element, however many
/// selectors repeat it (scroll-to-playing writes the same
/// `[role="row"]:has(a[href*=...])` for a dozen subjects), and not of the
/// elements inside a subtree the same change brought in (their cascades
/// ask it when they are matched): every ancestor of every row a scroll
/// brought in was asked each repeat, and each repeat searched the row.
/// Whatever a script does, the cascades kept are the ones a fresh match
/// gives.
#[test]
fn rows_brought_in_ask_each_has_compound_once() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
let css = "";
for (const t of ["a", "span", "b", "i", "u", "div", "li", "em"]) {
	css += `[aria-rowindex]:has(a[href*="/track/7"]) ${t} { color: rgb(1, 2, 3); }\n`;
	css += `[role="row"]:has(a[href*="/track/7"]):not(.q) ${t} { background-color: rgb(4, 5, 6); }\n`;
}
css += `.t li:has(> .e) i { margin-left: 1px; }\n.t div:has(.y):first-child b { opacity: 0.5; }\n.t span { color: rgb(7, 7, 7); }\n`;
st.textContent = css;
document.head.append(st);
const list = document.createElement("div");
list.className = "t";
document.body.append(list);
const color = (e) => getComputedStyle(e).color;
// Rows put together after they are in the list, as a renderer may.
const tests = D.hasAnswerTests ?? 0;
const searches = D.hasSearches ?? 0;
for (let i = 0; i < 20; i++) {
	const row = document.createElement("div");
	row.setAttribute("aria-rowindex", String(i));
	row.setAttribute("role", "row");
	list.append(row);
	const cell = document.createElement("div");
	row.append(cell);
	const inner = document.createElement("div");
	cell.append(inner);
	const a = document.createElement("a");
	a.setAttribute("href", `/track/${i}`);
	inner.append(a);
	const s = document.createElement("span");
	inner.append(s);
	for (const e of [row, cell, inner, a, s]) color(e);
}
R.perRow = ((D.hasAnswerTests ?? 0) - tests) / 20;
R.searchesPerRow = ((D.hasSearches ?? 0) - searches) / 20;
R.playing = color(list.children[7].querySelector("span"));
R.other = color(list.children[3].querySelector("span"));
let seed = 7;
const rnd = (n) => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed % n;
};
const TAGS = ["div", "li", "b", "u", "span", "i", "a"];
const CLS = ["a", "b", "c", "e", "x", "y", "q"];
const make = () => {
	const e = document.createElement(TAGS[rnd(TAGS.length)]);
	for (const c of CLS) if (rnd(4) === 0) e.classList.add(c);
	if (e.localName === "a") e.setAttribute("href", `/track/${rnd(2) ? 7 : 8}`);
	return e;
};
const els = () => [list, ...list.querySelectorAll("*")];
const show = (m) => (m ? JSON.stringify([...m]) : "null");
let checks = 0;
let differ = 0;
for (let step = 0; step < 150; step++) {
	const all = els();
	const pick = () => all[rnd(all.length)];
	const what = rnd(6);
	const e = pick();
	if (what === 0) e.append(make());
	else if (what === 1) {
		// A subtree filled in after it is in place.
		const n = make();
		e.append(n);
		n.append(make());
		n.firstChild.append(make());
	} else if (what === 2 && e !== list) e.remove();
	else if (what === 3 && e !== list) {
		const p = pick();
		if (!e.contains(p)) p.insertBefore(e, p.firstChild);
	} else if (what === 4) e.classList.toggle(CLS[rnd(CLS.length)]);
	else if (what === 5 && e.localName === "a") e.setAttribute("href", `/track/${rnd(2) ? 7 : 9}`);
	if (els().length > 400) for (const k of [...list.children].slice(0, 3)) k.remove();
	for (const x of els()) {
		checks++;
		const kept = show(D.cascadeOf(x, false));
		const fresh = show(D.cascadeOf(x, true));
		if (kept !== fresh) {
			differ++;
			R.first ??= `step ${step}: <${x.localName} class="${x.className}"> kept ${kept}, fresh ${fresh}`;
		}
	}
	if (step % 10 === 9) await new Promise((r) => setTimeout(r, 0));
}
R.checks = checks;
R.differ = differ;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 60, |r| r["done"] == json!(true));
    // Four compounds (two of them repeated for eight subjects each), asked
    // of the row's five elements and the list's ancestors: not of each
    // repeat, nor again of the elements inside the row.
    assert!(r["perRow"].as_f64().unwrap() < 40.0, "{r}");
    // One search of a row for each argument, not one for each selector
    // that repeats it.
    assert!(r["searchesPerRow"].as_f64().unwrap() < 6.0, "{r}");
    assert_eq!(r["playing"], json!("rgb(1, 2, 3)"), "{r}");
    assert_eq!(r["other"], json!("rgb(7, 7, 7)"), "{r}");
    assert!(r["checks"].as_u64().unwrap() > 10_000, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    finish(luna, dir);
}

/// The rules an element's own tag, id, classes and attributes rule out
/// are worked out once for each kind of element, not tried on each: every
/// element of a row a scroll brought in tried the twenty-odd
/// `[class*=...]` rules a theme or plugin writes, each failing on the
/// element itself. Whatever the elements and their attributes, an element
/// gets the rules `matches` says it matches.
#[test]
fn rules_an_elements_own_attributes_rule_out_are_not_tried_on_each() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
let seed = 3;
const rnd = (n) => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed % n;
};
const SELS = [];
for (let i = 0; i < 30; i++) SELS.push(`[class*="_zz${i}_"]`);
SELS.push(
	'[class^="_row"]',
	'[class$="cell"]',
	'[class~="b"]',
	'[class|="lead"]',
	'[class="a b"]',
	'[class*="X" i]',
	'div[data-k]',
	'[data-k="1"]',
	'[data-k^="2"] span',
	'span[data-v*="q"]',
	'[DATA-K="3"]',
	'[style*="red"]',
	'[style]',
	'a[href$=".mp3"]',
	':is(span, b)[data-v]',
	'b:not([data-v="q"])',
	'#one',
	'#one.a',
	'li.a.b',
	'._row_1234567',
	'[role="row"] [data-k]:first-child',
	':where(.a)[data-v="xq"]',
);
const st = document.createElement("style");
st.textContent = SELS.map((s, i) => `${s} { --r${i}: 1; }`).join("\n");
document.head.append(st);
const TAGS = ["div", "span", "b", "li", "a"];
const CLASSES = ["a", "b", "a b", "_row_1234567 cell", "_rowx", "lead-in", "_zz3_ x", "Xy", "", "_zz17_"];
const make = () => {
	const e = document.createElement(TAGS[rnd(TAGS.length)]);
	const c = CLASSES[rnd(CLASSES.length)];
	if (c) e.setAttribute("class", c);
	if (rnd(3) === 0) e.setAttribute("data-k", String(rnd(4)));
	if (rnd(3) === 0) e.setAttribute("data-v", ["q", "xq", "z"][rnd(3)]);
	if (rnd(5) === 0) e.setAttribute("id", "one");
	if (rnd(6) === 0) e.setAttribute("role", "row");
	if (rnd(6) === 0) e.style.color = ["red", "blue"][rnd(2)];
	if (e.localName === "a" && rnd(2)) e.setAttribute("href", rnd(2) ? "/x.mp3" : "/y");
	return e;
};
const box = document.createElement("div");
document.body.append(box);
for (let i = 0; i < 300; i++) {
	const all = [box, ...box.querySelectorAll("*")];
	all[rnd(all.length)].append(make());
}
// Many elements of one kind: each tries what its kind leaves.
const same = [];
for (let i = 0; i < 50; i++) {
	const e = document.createElement("div");
	e.setAttribute("class", "plain k");
	box.append(e);
	same.push(e);
}
const tried = D.candidatesTried ?? 0;
for (const e of same) D.cascadeOf(e, true);
R.perElement = ((D.candidatesTried ?? 0) - tried) / same.length;
let checks = 0;
let differ = 0;
for (let step = 0; step < 10; step++) {
	for (const e of box.querySelectorAll("*")) {
		const got = D.cascadeOf(e, true);
		for (let i = 0; i < SELS.length; i++) {
			checks++;
			if ((got?.has(`--r${i}`) ?? false) !== e.matches(SELS[i])) {
				differ++;
				R.first ??= `${SELS[i]} on ${e.outerHTML.slice(0, 120)}: cascade ${got?.has(`--r${i}`)}`;
			}
		}
	}
	const all = [...box.querySelectorAll("*")];
	for (let k = 0; k < 30; k++) {
		const e = all[rnd(all.length)];
		const what = rnd(4);
		if (what === 0) e.setAttribute("class", CLASSES[rnd(CLASSES.length)]);
		else if (what === 1) e.setAttribute("data-k", String(rnd(4)));
		else if (what === 2) e.style.color = ["red", "blue"][rnd(2)];
		else e.append(make());
	}
}
R.checks = checks;
R.differ = differ;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 30, |r| r["done"] == json!(true));
    assert!(r["perElement"].as_f64().unwrap() < 5.0, "{r}");
    assert!(r["checks"].as_u64().unwrap() > 100_000, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    finish(luna, dir);
}

/// An attribute whose value a rule tests makes one kind of element of all
/// the values that pass the same tests, and an id no rule names one of all
/// those ids: a row's link, its `data-track-id` and TIDAL's
/// `titleCell-123`, each its own in every row, made a new kind of every
/// element that holds them (each worked out again from all the rules)
/// under the plugins' `a[href*="/album/"]` and `[data-track-id="..."]`.
/// Whatever the values, an element gets the rules `matches` says it
/// matches.
#[test]
fn values_that_pass_the_same_attribute_tests_make_one_kind_of_element() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const SELS = [
	'a[href*="/album/"]',
	'a[href$="/7"]',
	'[href^="/ALBUM" i]',
	'[data-track-id="7"]',
	'div[data-track-id]',
	'[data-track-id|="1"]',
	'[aria-label~="Play"]',
	'[aria-label="Play 3"] span',
	'[data-k]',
	'#cell-7',
	'[id^="cell-1"] b',
	'div[id]',
	':is(#cell-9)',
	':is([data-only])',
	':where([data-track-id="9"])',
];
const st = document.createElement("style");
st.textContent = SELS.map((s, i) => `${s} { --r${i}: 1; }`).join("\n");
document.head.append(st);
const box = document.createElement("div");
document.body.append(box);
for (let i = 0; i < 200; i++) {
	const row = document.createElement("div");
	row.setAttribute("data-track-id", i % 50 === 0 ? `1-${i}` : String(i));
	row.setAttribute("aria-label", `Play ${i}`);
	if (i % 2) row.setAttribute("data-only", "");
	row.innerHTML = `<a href="${i % 3 ? "/album/" : "/artist/"}${i}">t</a><span data-k="${i}">x</span><div id="cell-${i}"><b>c</b></div>`;
	box.append(row);
}
const made = D.kindsMade ?? 0;
for (const e of box.querySelectorAll("*")) D.cascadeOf(e, true);
R.kinds = (D.kindsMade ?? 0) - made;
let checks = 0;
let differ = 0;
for (const e of [box, ...box.querySelectorAll("*")]) {
	const got = D.cascadeOf(e, true);
	for (let i = 0; i < SELS.length; i++) {
		checks++;
		if ((got?.has(`--r${i}`) ?? false) !== e.matches(SELS[i])) {
			differ++;
			R.first ??= `${SELS[i]} on ${e.outerHTML.slice(0, 120)}: cascade ${got?.has(`--r${i}`)}`;
		}
	}
}
R.checks = checks;
R.differ = differ;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 30, |r| r["done"] == json!(true));
    assert_eq!(r["differ"], json!(0), "{r}");
    assert!(r["checks"].as_u64().unwrap() > 5_000, "{r}");
    // A handful of kinds (rows, links, spans, by the tests they pass), not
    // one for each of the 1,000 elements.
    assert!(r["kinds"].as_u64().unwrap() < 20, "{r}");
    finish(luna, dir);
}

/// A rule whose selector list has `:hover` in only some of its selectors
/// (radiant-lyrics' `.rl-wbw-word.rl-wbw-word-hover, .rl-wbw-word:hover`)
/// restyles only what the pointer's move reaches, as one without the
/// other selectors does: the selectors without `:hover` cannot match
/// differently when it moves. Each move restyled the whole page before.
#[test]
fn a_hover_rule_listed_with_selectors_without_hover_restyles_only_what_it_reaches() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
.t .w.w-hover, .t .w:hover { color: rgb(1, 2, 3); }
.t .w { color: rgb(9, 9, 9); }
`;
document.head.append(st);
const root = document.createElement("div");
root.className = "t";
document.body.append(root);
for (let i = 0; i < 40; i++) {
	const w = document.createElement("div");
	w.className = i === 5 ? "w w-hover" : "w";
	w.innerHTML = "<span>a</span><span>b</span>";
	root.append(w);
}
const ws = [...root.children];
D.loadedDirty();
D.flush();
const color = (e) => getComputedStyle(e).color;
const hover = (e) => D.event(e ? { target: { kind: "node", id: e._vid }, type: "hover", data: null } : { target: { kind: "host", id: "" }, type: "hover", data: null });
const all = () => [document.documentElement, ...document.documentElement.querySelectorAll("*")];
for (const e of all()) color(e);
const resets = D.cascadeResets ?? 0;
const runs = D.cascadeRuns ?? 0;
R.ids = ws.every((w) => w._vid !== undefined);
const seen = [];
for (const i of [1, 2, 9, 30, 2]) {
	hover(ws[i]);
	seen.push([color(ws[i]), color(ws[i === 2 ? 1 : 2]), color(ws[5])].join(" / "));
}
hover(null);
seen.push(color(ws[2]));
R.seen = seen;
R.resets = (D.cascadeResets ?? 0) - resets;
R.runs = (D.cascadeRuns ?? 0) - runs;
let differ = 0;
for (const e of all()) if (JSON.stringify([...(D.cascadeOf(e, false) ?? [])]) !== JSON.stringify([...(D.cascadeOf(e, true) ?? [])])) differ++;
R.differ = differ;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 30, |r| r["done"] == json!(true));
    assert_eq!(r["ids"], json!(true), "{r}");
    let (lit, unlit) = ("rgb(1, 2, 3)", "rgb(9, 9, 9)");
    assert_eq!(
        r["seen"],
        json!([
            format!("{lit} / {unlit} / {lit}"),
            format!("{lit} / {unlit} / {lit}"),
            format!("{lit} / {unlit} / {lit}"),
            format!("{lit} / {unlit} / {lit}"),
            format!("{lit} / {unlit} / {lit}"),
            unlit
        ]),
        "{r}"
    );
    assert_eq!(r["resets"], json!(0), "{r}");
    // Six moves, each the element left and the one reached with what is
    // in them; not the page's 120 elements each time.
    assert!(r["runs"].as_u64().unwrap() < 60, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    finish(luna, dir);
}

/// A hover change restyles what its rules can reach, as Chromium's
/// invalidation sets say: a row's `:hover` on the row alone restyles the
/// row (what inherits from it follows), `[role="row"]:hover .btn` the
/// row's buttons, and `.row:hover + .row .btn` the next row's, not every
/// cell of every row the pointer crossed (and of every row after it).
/// The cascades kept are the ones a fresh match gives.
#[test]
fn a_hover_change_restyles_only_the_elements_its_rules_can_reach() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
.t .row:hover { background-color: rgb(1, 2, 3); }
.t [role="row"]:hover .btn { opacity: 1; }
.t .row:hover + .row .btn { color: rgb(4, 5, 6); }
.t .btn { opacity: 0; }
`;
document.head.append(st);
const root = document.createElement("div");
root.className = "t";
document.body.append(root);
for (let i = 0; i < 30; i++) {
	const row = document.createElement("div");
	row.className = "row";
	row.setAttribute("role", "row");
	row.innerHTML = "<div><span>1</span><span>t</span></div><div><i></i><i></i><i></i></div><div><b class='btn'></b></div><div><span>a</span><span>b</span><span>c</span></div>";
	root.append(row);
}
const rows = [...root.children];
D.loadedDirty();
D.flush();
const hover = (e) => D.event(e ? { target: { kind: "node", id: e._vid }, type: "hover", data: null } : { target: { kind: "host", id: "" }, type: "hover", data: null });
const all = () => [document.documentElement, ...document.documentElement.querySelectorAll("*")];
const look = (e) => getComputedStyle(e).backgroundColor + " " + getComputedStyle(e.querySelector(".btn")).opacity + " " + getComputedStyle(e.querySelector(".btn")).color;
for (const e of all()) getComputedStyle(e);
R.ids = rows.every((w) => w._vid !== undefined) && rows.every((w) => w.querySelector("span")._vid !== undefined);
const resets = D.cascadeResets ?? 0;
const runs = D.cascadeRuns ?? 0;
const seen = [];
for (const i of [3, 4, 10, 4]) {
	hover(rows[i].querySelector("span"));
	for (const e of all()) getComputedStyle(e);
	seen.push([look(rows[i]), look(rows[i + 1]), look(rows[i + 2])].join(" | "));
}
hover(null);
for (const e of all()) getComputedStyle(e);
seen.push(look(rows[4]) + " | " + look(rows[5]));
R.seen = seen;
R.resets = (D.cascadeResets ?? 0) - resets;
R.runs = (D.cascadeRuns ?? 0) - runs;
let differ = 0;
for (const e of all()) if (JSON.stringify([...(D.cascadeOf(e, false) ?? [])]) !== JSON.stringify([...(D.cascadeOf(e, true) ?? [])])) differ++;
R.differ = differ;
R.total = all().length;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 30, |r| r["done"] == json!(true));
    assert_eq!(r["ids"], json!(true), "{r}");
    let lit = "rgb(1, 2, 3) 1 rgb(255, 255, 255)";
    let next = "rgba(0, 0, 0, 0) 0 rgb(4, 5, 6)";
    let off = "rgba(0, 0, 0, 0) 0 rgb(255, 255, 255)";
    let seen = format!("{lit} | {next} | {off}");
    assert_eq!(
        r["seen"],
        json!([seen, seen, seen, seen, format!("{off} | {off}")]),
        "{r}"
    );
    assert_eq!(r["resets"], json!(0), "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    // Five moves, each the row left and the row reached, their buttons
    // and the next rows' buttons (the span and the cells hold `:hover`
    // too, but no rule asks them): about 30, not the 17 elements of each
    // row crossed and of every row after it (over 1,000).
    assert!(r["runs"].as_u64().unwrap() <= 40, "{r}");
    finish(luna, dir);
}

/// The hovered element taken out of the document (the row under the
/// pointer scrolled away) takes `:hover` from the elements that held it as
/// it goes, as in a browser: whatever a script does to the nodes and
/// wherever the pointer goes, the cascades kept are the ones a fresh match
/// gives, and the next hover event no longer restyles the whole page (with
/// eighteen plugins a full restyle a second while a list scrolled).
#[test]
fn kept_cascades_equal_fresh_ones_when_the_hovered_element_goes() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = `
.t .h:hover { color: rgb(1, 0, 0); }
.t .h:hover .x { background-color: rgb(2, 0, 0); }
.t .a:hover + .b { border-top-color: rgb(3, 0, 0); }
.t .a:hover ~ .c span { opacity: 0.5; }
.t .a:hover + .b + .c { padding-right: 4px; }
.t .a:hover + * ~ .y { padding-bottom: 5px; }
.t li:hover > i { margin-left: 1px; }
.t :hover > .y { padding-top: 2px; }
.t .b:is(.c:hover *) { margin-top: 3px; }
.t .c:not(:hover) { padding-left: 1px; }
.t .x { color: rgb(9, 9, 9); }
`;
document.head.append(st);
let seed = 11;
const rnd = (n) => {
	seed = (seed * 1103515245 + 12345) % 2147483648;
	return seed % n;
};
const TAGS = ["div", "li", "b", "span", "i"];
const CLS = ["a", "b", "c", "x", "y", "h"];
const make = () => {
	const e = document.createElement(TAGS[rnd(TAGS.length)]);
	for (const c of CLS) if (rnd(3) === 0) e.classList.add(c);
	return e;
};
const root = document.createElement("div");
root.className = "t h";
document.body.append(root);
const els = () => [root, ...root.querySelectorAll("*")];
for (let i = 0; i < 60; i++) {
	const all = els();
	all[rnd(all.length)].append(make());
}
// Hover events name an element by the id an overlay build gives it.
const build = () => {
	D.loadedDirty();
	D.flush();
};
let hovered = null;
const hover = (e) => {
	if (e) build();
	if (e && e._vid === undefined) return false;
	D.event(e ? { target: { kind: "node", id: e._vid }, type: "hover", data: null } : { target: { kind: "host", id: "" }, type: "hover", data: null });
	hovered = e;
	return true;
};
const show = (m) => (m ? JSON.stringify([...m]) : "null");
let checks = 0;
let differ = 0;
let gone = 0;
const check = (step) => {
	for (const e of els()) {
		checks++;
		const kept = show(D.cascadeOf(e, false));
		const fresh = show(D.cascadeOf(e, true));
		if (kept !== fresh) {
			differ++;
			R.first ??= `step ${step}: <${e.localName} class="${e.className}"> kept ${kept}, fresh ${fresh}`;
		}
	}
};
check(-1);
const resets = D.cascadeResets ?? 0;
for (let step = 0; step < 300; step++) {
	const all = els();
	const pick = () => all[rnd(all.length)];
	const what = rnd(7);
	const e = pick();
	if (what === 0) e.append(make());
	else if (what === 1 && e !== root) e.remove();
	else if (what === 2) e.classList.toggle(CLS[rnd(CLS.length)]);
	else if (what === 3 || what === 4) hover(pick());
	else if (what === 5 && hovered?.isConnected && hovered !== root) {
		// The hovered element, or one above it, taken out; or its parent's
		// children replaced.
		let x = hovered;
		for (let k = rnd(3); k > 0 && x.parentElement !== root && x.parentElement; k--) x = x.parentElement;
		gone++;
		if (rnd(3) === 0) x.parentElement.innerHTML = "<i class='a h'></i><span class='x'></span>";
		else if (rnd(2) === 0) x.parentElement.textContent = "";
		else x.remove();
	} else if (what === 6) hover(null);
	if (els().length > 120) for (const k of [...root.children].slice(0, 2)) k.remove();
	if (els().length < 20) for (let i = 0; i < 20; i++) pick().append(make());
	check(step);
	if (step % 10 === 9) await new Promise((r) => setTimeout(r, 0));
}
R.checks = checks;
R.differ = differ;
R.gone = gone;
R.resets = (D.cascadeResets ?? 0) - resets;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 60, |r| r["done"] == json!(true));
    assert!(r["checks"].as_u64().unwrap() > 10_000, "{r}");
    assert!(r["gone"].as_u64().unwrap() >= 10, "{r}");
    assert_eq!(r["differ"], json!(0), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// A plugin's own element restyled (a sprite's frame, an element moved each
/// step of a loop) changes that element and what takes a value from it: the
/// overlay is not built again for that, and comes out as a whole build makes
/// it. Each step below is followed by a whole build: when that one changes
/// nothing the patched overlay was already what it makes, so every step is
/// one published overlay. A build still comes when the styles cannot say
/// what changed.
#[test]
fn a_plugin_elements_inline_style_goes_out_without_a_build() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '.boxed::before { content: "x"; color: red; } @keyframes k { from { opacity: 0; } to { opacity: 1; } }';
document.head.append(st);
const make = (id, tag = "div", parent = document.body) => {
	const e = document.createElement(tag);
	e.id = id;
	e.style.position = "fixed";
	parent.append(e);
	return e;
};
const sprite = make("sprite");
const bare = make("bare");
bare.style.opacity = "0.25";
const wrap = make("wrap");
wrap.style.setProperty("--tint", "rgb(1, 2, 3)");
const kid = make("kid", "span", wrap);
kid.style.backgroundColor = "var(--tint)";
const ani = make("ani");
const boxed = make("boxed");
boxed.className = "boxed";
const input = make("box", "input");
const steps = [
	[1, () => (sprite.style.left = "101px")],
	[1, () => (sprite.style.left = "102px")],
	[1, () => {
		sprite.style.top = "5px";
		bare.style.opacity = "0.5";
	}],
	[1, () => wrap.style.setProperty("--tint", "rgb(4, 5, 6)")],
	[1, () => bare.removeAttribute("style")],
	[1, () => sprite.setAttribute("style", "position: fixed; left: 103px")],
	[0, () => (ani.style.animation = "k 1s")],
	[0, () => (boxed.style.color = "rgb(7, 8, 9)")],
	[0, () => (input.style.width = "77px")],
	[0, () => {
		sprite.style.left = "104px";
		sprite.textContent = "words";
	}],
	[0, () => {
		const reads = document.createElement("style");
		reads.textContent = '[style] > i { color: red; }';
		document.head.append(reads);
	}],
	[0, () => (sprite.style.left = "105px")],
];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
setTimeout(async () => {
	const got = [];
	for (const [, step] of steps) {
		const p = D.stylePatches ?? 0;
		const b = D.overlayBuilds;
		step();
		await wait(150);
		got.push([(D.stylePatches ?? 0) - p, D.overlayBuilds - b]);
		D.markDirty();
		await wait(150);
	}
	R.want = steps.map(([patch]) => (patch ? [1, 0] : [0, 1]));
	R.got = got;
	R.done = true;
}, 600);
"##,
    );
    // Every overlay published from the first step on.
    let left = |o: &DomOverlay, v: &str| {
        overlay_vnode(o, &|n| {
            n.attrs.iter().any(|(k, x)| k == "id" && x == "sprite")
        })
        .is_some_and(|n| n.style.left.as_deref() == Some(v))
    };
    let mut seen: Vec<Arc<DomOverlay>> = Vec::new();
    let end = Instant::now() + Duration::from_secs(30);
    let r = loop {
        let o = luna.dom_overlay();
        if seen.last().is_none_or(|l| l.version != o.version) {
            seen.push(o);
        }
        let r = dom_results(&dir);
        if r["done"] == json!(true) {
            break r;
        }
        assert!(Instant::now() < end, "timed out: {r}");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(r["got"], r["want"], "{r}");
    let first = seen
        .iter()
        .position(|o| left(o, "101px"))
        .expect("first step");
    assert_eq!(seen.len() - first, 12, "a whole build changed an overlay");
    let o = seen.last().unwrap();
    assert!(left(o, "105px"), "{o:?}");
    let by = |id: &'static str| {
        overlay_vnode(o, &move |n| {
            n.attrs.iter().any(|(k, x)| k == "id" && x == id)
        })
        .unwrap()
    };
    assert_eq!(
        by("kid").style.background_color.as_deref(),
        Some("rgb(4, 5, 6)")
    );
    assert_eq!(by("bare").style, Default::default());
    finish(luna, dir);
}

/// `observe` and `observePromise` through the page changing and the plugin
/// being disabled and enabled: a row shown again after the page was left is
/// reported again, nothing is reported to a disabled plugin, and enabling it
/// leaves one observer (what is already there is reported once).
#[test]
fn observers_follow_navigation_and_disable_and_enable() {
    let (luna, _host, dir) = start(
        r##"
const bump = (k) => (R[k] = (R[k] ?? 0) + 1);
lib.observe(unloads, '[data-test="tracklist-row"][data-track-id]', () => bump("rows"));
lib.observePromise(unloads, '[data-test="footer-track-title"]', 30000).then((el) => el && bump("promised"));
R.ready = (R.ready ?? 0) + 1;
"##,
    );
    let away = || {
        luna.sync_dom(HostTree {
            path: "/search".into(),
            ..HostTree::default()
        });
        luna.sync(Duration::from_secs(10));
    };
    let settle = |luna: &Luna| {
        std::thread::sleep(Duration::from_millis(300));
        luna.sync(Duration::from_secs(10));
    };
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| {
        r["rows"] == json!(3) && r["promised"] == json!(1)
    });
    settle(&luna);
    assert_eq!(dom_results(&dir)["rows"], json!(3), "{r}");

    away();
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["rows"] == json!(6));
    settle(&luna);
    assert_eq!(dom_results(&dir)["rows"], json!(6));
    assert_eq!(dom_results(&dir)["promised"], json!(1));

    luna.set_enabled("DomTest", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "DomTest").unwrap().status == PluginStatus::Disabled
    });
    away();
    luna.sync_dom(tree());
    settle(&luna);
    assert_eq!(
        dom_results(&dir)["rows"],
        json!(6),
        "reported while disabled"
    );

    luna.set_enabled("DomTest", true);
    results_until(&luna, &dir, 20, |r| {
        r["rows"] == json!(9) && r["promised"] == json!(2)
    });
    settle(&luna);
    let r = dom_results(&dir);
    assert_eq!(
        (r["rows"].clone(), r["promised"].clone()),
        (json!(9), json!(2)),
        "{r}"
    );
    away();
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["rows"] == json!(12));
    settle(&luna);
    assert_eq!(dom_results(&dir)["rows"], json!(12), "doubled by enabling");
    finish(luna, dir);
}
