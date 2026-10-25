// The plugins' DOM. Tidalfast has no web page; instead the UI describes its
// widgets as TIDAL-like markup (a HostTree, see src/luna/dom.rs), which is
// reconciled into this document by key. Plugins query, observe, restyle and
// extend it like TIDAL's page; what they change goes back to the UI as a
// DomOverlay (built here, coalesced to at most ~30 per second, only when
// something changed). Input on host widgets comes back as real DOM events.
//
// css.js (loaded next) parses stylesheets and computes the cascade.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const D = (L.dom = {});
	const flagged = new Set();
	// Flags the plugin changing the document (once); returns it.
	const touch = () => {
		if (reconciling) return undefined;
		const plugin = L.pluginFromStack();
		if (plugin === undefined || flagged.has(plugin)) return plugin;
		flagged.add(plugin);
		try {
			L.callOp("plugin.uses_dom", { plugin });
		} catch {}
		return plugin;
	};
	const SVG_NS = "http://www.w3.org/2000/svg";
	const HTML_NS = "http://www.w3.org/1999/xhtml";
	let reconciling = false;

	// ---------------------------------------------------------------- dirty / flush
	// Anything that can change the overlay calls markDirty(); the overlay is
	// rebuilt on a runtime timer, at most every 33 ms, and only then.
	let dirty = false;
	let flushQueued = false;
	let lastFlush = 0;
	// Bumped by every change: cached cascades hold until it moves.
	let domGen = 0;
	D.domGen = () => domGen;
	// Bumped by anything a frame changes (see animation frames).
	let frameWork = 0;
	const markDirty = () => {
		// TIDALFAST_LUNA_STATS: who makes every cascade start over.
		if (L.stat) L.stat(`restyle by ${L.pluginFromStack?.() ?? "host"}`, Date.now());
		domGen++;
		frameWork++;
		dirty = true;
		if (flushQueued) return;
		flushQueued = true;
		const wait = Math.max(0, 33 - (Date.now() - lastFlush));
		L.internalTimeout(() => D.flush(), wait);
	};
	D.markDirty = markDirty;
	// An element's inline style changed (an animation loop moving a sprite):
	// the document's structure and the matched rules stay, so the cascade of
	// every other element is kept (css.js) and only this one is redone.
	let styleGen = 0;
	const restyled = new Set();
	// The overlay must be built again, but no selector can match differently
	// (a text node's data, an aria-/data- attribute no rule names).
	const markContentDirty = () => {
		// The UI's own text and aria-/data- changes (the playing second, a
		// title) leave the overlay as it was: it carries what plugins did.
		// Plugins reacting to them mark it themselves.
		if (reconciling) return;
		contentDirty();
	};
	const contentDirty = () => {
		frameWork++;
		dirty = true;
		if (flushQueued) return;
		flushQueued = true;
		L.internalTimeout(() => D.flush(), Math.max(0, 33 - (Date.now() - lastFlush)));
	};
	const markStyleDirty = (el) => {
		restyled.add(el);
		styleGen++;
		noteLayout(el);
		contentDirty();
	};
	// The host rewrote an app element's inline style (`before` held its
	// declarations) and every property that changed is a custom property no
	// rule reads: TIDAL's progress bar sets `--progress` every second. No
	// element's style can change, so only that element's cascade is redone
	// (`getComputedStyle` reads the variable) and the overlay is not built
	// again; the overlay build that every second asked for came out the same
	// and cost the runtime most of its time at rest. Not on the page's roots
	// (their variables are the overlay's), nor round a plugin's element,
	// whose markup may name the variable.
	const NO_DECLS = new Map();
	const quietStyle = (el, before) => {
		if (!reconciling || el._hostKey === undefined || el === html || el === body || el === wimp) return false;
		const read = L.css?.varsRead?.();
		if (!read || L.css.attrMatters("style")) return false;
		const after = el._style?._m ?? NO_DECLS;
		const unread = (k) => k.startsWith("--") && !read.has(k);
		for (const [k, d] of after) {
			const b = before.get(k);
			if (b && b.value === d.value && !!b.important === !!d.important) continue;
			if (!unread(k)) return false;
		}
		for (const k of before.keys()) if (!after.has(k) && !unread(k)) return false;
		return !someDescendant(el, (n) => n._hostKey === undefined);
	};
	const styleSet = (el, before) => {
		if (!quietStyle(el, before)) return markStyleDirty(el);
		restyled.add(el);
		styleGen++;
	};
	const declsOf = (text) => (text ? new Map(parseDecls(text).map((d) => [d.name, d])) : NO_DECLS);
	// A style, class or matched attribute of an app element (or of the
	// page round them) changed: the boxes may move, and a box read before
	// the overlay goes out publishes it at once (see forced layout).
	let layoutDirty = false;
	const noteLayout = (el) => {
		if (el && (el._hostKey !== undefined || el.localName === "html" || el.localName === "body" || el.id === "wimp")) layoutDirty = true;
	};
	// Plugin elements whose own box a script read: the overlay lists them
	// (`measure`) and the UI's tree says where it drew them (`node_rects`).
	const wantRects = new Set();
	D.wantRect = (el) => {
		if (wantRects.has(el)) return;
		wantRects.add(el);
		layoutDirty = true;
		contentDirty();
	};
	// ---------------------------------------------------------------- style passes
	// A browser works out the style of an element it was given at its next
	// frame (or when `getComputedStyle` asks), and a change after that is
	// what a transition runs from: a canvas inserted at `opacity: 0` and given
	// `.visible` a frame later fades in. The overlay goes out at most every
	// 33 ms, so both can land in the first overlay that shows the element;
	// the style it had at its first pass goes with it (`VNode.from`), only
	// for elements with a `transition`.
	const fresh = new Set();
	let freshArmed = false;
	let shownBefore = new Map();
	const noteFresh = (el) => {
		fresh.add(el);
		if (freshArmed) return;
		freshArmed = true;
		L.internalTimeout(() => D.styleTick(), 16);
	};
	D.styleTick = () => {
		freshArmed = false;
		if (!fresh.size) return;
		const roots = [...fresh];
		fresh.clear();
		if (!L.css?.styleNow) return;
		let budget = 2000;
		const walk = (el) => {
			if (budget-- <= 0) return;
			if (el._vid === undefined || !shownBefore.has(el._vid)) {
				const st = L.css.styleNow(el);
				if (st) el._before = st;
			}
			for (const c of el.childNodes) if (c instanceof Element) walk(c);
		};
		for (const root of roots) if (root.isConnected) walk(root);
	};
	// The properties a transition can move, as the element had them at its
	// first style pass: all of them, so a missing one reads as unset.
	const MOVING = ["opacity", "transform", "color", "background-color", "border", "border-color", "width", "height", "transition"];
	const beforeOf = (before, now) => {
		let differs = false;
		const out = {};
		for (const k of MOVING) {
			if (before[k] !== undefined) out[k] = before[k];
			if (k !== "transition" && (before[k] ?? null) !== (now?.[k] ?? null)) differs = true;
		}
		return differs ? out : null;
	};
	// Attributes whose changes cannot affect matching unless a selector names
	// them (pseudo-classes such as :checked or :disabled read other ones).
	// Any other attribute no selector names (an SVG `width`, `d` or `fill`
	// a plugin repaints, a `title`) only changes what is drawn too, so the
	// cascades of every element stay (a full recascade of a big page costs
	// ~100 ms, and star-ratings repaints its stars every 1.5 s).
	const MATCH_ATTRS = new Set(["class", "id", "checked", "disabled", "selected", "open", "type", "hidden", "lang", "dir"]);
	const attrChanged = (k, el) => {
		const name = String(k).toLowerCase();
		if (!MATCH_ATTRS.has(name) && !L.css?.attrMatters(name)) markContentDirty();
		else if (el && !SHEET_TAGS.has(el.localName) && L.css?.structural?.() === false) {
			noteLayout(el);
			markMatchDirty(el);
		} else {
			noteLayout(el);
			if (L.stat) L.stat(`restyle for [${name}] on ${el?.localName}`, Date.now());
			markDirty();
		}
	};
	// Elements whose attributes change the style sheets themselves
	// (`disabled`, `media`), or whose subtree is the whole page anyway.
	const SHEET_TAGS = new Set(["style", "link", "html", "body"]);
	// A class, id or other matched attribute changed on `el`, and no rule
	// looks at siblings or children (`+`, `~`, `:has`, `:nth-child`...): only
	// `el` and what is inside it can match differently, so css.js matches
	// that subtree again and keeps every other element's cascade, as it does
	// for inserted nodes. tidal-genres and star-ratings retag their badges
	// several times a second; each was a full recascade of the page.
	const markMatchDirty = (el) => {
		if (L.stat) L.stat(`rematch by ${L.pluginFromStack?.() ?? "host"}`, Date.now());
		inserted.add(el);
		structGen++;
		contentDirty();
	};
	// Nodes inserted or removed: selectors that only look at an element and
	// its ancestors (no sibling, `:has` or position pseudo-classes) match
	// every other element as before, so css.js keeps their cascades and
	// redoes only the inserted subtrees (tidal-genres re-renders its badges
	// on every mutation in the document, each time a full recascade of the
	// page before).
	let structGen = 0;
	// Bumped when a node is put in or taken out of a parent: what is round
	// an element (the listeners an event on it reaches) may have changed.
	let treeGen = 0;
	const inserted = new Set();
	const markStructDirty = (node) => {
		if (L.stat) L.stat(`restructure by ${L.pluginFromStack?.() ?? "host"}`, Date.now());
		if (node) inserted.add(node);
		structGen++;
		contentDirty();
	};
	D.structGen = () => structGen;
	D.takeInserted = () => {
		const out = [...inserted];
		inserted.clear();
		return out;
	};
	// The parents nodes were removed from (`:has` may stop matching them).
	const detachedFrom = new Set();
	D.takeDetached = () => {
		const out = [...detachedFrom];
		detachedFrom.clear();
		return out;
	};	D.styleGen = () => styleGen;
	D.takeRestyled = () => {
		const out = [...restyled];
		restyled.clear();
		return out;
	};
	let cssVersion = 0;
	const cssChanged = () => {
		if (L.stat) L.stat("restyle for a style sheet", Date.now());
		cssVersion++;
		markDirty();
	};
	D.cssChanged = cssChanged;
	D.cssVersion = () => cssVersion;

	// ---------------------------------------------------------------- events
	const EP = g.Event.prototype;
	EP.stopPropagation = function () {
		this._stopProp = true;
	};
	EP.stopImmediatePropagation = function () {
		this._stopProp = true;
		this._stop = true;
	};
	EP.composedPath = function () {
		return this._path ? this._path.map((n) => (n === winTarget ? g : n)) : [];
	};
	Object.defineProperty(EP, "cancelBubble", { get() { return !!this._stopProp; }, set(v) { if (v) this._stopProp = true; }, configurable: true });
	Object.defineProperty(EP, "returnValue", { get() { return !this.defaultPrevented; }, set(v) { if (v === false) this.preventDefault(); }, configurable: true });
	Object.defineProperty(EP, "srcElement", { get() { return this.target; }, configurable: true });
	EP.initEvent = function (type, bubbles, cancelable) {
		this.type = type;
		this.bubbles = !!bubbles;
		this.cancelable = !!cancelable;
	};
	for (const [k, v] of Object.entries({ NONE: 0, CAPTURING_PHASE: 1, AT_TARGET: 2, BUBBLING_PHASE: 3 })) g.Event[k] = v;
	const pick = (o, init, defaults) => {
		for (const k of Object.keys(defaults)) o[k] = init?.[k] ?? defaults[k];
	};
	const modKeys = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
	class UIEvent extends g.Event {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { view: g, detail: 0, composed: false });
		}
	}
	class MouseEvent extends UIEvent {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { clientX: 0, clientY: 0, screenX: 0, screenY: 0, pageX: i.clientX ?? 0, pageY: i.clientY ?? 0, offsetX: 0, offsetY: 0, movementX: 0, movementY: 0, button: 0, buttons: 0, relatedTarget: null, ...modKeys });
			this.x = this.clientX;
			this.y = this.clientY;
		}
		getModifierState(k) {
			return { Control: this.ctrlKey, Shift: this.shiftKey, Alt: this.altKey, Meta: this.metaKey }[k] ?? false;
		}
	}
	class WheelEvent extends MouseEvent {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { deltaX: 0, deltaY: 0, deltaZ: 0, deltaMode: 0 });
			this.wheelDelta = -this.deltaY;
			this.wheelDeltaY = -this.deltaY;
		}
	}
	WheelEvent.DOM_DELTA_PIXEL = 0;
	WheelEvent.DOM_DELTA_LINE = 1;
	WheelEvent.DOM_DELTA_PAGE = 2;
	const keyCodes = { Enter: 13, Escape: 27, " ": 32, Space: 32, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Backspace: 8, Tab: 9, Delete: 46, Home: 36, End: 35, PageUp: 33, PageDown: 34 };
	const keyCodeOf = (key) => {
		if (keyCodes[key]) return keyCodes[key];
		const f = /^F(\d{1,2})$/.exec(key ?? "");
		if (f) return 111 + Number(f[1]);
		if (typeof key === "string" && key.length === 1) return key.toUpperCase().charCodeAt(0);
		return 0;
	};
	class KeyboardEvent extends UIEvent {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { key: "", code: "", location: 0, repeat: false, isComposing: false, ...modKeys });
			this.keyCode = i.keyCode ?? keyCodeOf(this.key);
			this.which = i.which ?? this.keyCode;
			this.charCode = i.charCode ?? 0;
		}
		getModifierState(k) {
			return MouseEvent.prototype.getModifierState.call(this, k);
		}
	}
	class FocusEvent extends UIEvent {
		constructor(t, i = {}) {
			super(t, i);
			this.relatedTarget = i.relatedTarget ?? null;
		}
	}
	class InputEvent extends UIEvent {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { data: null, inputType: "", isComposing: false });
		}
	}
	Object.assign(g, { UIEvent, MouseEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent });
	g.PointerEvent = class PointerEvent extends MouseEvent {
		constructor(t, i = {}) {
			super(t, i);
			pick(this, i, { pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1, pressure: 0 });
		}
	};
	g.DragEvent = class DragEvent extends MouseEvent {
		constructor(t, i = {}) {
			super(t, i);
			this.dataTransfer = i.dataTransfer ?? null;
		}
	};
	for (const n of ["ClipboardEvent", "AnimationEvent", "TransitionEvent", "ProgressEvent", "ErrorEvent", "PopStateEvent", "StorageEvent", "MessageEvent"])
		g[n] ??= class extends g.Event {
			constructor(t, i = {}) {
				super(t, i);
				for (const [k, v] of Object.entries(i)) if (!(k in this)) this[k] = v;
			}
		};

	// Listener storage shared by nodes and the window.
	class Target extends g.EventTarget {
		addEventListener(type, cb, opts) {
			const domish = this instanceof Node || this === winTarget;
			if (domish) touch();
			if (!cb) return;
			const capture = typeof opts === "boolean" ? opts : !!opts?.capture;
			const ls = (this._ls ??= new Map());
			let arr = ls.get(type);
			if (!arr) ls.set(type, (arr = []));
			if (arr.some((l) => l.cb === cb && l.capture === capture)) return;
			const l = { cb, capture, once: typeof opts === "object" && !!opts?.once, plugin: L.pluginFromStack() };
			arr.push(l);
			const signal = typeof opts === "object" ? opts?.signal : null;
			if (signal) {
				if (signal.aborted) return this.removeEventListener(type, cb, opts);
				signal.addEventListener?.("abort", () => this.removeEventListener(type, cb, opts));
			}
			if (domish) listensChanged();
		}
		removeEventListener(type, cb, opts) {
			const capture = typeof opts === "boolean" ? opts : !!opts?.capture;
			const arr = this._ls?.get(type);
			if (!arr) return;
			const i = arr.findIndex((l) => l.cb === cb && l.capture === capture);
			if (i < 0) return;
			arr.splice(i, 1);
			if (!arr.length) this._ls.delete(type);
			if (this instanceof Node || this === winTarget) listensChanged();
		}
		dispatchEvent(ev) {
			return dispatch(this, ev);
		}
	}
	let listenVersion = 0;
	const listensChanged = () => {
		listenVersion++;
		contentDirty();
	};
	const winTarget = new Target();
	const parentForEvents = (n) => n.parentNode ?? (n === g.document ? winTarget : n instanceof ShadowRoot ? n.host : null);
	const invoke = (node, ev, phase) => {
		// TIDAL's root (React's handlers run there) heard it bubble.
		if (node === D.wimp && phase !== 1) ev._root = true;
		const arr = node._ls?.get(ev.type);
		const self = node === winTarget ? g : node;
		ev.currentTarget = self;
		ev.eventPhase = phase;
		if (arr)
			for (const l of [...arr]) {
				if (phase === 1 && !l.capture) continue;
				if (phase === 3 && l.capture) continue;
				if (l.once) node.removeEventListener(ev.type, l.cb, { capture: l.capture });
				try {
					typeof l.cb === "function" ? l.cb.call(self, ev) : l.cb.handleEvent(ev);
				} catch (err) {
					L.reportError(err);
				}
				if (ev._stop) return;
			}
		if (phase !== 1) {
			const h = node._on?.[ev.type];
			if (typeof h === "function")
				try {
					if (h.call(self, ev) === false) ev.preventDefault();
				} catch (err) {
					L.reportError(err);
				}
		}
	};
	const dispatch = (target, ev) => {
		const path = [];
		for (let n = target; n; n = parentForEvents(n)) path.push(n);
		ev.target = target === winTarget ? g : target;
		ev._path = path;
		ev._stopProp = false;
		ev._stop = false;
		for (let i = path.length - 1; i > 0 && !ev._stopProp; i--) invoke(path[i], ev, 1);
		if (!ev._stopProp) {
			const arr = target._ls?.get(ev.type);
			if (arr?.some((l) => l.capture)) invoke(target, ev, 1);
			if (!ev._stopProp) invoke(target, ev, 2);
		}
		if (ev.bubbles) for (let i = 1; i < path.length && !ev._stopProp; i++) invoke(path[i], ev, 3);
		ev.currentTarget = null;
		ev.eventPhase = 0;
		return !ev.defaultPrevented;
	};
	// A click with the browser's activation behaviour: a checkbox or radio
	// button changes before the listeners run (back again when one cancels)
	// and then reports `input` and `change`; a click elsewhere in a `<label>`
	// clicks its control (multipleplaylists' rows, a box and a title).
	const clickWith = (el, ev) => {
		// A disabled button, input, select or text area, or something inside
		// one, takes no clicks, from the pointer or from `.click()`
		// (OneClickLyrics' button while the song has no words).
		for (let n = el; n && n.nodeType === 1; n = n.parentNode) {
			if (/^(button|input|select|textarea)$/.test(n.localName)) {
				if (n.hasAttribute("disabled")) return false;
				break;
			}
		}
		const kind = el?.localName === "input" ? String(el.getAttribute("type") ?? "").toLowerCase() : "";
		const box = (kind === "checkbox" || kind === "radio") && !el.hasAttribute("disabled");
		let was = false;
		let others = [];
		if (box) {
			was = el.checked;
			if (kind === "radio") {
				const name = el.getAttribute("name");
				others = name ? [...doc.querySelectorAll('input[type="radio"]')].filter((r) => r !== el && r.getAttribute("name") === name && r.checked) : [];
				for (const r of others) r.checked = false;
			}
			el.checked = kind === "checkbox" ? !was : true;
		}
		const ok = dispatch(el, ev);
		if (box) {
			if (!ok) {
				el.checked = was;
				for (const r of others) r.checked = true;
			} else if (el.checked !== was) {
				dispatch(el, new InputEvent("input", { bubbles: true, composed: true }));
				dispatch(el, new g.Event("change", { bubbles: true }));
			}
			return ok;
		}
		if (!ok || typeof el?.closest !== "function") return ok;
		const label = el.closest("label");
		if (!label) return ok;
		const inner = el.closest("a, button, input, select, textarea");
		if (inner && label.contains(inner)) return ok;
		const id = label.getAttribute("for");
		const control = id ? doc.getElementById(id) : label.querySelector("input, select, textarea, button");
		if (control && control !== el && !control.hasAttribute("disabled")) control.click();
		return ok;
	};
	const onTypes = ["click", "dblclick", "contextmenu", "wheel", "mousedown", "mouseup", "mousemove", "mouseenter", "mouseleave", "mouseover", "mouseout", "pointerdown", "pointerup", "pointermove", "pointerenter", "pointerleave", "keydown", "keyup", "keypress", "input", "change", "focus", "blur", "scroll", "resize", "load", "error", "submit", "fullscreenchange", "animationend", "transitionend", "dragstart", "drop", "dragover", "loadstart", "loadedmetadata", "loadeddata", "canplay", "canplaythrough", "play", "playing", "pause", "ended", "seeking", "seeked", "timeupdate", "durationchange", "ratechange", "emptied"];
	const defineOn = (obj, holder) => {
		for (const t of onTypes)
			Object.defineProperty(obj, `on${t}`, {
				get() {
					return holder(this)._on?.[t] ?? null;
				},
				set(v) {
					const h = holder(this);
					(h._on ??= {})[t] = typeof v === "function" ? v : null;
					touch();
					listensChanged();
				},
				configurable: true,
			});
	};
	defineOn(Target.prototype, (x) => x);
	g.addEventListener = (t, cb, o) => winTarget.addEventListener(t, cb, o);
	g.removeEventListener = (t, cb, o) => winTarget.removeEventListener(t, cb, o);
	g.dispatchEvent = (e) => dispatch(winTarget, e);
	defineOn(g, () => winTarget);

	// ---------------------------------------------------------------- mutation observers
	const observers = new Set();
	let deliverQueued = false;
	const deliver = () => {
		deliverQueued = false;
		for (const o of [...observers]) {
			if (!o._queue.length) continue;
			const recs = o._queue;
			o._queue = [];
			try {
				o._cb.call(o, recs, o);
			} catch (err) {
				L.reportError(err);
			}
		}
	};
	const record = (type, target, extra) => {
		if (!observers.size) return;
		for (const o of observers)
			for (const r of o._regs) {
				const opt = r.options;
				if (r.target !== target && !(opt.subtree && r.target.contains(target))) continue;
				if (type === "childList" && !opt.childList) continue;
				if (type === "attributes") {
					if (!opt.attributes) continue;
					if (opt.attributeFilter && !opt.attributeFilter.includes(extra.attributeName)) continue;
				}
				if (type === "characterData" && !opt.characterData) continue;
				const rec = { type, target, addedNodes: [], removedNodes: [], previousSibling: null, nextSibling: null, attributeName: null, attributeNamespace: null, oldValue: null, ...extra };
				// Made by the host tree (TIDAL's own re-render), not a plugin.
				if (reconciling) Object.defineProperty(rec, "_host", { value: true });
				if (!((type === "attributes" && opt.attributeOldValue) || (type === "characterData" && opt.characterDataOldValue))) rec.oldValue = null;
				o._queue.push(rec);
				if (!deliverQueued) {
					deliverQueued = true;
					Promise.resolve().then(deliver);
				}
				break;
			}
	};
	class MutationObserver {
		constructor(cb) {
			this._cb = cb;
			this._regs = [];
			this._queue = [];
			this._plugin = L.pluginFromStack();
		}
		observe(target, options = {}) {
			touch();
			if (!target) throw new TypeError("MutationObserver.observe: target is not a Node");
			const o = { ...options };
			if (o.attributeFilter || o.attributeOldValue) o.attributes ??= true;
			if (o.characterDataOldValue) o.characterData ??= true;
			this._regs = this._regs.filter((r) => r.target !== target);
			this._regs.push({ target, options: o });
			observers.add(this);
		}
		disconnect() {
			this._regs = [];
			this._queue = [];
			observers.delete(this);
		}
		takeRecords() {
			const r = this._queue;
			this._queue = [];
			return r;
		}
	}
	g.MutationObserver = MutationObserver;
	g.WebKitMutationObserver = MutationObserver;
	// The watchdog stopped a script (in an observer callback or elsewhere):
	// the stopped plugin's observers hear nothing more (its unloads still
	// run, and a callback that ran away runs away again), and the records
	// the other observers were waiting for in the delivery it cut short go
	// to them now, not when something else changes the page.
	const stopped = (plugin) => {
		if (plugin !== null) {
			for (const o of [...observers]) if (o._plugin === plugin) o.disconnect();
			for (const o of [...sizeObservers]) if (o._plugin === plugin) o.disconnect();
		}
		if (!deliverQueued && [...observers].some((o) => o._queue.length)) {
			deliverQueued = true;
			Promise.resolve().then(deliver);
		}
	};

	// ---------------------------------------------------------------- nodes
	const isStyleish = (n) => n?.localName === "style";
	const textChangedIn = (parent) => {
		for (let p = parent; p; p = p.parentNode) if (isStyleish(p)) return cssChanged();
	};
	class Node extends Target {
		constructor() {
			super();
			this.childNodes = [];
			this.parentNode = null;
		}
		get parentElement() {
			return this.parentNode instanceof Element ? this.parentNode : null;
		}
		get firstChild() {
			return this.childNodes[0] ?? null;
		}
		get lastChild() {
			return this.childNodes.at(-1) ?? null;
		}
		get nextSibling() {
			const s = this.parentNode?.childNodes;
			return s ? (s[s.indexOf(this) + 1] ?? null) : null;
		}
		get previousSibling() {
			const s = this.parentNode?.childNodes;
			if (!s) return null;
			const i = s.indexOf(this);
			return i > 0 ? s[i - 1] : null;
		}
		get isConnected() {
			let n = this;
			while (n.parentNode) n = n.parentNode;
			return n === g.document;
		}
		get ownerDocument() {
			return this === g.document ? null : g.document;
		}
		get nodeValue() {
			return this instanceof CharacterData ? this.data : null;
		}
		set nodeValue(v) {
			if (this instanceof CharacterData) this.data = v;
		}
		hasChildNodes() {
			return this.childNodes.length > 0;
		}
		getRootNode() {
			let n = this;
			while (n.parentNode) n = n.parentNode;
			return n;
		}
		contains(n) {
			for (; n; n = n.parentNode) if (n === this) return true;
			return false;
		}
		// Insert `c` before `ref` (null: at the end). Fragments insert their children.
		insertBefore(c, ref) {
			const by = touch();
			if (c === null || c === undefined) throw new TypeError("insertBefore: node is null");
			// The plugin that first put a node of its own into the document:
			// what it leaves behind goes when it stops (dropPlugin).
			if (by !== undefined && c._hostKey === undefined && c._by === undefined && !(c instanceof DocumentFragment)) c._by = by;
			if (ref === undefined) ref = null;
			if (c instanceof DocumentFragment) {
				for (const k of [...c.childNodes]) this.insertBefore(k, ref);
				return c;
			}
			if (c === ref) return c;
			if (c.contains(this)) throw new g.DOMException("The new child element contains the parent.", "HierarchyRequestError");
			if (c.parentNode) c.parentNode._detach(c);
			const i = ref ? this.childNodes.indexOf(ref) : -1;
			const at = i < 0 ? this.childNodes.length : i;
			this.childNodes.splice(at, 0, c);
			c.parentNode = this;
			if (c._hostKey !== undefined && !reconciling) {
				c._pluginRemoved = false;
				if (this !== c._hostParent) c._pluginMoved = true;
			}
			record("childList", this, { addedNodes: [c], previousSibling: this.childNodes[at - 1] ?? null, nextSibling: this.childNodes[at + 1] ?? null });
			if (isStyleish(c) || (c instanceof Element && c.querySelector("style"))) cssChanged();
			else if (c instanceof CharacterData) textChangedIn(this);
			D.connected?.(c);
			if (c instanceof Element && c._hostKey === undefined && !reconciling && this.isConnected) noteFresh(c);
			treeGen++;
			markStructDirty(c);
			return c;
		}
		_detach(c) {
			const i = this.childNodes.indexOf(c);
			if (i < 0) return c;
			const prev = this.childNodes[i - 1] ?? null;
			const next = this.childNodes[i + 1] ?? null;
			this.childNodes.splice(i, 1);
			c.parentNode = null;
			if (c._hostKey !== undefined && !reconciling) c._pluginRemoved = true;
			if (c === this._hostText && !reconciling) this._textTouched = true;
			record("childList", this, { removedNodes: [c], previousSibling: prev, nextSibling: next });
			if (isStyleish(c) || (c instanceof Element && c.querySelector("style"))) cssChanged();
			else if (c instanceof CharacterData) textChangedIn(this);
			if (this instanceof Element) detachedFrom.add(this);
			treeGen++;
			markStructDirty(null);
			return c;
		}
		appendChild(c) {
			return this.insertBefore(c, null);
		}
		removeChild(c) {
			if (c?.parentNode !== this) throw new g.DOMException("The node to be removed is not a child of this node.", "NotFoundError");
			return this._detach(c);
		}
		replaceChild(n, o) {
			this.insertBefore(n, o);
			return this.removeChild(o);
		}
		append(...cs) {
			for (const c of cs) this.appendChild(typeof c === "string" || typeof c === "number" ? new Text(c) : c);
		}
		prepend(...cs) {
			const ref = this.firstChild;
			for (const c of cs) this.insertBefore(typeof c === "string" || typeof c === "number" ? new Text(c) : c, ref);
		}
		replaceChildren(...cs) {
			for (const c of [...this.childNodes]) this._detach(c);
			this.append(...cs);
		}
		remove() {
			this.parentNode?._detach(this);
		}
		before(...ns) {
			const p = this.parentNode;
			if (p) for (const n of ns) p.insertBefore(typeof n === "string" ? new Text(n) : n, this);
		}
		after(...ns) {
			const p = this.parentNode;
			if (!p) return;
			const ref = this.nextSibling;
			for (const n of ns) p.insertBefore(typeof n === "string" ? new Text(n) : n, ref);
		}
		replaceWith(...ns) {
			const p = this.parentNode;
			if (!p) return;
			const ref = this.nextSibling;
			this.remove();
			for (const n of ns) p.insertBefore(typeof n === "string" ? new Text(n) : n, ref);
		}
		get textContent() {
			let s = "";
			for (const c of this.childNodes) if (!(c instanceof Comment)) s += c.textContent;
			return s;
		}
		set textContent(v) {
			touch();
			// The same text again (a style tag or label a plugin rewrites on
			// every mutation it observes) changes nothing: no mutation record,
			// no restyle, so observers reacting to each other settle.
			const same = v === null || v === undefined ? "" : String(v);
			const only = this.childNodes;
			if ((only.length === 0 && same === "") || (only.length === 1 && only[0] instanceof Text && only[0]._data === same)) return;
			const removed = [...this.childNodes];
			for (const c of removed) {
				c.parentNode = null;
				if (c._hostKey !== undefined && !reconciling) c._pluginRemoved = true;
			}
			this.childNodes = [];
			const s = v === null || v === undefined ? "" : String(v);
			if (s !== "") {
				const t = new Text(s);
				this.childNodes.push(t);
				t.parentNode = this;
			}
			if (this._hostKey !== undefined && !reconciling) this._textTouched = true;
			if (removed.length || s) record("childList", this, { removedNodes: removed, addedNodes: [...this.childNodes] });
			textChangedIn(this);
			// Text replacing text (a label a plugin rewrites every second)
			// cannot change what selectors match unless `:empty` flips: the
			// overlay is rebuilt, but every element keeps its cascade.
			const wasText = removed.every((c) => c instanceof Text || c instanceof Comment);
			const hadText = removed.some((c) => c instanceof Text && c._data !== "");
			if (wasText && hadText === (s !== "")) contentDirty();
			else {
				if (removed.some((c) => c instanceof Element)) detachedFrom.add(this);
				markStructDirty(this.firstChild);
			}
		}
		cloneNode(deep) {
			const c = this._cloneSelf();
			if (deep) for (const k of this.childNodes) c.appendChild(k.cloneNode(true));
			return c;
		}
		isEqualNode(o) {
			return !!o && o.nodeType === this.nodeType && (this.outerHTML ?? this.textContent) === (o.outerHTML ?? o.textContent);
		}
		isSameNode(o) {
			return o === this;
		}
		compareDocumentPosition(o) {
			if (o === this) return 0;
			if (this.contains(o)) return 20;
			if (o.contains(this)) return 10;
			const order = [];
			const walk = (n) => {
				order.push(n);
				for (const c of n.childNodes) walk(c);
			};
			walk(this.getRootNode());
			const a = order.indexOf(this);
			const b = order.indexOf(o);
			if (b < 0) return 1 | 32;
			return b > a ? 4 : 2;
		}
		normalize() {}
		lookupNamespaceURI() {
			return null;
		}
	}
	Object.assign(Node, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11, DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16 });
	for (const [k, v] of Object.entries({ ELEMENT_NODE: 1, TEXT_NODE: 3, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_FRAGMENT_NODE: 11 })) Node.prototype[k] = v;

	class CharacterData extends Node {
		constructor(t) {
			super();
			this._data = t === undefined || t === null ? "" : String(t);
		}
		get data() {
			return this._data;
		}
		set data(v) {
			const old = this._data;
			v = v === null || v === undefined ? "" : String(v);
			if (v === old) return;
			this._data = v;
			if (!reconciling && this.parentNode?._hostText === this) this.parentNode._textTouched = true;
			record("characterData", this, { oldValue: old });
			textChangedIn(this.parentNode);
			markContentDirty();
		}
		get length() {
			return this._data.length;
		}
		get textContent() {
			return this._data;
		}
		set textContent(v) {
			this.data = v;
		}
		get nodeName() {
			return this.nodeType === 8 ? "#comment" : "#text";
		}
		get wholeText() {
			return this._data;
		}
		appendData(s) {
			this.data = this._data + s;
		}
		substringData(o, n) {
			return this._data.substr(o, n);
		}
		splitText(o) {
			const rest = new Text(this._data.slice(o));
			this.data = this._data.slice(0, o);
			this.after(rest);
			return rest;
		}
	}
	class Text extends CharacterData {
		get nodeType() {
			return 3;
		}
		_cloneSelf() {
			return new Text(this._data);
		}
	}
	class Comment extends CharacterData {
		get nodeType() {
			return 8;
		}
		_cloneSelf() {
			return new Comment(this._data);
		}
	}

	// ---------------------------------------------------------------- style declarations
	const kebab = (k) => {
		if (k.startsWith("--")) return k;
		if (k === "cssFloat") return "float";
		const s = k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
		return /^(webkit|moz|ms)-/.test(s) ? `-${s}` : s;
	};
	const camel = (k) => k.replace(/^-(webkit|moz|ms)-/, "$1-").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
	// Split `a; b; c` at top level (outside parens and strings).
	const splitDecls = (text) => {
		const out = [];
		let depth = 0,
			q = null,
			cur = "";
		for (let i = 0; i < text.length; i++) {
			const ch = text[i];
			if (q) {
				if (ch === "\\") {
					cur += ch + (text[++i] ?? "");
					continue;
				}
				if (ch === q) q = null;
			} else if (ch === '"' || ch === "'") q = ch;
			else if (ch === "(") depth++;
			else if (ch === ")") depth = Math.max(0, depth - 1);
			else if (ch === ";" && depth === 0) {
				out.push(cur);
				cur = "";
				continue;
			}
			cur += ch;
		}
		out.push(cur);
		return out;
	};
	const parseDecls = (text) => {
		const out = [];
		for (const part of splitDecls(String(text ?? ""))) {
			const i = part.indexOf(":");
			if (i < 0) continue;
			const name = part.slice(0, i).trim();
			let value = part.slice(i + 1).trim();
			if (!name || !value) continue;
			let important = false;
			const m = /\s*!\s*important\s*$/i.exec(value);
			if (m) {
				important = true;
				value = value.slice(0, m.index).trim();
			}
			out.push({ name: name.startsWith("--") ? name : name.toLowerCase(), value, important });
		}
		return out;
	};
	D.parseDecls = parseDecls;
	D.splitDecls = splitDecls;
	// A CSSStyleDeclaration: ordered property -> { value, important }. `owner`
	// is told about changes (an element's inline style, or a CSS rule).
	// `shared`: a map handed out to many (a computed style's), copied before
	// this declaration changes it.
	class CSSStyleDeclaration {
		constructor(owner, shared) {
			Object.defineProperty(this, "_m", { value: shared ?? new Map(), writable: true });
			Object.defineProperty(this, "_owner", { value: owner ?? null, writable: true });
			if (shared) Object.defineProperty(this, "_shared", { value: true, writable: true });
		}
		_own() {
			if (this._shared) {
				this._m = new Map(this._m);
				this._shared = false;
			}
		}
		_changed() {
			this._owner?._styleChanged?.();
		}
		getPropertyValue(k) {
			return this._m.get(String(k).startsWith("--") ? String(k) : String(k).toLowerCase())?.value ?? "";
		}
		getPropertyPriority(k) {
			return this._m.get(String(k))?.important ? "important" : "";
		}
		setProperty(k, v, prio) {
			k = String(k);
			if (!k.startsWith("--")) k = k.toLowerCase();
			if (v === null || v === undefined || String(v).trim() === "") return this.removeProperty(k);
			let value = String(v).trim();
			let important = String(prio ?? "").toLowerCase() === "important";
			const m = /\s*!\s*important\s*$/i.exec(value);
			if (m) {
				important = true;
				value = value.slice(0, m.index).trim();
			}
			const cur = this._m.get(k);
			if (cur && cur.value === value && cur.important === important) return;
			this._own();
			this._m.set(k, { value, important });
			this._changed();
		}
		removeProperty(k) {
			k = String(k);
			const cur = this._m.get(k);
			if (!cur) return "";
			this._own();
			this._m.delete(k);
			this._changed();
			return cur.value;
		}
		get cssText() {
			return [...this._m].map(([k, d]) => `${k}: ${d.value}${d.important ? " !important" : ""};`).join(" ");
		}
		set cssText(v) {
			this._m = new Map();
			if (this._shared) this._shared = false;
			for (const d of parseDecls(v)) this._m.set(d.name, { value: d.value, important: d.important });
			this._changed();
		}
		_setSilently(text) {
			this._m = new Map();
			if (this._shared) this._shared = false;
			for (const d of parseDecls(text)) this._m.set(d.name, { value: d.value, important: d.important });
		}
		get length() {
			return this._m.size;
		}
		item(i) {
			return [...this._m.keys()][i] ?? "";
		}
		*[Symbol.iterator]() {
			yield* this._m.keys();
		}
		get parentRule() {
			return this._owner?._isRule ? this._owner : null;
		}
	}
	// One handler for every declaration (a computed style is one per call).
	const DECL_HANDLER = {
		get(t, k, r) {
			if (typeof k !== "string" || k in t) {
				const v = Reflect.get(t, k, t);
				return typeof v === "function" ? v.bind(t) : v;
			}
			if (/^\d+$/.test(k)) return t.item(Number(k));
			return t.getPropertyValue(kebabOf(k));
		},
		set(t, k, v) {
			if (typeof k !== "string") return true;
			if (k in t) return Reflect.set(t, k, v, t);
			t.setProperty(kebabOf(k), v === null || v === undefined ? "" : String(v));
			return true;
		},
		has(t, k) {
			return k in t || (typeof k === "string" && t._m.has(kebabOf(k)));
		},
		ownKeys(t) {
			return [...new Set([...Reflect.ownKeys(t), ...[...t._m.keys()].map(camel)])];
		},
		getOwnPropertyDescriptor(t, k) {
			if (typeof k === "string" && t._m.has(kebabOf(k))) return { value: t.getPropertyValue(kebabOf(k)), enumerable: true, configurable: true, writable: true };
			return Reflect.getOwnPropertyDescriptor(t, k);
		},
	};
	// Property names as scripts write them (`backgroundImage`), converted
	// once each.
	const kebabs = new Map();
	const kebabOf = (k) => {
		let s = kebabs.get(k);
		if (s === undefined) {
			if (kebabs.size > 4096) kebabs.clear();
			s = kebab(k);
			kebabs.set(k, s);
		}
		return s;
	};
	const declProxy = (decl) => new Proxy(decl, DECL_HANDLER);
	D.CSSStyleDeclaration = CSSStyleDeclaration;
	D.declProxy = declProxy;
	D.kebab = kebab;
	g.CSSStyleDeclaration = CSSStyleDeclaration;

	// ---------------------------------------------------------------- class list / dataset
	class DOMTokenList {
		constructor(el, attr) {
			this._el = el;
			this._attr = attr;
		}
		_get() {
			return (this._el.getAttribute(this._attr) ?? "").split(/\s+/).filter(Boolean);
		}
		_set(list) {
			this._el.setAttribute(this._attr, list.join(" "));
		}
		add(...cs) {
			touch();
			const l = this._get();
			let changed = false;
			for (const c of cs.map(String))
				if (!l.includes(c)) {
					l.push(c);
					changed = true;
				}
			if (changed || !this._el.hasAttribute(this._attr)) this._set(l);
		}
		remove(...cs) {
			const l = this._get();
			const n = l.filter((c) => !cs.map(String).includes(c));
			if (n.length !== l.length) this._set(n);
		}
		toggle(c, force) {
			c = String(c);
			const has = this._get().includes(c);
			const on = force === undefined ? !has : !!force;
			if (on && !has) this.add(c);
			if (!on && has) this.remove(c);
			return on;
		}
		contains(c) {
			return this._get().includes(String(c));
		}
		replace(a, b) {
			const l = this._get();
			const i = l.indexOf(a);
			if (i < 0) return false;
			l[i] = b;
			this._set([...new Set(l)]);
			return true;
		}
		item(i) {
			return this._get()[i] ?? null;
		}
		supports() {
			return true;
		}
		get length() {
			return this._get().length;
		}
		get value() {
			return this._el.getAttribute(this._attr) ?? "";
		}
		set value(v) {
			this._el.setAttribute(this._attr, v);
		}
		toString() {
			return this.value;
		}
		forEach(cb, self) {
			this._get().forEach((c, i) => cb.call(self, c, i, this));
		}
		entries() {
			return this._get().entries();
		}
		keys() {
			return this._get().keys();
		}
		values() {
			return this._get().values();
		}
		[Symbol.iterator]() {
			return this._get()[Symbol.iterator]();
		}
	}
	g.DOMTokenList = DOMTokenList;
	const dataKey = (k) => `data-${k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
	const datasetOf = (el) =>
		new Proxy(
			{},
			{
				get: (_, k) => (typeof k === "string" ? (el.getAttribute(dataKey(k)) ?? undefined) : undefined),
				set: (_, k, v) => (el.setAttribute(dataKey(String(k)), v), true),
				deleteProperty: (_, k) => (el.removeAttribute(dataKey(String(k))), true),
				has: (_, k) => typeof k === "string" && el.hasAttribute(dataKey(k)),
				ownKeys: () => el.getAttributeNames().filter((n) => n.startsWith("data-")).map((n) => n.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())),
				getOwnPropertyDescriptor: (_, k) => (typeof k === "string" && el.hasAttribute(dataKey(k)) ? { value: el.getAttribute(dataKey(k)), enumerable: true, configurable: true, writable: true } : undefined),
			},
		);

	// ---------------------------------------------------------------- elements
	const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
	const rectObj = (r) => {
		const [x, y, w, h] = r ?? [0, 0, 0, 0];
		return { x, y, left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, toJSON() { return { x, y, width: w, height: h, top: y, left: x, right: x + w, bottom: y + h }; } };
	};
	// The rect plugins see: an element's own (host elements), else the nearest
	// ancestor's that has one (the UI leaves small parts such as cells
	// without a rect; parts not on screen are not in the tree at all).
	// A `position: fixed|absolute` element a plugin appended to the page (a
	// context menu, a tooltip) has no box of the UI's; it measures itself,
	// then places itself (`min(clientX, innerWidth - width - 10)`). Its size
	// is what its style declares, as lengths in px, em, %, vw or vh, not
	// the viewport's, which would push it to the page's corner.
	// A size its style leaves to its content (a menu's rows) is the size the
	// UI laid it out at (`_rect`, see `floatsUnsized`); its place is always
	// its style's, so a script that moves it reads the new place at once.
	const floatingRect = (el) => {
		for (let n = el; n && n !== html && n !== body && n !== wimp; n = n.parentNode) {
			if (n.nodeType !== 1 || n._hostKey !== undefined) continue;
			// The declared style only: a computed width asks for this very box.
			const props = L.css?.cascaded(n).props;
			if (!props) continue;
			const pos = String(props.get("position") ?? "").trim();
			if (pos !== "fixed" && pos !== "absolute") continue;
			const len = (name, base) => {
				const m = /^(-?[\d.]+)(px|%|vw|vh|em|rem)?$/.exec(String(props.get(name) ?? "").trim());
				if (!m) return null;
				const v = parseFloat(m[1]);
				if (m[2] === "%") return (base * v) / 100;
				if (m[2] === "vw") return (g.innerWidth * v) / 100;
				if (m[2] === "vh") return (g.innerHeight * v) / 100;
				// TIDAL's root font is 14 px (`:root { --baseFontSize: 14 }`).
				return m[2] === "rem" ? v * 14 : m[2] === "em" ? v * 16 : v;
			};
			const laid = n._rect;
			const w = len("width", g.innerWidth) ?? laid?.[2] ?? len("min-width", g.innerWidth);
			const h = len("height", g.innerHeight) ?? laid?.[3] ?? len("min-height", g.innerHeight);
			if (w === null && h === null) continue;
			return [len("left", g.innerWidth) ?? 0, len("top", g.innerHeight) ?? 0, w ?? 0, h ?? 0];
		}
		return null;
	};
	// A `position: fixed|absolute` element a plugin appended to the page
	// whose style leaves its width or height to its content: only the UI
	// knows that size, so a read of its box asks where the UI drew it.
	const floatsUnsized = (el) => {
		const up = el.parentNode;
		if (el._hostKey !== undefined || (up !== html && up !== body && up !== wimp)) return false;
		const props = L.css?.cascaded(el).props;
		if (!props) return false;
		const pos = String(props.get("position") ?? "").trim();
		if (pos !== "fixed" && pos !== "absolute") return false;
		const sized = (name) => /^-?[\d.]+(px|%|vw|vh|em|rem)?$/.test(String(props.get(name) ?? "").trim());
		return !sized("width") || !sized("height");
	};
	// A `position: fixed` element is placed against the window wherever it
	// was inserted (radiant-lyrics' backdrop, `100vw` x `100vh` inside
	// `main`): it does not take the box of the app element round it.
	const fixedPlugin = (n) => n.nodeType === 1 && n._hostKey === undefined && String(L.css?.cascaded(n)?.props?.get("position") ?? "").trim() === "fixed";
	const rectOf = (el) => {
		// Measured once (`_rect`), it is still placed by its style.
		if (el._rect && el._hostKey === undefined && floatsUnsized(el)) {
			const r = floatingRect(el);
			if (r) return r;
		}
		for (let n = el; n; n = n.parentNode) {
			if (n._rect && n !== html && n !== body && n !== wimp) return n._rect;
			if (n === html || n === body || n === wimp) {
				return floatingRect(el) ?? n._rect ?? [0, 0, g.innerWidth, g.innerHeight];
			}
			if (fixedPlugin(n)) {
				const r = floatingRect(el);
				if (r) return r;
			}
		}
		return null;
	};
	D.rectOf = rectOf;
	class Element extends Node {
		constructor(tag = "div", ns = HTML_NS) {
			super();
			this.namespaceURI = ns;
			this.localName = ns === HTML_NS ? String(tag).toLowerCase() : String(tag);
			this._attrs = new Map();
			this._style = null;
		}
		get nodeType() {
			return 1;
		}
		get tagName() {
			return this.namespaceURI === HTML_NS ? this.localName.toUpperCase() : this.localName;
		}
		get nodeName() {
			return this.tagName;
		}
		get prefix() {
			return null;
		}
		_cloneSelf() {
			const c = createEl(this.localName, this.namespaceURI);
			for (const [k, v] of this._attrs) c._attrs.set(k, v);
			if (this._attrs.has("style")) c.style.cssText = this._attrs.get("style");
			if (this._attrs.has("src")) c._load?.();
			if (this._value !== undefined) c._value = this._value;
			return c;
		}
		_name(k) {
			k = String(k);
			return this.namespaceURI === HTML_NS ? k.toLowerCase() : k;
		}
		getAttribute(k) {
			k = this._name(k);
			if (k === "style" && this._style) return this._style.length ? this._style.cssText : (this._attrs.get("style") ?? null);
			return this._attrs.get(k) ?? null;
		}
		hasAttribute(k) {
			return this._attrs.has(this._name(k)) || (this._name(k) === "style" && !!this._style?.length);
		}
		getAttributeNames() {
			return [...this._attrs.keys()];
		}
		setAttribute(k, v) {
			touch();
			k = this._name(k);
			v = String(v);
			if (!reconciling && this._hostKey !== undefined && k !== "class" && k !== "style") (this._pluginAttrs ??= new Set()).add(k);
			this._setAttr(k, v);
		}
		_setAttr(k, v) {
			const old = this._attrs.get(k) ?? null;
			if (old === v && k !== "style") return;
			this._attrs.set(k, v);
			let before = NO_DECLS;
			if (k === "style") {
				before = this._style?._m ?? declsOf(old);
				this._styleSync = true;
				(this._style ??= new CSSStyleDeclaration(this))._setSilently(v);
				this._styleSync = false;
			}
			record("attributes", this, { attributeName: k, oldValue: old });
			// An element outside the document changes nothing drawn; it is
			// matched afresh when it is inserted.
			if (!this.isConnected) {
			} else if (k === "style") styleSet(this, before);
			else attrChanged(k, this);
			if (k === "href" || k === "src" || k === "rel") D.connected?.(this);
		}
		removeAttribute(k) {
			k = this._name(k);
			this._pluginAttrs?.delete(k);
			this._removeAttr(k);
		}
		_removeAttr(k) {
			if (!this._attrs.has(k)) return;
			const old = this._attrs.get(k);
			this._attrs.delete(k);
			const before = k === "style" ? (this._style?._m ?? declsOf(old)) : NO_DECLS;
			if (k === "style" && this._style) this._style._setSilently("");
			record("attributes", this, { attributeName: k, oldValue: old });
			// An element outside the document changes nothing drawn; it is
			// matched afresh when it is inserted.
			if (!this.isConnected) {
			} else if (k === "style") styleSet(this, before);
			else attrChanged(k, this);
		}
		toggleAttribute(k, f) {
			const on = f === undefined ? !this.hasAttribute(k) : !!f;
			on ? this.setAttribute(k, "") : this.removeAttribute(k);
			return on;
		}
		getAttributeNS(_, k) {
			return this.getAttribute(k);
		}
		setAttributeNS(_, k, v) {
			return this.setAttribute(String(k), v);
		}
		removeAttributeNS(_, k) {
			return this.removeAttribute(k);
		}
		hasAttributeNS(_, k) {
			return this.hasAttribute(k);
		}
		get attributes() {
			const list = [...this._attrs].map(([name, value]) => ({ name, value, localName: name, nodeName: name, specified: true }));
			return Object.assign(list, { getNamedItem: (n) => list.find((a) => a.name === n) ?? null, item: (i) => list[i] ?? null });
		}
		hasAttributes() {
			return this._attrs.size > 0;
		}
		get style() {
			if (!this._style) {
				this._style = new CSSStyleDeclaration(this);
				if (this._attrs.has("style")) this._style._setSilently(this._attrs.get("style"));
			}
			return (this._styleProxy ??= declProxy(this._style));
		}
		set style(v) {
			this.style.cssText = v;
		}
		_styleChanged() {
			if (this._styleSync) return;
			touch();
			const old = this._attrs.get("style") ?? null;
			const text = this._style.cssText;
			if (text) this._attrs.set("style", text);
			else this._attrs.delete("style");
			record("attributes", this, { attributeName: "style", oldValue: old });
			if (this.isConnected) markStyleDirty(this);
		}
		get id() {
			return this.getAttribute("id") ?? "";
		}
		set id(v) {
			this.setAttribute("id", v);
		}
		get className() {
			return this.getAttribute("class") ?? "";
		}
		set className(v) {
			this.setAttribute("class", v);
		}
		get classList() {
			return (this._cl ??= new DOMTokenList(this, "class"));
		}
		get dataset() {
			return (this._ds ??= datasetOf(this));
		}
		get children() {
			const c = this.childNodes.filter((n) => n instanceof Element);
			return Object.assign(c, { item: (i) => c[i] ?? null, namedItem: (n) => c.find((e) => e.id === n || e.getAttribute("name") === n) ?? null });
		}
		get childElementCount() {
			return this.children.length;
		}
		get firstElementChild() {
			return this.childNodes.find((n) => n instanceof Element) ?? null;
		}
		get lastElementChild() {
			return this.childNodes.findLast((n) => n instanceof Element) ?? null;
		}
		get nextElementSibling() {
			const s = this.parentNode?.childNodes ?? [];
			for (let i = s.indexOf(this) + 1; i < s.length && i > 0; i++) if (s[i] instanceof Element) return s[i];
			return null;
		}
		get previousElementSibling() {
			const s = this.parentNode?.childNodes ?? [];
			for (let i = s.indexOf(this) - 1; i >= 0; i--) if (s[i] instanceof Element) return s[i];
			return null;
		}
		get innerHTML() {
			return this.childNodes.map(serialize).join("");
		}
		set innerHTML(v) {
			touch();
			const nodes = parseHTML(String(v ?? ""), this.namespaceURI === SVG_NS || this.localName === "svg", this.localName);
			const removed = [...this.childNodes];
			for (const c of removed) {
				c.parentNode = null;
				if (c._hostKey !== undefined && !reconciling) c._pluginRemoved = true;
			}
			this.childNodes = [];
			for (const n of nodes) {
				this.childNodes.push(n);
				n.parentNode = this;
			}
			if (this._hostKey !== undefined && !reconciling) this._textTouched = true;
			record("childList", this, { removedNodes: removed, addedNodes: [...nodes] });
			if (isStyleish(this) || nodes.some((n) => isStyleish(n) || (n instanceof Element && n.querySelector("style")))) cssChanged();
			for (const n of nodes) D.connected?.(n);
			for (const n of nodes) inserted.add(n);
			if (removed.length) detachedFrom.add(this);
			markStructDirty(null);
		}
		get outerHTML() {
			return serialize(this);
		}
		set outerHTML(v) {
			const p = this.parentNode;
			if (!p) return;
			const nodes = parseHTML(String(v ?? ""), false, p.localName);
			const ref = this.nextSibling;
			this.remove();
			for (const n of nodes) p.insertBefore(n, ref);
		}
		get innerText() {
			return this.textContent;
		}
		set innerText(v) {
			this.textContent = v;
		}
		get outerText() {
			return this.textContent;
		}
		insertAdjacentElement(pos, el) {
			switch (String(pos).toLowerCase()) {
				case "beforebegin":
					if (!this.parentNode) return null;
					this.parentNode.insertBefore(el, this);
					return el;
				case "afterbegin":
					return this.insertBefore(el, this.firstChild);
				case "beforeend":
					return this.appendChild(el);
				case "afterend":
					if (!this.parentNode) return null;
					this.parentNode.insertBefore(el, this.nextSibling);
					return el;
			}
			throw new g.DOMException(`Invalid position ${pos}`, "SyntaxError");
		}
		insertAdjacentHTML(pos, html) {
			touch();
			const frag = new DocumentFragment();
			for (const n of parseHTML(String(html ?? ""), this.namespaceURI === SVG_NS, this.localName)) frag.appendChild(n);
			this.insertAdjacentElement(pos, frag);
		}
		insertAdjacentText(pos, text) {
			this.insertAdjacentElement(pos, new Text(text));
		}
		// ------------------------------------------------ geometry
		getBoundingClientRect() {
			touch();
			// A plugin element inside another one, or one floating over the
			// page whose size is its content's, has no box of its own yet:
			// the UI is asked where it draws it, and this read waits for that.
			const up = this.parentNode;
			const nested = up instanceof Element && up._hostKey === undefined && up !== html && up !== body && up !== wimp;
			if (this._hostKey === undefined && !this._rect && this.isConnected && !this._pluginRemoved && (nested || floatsUnsized(this))) D.wantRect(this);
			forceLayout();
			const r = rectOf(this);
			if (!r || this._pluginRemoved || !this.isConnected) return rectObj(null);
			if (this._rect) return rectObj(r);
			return rectObj(r);
		}
		getClientRects() {
			const r = this.getBoundingClientRect();
			return r.width || r.height ? [r] : [];
		}
		get offsetWidth() {
			return this.getBoundingClientRect().width;
		}
		get offsetHeight() {
			return this.getBoundingClientRect().height;
		}
		get clientWidth() {
			return this.getBoundingClientRect().width;
		}
		get clientHeight() {
			return this.getBoundingClientRect().height;
		}
		get offsetTop() {
			const p = this.offsetParent;
			return this.getBoundingClientRect().top - (p ? p.getBoundingClientRect().top : 0);
		}
		get offsetLeft() {
			const p = this.offsetParent;
			return this.getBoundingClientRect().left - (p ? p.getBoundingClientRect().left : 0);
		}
		get offsetParent() {
			return this.parentElement;
		}
		get clientTop() {
			return 0;
		}
		get clientLeft() {
			return 0;
		}
		get scrollWidth() {
			return this.clientWidth;
		}
		get scrollHeight() {
			if (this._scroll) return Math.max(this._scroll[1], this.clientHeight);
			return this.clientHeight;
		}
		get scrollTop() {
			return this._scrollTop ?? this._scroll?.[0] ?? 0;
		}
		set scrollTop(v) {
			this.scrollTo({ top: Number(v) || 0 });
		}
		get scrollLeft() {
			return 0;
		}
		set scrollLeft(_) {}
		scrollTo(a, b) {
			const top = typeof a === "object" && a !== null ? a.top : b;
			if (top === undefined || top === null || Number.isNaN(Number(top))) return;
			const max = Math.max(0, this.scrollHeight - this.clientHeight);
			const t = Math.max(0, Math.min(Number(top), max || Number(top)));
			this._scrollTop = t;
			D.requestScroll(this, t);
		}
		scroll(a, b) {
			this.scrollTo(a, b);
		}
		scrollBy(a, b) {
			const dy = typeof a === "object" && a !== null ? (a.top ?? 0) : (b ?? 0);
			this.scrollTo({ top: this.scrollTop + Number(dy) });
		}
		scrollIntoView(opts) {
			touch();
			D.scrollIntoView(this, opts);
		}
		scrollIntoViewIfNeeded(center) {
			this.scrollIntoView({ block: center === false ? "nearest" : "center" });
		}
		focus() {
			if (focused === this) return;
			const prev = focused;
			focused = this;
			if (prev) dispatch(prev, new FocusEvent("blur", { relatedTarget: this }));
			dispatch(this, new FocusEvent("focus", { relatedTarget: prev }));
		}
		blur() {
			if (focused !== this) return;
			focused = null;
			dispatch(this, new FocusEvent("blur"));
		}
		click() {
			const ok = clickWith(this, new MouseEvent("click", { bubbles: true, cancelable: true }));
			// A click a plugin makes on a native widget reaches the widget.
			if (this._hostKey !== undefined && (ok || !this.hasAttribute("disabled"))) D.hostClick(this._hostKey);
		}
		animate() {
			return { cancel() {}, finish() {}, play() {}, pause() {}, reverse() {}, finished: Promise.resolve(), onfinish: null, addEventListener() {}, removeEventListener() {} };
		}
		getAnimations() {
			return [];
		}
		attachShadow() {
			this._shadow ??= new ShadowRoot(this);
			return this._shadow;
		}
		get shadowRoot() {
			return this._shadow ?? null;
		}
		requestFullscreen() {
			return D.requestFullscreen(this);
		}
		webkitRequestFullscreen() {
			return D.requestFullscreen(this);
		}
		setPointerCapture() {}
		releasePointerCapture() {}
		hasPointerCapture() {
			return false;
		}
		select() {
			selection._ranges = [];
			selection._control = this;
		}
		setSelectionRange() {}
		setRangeText() {}
		checkVisibility() {
			return this.isConnected && !this._pluginRemoved;
		}
		get value() {
			if (this._value !== undefined) return this._value;
			if (this._hostValue !== undefined) return this._hostValue;
			if (this.localName === "textarea") return this.textContent;
			if (this.localName === "select") return this.querySelector("option[selected]")?.value ?? this.querySelector("option")?.value ?? "";
			return this._attrs.get("value") ?? (this.localName === "option" ? this.textContent : "");
		}
		set value(v) {
			this._value = v === null || v === undefined ? "" : String(v);
			markDirty();
		}
		get valueAsNumber() {
			return Number(this.value);
		}
		set valueAsNumber(v) {
			this.value = String(v);
		}
		get checked() {
			return this._checked ?? this._attrs.has("checked");
		}
		set checked(v) {
			this._checked = !!v;
			markDirty();
		}
		get disabled() {
			return this._attrs.has("disabled");
		}
		set disabled(v) {
			this.toggleAttribute("disabled", !!v);
		}
		get hidden() {
			return this._attrs.has("hidden");
		}
		set hidden(v) {
			this.toggleAttribute("hidden", !!v);
		}
		get tabIndex() {
			return Number(this._attrs.get("tabindex") ?? -1);
		}
		set tabIndex(v) {
			this.setAttribute("tabindex", v);
		}
		get isContentEditable() {
			return false;
		}
		get assignedSlot() {
			return null;
		}
	}
	// Reflected string attributes.
	for (const [prop, attr] of Object.entries({ title: "title", lang: "lang", dir: "dir", name: "name", type: "type", placeholder: "placeholder", alt: "alt", rel: "rel", target: "target", htmlFor: "for", role: "role", min: "min", max: "max", step: "step", draggable: "draggable", ariaLabel: "aria-label", ariaHidden: "aria-hidden", ariaPressed: "aria-pressed", slot: "slot", width: "width", height: "height" }))
		Object.defineProperty(Element.prototype, prop, {
			get() {
				return this.getAttribute(attr) ?? "";
			},
			set(v) {
				this.setAttribute(attr, v);
			},
			configurable: true,
		});
	for (const prop of ["src", "href"])
		Object.defineProperty(Element.prototype, prop, {
			get() {
				const v = this.getAttribute(prop);
				if (v === null) return "";
				if (/^[a-z][\w+.-]*:/i.test(v) || v.startsWith("#")) return v;
				return v.startsWith("/") ? `https://desktop.tidal.com${v}` : v;
			},
			set(v) {
				this.setAttribute(prop, v);
			},
			configurable: true,
		});
	class HTMLElement extends Element {}
	class SVGElement extends Element {
		constructor(tag = "svg") {
			super(tag, SVG_NS);
		}
		get ownerSVGElement() {
			return this.closest?.("svg") ?? null;
		}
		getBBox() {
			const r = this.getBoundingClientRect();
			return { x: 0, y: 0, width: r.width, height: r.height };
		}
	}
	class DocumentFragment extends Node {
		get nodeType() {
			return 11;
		}
		get nodeName() {
			return "#document-fragment";
		}
		_cloneSelf() {
			return new DocumentFragment();
		}
		get children() {
			return this.childNodes.filter((n) => n instanceof Element);
		}
		get firstElementChild() {
			return this.children[0] ?? null;
		}
		get innerHTML() {
			return this.childNodes.map(serialize).join("");
		}
		getElementById(id) {
			return this.querySelector(`#${CSS.escape(id)}`);
		}
	}
	class ShadowRoot extends DocumentFragment {
		constructor(host) {
			super();
			this.host = host;
			this.mode = "open";
			this.adoptedStyleSheets = [];
		}
		set innerHTML(v) {
			this.replaceChildren(...parseHTML(String(v ?? ""), false, "div"));
		}
		get innerHTML() {
			return this.childNodes.map(serialize).join("");
		}
	}

	// ---------------------------------------------------------------- element classes
	const tagClass = {};
	const elementClasses = {
		HTMLDivElement: ["div"], HTMLSpanElement: ["span"], HTMLButtonElement: ["button"], HTMLInputElement: ["input"], HTMLStyleElement: ["style"], HTMLImageElement: ["img"], HTMLAnchorElement: ["a"], HTMLVideoElement: ["video"], HTMLAudioElement: ["audio"], HTMLCanvasElement: ["canvas"], HTMLSelectElement: ["select"], HTMLTextAreaElement: ["textarea"], HTMLLIElement: ["li"], HTMLUListElement: ["ul"], HTMLOListElement: ["ol"], HTMLParagraphElement: ["p"], HTMLLinkElement: ["link"], HTMLScriptElement: ["script"], HTMLIFrameElement: ["iframe"], HTMLHeadingElement: ["h1", "h2", "h3", "h4", "h5", "h6"], HTMLTableElement: ["table"], HTMLTableRowElement: ["tr"], HTMLTableCellElement: ["td", "th"], HTMLLabelElement: ["label"], HTMLFormElement: ["form"], HTMLOptionElement: ["option"], HTMLPreElement: ["pre"], HTMLBRElement: ["br"], HTMLHRElement: ["hr"], HTMLSourceElement: ["source"], HTMLMetaElement: ["meta"], HTMLTemplateElement: ["template"], HTMLDialogElement: ["dialog"], HTMLProgressElement: ["progress"], HTMLSlotElement: ["slot"], HTMLBodyElement: ["body"], HTMLHeadElement: ["head"], HTMLHtmlElement: ["html"], HTMLUnknownElement: [], HTMLMediaElement: [],
	};
	for (const [n, tags] of Object.entries(elementClasses)) {
		g[n] = n === "HTMLVideoElement" || n === "HTMLAudioElement" ? class extends (g.HTMLMediaElement ?? HTMLElement) {} : class extends HTMLElement {};
		Object.defineProperty(g[n], "name", { value: n });
		for (const t of tags) tagClass[t] = n;
	}
	g.HTMLVideoElement = class HTMLVideoElement extends g.HTMLMediaElement {};
	g.HTMLAudioElement = class HTMLAudioElement extends g.HTMLMediaElement {};
	tagClass.video = "HTMLVideoElement";
	tagClass.audio = "HTMLAudioElement";
	g.SVGSVGElement = class SVGSVGElement extends SVGElement {};
	g.SVGPathElement = class SVGPathElement extends SVGElement {};
	g.SVGGraphicsElement = SVGElement;
	const createEl = (tag, ns) => {
		tag = String(tag);
		if (ns === SVG_NS || (ns === undefined && tag === "svg")) {
			const C = tag === "svg" ? g.SVGSVGElement : tag === "path" ? g.SVGPathElement : SVGElement;
			return new C(tag);
		}
		const C = g[tagClass[tag.toLowerCase()]] ?? HTMLElement;
		const el = new C(tag, HTML_NS);
		if (tag.toLowerCase() === "template") el.content = new DocumentFragment();
		return el;
	};
	D.createEl = createEl;
	Object.assign(g.HTMLMediaElement.prototype, { play: () => Promise.resolve(), pause() {}, load() {}, canPlayType: () => "", paused: true, currentTime: 0, duration: NaN, volume: 1, muted: false, readyState: 0, playbackRate: 1, ended: false });
	Object.assign(g.HTMLImageElement.prototype, { complete: true, naturalWidth: 0, naturalHeight: 0, decode: () => Promise.resolve(), loading: "eager" });
	// An image's and a source's `srcset` and `sizes` reflect their attributes,
	// as in a browser: a script that clears them (uwu-ifier, on every picture
	// it replaces) changes the attribute, which observers and the page see.
	for (const C of [g.HTMLImageElement, g.HTMLSourceElement])
		for (const prop of ["srcset", "sizes"])
			Object.defineProperty(C.prototype, prop, {
				get() {
					return this.getAttribute(prop) ?? "";
				},
				set(v) {
					this.setAttribute(prop, v);
				},
				configurable: true,
			});
	g.Audio = class Audio extends g.HTMLAudioElement {
		constructor(src) {
			super("audio");
			if (src) this.setAttribute("src", src);
		}
	};
	g.Image = class Image extends g.HTMLImageElement {
		constructor(w, h) {
			super("img");
			if (w !== undefined) this.setAttribute("width", w);
			if (h !== undefined) this.setAttribute("height", h);
		}
	};
	Object.defineProperty(g.HTMLStyleElement.prototype, "sheet", {
		get() {
			return L.css?.sheetOf(this) ?? null;
		},
		configurable: true,
	});
	Object.defineProperty(g.HTMLSelectElement.prototype, "options", { get() { return this.querySelectorAll("option"); }, configurable: true });
	Object.defineProperty(g.HTMLSelectElement.prototype, "selectedIndex", { get() { return Math.max(0, [...this.querySelectorAll("option")].findIndex((o) => o.value === this.value)); }, set(i) { const o = this.querySelectorAll("option")[i]; if (o) this.value = o.value; }, configurable: true });

	// ---------------------------------------------------------------- HTML parsing / serialising
	const ENT = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", copy: "©", reg: "®", hellip: "…", mdash: "—", ndash: "–", middot: "·", bull: "•", times: "×", rarr: "→", larr: "←", uarr: "↑", darr: "↓", laquo: "«", raquo: "»", deg: "°", trade: "™" };
	const decode = (s) =>
		s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) => {
			if (e[0] === "#") {
				const n = e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
				return Number.isFinite(n) ? String.fromCodePoint(n) : m;
			}
			return ENT[e.toLowerCase()] ?? m;
		});
	const escText = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/ /g, "&nbsp;");
	const escAttr = (s) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/ /g, "&nbsp;");
	const RAW = new Set(["style", "script", "textarea", "title"]);
	// A forgiving HTML(+SVG) parser: tags, attributes, text, comments, raw-text
	// elements, void elements; unclosed tags close at their parent's end.
	const parseHTML = (html, inSvg, context) => {
		const root = new DocumentFragment();
		const stack = [{ el: root, svg: !!inSvg }];
		const top = () => stack[stack.length - 1];
		let i = 0;
		const n = html.length;
		const addText = (t) => {
			if (!t) return;
			const parent = top().el;
			const last = parent.childNodes.at(-1);
			if (last instanceof Text) last._data += decode(t);
			else {
				const tn = new Text(decode(t));
				parent.childNodes.push(tn);
				tn.parentNode = parent;
			}
		};
		if (RAW.has(context)) {
			addText(html);
			return [...root.childNodes].map((c) => ((c.parentNode = null), c));
		}
		while (i < n) {
			const lt = html.indexOf("<", i);
			if (lt < 0) {
				addText(html.slice(i));
				break;
			}
			if (lt > i) addText(html.slice(i, lt));
			i = lt;
			if (html.startsWith("<!--", i)) {
				const end = html.indexOf("-->", i + 4);
				const body = html.slice(i + 4, end < 0 ? n : end);
				const c = new Comment(body);
				top().el.childNodes.push(c);
				c.parentNode = top().el;
				i = end < 0 ? n : end + 3;
				continue;
			}
			if (html[i + 1] === "!" || html[i + 1] === "?") {
				const end = html.indexOf(">", i);
				i = end < 0 ? n : end + 1;
				continue;
			}
			if (html[i + 1] === "/") {
				const end = html.indexOf(">", i);
				const name = html.slice(i + 2, end < 0 ? n : end).trim().toLowerCase();
				i = end < 0 ? n : end + 1;
				for (let k = stack.length - 1; k > 0; k--)
					if (stack[k].el.localName.toLowerCase() === name) {
						stack.length = k;
						break;
					}
				continue;
			}
			const m = /^<([A-Za-z][\w:-]*)/.exec(html.slice(i, i + 64));
			if (!m) {
				addText("<");
				i++;
				continue;
			}
			i += m[0].length;
			const rawName = m[1];
			const svg = top().svg || rawName.toLowerCase() === "svg";
			const el = svg ? createEl(rawName, SVG_NS) : createEl(rawName.toLowerCase());
			// attributes
			let selfClose = false;
			for (;;) {
				while (i < n && /\s/.test(html[i])) i++;
				if (i >= n) break;
				if (html[i] === ">") {
					i++;
					break;
				}
				if (html[i] === "/" && html[i + 1] === ">") {
					selfClose = true;
					i += 2;
					break;
				}
				const am = /^[^\s"'>\/=]+/.exec(html.slice(i, i + 256));
				if (!am) {
					i++;
					continue;
				}
				i += am[0].length;
				let val = "";
				while (i < n && /\s/.test(html[i])) i++;
				if (html[i] === "=") {
					i++;
					while (i < n && /\s/.test(html[i])) i++;
					const q = html[i];
					if (q === '"' || q === "'") {
						const end = html.indexOf(q, i + 1);
						val = html.slice(i + 1, end < 0 ? n : end);
						i = end < 0 ? n : end + 1;
					} else {
						const vm = /^[^\s>]*/.exec(html.slice(i));
						val = vm[0];
						i += val.length;
					}
				}
				const an = svg ? am[0] : am[0].toLowerCase();
				el._attrs.set(an, decode(val));
			}
			if (el._attrs.has("style")) el.style.cssText = el._attrs.get("style");
			if (el._attrs.has("src")) el._load?.();
			const parent = top().el;
			parent.childNodes.push(el);
			el.parentNode = parent;
			const lname = el.localName.toLowerCase();
			if (selfClose || (!svg && VOID.has(lname))) continue;
			if (!svg && RAW.has(lname)) {
				const re = new RegExp(`</${lname}\\s*>`, "i");
				const rest = html.slice(i);
				const em = re.exec(rest);
				const body = em ? rest.slice(0, em.index) : rest;
				if (body) {
					const tn = new Text(lname === "textarea" || lname === "title" ? decode(body) : body);
					el.childNodes.push(tn);
					tn.parentNode = el;
				}
				i += em ? em.index + em[0].length : rest.length;
				continue;
			}
			if (lname === "template") {
				stack.push({ el: el.content ?? el, svg });
				continue;
			}
			stack.push({ el, svg });
		}
		const out = [...root.childNodes];
		for (const c of out) c.parentNode = null;
		return out;
	};
	D.parseHTML = parseHTML;
	const serialize = (node) => {
		if (node instanceof Text) return node.parentNode && RAW.has(node.parentNode.localName) ? node._data : escText(node._data);
		if (node instanceof Comment) return `<!--${node._data}-->`;
		if (node instanceof DocumentFragment) return node.childNodes.map(serialize).join("");
		if (!(node instanceof Element)) return "";
		const tag = node.localName;
		let s = `<${tag}`;
		for (const k of node._attrs.keys()) s += ` ${k}="${escAttr(node.getAttribute(k) ?? "")}"`;
		if (node._style?.length && !node._attrs.has("style")) s += ` style="${escAttr(node._style.cssText)}"`;
		if (node.namespaceURI === HTML_NS && VOID.has(tag)) return `${s}>`;
		const inner = (tag === "template" && node.content ? node.content.childNodes : node.childNodes).map(serialize).join("");
		return `${s}>${inner}</${tag}>`;
	};
	D.serialize = serialize;

	// ---------------------------------------------------------------- selectors
	// A real selector parser: compound selectors with tag, #id, .class,
	// [attr op value i], pseudo-classes (:not/:is/:where/:has/:nth-*/...),
	// pseudo-elements, and the four combinators. Dynamic states (:hover,
	// :focus, ...) and pseudo-elements never match.
	const unesc = (s) => s.replace(/\\([0-9a-fA-F]{1,6}\s?|[\s\S])/g, (_, e) => (/^[0-9a-fA-F]/.test(e) ? String.fromCodePoint(parseInt(e, 16)) : e));
	class SelErr extends Error {}
	const parseSelectors = (src, relative) => {
		const s = String(src);
		let i = 0;
		const ws = () => {
			while (i < s.length && /\s/.test(s[i])) i++;
		};
		const ident = () => {
			const m = /^(?:--|-?(?:[_a-zA-Z -￿]|\\[\s\S]))(?:[\w\- -￿]|\\[0-9a-fA-F]{1,6}\s?|\\[\s\S])*/.exec(s.slice(i));
			if (!m) throw new SelErr(`bad identifier at ${i} in ${s}`);
			i += m[0].length;
			return unesc(m[0]);
		};
		const balanced = () => {
			// after "(": return text until matching ")"
			let depth = 1,
				q = null;
			const st = i;
			for (; i < s.length; i++) {
				const ch = s[i];
				if (q) {
					if (ch === "\\") i++;
					else if (ch === q) q = null;
				} else if (ch === '"' || ch === "'") q = ch;
				else if (ch === "(") depth++;
				else if (ch === ")" && --depth === 0) break;
			}
			const body = s.slice(st, i);
			i++;
			return body;
		};
		const parseNth = (txt) => {
			let t = txt.trim();
			let of = null;
			const om = /\s+of\s+/i.exec(t);
			if (om) {
				of = parseSelectors(t.slice(om.index + om[0].length));
				t = t.slice(0, om.index).trim();
			}
			t = t.toLowerCase().replace(/\s+/g, "");
			if (t === "odd") return { a: 2, b: 1, of };
			if (t === "even") return { a: 2, b: 0, of };
			const m = /^([+-]?\d*)?n([+-]\d+)?$/.exec(t);
			if (m) {
				const a = m[1] === undefined || m[1] === "" || m[1] === "+" ? 1 : m[1] === "-" ? -1 : Number(m[1]);
				return { a, b: Number(m[2] ?? 0), of };
			}
			if (/^[+-]?\d+$/.test(t)) return { a: 0, b: Number(t), of };
			throw new SelErr(`bad nth ${txt}`);
		};
		const compound = () => {
			const c = { tag: null, ids: [], classes: [], attrs: [], pseudos: [], pe: false, scope: false };
			let any = false;
			for (;;) {
				const ch = s[i];
				if (ch === "*") {
					i++;
					any = true;
					if (s[i] === "|") i++;
				} else if (ch === "#") {
					i++;
					c.ids.push(ident());
					any = true;
				} else if (ch === ".") {
					i++;
					c.classes.push(ident());
					any = true;
				} else if (ch === "[") {
					i++;
					ws();
					let name = ident();
					if (s[i] === "|" && s[i + 1] !== "=") {
						i++;
						name = ident();
					}
					ws();
					let op = null,
						val = "",
						flag = "";
					const om = /^([~|^$*]?=)/.exec(s.slice(i));
					if (om) {
						op = om[1];
						i += op.length;
						ws();
						const q = s[i];
						if (q === '"' || q === "'") {
							let j = i + 1,
								out = "";
							for (; j < s.length && s[j] !== q; j++) {
								if (s[j] === "\\") {
									out += s[j] + (s[j + 1] ?? "");
									j++;
								} else out += s[j];
							}
							val = unesc(out);
							i = j + 1;
						} else val = ident();
						ws();
						const fm = /^([iIsS])(?![\w-])/.exec(s.slice(i));
						if (fm) {
							flag = fm[1].toLowerCase();
							i++;
							ws();
						}
					}
					if (s[i] !== "]") throw new SelErr(`bad attribute selector in ${s}`);
					i++;
					c.attrs.push({ name: name.toLowerCase(), op, val, ci: flag === "i" });
					any = true;
				} else if (ch === ":") {
					i++;
					let pe = false;
					if (s[i] === ":") {
						i++;
						pe = true;
					}
					const name = ident().toLowerCase();
					let arg = null;
					if (s[i] === "(") {
						i++;
						arg = balanced();
					}
					if (pe || ["before", "after", "first-line", "first-letter", "selection", "placeholder", "marker", "backdrop"].includes(name) || name.startsWith("-webkit-") || name.startsWith("-moz-")) {
						if (name === "-webkit-any" && !pe) c.pseudos.push({ name: "is", list: parseSelectors(arg ?? "") });
						else {
							c.pe = true;
							// ::before / ::after: the stylesheet index keeps
							// these for the anchors' box layers.
							if (name === "before" || name === "after" || name === "selection") c.peName = name;
							// A range input's track and thumb, as Chromium and
							// Firefox name them (HorizontalVolumeSlider's bar).
							else if (name === "-webkit-slider-runnable-track" || name === "-moz-range-track") c.peName = "track";
							else if (name === "-webkit-slider-thumb" || name === "-moz-range-thumb") c.peName = "thumb";
						}
					} else if (name === "not" || name === "is" || name === "where" || name === "matches" || name === "any") c.pseudos.push({ name: name === "not" ? "not" : name === "where" ? "where" : "is", list: parseSelectors(arg ?? "") });
					else if (name === "has") c.pseudos.push({ name, list: parseSelectors(arg ?? "", true) });
					else if (name.startsWith("nth-")) c.pseudos.push({ name, nth: parseNth(arg ?? "") });
					else if (name === "scope") c.scope = true;
					else c.pseudos.push({ name, arg });
					any = true;
				} else if (ch === "&") {
					i++;
					c.scope = true;
					any = true;
				} else if (/[\w\-\\ -￿]/.test(ch ?? "")) {
					if (any) throw new SelErr(`unexpected ${ch} in ${s}`);
					c.tag = ident();
					if (s[i] === "|") {
						i++;
						c.tag = s[i] === "*" ? (i++, null) : ident();
					}
					any = true;
				} else break;
			}
			if (!any) throw new SelErr(`empty compound in ${s}`);
			return c;
		};
		const complex = () => {
			const steps = [];
			ws();
			let comb = null;
			if (/[>+~]/.test(s[i] ?? "")) {
				if (!relative) throw new SelErr(`leading combinator in ${s}`);
				steps.push({ comb: null, c: { tag: null, ids: [], classes: [], attrs: [], pseudos: [], pe: false, scope: true } });
				comb = s[i++];
				ws();
			} else if (relative) {
				steps.push({ comb: null, c: { tag: null, ids: [], classes: [], attrs: [], pseudos: [], pe: false, scope: true } });
				comb = " ";
			}
			for (;;) {
				steps.push({ comb, c: compound() });
				const st = i;
				ws();
				const ch = s[i];
				if (ch === undefined || ch === "," || ch === ")") break;
				if (/[>+~]/.test(ch)) {
					comb = ch;
					i++;
					ws();
				} else if (i > st) comb = " ";
				else throw new SelErr(`unexpected ${ch} in ${s}`);
			}
			return steps;
		};
		const list = [];
		for (;;) {
			const steps = complex();
			list.push({ steps, spec: specOf(steps) });
			ws();
			if (i >= s.length) break;
			if (s[i] === ",") {
				i++;
				continue;
			}
			throw new SelErr(`unexpected ${s[i]} in ${s}`);
		}
		return list;
	};
	const specOf = (steps) => {
		let a = 0,
			b = 0,
			c = 0;
		for (const { c: k } of steps) {
			a += k.ids.length;
			b += k.classes.length + k.attrs.length;
			if (k.tag && k.tag !== "*") c++;
			if (k.pe) c++;
			for (const p of k.pseudos) {
				if (p.name === "where") continue;
				if (p.list) {
					const m = p.list.reduce((best, x) => (cmpSpec(x.spec, best) > 0 ? x.spec : best), [0, 0, 0]);
					a += m[0];
					b += m[1];
					c += m[2];
				} else {
					b++;
					if (p.nth?.of) {
						const m = p.nth.of.reduce((best, x) => (cmpSpec(x.spec, best) > 0 ? x.spec : best), [0, 0, 0]);
						a += m[0];
						b += m[1];
						c += m[2];
					}
				}
			}
		}
		return [a, b, c];
	};
	const cmpSpec = (x, y) => x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
	// TIDAL's CSS-module classes are `_name_<hash>`, and the hash changes
	// with TIDAL builds and between modules of one name; the anchors carry
	// hashes of their own (TIDAL's exact one only where themes pin one
	// module to one hash). A theme naming a module by a full hashed class
	// (`._dataContainer_5bb6028`, `[class="_wrapper_512d5f4"]`) matches
	// that module's anchor whatever the hash, unless the name is too
	// generic to say which module it is or the anchor carries TIDAL's
	// exact hash (then that one must match).
	const MODULE_RE = /^_([A-Za-z][A-Za-z0-9]*)_[0-9a-f]{7}$/;
	const GENERIC_MODULES = new Set(["container", "wrapper", "header", "title", "button", "icon", "link", "item", "items", "text", "content", "inner", "root", "list", "row", "image", "label", "cell", "column", "section", "body", "footer", "grid", "info", "name", "subtitle", "description", "actions", "active", "selected", "visible", "hidden", "small", "large", "primary", "secondary", "default", "base", "outer", "main", "img", "overlay", "background", "gradient"]);
	const literalModules = new Set(g.__lunaModuleLiterals ?? []);
	const canonCache = new Map();
	const canonClass = (t) => {
		let c = canonCache.get(t);
		if (c === undefined) {
			const m = MODULE_RE.exec(t);
			c = m && !GENERIC_MODULES.has(m[1]) && !literalModules.has(m[1]) ? `_${m[1]}_*` : t;
			if (canonCache.size < 8192) canonCache.set(t, c);
		}
		return c;
	};
	const canonClasses = (v) =>
		String(v)
			.split(/\s+/)
			.filter(Boolean)
			.map(canonClass)
			.join(" ");
	// An element's canonical classes, kept until its class attribute changes.
	const elClasses = (e) => {
		const src = e.getAttribute("class") ?? "";
		let cc = e._canon;
		if (!cc || cc.src !== src) cc = e._canon = { src, set: new Set(src.split(/\s+/).filter(Boolean).map(canonClass)), str: canonClasses(src) };
		return cc;
	};
	D.canonClass = canonClass;
	const attrVal = (e, name) => {
		if (name === "class" || name === "id" || name === "style") return e.getAttribute(name);
		const v = e._attrs.get(name);
		if (v !== undefined) return v;
		// SVG's camelCase names (`viewBox`); without copying the names, as
		// this runs for every candidate rule and ancestor.
		for (const k of e._attrs.keys()) if (k.toLowerCase() === name) return e._attrs.get(k);
		return null;
	};
	const attrTest = (e, t) => {
		let a = attrVal(e, t.name);
		if (a === null || a === undefined) return false;
		if (t.op === null) return true;
		let v = t.val;
		if (t.name === "class" && t.op !== "|=" && t.op !== "$=" && /_[0-9a-f]{7}/.test(v)) {
			a = elClasses(e).str;
			v = t.cval ??= canonClasses(v) + (/\s$/.test(v) ? " " : "");
		}
		if (t.ci) {
			a = a.toLowerCase();
			v = v.toLowerCase();
		}
		switch (t.op) {
			case "=":
				return a === v;
			case "~=":
				return v !== "" && !/\s/.test(v) && a.split(/\s+/).includes(v);
			case "|=":
				return a === v || a.startsWith(`${v}-`);
			case "^=":
				return v !== "" && a.startsWith(v);
			case "$=":
				return v !== "" && a.endsWith(v);
			case "*=":
				return v !== "" && a.includes(v);
		}
		return false;
	};
	const elSiblings = (e) => (e.parentNode ? e.parentNode.childNodes.filter((n) => n instanceof Element) : [e]);
	const nthMatch = (pos, { a, b }) => (a === 0 ? pos === b : (pos - b) / a >= 0 && (pos - b) % a === 0);
	const pseudoTest = (e, p, scope) => {
		switch (p.name) {
			case "not":
				return !p.list.some((x) => matchSteps(e, x.steps, scope));
			case "is":
			case "where":
				return p.list.some((x) => matchSteps(e, x.steps, scope));
			case "has": {
				// Remembered until the document changes: a rule such as
				// scroll-to-playing's `[aria-rowindex]:has(a[href*=...]) span`
				// asks it of a row once for every element inside the row.
				if (hasGens[0] !== domGen || hasGens[1] !== structGen || hasGens[2] !== styleGen || hasGens[3] !== hovered || hasGens[4] !== focused) {
					hasMemo = new WeakMap();
					hasGens = [domGen, structGen, styleGen, hovered, focused];
				}
				// (Only in the document: a detached tree changes silently.)
				let m = e.isConnected ? hasMemo.get(e) : new Map();
				if (!m) hasMemo.set(e, (m = new Map()));
				let hit = m.get(p);
				if (hit === undefined) {
					hit = p.list.some((x) => {
						const test = (cnd) => matchSteps(cnd, x.steps, e);
						if (x.steps[1]?.comb === "+" || x.steps[1]?.comb === "~") {
							for (let n = e.nextElementSibling; n; n = n.nextElementSibling) if (test(n) || someDescendant(n, test)) return true;
							return false;
						}
						return someDescendant(e, test);
					});
					m.set(p, hit);
				}
				return hit;
			}
			case "first-child":
				return elSiblings(e)[0] === e;
			case "last-child":
				return elSiblings(e).at(-1) === e;
			case "only-child":
				return elSiblings(e).length === 1;
			case "first-of-type":
				return elSiblings(e).find((x) => x.localName === e.localName) === e;
			case "last-of-type":
				return elSiblings(e).findLast((x) => x.localName === e.localName) === e;
			case "only-of-type":
				return elSiblings(e).filter((x) => x.localName === e.localName).length === 1;
			case "nth-child":
			case "nth-last-child":
			case "nth-of-type":
			case "nth-last-of-type": {
				let sibs = elSiblings(e);
				if (p.name.endsWith("of-type")) sibs = sibs.filter((x) => x.localName === e.localName);
				else if (p.nth.of) sibs = sibs.filter((x) => p.nth.of.some((y) => matchSteps(x, y.steps, scope)));
				let idx = sibs.indexOf(e);
				if (idx < 0) return false;
				if (p.name.includes("last")) idx = sibs.length - 1 - idx;
				return nthMatch(idx + 1, p.nth);
			}
			case "empty":
				return !e.childNodes.some((n) => n instanceof Element || (n instanceof Text && n._data !== ""));
			case "root":
				return e === g.document.documentElement;
			case "checked":
				return !!e.checked || (e.localName === "option" && e.hasAttribute("selected"));
			case "disabled":
				return e.hasAttribute("disabled");
			case "enabled":
				return /^(button|input|select|textarea|option)$/.test(e.localName) && !e.hasAttribute("disabled");
			case "link":
			case "any-link":
				return e.localName === "a" && e.hasAttribute("href");
			case "defined":
				return true;
			case "lang":
				return (e.closest("[lang]")?.getAttribute("lang") ?? "en").toLowerCase().startsWith(String(p.arg ?? "").trim().toLowerCase());
			case "focus":
			case "focus-within":
			case "focus-visible":
				return focused !== null && (p.name === "focus-within" ? e.contains(focused) : focused === e);
			case "hover":
				return hovered !== null && hovered.isConnected && (hovered === e || e.contains(hovered));
			default:
				// :active, :visited, :target, ...: states that never hold here.
				return false;
		}
	};
	const compoundTest = (e, c, scope) => {
		if (!(e instanceof Element)) return false;
		if (c.pe) return false;
		if (c.scope && e !== (scope ?? g.document.documentElement)) return false;
		if (c.tag && c.tag !== "*" && c.tag.toLowerCase() !== e.localName.toLowerCase()) return false;
		for (const id of c.ids) if (e.getAttribute("id") !== id) return false;
		if (c.classes.length) {
			const cls = elClasses(e).set;
			for (const k of (c.cclasses ??= c.classes.map(canonClass))) if (!cls.has(k)) return false;
		}
		for (const a of c.attrs) if (!attrTest(e, a)) return false;
		for (const p of c.pseudos) if (!pseudoTest(e, p, scope)) return false;
		return true;
	};
	// Whether an element or one of its ancestors matches the first steps of a
	// selector, remembered until the document changes: the cascade asks it of
	// the same row for every element inside the row (`.row .x span`).
	let ancMemo = new WeakMap();
	let ancGens = [];
	// `memo`: only for elements in the document (css.js's cascade), whose
	// every change moves one of the generations below; a detached tree's
	// attributes change silently.
	const matchSteps = (e, steps, scope, memo = false) => {
		if (memo && (ancGens[0] !== domGen || ancGens[1] !== structGen || ancGens[2] !== styleGen || ancGens[3] !== hovered || ancGens[4] !== focused)) {
			ancMemo = new WeakMap();
			ancGens = [domGen, structGen, styleGen, hovered, focused];
		}
		// Some ancestor of `el` matches steps[0..=i].
		const anc = (el, i) => {
			const p = el.parentElement;
			if (!p) return false;
			let m = ancMemo.get(p);
			if (!m) ancMemo.set(p, (m = new Map()));
			let s = m.get(steps);
			if (!s) m.set(steps, (s = []));
			let r = s[i];
			if (r === undefined) s[i] = r = at(p, i) || anc(p, i);
			return r;
		};
		const at = (el, i) => {
			if (!compoundTest(el, steps[i].c, scope)) return false;
			if (i === 0) return true;
			const comb = steps[i].comb;
			if (comb === ">") return !!el.parentElement && at(el.parentElement, i - 1);
			if (comb === "+") {
				const p = el.previousElementSibling;
				return !!p && at(p, i - 1);
			}
			if (comb === "~") {
				for (let p = el.previousElementSibling; p; p = p.previousElementSibling) if (at(p, i - 1)) return true;
				return false;
			}
			if (memo) return anc(el, i - 1);
			for (let p = el.parentElement; p; p = p.parentElement) if (at(p, i - 1)) return true;
			return false;
		};
		return at(e, steps.length - 1);
	};
	const selCache = new Map();
	const compileList = (sel) => {
		sel = String(sel);
		let l = selCache.get(sel);
		if (l === undefined) {
			try {
				l = parseSelectors(sel);
			} catch (err) {
				if (!(err instanceof SelErr)) throw err;
				l = null;
			}
			if (selCache.size > 5000) selCache.clear();
			selCache.set(sel, l);
		}
		if (l === null) throw new g.DOMException(`'${sel}' is not a valid selector`, "SyntaxError");
		return l;
	};
	const matchesList = (e, list, scope) => list.some((x) => matchSteps(e, x.steps, scope));
	D.sel = { parse: parseSelectors, compile: compileList, matches: matchesList, matchSteps, cmpSpec, SelErr };
	// The element descendants of `root` in document order, until `fn`
	// answers true (then true). A loop with its own stack: a recursive
	// generator costs a step per level for every element it yields, and
	// TIDAL's pages are deep.
	const someDescendant = (root, fn) => {
		const kidsOf = (n) => (n.localName === "template" && n.content ? n.content.childNodes : n.childNodes) ?? [];
		const stack = [];
		const push = (kids) => {
			for (let i = kids.length - 1; i >= 0; i--) if (kids[i] instanceof Element) stack.push(kids[i]);
		};
		push(kidsOf(root));
		while (stack.length) {
			const c = stack.pop();
			if (fn(c)) return true;
			push(kidsOf(c));
		}
		return false;
	};
	const descendants = (root) => {
		const out = [];
		someDescendant(root, (e) => {
			out.push(e);
		});
		return out;
	};
	D.descendants = descendants;
	let hasMemo = new WeakMap();
	let hasGens = [];
	const qsa = (root, sel, first) => {
		touch();
		const list = compileList(sel);
		const scope = root instanceof Element ? root : null;
		const out = [];
		someDescendant(root, (e) => {
			if (!matchesList(e, list, scope)) return false;
			out.push(e);
			return !!first;
		});
		return out;
	};
	const nodeList = (arr) => Object.assign(arr, { item: (i) => arr[i] ?? null });
	for (const C of [Element, DocumentFragment]) {
		C.prototype.querySelector = function (sel) {
			return qsa(this, sel, true)[0] ?? null;
		};
		C.prototype.querySelectorAll = function (sel) {
			return nodeList(qsa(this, sel));
		};
		C.prototype.getElementsByClassName = function (names) {
			const ns = String(names).trim().split(/\s+/).filter(Boolean);
			if (!ns.length) return nodeList([]);
			return nodeList([...descendants(this)].filter((e) => ns.every((n) => e.classList.contains(n))));
		};
		C.prototype.getElementsByTagName = function (tag) {
			touch();
			const t = String(tag).toLowerCase();
			return nodeList([...descendants(this)].filter((e) => t === "*" || e.localName.toLowerCase() === t));
		};
	}
	Element.prototype.matches = function (sel) {
		return matchesList(this, compileList(sel), null);
	};
	Element.prototype.webkitMatchesSelector = Element.prototype.matches;
	Element.prototype.closest = function (sel) {
		const list = compileList(sel);
		for (let e = this; e instanceof Element; e = e.parentNode) if (matchesList(e, list, null)) return e;
		return null;
	};

	// ---------------------------------------------------------------- document
	let focused = null;
	// The element under the pointer, as the UI reports it (`hover` events),
	// so `:hover` rules apply to it and its ancestors.
	let hovered = null;
	class Document extends Node {
		constructor() {
			super();
			this.readyState = "complete";
			this.visibilityState = "visible";
			this.hidden = false;
			this.title = "TIDAL";
			this.cookie = "";
			this.characterSet = "UTF-8";
			this.contentType = "text/html";
			this.compatMode = "CSS1Compat";
			this._adopted = [];
		}
		get nodeType() {
			return 9;
		}
		get nodeName() {
			return "#document";
		}
		get documentElement() {
			return this.childNodes.find((n) => n instanceof Element) ?? null;
		}
		get head() {
			return this.documentElement?.childNodes.find((n) => n.localName === "head") ?? null;
		}
		get body() {
			return this.documentElement?.childNodes.find((n) => n.localName === "body") ?? null;
		}
		get children() {
			return this.childNodes.filter((n) => n instanceof Element);
		}
		get firstElementChild() {
			return this.documentElement;
		}
		get activeElement() {
			return focused ?? this.body;
		}
		get defaultView() {
			return g;
		}
		get scrollingElement() {
			return this.documentElement;
		}
		get location() {
			return g.location;
		}
		get URL() {
			return g.location.href;
		}
		get documentURI() {
			return g.location.href;
		}
		get baseURI() {
			return g.location.href;
		}
		get styleSheets() {
			return L.css?.documentSheets() ?? [];
		}
		get adoptedStyleSheets() {
			return this._adopted;
		}
		set adoptedStyleSheets(v) {
			touch();
			this._adopted = [...(v ?? [])];
			cssChanged();
		}
		get fullscreenElement() {
			return fsElement;
		}
		get webkitFullscreenElement() {
			return fsElement;
		}
		get fullscreenEnabled() {
			return true;
		}
		get webkitIsFullScreen() {
			return !!fsElement;
		}
		get fullscreen() {
			return !!fsElement;
		}
		exitFullscreen() {
			return D.exitFullscreen();
		}
		webkitExitFullscreen() {
			return D.exitFullscreen();
		}
		hasFocus() {
			return true;
		}
		createElement(tag) {
			touch();
			return createEl(String(tag));
		}
		createElementNS(ns, tag) {
			touch();
			return createEl(String(tag), ns === SVG_NS ? SVG_NS : String(tag) === "svg" ? SVG_NS : HTML_NS);
		}
		createTextNode(t) {
			return new Text(t);
		}
		createComment(t) {
			return new Comment(t ?? "");
		}
		createDocumentFragment() {
			return new DocumentFragment();
		}
		createEvent(t) {
			const C = { mouseevent: MouseEvent, mouseevents: MouseEvent, keyboardevent: KeyboardEvent, customevent: g.CustomEvent, uievent: UIEvent, focusevent: FocusEvent }[String(t).toLowerCase()] ?? g.Event;
			const e = new C("");
			if (C === g.CustomEvent)
				e.initCustomEvent = function (type, b, c, detail) {
					this.initEvent(type, b, c);
					this.detail = detail;
				};
			return e;
		}
		getElementById(id) {
			touch();
			const want = String(id);
			let found = null;
			someDescendant(this, (e) => e.getAttribute("id") === want && ((found = e), true));
			return found;
		}
		getElementsByName(n) {
			return this.querySelectorAll(`[name="${CSS.escape(n)}"]`);
		}
		querySelector(sel) {
			return qsa(this, sel, true)[0] ?? null;
		}
		querySelectorAll(sel) {
			return nodeList(qsa(this, sel));
		}
		getElementsByClassName(n) {
			return Element.prototype.getElementsByClassName.call(this, n);
		}
		getElementsByTagName(t) {
			return Element.prototype.getElementsByTagName.call(this, t);
		}
		elementFromPoint(x, y) {
			return this.elementsFromPoint(x, y)[0] ?? null;
		}
		elementsFromPoint(x, y) {
			const out = [];
			for (const e of descendants(this)) {
				const r = e._rect;
				if (r && x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]) out.push(e);
			}
			return out.reverse();
		}
		execCommand(cmd) {
			if (String(cmd).toLowerCase() === "copy") {
				const s = String(g.getSelection());
				if (s) {
					try {
						L.callOp("clipboard", { text: s });
					} catch {}
					return true;
				}
			}
			return false;
		}
		adoptNode(n) {
			n.remove?.();
			return n;
		}
		importNode(n, deep) {
			return n.cloneNode(deep);
		}
		open() {
			return this;
		}
		write() {}
		writeln() {}
		close() {}
		_cloneSelf() {
			return new Document();
		}
	}
	defineOn(Document.prototype, (x) => x);
	Document.prototype.addEventListener = Target.prototype.addEventListener;
	const doc = new Document();
	const html = createEl("html");
	html._attrs.set("lang", "en");
	const head = createEl("head");
	const body = createEl("body");
	const wimp = createEl("div");
	wimp._attrs.set("id", "wimp");
	// A plugin window's page (`L.page`, src/luna/page.rs) is a plain HTML
	// document: its body holds the page, with no app shell in it.
	for (const [p, c] of [
		[doc, html],
		[html, head],
		[html, body],
		...(L.page ? [] : [[body, wimp]]),
	]) {
		p.childNodes.push(c);
		c.parentNode = p;
	}
	g.document = doc;
	D.wimp = wimp;
	// TIDAL's sprite: `<use href="#player__shuffle">` finds its symbol here.
	// The sprite is hidden (never drawn as a plugin element); the UI gets the
	// symbols inlined where an svg uses them.
	const SPRITE = {
		// The shuffle Tidalfast draws for TIDAL's own button, a stroked
		// icon (the others are filled glyphs).
		player__shuffle: { stroke: "M18 14l4 4-4 4M18 2l4 4-4 4M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22M2 6h1.972a4 4 0 0 1 3.6 2.2M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45" },
		player__play: "M8 5v14l11-7z",
		player__pause: "M6 19h4V5H6v14zm8-14v14h4V5h-4z",
		player__next: "M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z",
		player__previous: "M6 6h2v12H6zm3.5 6l8.5 6V6z",
		player__repeat: "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z",
		player__maximize: "M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z",
		"general__lyrics-sync": "M12 4V1L8 5l4 4V6c3.31 0 6 2.69 6 6 0 1.01-.25 1.97-.7 2.8l1.46 1.46C19.54 15.03 20 13.57 20 12c0-4.42-3.58-8-8-8zm0 14c-3.31 0-6-2.69-6-6 0-1.01.25-1.97.7-2.8L5.24 7.74C4.46 8.97 4 10.43 4 12c0 4.42 3.58 8 8 8v3l4-4-4-4v3z",
		player__heart: "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z",
	};
	// Not in a plugin window's page: that document is the page's alone.
	if (!L.page) {
		const sprite = createEl("svg", SVG_NS);
		sprite._sprite = true;
		sprite._attrs.set("id", "svg-sprite");
		sprite._attrs.set("aria-hidden", "true");
		sprite._attrs.set("style", "display:none");
		for (const [id, glyph] of Object.entries(SPRITE)) {
			const symbol = createEl("symbol", SVG_NS);
			symbol._attrs.set("id", id);
			symbol._attrs.set("viewBox", "0 0 24 24");
			const path = createEl("path", SVG_NS);
			path._attrs.set("d", glyph.stroke ?? glyph);
			path._attrs.set("fill", glyph.stroke ? "none" : "currentColor");
			if (glyph.stroke) path._attrs.set("stroke", "currentColor");
			symbol.childNodes.push(path);
			path.parentNode = symbol;
			sprite.childNodes.push(symbol);
			symbol.parentNode = sprite;
		}
		body.childNodes.push(sprite);
		sprite.parentNode = body;
	}
	// A symbol is drawn in its own 24-unit box scaled to the `<use>`'s
	// (the whole svg here), and its paths take the paint CSS gives the
	// svg (`fill`, inherited: LuckyLP's 60 % white), else the text colour:
	// the renderer puts that in for `__luna-paint__`.
	const inlineUses = (m) =>
		m.replace(/<use\b([^>]*?)\/?>(?:<\/use>)?/g, (all, attrs) => {
			const id = /\bhref="#([^"]+)"/.exec(attrs)?.[1];
			const glyph = !L.page && id && SPRITE[id];
			if (!glyph) return all;
			const path = glyph.stroke
				? `<path d="${glyph.stroke}" fill="none" stroke="__luna-paint__" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>`
				: `<path d="${glyph}" fill="__luna-paint__"/>`;
			return `<svg viewBox="0 0 24 24" width="100%" height="100%">${path}</svg>`;
		});
	Object.assign(g, { Node, CharacterData, Text, Comment, Element, HTMLElement, SVGElement, Document, HTMLDocument: Document, DocumentFragment, ShadowRoot });
	g.EventTarget = Target;

	// ---------------------------------------------------------------- window bits
	g.window = g;
	g.self = g;
	g.top = g;
	g.parent = g;
	g.frames = g;
	g.opener = null;
	g.innerWidth = 1280;
	g.innerHeight = 800;
	g.outerWidth = 1280;
	g.outerHeight = 800;
	g.scrollX = g.scrollY = g.pageXOffset = g.pageYOffset = 0;
	// The display the window is on and its scale, from the UI (op `screen`,
	// DIPs as Chromium gives them), read again at most once a second.
	let screenRead = null;
	let screenAt = 0;
	const screenInfo = () => {
		const now = Date.now();
		if (!screenRead || now - screenAt >= 1000) {
			try {
				screenRead = L.callOp("screen", null) ?? {};
			} catch {
				screenRead = {};
			}
			screenAt = now;
		}
		return screenRead;
	};
	const ONE_SCREEN = { bounds: { x: 0, y: 0, width: 1920, height: 1080 }, workArea: { x: 0, y: 0, width: 1920, height: 1080 } };
	const onScreen = () => {
		const s = screenInfo();
		const all = Array.isArray(s.displays) ? s.displays : [];
		return all.find((d) => d.id === s.current) ?? all[0] ?? ONE_SCREEN;
	};
	class ScreenOrientation extends Target {
		get type() {
			const b = onScreen().bounds;
			return b.width >= b.height ? "landscape-primary" : "portrait-primary";
		}
		get angle() {
			return 0;
		}
		lock() {
			return Promise.reject(new g.DOMException("The page cannot lock the screen's orientation.", "NotSupportedError"));
		}
		unlock() {}
	}
	const orientation = new ScreenOrientation();
	class Screen extends Target {
		get width() {
			return onScreen().bounds.width;
		}
		get height() {
			return onScreen().bounds.height;
		}
		get availWidth() {
			return onScreen().workArea.width;
		}
		get availHeight() {
			return onScreen().workArea.height;
		}
		get availLeft() {
			return onScreen().workArea.x;
		}
		get availTop() {
			return onScreen().workArea.y;
		}
		get colorDepth() {
			return 24;
		}
		get pixelDepth() {
			return 24;
		}
		get isExtended() {
			return (screenInfo().displays?.length ?? 1) > 1;
		}
		get orientation() {
			return orientation;
		}
	}
	g.Screen = Screen;
	g.ScreenOrientation = ScreenOrientation;
	g.screen = new Screen();
	// The window's scale, its zoom included; a page may replace it, as in
	// a browser.
	let dprSet;
	Object.defineProperty(g, "devicePixelRatio", {
		get() {
			return dprSet !== undefined ? dprSet : Number(screenInfo().dpr) || 1;
		},
		set(v) {
			dprSet = v;
		},
		configurable: true,
		enumerable: true,
	});
	const loc = { href: "https://desktop.tidal.com/", origin: "https://desktop.tidal.com", protocol: "https:", host: "desktop.tidal.com", hostname: "desktop.tidal.com", port: "", pathname: "/", search: "", hash: "", reload() {}, assign() {}, replace() {}, toString() { return this.href; } };
	g.location = loc;
	const setPath = (path) => {
		const p = String(path || "/");
		const q = p.indexOf("?");
		loc.pathname = q < 0 ? p : p.slice(0, q);
		loc.search = q < 0 ? "" : p.slice(q);
		loc.href = `${loc.origin}${loc.pathname}${loc.search}${loc.hash}`;
	};
	g.history = { pushState() {}, replaceState() {}, back() {}, forward() {}, go() {}, state: null, length: 1, scrollRestoration: "auto" };
	g.scrollTo = g.scroll = g.scrollBy = () => {};
	g.getSelection = () => selection;
	Document.prototype.getSelection = g.getSelection;

	// ---------------------------------------------------------------- fullscreen
	let fsElement = null;
	const fireFullscreen = (target) => {
		Promise.resolve().then(() => dispatch(target ?? doc, new g.Event("fullscreenchange", { bubbles: true })));
	};
	D.requestFullscreen = (el) => {
		touch();
		const was = fsElement;
		fsElement = el;
		try {
			L.callOp("window.fullscreen", { value: true });
		} catch {}
		if (was !== el) fireFullscreen(el);
		return Promise.resolve();
	};
	D.exitFullscreen = () => {
		if (!fsElement) return Promise.resolve();
		const was = fsElement;
		fsElement = null;
		try {
			L.callOp("window.fullscreen", { value: false });
		} catch {}
		fireFullscreen(was.isConnected ? was : doc);
		return Promise.resolve();
	};

	// ---------------------------------------------------------------- scrolling
	let scrollSeq = 0;
	let clickSeq = 0;
	// The last clicks, not only the newest: two in one turn (the player
	// bar, then the Lyrics toggle) must both reach their widgets.
	const hostClicks = [];
	D.hostClick = (key) => {
		hostClicks.push([key, ++clickSeq]);
		if (hostClicks.length > 16) hostClicks.shift();
		contentDirty();
	};
	let scrollTo = null;
	let scrollTop = null;
	const hostAnchorOf = (el) => {
		for (let n = el; n; n = n.parentNode) if (n._hostKey !== undefined) return n;
		return null;
	};
	// Scrolls asked of the plugins' own elements (in the app or a plugin
	// window's page), the newest per element, for the UI. The count starts
	// from the clock so a restarted runtime's requests are newer still.
	const nodeScrolls = new Map();
	let nodeScrollSeq = Date.now();
	const ownNode = (el) => el instanceof Element && el._hostKey === undefined && el.isConnected && vidOf(el) !== undefined;
	const askNodeScroll = (el, ask) => {
		nodeScrolls.delete(el._vid);
		nodeScrolls.set(el._vid, { id: el._vid, ...ask, seq: ++nodeScrollSeq });
		for (const id of nodeScrolls.keys()) {
			if (nodeScrolls.size <= 64) break;
			nodeScrolls.delete(id);
		}
		contentDirty();
	};
	D.nodeScrolls = () => [...nodeScrolls.values()].filter((s) => vnodes.has(s.id)).sort((a, b) => a.id - b.id);
	D.scrollIntoView = (el, opts) => {
		if (ownNode(el)) {
			const o = typeof opts === "object" && opts !== null ? opts : { block: opts === false ? "end" : "start" };
			const block = ["start", "center", "end", "nearest"].includes(o.block) ? o.block : "start";
			askNodeScroll(el, { block, smooth: o.behavior === "smooth" });
			return;
		}
		const h = hostAnchorOf(el);
		if (!h) return;
		scrollTo = [h._hostKey, ++scrollSeq];
		contentDirty();
	};
	// A scroll position asked of a host container: remembered for the UI, and
	// translated to the host row nearest the middle of the new viewport.
	D.requestScroll = (el, top) => {
		if (ownNode(el)) {
			askNodeScroll(el, { top });
			return;
		}
		const h = hostAnchorOf(el);
		if (!h) return;
		scrollTop = [h._hostKey, top, ++scrollSeq];
		const cr = rectOf(h);
		if (cr) {
			const cur = h._scroll?.[0] ?? 0;
			const center = cr[1] + cr[3] / 2 + (top - cur);
			let best = null,
				dist = Infinity;
			for (const e of descendants(h)) {
				if (e._hostKey === undefined || !e._rect || e.getAttribute("role") !== "row") continue;
				const d = Math.abs(e._rect[1] + e._rect[3] / 2 - center);
				if (d < dist) {
					dist = d;
					best = e;
				}
			}
			if (best) scrollTo = [best._hostKey, scrollSeq];
		}
		markDirty();
	};

	// ---------------------------------------------------------------- animation frames
	// Callbacks wait for the next ~16 ms frame boundary; nothing runs while
	// none are queued.
	let rafQ = new Map();
	let rafSeq = 1;
	let rafArmed = false;
	let lastFrame = 0;
	// Frames at ~60 Hz while they change something (the document, a style,
	// a canvas); after IDLE_AFTER frames in a row that changed nothing (a loop
	// waiting for its next step, a sleeping sprite), ~10 Hz until one does
	// again or the user acts. Tidalfast repaints only on change, so an idle
	// loop at full rate would cost CPU for no visible frame.
	const FRAME_MS = 16;
	const IDLE_FRAME_MS = 100;
	const IDLE_AFTER = 10;
	// While the window is in the background (not focused), animations that
	// change every frame (spinning covers, visualisers) run at ~30 Hz;
	// loops that change now and then (oneko's sleeping cat flips a sprite
	// every few steps) and idle ones run at ~4 Hz, so a sprite's change
	// does not bring back a fast loop.
	const BACKGROUND_FRAME_MS = 33;
	const BACKGROUND_IDLE_MS = 250;
	const BACKGROUND_STEADY = 3;
	let idleFrames = 0;
	// Frames in a row that changed something.
	let busyFrames = 0;
	D.wakeFrames = () => {
		idleFrames = 0;
	};
	if (g.__canvas)
		for (const k of ["publish", "glPublish"]) {
			const f = g.__canvas[k];
			if (typeof f === "function")
				g.__canvas[k] = (...a) => {
					frameWork++;
					return f(...a);
				};
		}
	const armFrame = () => {
		if (rafArmed || L.hidden || !rafQ.size) return;
		rafArmed = true;
		const idle = idleFrames >= IDLE_AFTER;
		const every = L.unfocused ? (busyFrames >= BACKGROUND_STEADY ? BACKGROUND_FRAME_MS : BACKGROUND_IDLE_MS) : idle ? IDLE_FRAME_MS : FRAME_MS;
		L.internalTimeout(runFrame, Math.max(0, every - (Date.now() - lastFrame)));
	};
	const runFrame = () => {
		rafArmed = false;
		// A hidden window draws no frames: callbacks wait until it shows.
		if (L.hidden) return;
		lastFrame = Date.now();
		const q = rafQ;
		rafQ = new Map();
		const t = g.performance.now();
		const work = frameWork;
		inFrame = true;
		for (const { cb, plugin } of q.values()) {
			L.running = plugin;
			try {
				cb(t);
			} catch (err) {
				L.reportError(err);
			} finally {
				L.running = undefined;
			}
		}
		inFrame = false;
		// The frame's style pass, after its callbacks, as in a browser.
		if (fresh.size) D.styleTick();
		idleFrames = frameWork === work ? idleFrames + 1 : 0;
		busyFrames = frameWork === work ? 0 : busyFrames + 1;
		armFrame();
	};
	let inFrame = false;
	g.requestAnimationFrame = (cb) => {
		const id = rafSeq++;
		rafQ.set(id, { cb, plugin: L.running ?? L.pluginFromStack() });
		if (!inFrame) armFrame();
		return id;
	};
	// The window was hidden or shown (Msg::Hidden): document visibility,
	// `visibilitychange`, paused animation frames, throttled plugin timers.
	D.setHidden = (hidden) => {
		hidden = !!hidden;
		if (hidden === L.hidden) return;
		L.setTimersHidden(hidden);
		const doc = g.document;
		doc.hidden = hidden;
		doc.visibilityState = hidden ? "hidden" : "visible";
		try {
			doc.dispatchEvent(new g.Event("visibilitychange"));
		} catch (err) {
			L.reportError(err);
		}
		armFrame();
	};
	D.setFocused = (focused) => {
		L.unfocused = !focused;
	};
	g.cancelAnimationFrame = (id) => rafQ.delete(id);
	g.webkitRequestAnimationFrame = g.requestAnimationFrame;
	D.rafPending = () => rafQ.size;

	// ---------------------------------------------------------------- resize / intersection observers
	const sizeObservers = new Set();
	class ResizeObserver {
		constructor(cb) {
			this._cb = cb;
			this._t = new Map();
			this._plugin = L.pluginFromStack();
		}
		observe(el) {
			touch();
			// Already observed: nothing new, as in a browser (tidal-genres
			// observes every row again after each change on the page).
			if (this._t.has(el)) return;
			this._t.set(el, null);
			sizeObservers.add(this);
			// Its first report comes with the next flush; no style changed.
			contentDirty();
		}
		unobserve(el) {
			this._t.delete(el);
		}
		disconnect() {
			this._t.clear();
			sizeObservers.delete(this);
		}
		_check() {
			const entries = [];
			for (const [el, last] of this._t) {
				const r = el.getBoundingClientRect();
				const sig = `${r.width}x${r.height}`;
				if (sig === last) continue;
				this._t.set(el, sig);
				const box = [{ inlineSize: r.width, blockSize: r.height }];
				entries.push({ target: el, contentRect: { x: 0, y: 0, top: 0, left: 0, width: r.width, height: r.height, right: r.width, bottom: r.height }, borderBoxSize: box, contentBoxSize: box, devicePixelContentBoxSize: box });
			}
			if (entries.length)
				try {
					this._cb.call(this, entries, this);
				} catch (err) {
					L.reportError(err);
				}
		}
	}
	class IntersectionObserver {
		constructor(cb, opts = {}) {
			this._cb = cb;
			this._t = new Map();
			this.root = opts.root ?? null;
			this.rootMargin = opts.rootMargin ?? "0px";
			const th = opts.threshold ?? 0;
			this.thresholds = Array.isArray(th) ? th : [th];
			this._plugin = L.pluginFromStack();
		}
		observe(el) {
			touch();
			// Already observed: nothing new, as in a browser (tidal-genres
			// observes every row again after each change on the page).
			if (this._t.has(el)) return;
			this._t.set(el, null);
			sizeObservers.add(this);
			// Its first report comes with the next flush; no style changed.
			contentDirty();
		}
		unobserve(el) {
			this._t.delete(el);
		}
		disconnect() {
			this._t.clear();
			sizeObservers.delete(this);
		}
		takeRecords() {
			return [];
		}
		_check() {
			const rr = this.root ? this.root.getBoundingClientRect() : rectObj([0, 0, g.innerWidth, g.innerHeight]);
			const entries = [];
			for (const [el, last] of this._t) {
				const r = el.getBoundingClientRect();
				const connected = el.isConnected && !el._pluginRemoved;
				const ix = Math.max(0, Math.min(r.right, rr.right) - Math.max(r.left, rr.left));
				const iy = Math.max(0, Math.min(r.bottom, rr.bottom) - Math.max(r.top, rr.top));
				const area = r.width * r.height;
				const ratio = connected && area > 0 ? (ix * iy) / area : 0;
				const inter = connected && area > 0 && ix * iy > 0;
				const bucket = this.thresholds.filter((t) => ratio >= t && (t > 0 || inter)).length;
				const sig = `${inter}:${bucket}`;
				if (sig === last) continue;
				this._t.set(el, sig);
				entries.push({ target: el, isIntersecting: inter, intersectionRatio: ratio, boundingClientRect: r, rootBounds: rr, intersectionRect: rectObj([Math.max(r.left, rr.left), Math.max(r.top, rr.top), ix, iy]), time: g.performance.now(), isVisible: inter });
			}
			if (entries.length)
				try {
					this._cb.call(this, entries, this);
				} catch (err) {
					L.reportError(err);
				}
		}
	}
	g.ResizeObserver = ResizeObserver;
	g.IntersectionObserver = IntersectionObserver;
	const checkGeometry = () => {
		const busy = layoutBusy;
		layoutBusy = true;
		try {
			for (const o of [...sizeObservers]) o._check();
		} finally {
			layoutBusy = busy;
		}
	};

	// ---------------------------------------------------------------- forced layout
	// A browser lays the page out when a script reads a box after changing
	// a style. Here the UI lays it out: once an overlay that can move boxes
	// went out (`dom.overlay` says so), the first read of a box waits for
	// the UI's tree laid out from it, on the UI's frame that draws it
	// (`layout.rs`), so a plugin that measures after its change
	// (radiant-lyrics hangs its menu under its widened Lyrics tab 200 ms
	// after the click) reads the new box. One wait per change, none while
	// the window is hidden, and none at all while nothing changed.
	let boxesPending = false;
	let layoutBusy = false;
	// The least time the UI gets to start drawing the change.
	const LAYOUT_WAIT_MS = 64;
	const forceLayout = () => {
		if (layoutBusy || reconciling || L.hidden) return;
		// The change is not out yet (the overlay goes at most every 33 ms):
		// out with it now, as a browser works out styles on such a read.
		if (layoutDirty && dirty && !inFrame) {
			layoutBusy = true;
			try {
				D.flush();
			} finally {
				layoutBusy = false;
			}
		}
		if (!boxesPending) return;
		boxesPending = false;
		let tree = null;
		const t0 = L.stat ? Date.now() : 0;
		try {
			tree = L.callOp("dom.wait_layout", { ms: LAYOUT_WAIT_MS });
		} catch (err) {
			L.reportError(err);
		}
		if (L.stat) L.stat(tree ? "layout wait" : "layout wait, no tree", t0);
		// Observers and `resize` hear of it as of a frame, not inside the
		// read.
		if (tree) sync(tree, true);
	};

	// ---------------------------------------------------------------- unload hygiene
	// A plugin's observers, frame callbacks and listeners go with it.
	const dropPlugin = (plugin) => {
		for (const o of [...observers]) if (o._plugin === plugin) o.disconnect();
		for (const o of [...sizeObservers]) if (o._plugin === plugin) o.disconnect();
		for (const [id, r] of rafQ) if (r.plugin === plugin) rafQ.delete(id);
		const strip = (t) => {
			if (!t._ls) return;
			for (const [type, arr] of t._ls) {
				const keep = arr.filter((l) => l.plugin !== plugin);
				if (keep.length !== arr.length) {
					keep.length ? t._ls.set(type, keep) : t._ls.delete(type);
					listensChanged();
				}
			}
		};
		strip(winTarget);
		strip(doc);
		for (const e of descendants(doc)) strip(e);
		// Elements (and <style> sheets) it added and did not take away.
		const left = [];
		for (const e of descendants(doc)) if (e._by === plugin) left.push(e);
		for (const e of left) e.parentNode?._detach(e);
	};
	L.domFlagReset = (plugin) => {
		flagged.delete(plugin);
		dropPlugin(plugin);
	};

	// ---------------------------------------------------------------- host tree reconcile
	const hosts = new Map();
	D.hosts = hosts;
	const classDiff = (el, prev, next) => {
		const p = prev.split(/\s+/).filter(Boolean);
		const n = next.split(/\s+/).filter(Boolean);
		const cur = (el._attrs.get("class") ?? "").split(/\s+/).filter(Boolean);
		// Host classes first (in host order), then what plugins added.
		const extra = cur.filter((c) => !p.includes(c) && !n.includes(c));
		const removedByPlugin = p.filter((c) => !cur.includes(c));
		const out = [...n.filter((c) => !removedByPlugin.includes(c) || !p.includes(c)), ...extra];
		const v = [...new Set(out)].join(" ");
		if (v !== (el._attrs.get("class") ?? "")) el._setAttr("class", v);
	};
	// Count of anchor trees received, so waits for TIDAL's elements can tell
	// whether the app has drawn since they began.
	let syncs = 0;
	D.syncCount = () => syncs;
	const sync = (tree, forced) => {
		syncs++;
		reconciling = true;
		let resized = false;
		try {
			const [w, h] = tree.viewport ?? [];
			if (w > 0 && h > 0 && (w !== g.innerWidth || h !== g.innerHeight)) {
				g.innerWidth = g.outerWidth = w;
				g.innerHeight = g.outerHeight = h;
				resized = true;
			}
			// `prefers-color-scheme` follows the app's Light/Dark choice.
			const scheme = tree.light ? "light" : "dark";
			if (scheme !== D.colorScheme) {
				D.colorScheme = scheme;
				resized = true;
			}
			// `prefers-reduced-motion` follows the desktop, as the app does.
			const reduce = !!tree.reduced_motion;
			if (reduce !== !!D.reducedMotion) {
				D.reducedMotion = reduce;
				resized = true;
			}
			if (!L.page && typeof tree.path === "string" && (tree.path !== loc.pathname + loc.search || !synced)) {
				setPath(tree.path);
				pageChanged = true;
			}
			synced = true;
			const seen = new Set();
			const order = new Map();
			for (const n of tree.nodes ?? []) {
				if (!n?.key || seen.has(n.key)) continue;
				const parent = n.parent === undefined || n.parent === null ? wimp : hosts.get(n.parent);
				if (!parent || !seen.has(n.parent ?? "") && parent !== wimp) continue;
				seen.add(n.key);
				const tag = String(n.tag || "div");
				let el = hosts.get(n.key);
				if (el && el.localName.toLowerCase() !== tag.toLowerCase()) {
					el.remove();
					hosts.delete(n.key);
					el = undefined;
				}
				// TIDAL renders a new page element when the page changes, so
				// plugins that observe pages see them arrive.
				const pid = (n.attrs ?? []).find(([k]) => String(k).toLowerCase() === "data-track--page-id")?.[1];
				const was = el?._hostAttrs?.get("data-track--page-id");
				if (el && pid !== undefined && was !== undefined && String(pid) !== was) {
					el.remove();
					hosts.delete(n.key);
					el = undefined;
				}
				const fresh = !el;
				if (fresh) {
					el = createEl(tag, tag === "svg" ? SVG_NS : undefined);
					el._hostKey = n.key;
					el._hostAttrs = new Map();
					hosts.set(n.key, el);
				}
				el._hostParent = parent;
				// attributes: apply what the host changed since last time
				const next = new Map((n.attrs ?? []).map(([k, v]) => [String(k).toLowerCase(), String(v)]));
				const prev = el._hostAttrs;
				for (const [k, v] of next) {
					if (!fresh && prev.get(k) === v) continue;
					if (k === "class") classDiff(el, prev.get(k) ?? "", v);
					else el._setAttr(k, String(v));
				}
				for (const [k] of prev) if (!next.has(k)) k === "class" ? classDiff(el, prev.get(k), "") : el._removeAttr(k);
				el._hostAttrs = next;
				// own text
				const text = n.text ?? null;
				if (text !== (el._hostTextVal ?? null) || fresh) {
					el._hostTextVal = text;
					if (text === null) {
						if (el._hostText?.parentNode === el) el._detach(el._hostText);
						el._hostText = null;
					} else if (el._hostText) el._hostText.data = text;
					else {
						el._hostText = new Text(text);
						if (!el._textTouched) el.insertBefore(el._hostText, el.firstChild);
					}
				}
				el._rect = Array.isArray(n.rect) ? n.rect : null;
				el._scroll = Array.isArray(n.scroll) ? n.scroll : null;
				if (el._scroll) el._scrollTop = undefined;
				const value = n.value ?? undefined;
				if (value !== el._hostValue) {
					el._hostValue = value;
					el._value = undefined;
				}
				if (!order.has(parent)) order.set(parent, []);
				order.get(parent).push(el);
			}
			for (const [k, el] of [...hosts]) {
				if (seen.has(k)) continue;
				hosts.delete(k);
				if (el.parentNode && !el._pluginMoved) el.remove();
				el._hostKey = undefined;
			}
			// placement: host children in tree order; plugin nodes keep their spot.
			// The deepest parents first, as React commits a new subtree: it is
			// put together while detached and goes in as one node, so a new
			// page is one mutation record with its root (TIDAL's), not one per
			// element (an observer that scans the page for every added node
			// scanned it hundreds of times). A parent's entry comes before its
			// descendants' (`order` is filled in tree order), so backwards is
			// deepest first.
			for (const [parent, want] of [...order].reverse()) {
				let prev = null;
				for (const el of want) {
					if (el._pluginRemoved || (el._pluginMoved && el.parentNode !== parent)) continue;
					const kids = parent.childNodes;
					let ok;
					if (el.parentNode !== parent) ok = false;
					else {
						let p = null;
						for (let i = kids.indexOf(el) - 1; i >= 0; i--)
							if (kids[i]._hostKey !== undefined && kids[i]._hostParent === parent) {
								p = kids[i];
								break;
							}
						ok = p === prev;
					}
					if (!ok) {
						let ref = null;
						if (prev) {
							for (let i = kids.indexOf(prev) + 1; i < kids.length; i++)
								if (kids[i] !== el && kids[i]._hostKey !== undefined && kids[i]._hostParent === parent) {
									ref = kids[i];
									break;
								}
						} else ref = kids.find((k) => k !== el && k._hostKey !== undefined && k._hostParent === parent) ?? null;
						parent.insertBefore(el, ref);
						el._pluginMoved = false;
					}
					prev = el;
				}
			}
		} finally {
			reconciling = false;
		}
		// Where the UI drew the plugin elements scripts read the boxes of.
		for (const [id, r] of tree.node_rects ?? []) {
			const el = vnodes.get(Number(id));
			if (el && Array.isArray(r) && r.length === 4) el._rect = r.map(Number);
		}
		const after = () => {
			if (resized) dispatch(winTarget, new g.Event("resize"));
			checkGeometry();
			// Structure, class and attribute changes marked the overlay while
			// they were applied; a new viewport or colour scheme can change
			// what @media rules match.
			if (resized) markDirty();
		};
		if (forced) Promise.resolve().then(after);
		else after();
		// TIDAL announces each rendered page; DOM plugins hook their elements then.
		if (pageChanged) {
			pageChanged = false;
			const pageId = doc.querySelector("[data-track--page-id]")?.getAttribute("data-track--page-id") ?? (loc.pathname.split("/")[1] || "home");
			Promise.resolve().then(() => {
				try {
					L.hostDispatch?.("page/SET_PAGE_ID", JSON.stringify({ pageId }));
				} catch (err) {
					L.reportError(err);
				}
			});
		}
	};
	let synced = false;
	let pageChanged = false;
	D.sync = sync;

	// ---------------------------------------------------------------- input from the UI
	const eventFor = (type, data) => {
		const d = data && typeof data === "object" ? data : {};
		const noBubble = /^(mouseenter|mouseleave|pointerenter|pointerleave|focus|blur|scroll|resize|load|error)$/.test(type);
		const init = { ...d, bubbles: !noBubble, cancelable: !/^(scroll|resize|focus|blur|mouseenter|mouseleave)$/.test(type), composed: true };
		let ev;
		if (type === "wheel") ev = new WheelEvent(type, init);
		else if (/^key/.test(type)) ev = new KeyboardEvent(type, init);
		else if (/^(click|dblclick|contextmenu|auxclick|mouse)/.test(type)) ev = new MouseEvent(type, init);
		else if (/^pointer/.test(type)) ev = new g.PointerEvent(type, init);
		else if (type === "focus" || type === "blur" || type === "focusin" || type === "focusout") ev = new FocusEvent(type, init);
		else if (type === "input") ev = new InputEvent(type, init);
		// The UI's transition and animation events: `propertyName`,
		// `animationName`, `elapsedTime` as the event's own fields; they
		// bubble and cannot be cancelled, as in Chromium.
		else if (/^transition(run|start|end|cancel)$/.test(type)) ev = new g.TransitionEvent(type, { ...init, cancelable: false });
		else if (/^animation(start|iteration|end|cancel)$/.test(type)) ev = new g.AnimationEvent(type, { ...init, cancelable: false });
		else if (type === "drop" || type === "dragover" || type === "dragenter") {
			// Files dropped on the window: [{ name, type, b64 }] become File objects.
			const files = (d.files ?? []).map((f) => {
				const bin = g.atob(f.b64 ?? "");
				const bytes = new Uint8Array(bin.length);
				for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
				return new g.File([bytes], f.name ?? "", { type: f.type ?? "" });
			});
			const items = files.map((file) => ({ kind: "file", type: file.type, getAsFile: () => file }));
			init.dataTransfer = { files, items, types: files.length ? ["Files"] : [], dropEffect: "copy", effectAllowed: "all", getData: () => "", setData() {}, clearData() {} };
			ev = new g.DragEvent(type, init);
		} else ev = new g.Event(type, init);
		Object.defineProperty(ev, "isTrusted", { value: true });
		return ev;
	};
	const handleEvent = ({ target, type, data }) => {
		let el = null;
		if (target?.kind === "node") el = vnodes.get(Number(target.id)) ?? null;
		else if (target?.kind === "host") el = target.id ? (hosts.get(target.id) ?? null) : null;
		if (type === "measure") {
			// A plugin element's own box, from the UI's layout (no event).
			if (target?.kind === "node" && el && Array.isArray(data?.rect) && data.rect.length === 4) {
				el._rect = data.rect.map(Number);
				markDirty();
			}
			return;
		}
		if (type === "hover") {
			// Pointer moved onto another element: only `:hover` styles change.
			if (hovered !== el) {
				hovered = el;
				if (L.css?.usesHover?.()) markDirty();
			}
			return;
		}
		if (target?.kind === "node" && !el) return;
		if (target?.kind === "host" && target.id && !el) return;
		const d = data && typeof data === "object" ? data : {};
		// A plugin element's drawn box comes with its events: from then on
		// `getBoundingClientRect()` is its own (and its children's, until they
		// get theirs), not the anchor's.
		if (target?.kind === "node" && Array.isArray(d.rect) && d.rect.length === 4) el._rect = d.rect.map(Number);
		if (type === "fullscreenchange") {
			const on = d.fullscreen ?? d.value ?? !fsElement;
			const was = fsElement;
			fsElement = on ? (fsElement ?? html) : null;
			if (was !== fsElement) dispatch(was ?? fsElement ?? doc, eventFor(type, d));
			return;
		}
		if (type === "resize") {
			if (d.width > 0 && d.height > 0) {
				g.innerWidth = d.width;
				g.innerHeight = d.height;
			}
			dispatch(winTarget, eventFor(type, d));
			checkGeometry();
			return;
		}
		if (!el) el = /^key/.test(type) ? (focused ?? body) : body;
		if ((type === "input" || type === "change") && d.value !== undefined) {
			const was = [el.checked, el.value];
			if (d.checked !== undefined) el._checked = !!d.checked;
			el._value = String(d.value);
			if (el._hostKey !== undefined) el._hostValue = String(d.value);
			// What the UI shows of a box, a field or a choice is the
			// overlay's value, so a change it made goes out as one (and
			// `:checked` styles follow it), whatever else is or is not
			// changing the page.
			if (el.checked !== was[0] || el.value !== was[1]) markDirty();
		}
		if (type === "scroll" && d.scrollTop !== undefined) {
			el._scroll = [Number(d.scrollTop) || 0, d.scrollHeight !== undefined ? Number(d.scrollHeight) || 0 : (el._scroll?.[1] ?? 0)];
			el._scrollTop = undefined;
			// What the box holds changed, or its first report: no scroll happened.
			if (d.quiet === true) return;
		}
		if (type === "focus") {
			focused = el;
		} else if (type === "blur" && focused === el) focused = null;
		if (type === "mousedown" && !(d.button > 0)) {
			dragFrom = { el, point: textPoint(d) };
			selection.removeAllRanges();
		} else if (type === "mouseup" && !(d.button > 0) && dragFrom) {
			const from = dragFrom;
			dragFrom = null;
			selectBetween(from.el, el, d.dragged === true, from.point, textPoint(d));
		}
		if (type === "click") clickWith(el, eventFor(type, d));
		else {
			const ev = eventFor(type, d);
			dispatch(el, ev);
			// A right-click a listener cancelled, or stopped before TIDAL's
			// root heard it: TIDAL's own menu does not open.
			if (type === "contextmenu") return { cancelled: ev.defaultPrevented || (wimp.contains(el) && !ev._root) };
		}
		if (type === "scroll") checkGeometry();
	};
	D.event = handleEvent;

	// ---------------------------------------------------------------- overlay
	let vnodes = new Map();
	// A plugin window's elements are numbered apart from the app page's
	// (`L.page.vidBase`): the UI keys its per-element state by these ids.
	let vidSeq = L.page?.vidBase ?? 1;
	let lastJson = "";
	const SKIP = new Set(["style", "script", "link", "meta", "template", "head", "title", "noscript"]);
	const listenTypes = (n) => {
		const out = [];
		if (n._ls) for (const k of n._ls.keys()) out.push(k);
		if (n._on) for (const [k, v] of Object.entries(n._on)) if (v) out.push(k);
		return out;
	};
	// Types listened for on `el` and every node an event on it reaches.
	const listensUp = (el, stopAt) => {
		const set = new Set();
		for (let n = el; n; n = parentForEvents(n)) {
			for (const t of listenTypes(n)) set.add(t);
			if (n === stopAt) break;
		}
		return [...set].sort();
	};
	// The most plugin nodes the overlay carries (each is drawn every frame):
	// a plugin adding a row per event without removing old ones would make
	// every flush and frame slower until the heap fills.
	const MAX_OVERLAY_NODES = 10000;
	let nodeBudget = MAX_OVERLAY_NODES;
	let overBudget = false;
	let warnedBudget = false;
	// `var(--x)` in SVG markup takes the variables in scope at the element, so
	// a custom property set on an ancestor (star-ratings' `--star-on` per
	// widget) colours it; names nobody defines stay for the UI's theme values.
	const svgVars = (m, vars) =>
		vars?.size
			? m.replace(/var\(\s*(--[\w-]+)\s*(?:,(?:[^()]|\([^()]*\))*)?\)/g, (all, name) => {
					const v = vars.get(name);
					return v !== undefined && v.trim() !== "" ? D.resolveVars(v, vars).trim() : all;
				})
			: m;
	const buildVNode = (n, comp, anchor) => {
		if (nodeBudget <= 0) {
			overBudget = true;
			return null;
		}
		nodeBudget--;
		if (n instanceof Text) {
			if (n._data.trim() !== "") return { id: vidOf(n), tag: "#text", text: n._data };
			// White space between two siblings is the gap between words
			// that flow in a line (radiant-lyrics' word spans, a " " text
			// node apart); the UI drops it where boxes do not flow in lines,
			// as a browser does in flex and grid boxes.
			return n.previousSibling && n.nextSibling ? { id: vidOf(n), tag: "#text", text: " " } : null;
		}
		if (!(n instanceof Element) || SKIP.has(n.localName)) return null;
		const v = { id: vidOf(n), tag: n.localName };
		const attrs = [];
		for (const k of n._attrs.keys()) if (k !== "style") attrs.push([k, n.getAttribute(k)]);
		if (attrs.length) v.attrs = attrs;
		const st = comp.get(n);
		if (st) v.style = st;
		// First shown with a style newer than its first style pass: where its
		// transitions start from.
		if (n._before) {
			if (!shownBefore.has(v.id)) {
				const from = beforeOf(n._before, st);
				if (from) v.from = from;
			}
			n._before = undefined;
		}
		const ls = listensUp(n, anchor);
		if (ls.length) v.listens = ls;
		if (n.localName === "svg") {
			let m = serialize(n);
			if (!/^<svg[^>]*\sxmlns=/.test(m)) m = m.replace(/^<svg/, `<svg xmlns="${SVG_NS}"`);
			if (m.includes("var(")) m = svgVars(m, n._cc?.myVars);
			v.markup = m.includes("<use") ? inlineUses(m) : m;
			return v;
		}
		if (n.localName === "img") {
			const src = n.getAttribute("src");
			if (src) v.markup = n.src;
			if (n._nw > 0 && n._nh > 0) v.natural = [n._nw, n._nh];
		}
		if (n.localName === "canvas") {
			const key = L.media?.canvasKey(n);
			if (key != null) v.markup = `canvas:${key}`;
		}
		if (n.localName === "video") {
			const key = L.media?.videoKey?.(n);
			if (key != null) v.markup = `video:${key}`;
		}
		if (/^(input|textarea|select)$/.test(n.localName)) {
			const t = (n.getAttribute("type") ?? "").toLowerCase();
			v.value = t === "checkbox" || t === "radio" ? (n.checked ? "true" : "false") : n.value;
			if (t === "range" && L.css?.rangeParts) {
				const parts = L.css.rangeParts(n);
				if (parts?.track) v.track = parts.track;
				if (parts?.thumb) v.thumb = parts.thumb;
			}
		}
		const kids = [];
		for (const c of n.childNodes) {
			const k = buildVNode(c, comp, anchor);
			if (k) kids.push(k);
		}
		// Its own `::before` and `::after` boxes, as its first and last child
		// (ids of their own, kept with the element; events go to it).
		const boxes = curBoxes?.get(n);
		if (boxes) {
			n._pv ??= { before: vidSeq++, after: vidSeq++, beforeText: vidSeq++, afterText: vidSeq++ };
			for (const kind of ["before", "after"]) {
				const b = boxes[kind];
				if (!b) continue;
				vnodes.set(n._pv[kind], n);
				const p = { id: n._pv[kind], tag: `::${kind}`, style: b.style };
				if (b.text) p.children = [{ id: n._pv[`${kind}Text`], tag: "#text", text: b.text }];
				if (kind === "before") kids.unshift(p);
				else kids.push(p);
			}
		}
		if (kids.length) v.children = kids;
		return v;
	};
	let curBoxes = null;
	const vidOf = (n) => {
		n._vid ??= vidSeq++;
		vnodes.set(n._vid, n);
		return n._vid;
	};
	const isHost = (n) => n._hostKey !== undefined;
	const build = () => {
		// Counted for the tests that check what a change builds again.
		D.overlayBuilds = (D.overlayBuilds ?? 0) + 1;
		// Elements this build shows for the first time get their first style
		// here: no earlier pass to start a transition from.
		fresh.clear();
		shownBefore = vnodes;
		vnodes = new Map();
		nodeBudget = MAX_OVERLAY_NODES;
		overBudget = false;
		const t0 = L.stat ? Date.now() : 0;
		const css = L.css ? L.css.compute() : { styles: new Map(), vars: {}, page: {}, fonts: [], bytes: 0 };
		if (L.stat) L.stat("overlay css", t0);
		const comp = css.styles;
		curBoxes = css.boxes ?? null;
		// What CSS hides with an element, as a browser does: `display: none`
		// takes the element and everything in it out of the page (element-
		// hider's "Hide This Element" on a sidebar or a header), and
		// `visibility` is inherited, so the nearest element that sets it
		// decides (`visible` on a child shows it again).
		const goneMemo = new Map();
		const goneAt = (el) => {
			if (!(el instanceof Element)) return false;
			let v = goneMemo.get(el);
			if (v === undefined) {
				v = /^none$/i.test(String(comp.get(el)?.display ?? "").trim()) || goneAt(el.parentElement);
				goneMemo.set(el, v);
			}
			return v;
		};
		const ownVisibility = (el) => {
			const v = String(comp.get(el)?.visibility ?? "")
				.trim()
				.toLowerCase();
			if (v === "" || v === "inherit" || v === "unset") return undefined;
			return v === "hidden" || v === "collapse";
		};
		const invisMemo = new Map();
		const invisibleAt = (el) => {
			if (!(el instanceof Element)) return false;
			let v = invisMemo.get(el);
			if (v === undefined) {
				v = ownVisibility(el) ?? invisibleAt(el.parentElement);
				invisMemo.set(el, v);
			}
			return v;
		};
		// The style an element in `parent` is drawn with when CSS hides it
		// through `parent`, else its own.
		const hiddenBy = (parent, el, style) => {
			if (goneAt(parent)) return { ...(style ?? {}), display: "none" };
			if (invisibleAt(parent) && (!(el instanceof Element) || ownVisibility(el) === undefined)) return { ...(style ?? {}), visibility: "hidden" };
			return style;
		};
		const hostsOut = {};
		const injected = [];
		const push = (anchor, position, node) => {
			const last = injected.at(-1);
			if (last && last.anchor === anchor && last.position === position) last.nodes.push(node);
			else injected.push({ anchor, position, nodes: [node] });
		};
		// Plugin nodes, grouped by the host node they sit in or next to.
		const scan = (parent, anchorKey) => {
			const kids = parent.childNodes;
			const hostIdx = [];
			kids.forEach((k, i) => {
				if (isHost(k) || k === parent._hostText) hostIdx.push(i);
			});
			kids.forEach((k, i) => {
				if (isHost(k)) return scan(k, k._hostKey);
				if (k === parent._hostText) return;
				// Text a plugin wrote straight into a host element is the
				// element's own text (`text` below), not an element beside
				// it: custom-fonts' `title.innerHTML += ...` showed the
				// title twice.
				if (k instanceof Text && isHost(parent) && parent._textTouched) return;
				if (k._sprite || (k instanceof Element && SKIP.has(k.localName))) return;
				if (k === wimp || k === body || k === html) return scan(k, "");
				const before = hostIdx.filter((j) => j < i);
				const after = hostIdx.filter((j) => j > i);
				let anchor = anchorKey,
					pos = "append";
				const prevHost = before.length ? kids[before.at(-1)] : null;
				if (!hostIdx.length) pos = "append";
				else if (!before.length) pos = "prepend";
				else if (!after.length) pos = "append";
				else if (prevHost && isHost(prevHost)) {
					anchor = prevHost._hostKey;
					pos = "after";
				} else pos = "prepend";
				if (anchorKey === "" && (prevHost || after.length) && parent === wimp) {
					const h = prevHost && isHost(prevHost) ? prevHost : kids[after[0]];
					if (h && isHost(h)) {
						anchor = h._hostKey;
						pos = prevHost && isHost(prevHost) ? "after" : "before";
					}
				}
				if (anchor === undefined || anchor === null) return;
				const v = buildVNode(k, comp, isHost(parent) ? parent : null);
				if (!v) return;
				// Inside an app element CSS hides, a plugin's element is
				// hidden with it.
				const style = hiddenBy(parent, k, v.style);
				if (style !== v.style) v.style = style;
				push(anchor, pos, v);
			});
		};
		// A plugin window's page is drawn whole, from its root element, as
		// a browser draws a document; the app page's plugin elements sit at
		// the anchors around them.
		const ts = L.stat ? Date.now() : 0;
		if (L.page) {
			const v = buildVNode(html, comp, null);
			if (v) push("", "append", v);
		} else scan(html, "");
		if (L.stat) L.stat("overlay scan", ts);
		const th = L.stat ? Date.now() : 0;
		for (const [key, el] of hosts) {
			// What the entry is made of. While all of it is as it was (the
			// clock's second rewrites the tree every second, and the progress
			// bar's inline style with it), the entry made last time is the
			// entry: making it again for every app element cost an overlay
			// build most of its time with nothing changed.
			const st = comp.get(el);
			const ps = css.pseudo?.get(el);
			const sel = !!css.selectable?.has(el);
			const gone = !!el._pluginRemoved || (!el.isConnected && !el._pluginMoved);
			const up = gone ? null : el.parentElement;
			const hidden = up ? goneAt(up) : false;
			const invisible = up ? invisibleAt(up) : false;
			const hostClass = el._hostAttrs.get("class") ?? "";
			const curClass = el._attrs.get("class") ?? "";
			const memo = el._ovMemo;
			if (
				memo &&
				!el._pluginAttrs?.size &&
				!el._textTouched &&
				memo.st === st &&
				memo.ps === ps &&
				memo.sel === sel &&
				memo.gone === gone &&
				memo.up === up &&
				memo.hidden === hidden &&
				memo.invisible === invisible &&
				memo.hostClass === hostClass &&
				memo.curClass === curClass &&
				memo.listens === listenVersion &&
				memo.tree === treeGen
			) {
				if (memo.keep) hostsOut[key] = memo.o;
				continue;
			}
			D.hostBuilds = (D.hostBuilds ?? 0) + 1;
			const o = {};
			if (st) o.style = { ...st };
			// TIDAL's own panel glass is drawn natively: the host says only
			// that a theme's rules left it in place.
			const glass = L.css?.panelGlass;
			if (glass && o.style && String(o.style["background-image"] ?? "").replace(/\s+/g, " ").trim() === glass) {
				delete o.style["background-image"];
				o.glass = true;
				if (!Object.keys(o.style).length) delete o.style;
			}
			// So is its own border while a theme leaves it as it is.
			const line = L.css?.panelBorder;
			if (line && o.style && String(o.style.border ?? "").replace(/\s+/g, " ").trim() === line && /(^|\s)_container_b3e8f28(\s|$)/.test(el.getAttribute("class") ?? "")) {
				delete o.style.border;
				if (!Object.keys(o.style).length) delete o.style;
			}
			if (ps?.before) o.before = ps.before;
			if (ps?.after) o.after = ps.after;
			if (ps?.selection) o.selection = ps.selection;
			if (sel) o.selectable = true;
			if (gone) o.style = { ...(o.style ?? {}), display: "none" };
			else {
				// An app element inside one CSS hides is hidden with it.
				const style = hiddenBy(up, el, o.style);
				if (style !== o.style) o.style = style;
			}
			const hostCls = hostClass.split(/\s+/);
			const curCls = curClass.split(/\s+/);
			const added = curCls.filter((c) => c && !hostCls.includes(c));
			if (added.length) o.classes = added;
			// TIDAL's own classes a plugin took away (AltPlay moves the
			// sidebar's `_selected_` to its own link).
			const removed = hostCls.filter((c) => c && !curCls.includes(c));
			if (removed.length) o.removed = removed;
			if (el._pluginAttrs?.size) {
				const at = [];
				for (const k of el._pluginAttrs) if (el._attrs.has(k) && el._attrs.get(k) !== el._hostAttrs.get(k)) at.push([k, el._attrs.get(k)]);
				if (at.length) o.attrs = at;
			}
			if (el._textTouched) {
				let own = "";
				for (const c of el.childNodes) if (c instanceof Text) own += c._data;
				if (own !== (el._hostTextVal ?? "")) o.text = own;
			}
			const ls = listensUp(el);
			if (ls.length) o.listens = ls;
			const keep = Object.keys(o).length > 0;
			el._ovMemo = { st, ps, sel, gone, up, hidden, invisible, hostClass, curClass, listens: listenVersion, tree: treeGen, keep, o };
			if (keep) hostsOut[key] = o;
		}
		if (L.stat) L.stat("overlay hosts", th);
		const docLs = [...new Set([...listenTypes(doc), ...listenTypes(winTarget), ...listenTypes(body), ...listenTypes(html)])].sort();
		if (docLs.length) hostsOut[""] = { listens: docLs };
		const ov = { vars: css.vars, page: css.page, hosts: hostsOut, injected, fonts: [...css.fonts, ...fontFaces()], scroll_to: scrollTo, css_bytes: css.bytes };
		if (css.wimpFill) ov.wimp_fill = css.wimpFill;
		if (scrollTop) ov.scroll_top = scrollTop;
		const ns = D.nodeScrolls();
		if (ns.length) ov.node_scrolls = ns;
		if (hostClicks.length) ov.clicks = hostClicks.slice();
		if (css.keyframes && Object.keys(css.keyframes).length) ov.keyframes = css.keyframes;
		if (css.hover) ov.hover = true;
		const measure = [];
		for (const el of wantRects) {
			if (!el.isConnected || el._pluginRemoved) wantRects.delete(el);
			else if (el._vid !== undefined && vnodes.get(el._vid) === el) measure.push(el._vid);
		}
		if (measure.length) ov.measure = measure.sort((a, b) => a - b);
		if (overBudget && !warnedBudget) L.callOp("log", { plugin: null, level: "warn", msg: `Plugins added more than ${MAX_OVERLAY_NODES} elements to the page; the rest are not shown` });
		warnedBudget = overBudget;
		return ov;
	};
	D.build = build;
	D.flush = () => {
		flushQueued = false;
		if (!dirty) return;
		// A plugin window's page waits for the stylesheets its parser met
		// (page.js), as Chromium does not draw a page before them.
		if (D.holdPaint?.()) return;
		dirty = false;
		layoutDirty = false;
		lastFlush = Date.now();
		checkGeometry();
		let ov;
		try {
			ov = build();
		} catch (err) {
			L.reportError(err);
			return;
		}
		const t0 = L.stat ? Date.now() : 0;
		const json = JSON.stringify(ov);
		if (L.stat) L.stat("overlay json", t0);
		if (json === lastJson) return;
		lastJson = json;
		const t1 = L.stat ? Date.now() : 0;
		try {
			if (L.callOp("dom.overlay", ov) === true) boxesPending = true;
		} catch (err) {
			L.reportError(err);
		}
		if (L.stat) L.stat(`overlay sent (~${Math.round(json.length / 1000)} kB)`, t1);
	};
	if (L.stat) {
		const flush = D.flush;
		D.flush = () => {
			const t0 = Date.now();
			const was = dirty;
			flush();
			if (was) L.stat("overlay build", t0);
		};
	}
	g.__luna_dom = (kind, json) => {
		if (L.stat) {
			const t0 = Date.now();
			try {
				return domCall(kind, json);
			} finally {
				L.stat(`dom ${kind === "event" ? `event ${JSON.parse(json).kind}` : kind}`, t0);
			}
		}
		return domCall(kind, json);
	};
	const domCall = (kind, json) => {
		const arg = json ? JSON.parse(json) : {};
		switch (kind) {
			case "sync":
				return void sync(arg);
			case "event":
				D.wakeFrames();
				return handleEvent(arg) ?? null;
			case "flush":
				dirty = true;
				return void D.flush();
			case "stopped":
				return void stopped(arg.name ?? null);
			default:
				throw new Error(`Unknown DOM call ${kind}`);
		}
	};

	// ---------------------------------------------------------------- media queries
	const evalMedia = (q) => {
		const s = String(q ?? "").trim().toLowerCase();
		if (!s || s === "all" || s === "screen") return true;
		return s.split(",").some((part) => {
			let p = part.trim();
			let neg = false;
			if (p.startsWith("not ")) {
				neg = true;
				p = p.slice(4);
			}
			p = p.replace(/^only\s+/, "");
			const ok = p.split(/\s+and\s+/).every((c) => {
				c = c.trim();
				if (c === "screen" || c === "all") return true;
				if (c === "print" || c === "speech") return false;
				const m = /^\(\s*([\w-]+)\s*(?::\s*([^)]*))?\)$/.exec(c);
				if (!m) return false;
				const [, f, v0] = m;
				const v = (v0 ?? "").trim();
				const px = (x) => (x.endsWith("em") ? parseFloat(x) * 16 : parseFloat(x));
				switch (f) {
					case "prefers-color-scheme":
						return v === D.colorScheme;
					// The desktop's wish for less motion, as the app follows it.
					case "prefers-reduced-motion":
						return v === "" ? !!D.reducedMotion : v === (D.reducedMotion ? "reduce" : "no-preference");
					case "prefers-contrast":
					case "forced-colors":
						return v === "no-preference" || v === "none";
					case "min-width":
						return g.innerWidth >= px(v);
					case "max-width":
						return g.innerWidth <= px(v);
					case "min-height":
						return g.innerHeight >= px(v);
					case "max-height":
						return g.innerHeight <= px(v);
					case "width":
						return g.innerWidth === px(v);
					case "orientation":
						return v === (g.innerWidth >= g.innerHeight ? "landscape" : "portrait");
					case "hover":
					case "any-hover":
						return v === "" || v === "hover";
					case "pointer":
					case "any-pointer":
						return v === "" || v === "fine";
					case "display-mode":
						return v === "browser";
					case "min-resolution":
					case "-webkit-min-device-pixel-ratio":
						return parseFloat(v) <= 1;
					default:
						return false;
				}
			});
			return neg ? !ok : ok;
		});
	};
	D.evalMedia = evalMedia;
	D.colorScheme = "dark";
	D.reducedMotion = false;
	g.matchMedia = (q) => {
		const t = new Target();
		return Object.assign(t, { media: String(q), get matches() { return evalMedia(q); }, onchange: null, addListener() {}, removeListener() {} });
	};
	g.CSS = {
		supports: (a, b) => (b === undefined ? !/^\s*not\b/i.test(String(a)) : true),
		escape: (s) =>
			String(s).replace(/([\0-\x1f\x7f]|^-?\d)|^-$|[^\w-\u0080-￿]/g, (ch, ctl) => {
				if (ch === "\0") return "�";
				if (ctl) return `\\${ch.charCodeAt(ch.length - 1).toString(16)} `.replace(/^\\2d /, "-").replace(/^-(\d)/, (m, d) => `-\\${d.charCodeAt(0).toString(16)} `);
				return `\\${ch}`;
			}),
		registerProperty() {},
		px: (n) => `${n}px`,
	};

	// ---------------------------------------------------------------- tree walking
	g.NodeFilter = {
		FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3,
		SHOW_ALL: 0xffffffff, SHOW_ELEMENT: 0x1, SHOW_ATTRIBUTE: 0x2, SHOW_TEXT: 0x4, SHOW_CDATA_SECTION: 0x8, SHOW_PROCESSING_INSTRUCTION: 0x40, SHOW_COMMENT: 0x80, SHOW_DOCUMENT: 0x100, SHOW_DOCUMENT_TYPE: 0x200, SHOW_DOCUMENT_FRAGMENT: 0x400,
	};
	const shows = (what, n) => (what & (n.nodeType === 3 ? 0x4 : n.nodeType === 1 ? 0x1 : n.nodeType === 9 ? 0x100 : n.nodeType === 11 ? 0x400 : 0x80)) !== 0;
	class TreeWalker {
		constructor(root, what = 0xffffffff, filter = null) {
			touch();
			this.root = root;
			this.whatToShow = what;
			this.filter = filter;
			this.currentNode = root;
			this._list = null;
		}
		_nodes() {
			if (!this._list) {
				const out = [];
				const walk = (n) => {
					for (const c of n.childNodes ?? []) {
						let r = shows(this.whatToShow, c) ? 1 : 3;
						if (r === 1 && this.filter) r = typeof this.filter === "function" ? this.filter(c) : this.filter.acceptNode(c);
						if (r === 1) out.push(c);
						if (r !== 2) walk(c);
					}
				};
				walk(this.root);
				this._list = out;
			}
			return this._list;
		}
		nextNode() {
			const l = this._nodes();
			const n = l[l.indexOf(this.currentNode) + 1] ?? null;
			if (n) this.currentNode = n;
			return n;
		}
		previousNode() {
			const l = this._nodes();
			const i = l.indexOf(this.currentNode);
			const n = i > 0 ? l[i - 1] : null;
			if (n) this.currentNode = n;
			return n;
		}
		firstChild() {
			const n = this._nodes().find((x) => x.parentNode === this.currentNode) ?? null;
			if (n) this.currentNode = n;
			return n;
		}
		lastChild() {
			const n = this._nodes().findLast((x) => x.parentNode === this.currentNode) ?? null;
			if (n) this.currentNode = n;
			return n;
		}
		parentNode() {
			const p = this.currentNode === this.root ? null : this.currentNode.parentNode;
			if (p) this.currentNode = p;
			return p;
		}
		nextSibling() {
			const n = this.currentNode.nextSibling;
			if (n) this.currentNode = n;
			return n;
		}
		previousSibling() {
			const n = this.currentNode.previousSibling;
			if (n) this.currentNode = n;
			return n;
		}
	}
	g.TreeWalker = TreeWalker;
	Document.prototype.createTreeWalker = (root, what, filter) => new TreeWalker(root, what, filter);
	Document.prototype.createNodeIterator = (root, what, filter) => {
		const w = new TreeWalker(root, what, filter);
		const l = () => w._nodes();
		let i = -1;
		return { nextNode: () => l()[++i] ?? null, previousNode: () => (i > 0 ? l()[--i] : null), detach() {}, root };
	};
	// A boundary point (node, offset) as the child indices down to its node,
	// then the offset: two points compare lexicographically, a prefix first.
	const pointPath = (n, o) => {
		const p = [o];
		for (let c = n; c.parentNode; c = c.parentNode) p.unshift(Array.prototype.indexOf.call(c.parentNode.childNodes, c));
		return p;
	};
	const cmpPath = (a, b) => {
		for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
		return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
	};
	const nodeLen = (n) => (n.nodeType === 3 || n.nodeType === 8 ? n._data.length : n.childNodes.length);
	const textNodes = (root, out = []) => {
		if (root.nodeType === 3) out.push(root);
		else for (const c of root.childNodes) textNodes(c, out);
		return out;
	};
	class Range {
		constructor() {
			this.startContainer = this.endContainer = doc;
			this.startOffset = this.endOffset = 0;
		}
		get commonAncestorContainer() {
			let a = this.startContainer;
			while (a && !a.contains(this.endContainer)) a = a.parentNode;
			return a ?? doc;
		}
		get collapsed() {
			return this.startContainer === this.endContainer && this.startOffset === this.endOffset;
		}
		_start() {
			return pointPath(this.startContainer, this.startOffset);
		}
		_end() {
			return pointPath(this.endContainer, this.endOffset);
		}
		setStart(n, o) {
			this.startContainer = n;
			this.startOffset = o;
			if (cmpPath(this._start(), this._end()) > 0) {
				this.endContainer = n;
				this.endOffset = o;
			}
		}
		setEnd(n, o) {
			this.endContainer = n;
			this.endOffset = o;
			if (cmpPath(this._start(), this._end()) > 0) {
				this.startContainer = n;
				this.startOffset = o;
			}
		}
		setStartBefore(n) {
			this.setStart(n.parentNode, Array.prototype.indexOf.call(n.parentNode.childNodes, n));
		}
		setStartAfter(n) {
			this.setStart(n.parentNode, Array.prototype.indexOf.call(n.parentNode.childNodes, n) + 1);
		}
		setEndBefore(n) {
			this.setEnd(n.parentNode, Array.prototype.indexOf.call(n.parentNode.childNodes, n));
		}
		setEndAfter(n) {
			this.setEnd(n.parentNode, Array.prototype.indexOf.call(n.parentNode.childNodes, n) + 1);
		}
		selectNode(n) {
			const parent = n.parentNode;
			if (!parent) return this.selectNodeContents(n);
			const i = Array.prototype.indexOf.call(parent.childNodes, n);
			this.startContainer = this.endContainer = parent;
			this.startOffset = i;
			this.endOffset = i + 1;
		}
		selectNodeContents(n) {
			this.startContainer = this.endContainer = n;
			this.startOffset = 0;
			this.endOffset = nodeLen(n);
		}
		collapse(toStart) {
			if (toStart) {
				this.endContainer = this.startContainer;
				this.endOffset = this.startOffset;
			} else {
				this.startContainer = this.endContainer;
				this.startOffset = this.endOffset;
			}
		}
		cloneRange() {
			const r = new Range();
			Object.assign(r, { startContainer: this.startContainer, startOffset: this.startOffset, endContainer: this.endContainer, endOffset: this.endOffset });
			return r;
		}
		intersectsNode(n) {
			const parent = n.parentNode;
			if (!parent) return true;
			const i = Array.prototype.indexOf.call(parent.childNodes, n);
			return cmpPath(this._end(), pointPath(parent, i)) > 0 && cmpPath(this._start(), pointPath(parent, i + 1)) < 0;
		}
		getBoundingClientRect() {
			return this.startContainer instanceof Element ? this.startContainer.getBoundingClientRect() : rectObj(null);
		}
		getClientRects() {
			return [];
		}
		createContextualFragment(h) {
			const f = new DocumentFragment();
			for (const n of parseHTML(String(h ?? ""), false, "div")) f.appendChild(n);
			return f;
		}
		cloneContents() {
			return new DocumentFragment();
		}
		deleteContents() {}
		insertNode(n) {
			this.startContainer.insertBefore?.(n, this.startContainer.childNodes?.[this.startOffset] ?? null);
		}
		surroundContents() {}
		detach() {}
		// The text between the two points: the text nodes in between, cut at
		// the ends.
		toString() {
			const s = this._start();
			const e = this._end();
			if (cmpPath(s, e) >= 0) return "";
			let out = "";
			for (const t of textNodes(this.commonAncestorContainer)) {
				const base = pointPath(t, 0).slice(0, -1);
				const len = t._data.length;
				const lo = t === this.startContainer ? this.startOffset : cmpPath(s, [...base, 0]) <= 0 ? 0 : -1;
				const hi = t === this.endContainer ? this.endOffset : cmpPath([...base, len], e) <= 0 ? len : -1;
				if (lo >= 0 && hi > lo) out += t._data.slice(lo, hi);
			}
			return out;
		}
	}
	g.Range = Range;
	// `window.getSelection()`: one range (as in Chrome), or the contents of
	// the input a plugin called `select()` on (the copy-to-clipboard trick).
	const selection = {
		_ranges: [],
		_control: null,
		get rangeCount() {
			return this._ranges.length;
		},
		get isCollapsed() {
			return !this._control && this._ranges.every((r) => r.collapsed);
		},
		get type() {
			return this._control || this._ranges.some((r) => !r.collapsed) ? "Range" : this._ranges.length ? "Caret" : "None";
		},
		get anchorNode() {
			return this._ranges[0]?.startContainer ?? null;
		},
		get anchorOffset() {
			return this._ranges[0]?.startOffset ?? 0;
		},
		get focusNode() {
			return this._ranges[0]?.endContainer ?? null;
		},
		get focusOffset() {
			return this._ranges[0]?.endOffset ?? 0;
		},
		getRangeAt(i) {
			const r = this._ranges[i];
			if (!r) throw new g.DOMException("The index is not in the allowed range.", "IndexSizeError");
			return r;
		},
		addRange(r) {
			this._control = null;
			if (this._ranges.length === 0) this._ranges = [r];
		},
		removeRange(r) {
			this._ranges = this._ranges.filter((x) => x !== r);
		},
		removeAllRanges() {
			this._ranges = [];
			this._control = null;
		},
		empty() {
			this.removeAllRanges();
		},
		collapse(n, o = 0) {
			this._control = null;
			if (!n) return this.removeAllRanges();
			const r = new Range();
			r.setStart(n, o);
			r.setEnd(n, o);
			this._ranges = [r];
		},
		setPosition(n, o) {
			this.collapse(n, o);
		},
		collapseToStart() {
			this._ranges[0]?.collapse(true);
		},
		collapseToEnd() {
			this._ranges[0]?.collapse(false);
		},
		selectAllChildren(n) {
			this._control = null;
			const r = new Range();
			r.selectNodeContents(n);
			this._ranges = [r];
		},
		setBaseAndExtent(a, ao, f, fo) {
			this._control = null;
			const r = new Range();
			r.setStart(a, ao);
			r.setEnd(f, fo);
			this._ranges = [r];
		},
		containsNode(n, partial) {
			const parent = n.parentNode;
			if (!parent) return false;
			const i = Array.prototype.indexOf.call(parent.childNodes, n);
			return this._ranges.some((r) => (partial ? r.intersectsNode(n) : cmpPath(r._start(), pointPath(parent, i)) <= 0 && cmpPath(pointPath(parent, i + 1), r._end()) <= 0));
		},
		modify() {},
		toString() {
			if (this._control) return String(this._control.value ?? this._control._value ?? "");
			return this._ranges.map((r) => r.toString()).join("");
		},
	};
	let dragFrom = null;
	// Where in a host's text the UI measured the pointer (a character
	// offset, UTF-16 as in the DOM): `{ el, offset }`, or null.
	const textPoint = (d) => {
		const p = d?.textPoint;
		const el = p && typeof p.key === "string" ? hosts.get(p.key) : null;
		return el && Number.isFinite(p.offset) ? { el, offset: Math.max(0, Math.floor(p.offset)) } : null;
	};
	// The text node and offset in it `offset` characters into `el`'s text.
	const textAt = (el, offset) => {
		const ts = textNodes(el);
		for (const t of ts) {
			if (offset <= t._data.length) return [t, offset];
			offset -= t._data.length;
		}
		const last = ts.at(-1);
		return last ? [last, last._data.length] : null;
	};
	// A press on one element and a release on another select the text from
	// one to the other, whole elements (the UI draws lyrics as one piece, so
	// no character positions come with a press): what a drag over text does
	// in a browser, for elements that allow it (`user-select`).
	const selectBetween = (a, b, dragged, pa, pb) => {
		// The UI measured both ends in characters (lyric lines): the
		// characters between them, as a browser selects them.
		if (pa && pb) {
			a = pa.el;
			b = pb.el;
		} else if (a === b && !dragged) return;
		const allowed = (el) => {
			try {
				return String(L.css?.computedStyle(el)?.getPropertyValue("user-select") ?? "").trim() !== "none";
			} catch {
				return true;
			}
		};
		if (!allowed(a) || !allowed(b)) return;
		if (pa && pb) {
			let s = textAt(pa.el, pa.offset);
			let e = textAt(pb.el, pb.offset);
			if (!s || !e) return;
			if (cmpPath(pointPath(...s), pointPath(...e)) > 0) [s, e] = [e, s];
			const r = new Range();
			r.setStart(...s);
			r.setEnd(...e);
			selection._control = null;
			selection._ranges = r.collapsed ? [] : [r];
			return;
		}
		if (a !== b && a.compareDocumentPosition(b) & 2) [a, b] = [b, a];
		const ta = textNodes(a);
		const tb = textNodes(b);
		if (ta.length === 0 || tb.length === 0) return;
		const r = new Range();
		r.setStart(ta[0], 0);
		r.setEnd(tb[tb.length - 1], tb[tb.length - 1]._data.length);
		selection._control = null;
		selection._ranges = [r];
	};
	Document.prototype.createRange = () => new Range();

	// ---------------------------------------------------------------- fonts
	class FontFace {
		constructor(family, source, desc = {}) {
			Object.assign(this, { family: String(family), source, status: "loaded", style: "normal", weight: "normal", display: "auto", ...desc });
			this.loaded = Promise.resolve(this);
		}
		load() {
			return Promise.resolve(this);
		}
	}
	g.FontFace = FontFace;
	const fontSet = new Set();
	const fontFaces = () => {
		const out = [];
		for (const f of fontSet) {
			const src = typeof f.source === "string" ? f.source : null;
			if (src) {
				const weight = String(f.weight ?? "").trim();
				const face = { family: f.family.replace(/^["']|["']$/g, ""), src };
				if (weight && weight !== "normal") face.weight = weight;
				out.push(face);
			}
		}
		return out;
	};
	const fonts = {
		status: "loaded",
		add: (f) => (fontSet.add(f), markDirty(), fonts),
		delete: (f) => (markDirty(), fontSet.delete(f)),
		clear: () => (markDirty(), fontSet.clear()),
		has: (f) => fontSet.has(f),
		check: () => true,
		load: async () => [],
		forEach: (cb) => fontSet.forEach((f) => cb(f, f, fonts)),
		values: () => fontSet.values(),
		[Symbol.iterator]: () => fontSet.values(),
		get size() {
			return fontSet.size;
		},
		addEventListener() {},
		removeEventListener() {},
		onloadingdone: null,
	};
	fonts.ready = Promise.resolve(fonts);
	Object.defineProperty(Document.prototype, "fonts", { get: () => fonts, configurable: true });
	g.queryLocalFonts = async () => [];

	// ---------------------------------------------------------------- storage
	// localStorage: one persisted store shared by all plugins, like TIDAL's origin.
	const lsGet = () => L.callOp("storage.get", { store: "@luna/localStorage", key: "data" }) ?? {};
	// A full store throws what Chromium throws: a DOMException named
	// QuotaExceededError (code 22), which plugins check for to make room.
	const lsPut = (d) => {
		try {
			return L.callOp("storage.set", { store: "@luna/localStorage", key: "data", value: d });
		} catch (err) {
			if (/^QuotaExceededError\b/.test(String(err?.message))) throw new g.DOMException(err.message, "QuotaExceededError");
			throw err;
		}
	};
	g.localStorage = {
		getItem: (k) => lsGet()[k] ?? null,
		setItem: (k, v) => {
			const d = lsGet();
			d[k] = String(v);
			lsPut(d);
		},
		removeItem: (k) => {
			const d = lsGet();
			delete d[k];
			lsPut(d);
		},
		clear: () => lsPut({}),
		key: (i) => Object.keys(lsGet())[i] ?? null,
		get length() {
			return Object.keys(lsGet()).length;
		},
	};
	const ss = new Map();
	g.sessionStorage = {
		getItem: (k) => ss.get(k) ?? null,
		setItem: (k, v) => ss.set(k, String(v)),
		removeItem: (k) => ss.delete(k),
		clear: () => ss.clear(),
		key: (i) => [...ss.keys()][i] ?? null,
		get length() {
			return ss.size;
		},
	};

	// <canvas>, Path2D, ImageData, Image loading and createImageBitmap are
	// in media.js.
	g.DOMParser = class DOMParser {
		parseFromString(text, type) {
			touch();
			const d = new Document();
			const svg = /svg|xml/.test(String(type ?? ""));
			const h = createEl("html");
			const b = createEl("body");
			d.childNodes.push(h);
			h.parentNode = d;
			h.childNodes.push(createEl("head"), b);
			for (const c of h.childNodes) c.parentNode = h;
			const nodes = parseHTML(String(text ?? ""), svg, "div");
			if (svg && nodes.find((n) => n instanceof Element)) {
				d.childNodes = [nodes.find((n) => n instanceof Element)];
				d.childNodes[0].parentNode = d;
				return d;
			}
			for (const n of nodes) {
				b.childNodes.push(n);
				n.parentNode = b;
			}
			return d;
		}
	};
	g.XMLSerializer = class XMLSerializer {
		serializeToString(n) {
			return serialize(n);
		}
	};
	g.requestIdleCallback = (cb) => setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 10 }), 1);
	g.cancelIdleCallback = (id) => clearTimeout(id);
	g.customElements = { define() {}, get: () => undefined, whenDefined: () => Promise.resolve(), upgrade() {} };
	g.getComputedStyle = (el, pseudo) => {
		// Asking for a style runs the style pass a frame would.
		if (fresh.size) D.styleTick();
		return L.css ? L.css.computedStyle(el, pseudo) : declProxy(new CSSStyleDeclaration());
	};
})();
