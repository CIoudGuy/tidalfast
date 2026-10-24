// Plugin record (development only, demo builds): what one plugin does to
// the page while it runs. The same script runs in two places so the two
// records compare: in TIDAL's page in a browser (the plugin's own module URL
// in the call stack says which calls are its own) and in this runtime (the
// engine's frames say it, as for permissions).
//
// Recorded: the DOM changes the plugin's own calls make (each mutating DOM
// call it makes is followed by MutationObserver.takeRecords(), so a change
// counts however the call made it), the redux actions it dispatches, the
// requests it starts (fetch, XMLHttpRequest), what it logs as an error or
// warning, and every second the elements it put in the page that are still
// there, with their computed style. What the page does in reply to the
// plugin (a click it sends, an action it dispatches) is not the plugin's
// own change and is left out.
//
// __plugrecInstall({ owner, write, hostFlag, blockWrites }) -> recorder:
//   owner()      true when the plugin's code made the current call
//   write(json)  optional: the record is handed to it every second when it changed
//   hostFlag     records carry `_host` when the host tree made them (this runtime)
//   blockWrites  a request other than GET/HEAD/OPTIONS is refused (and recorded)
(function (g) {
	const CS_KEYS = ["display", "visibility", "position", "color", "background-color", "background-image", "font-size", "font-weight", "font-family", "border-radius", "opacity", "width", "height", "cursor", "z-index", "fill"];
	const MAX_EFFECTS = 3000;
	const clip = (s, n) => {
		s = String(s ?? "");
		return s.length > n ? `${s.slice(0, n)}…` : s;
	};
	const squash = (s, n) => clip(String(s ?? "").replace(/\s+/g, " ").trim(), n);
	g.__plugrecInstall = (opts) => {
		if (g.__plugrec) return g.__plugrec;
		const owner = opts.owner;
		const t0 = Date.now();
		const effects = new Map();
		const actions = [];
		const requests = [];
		const errors = [];
		const snaps = [];
		const own = new WeakSet();
		let busy = 0;
		let secondary = 0;
		let missed = 0;
		let dirty = true;
		let lastSnap = "";
		const isEl = (n) => n && n.nodeType === 1;
		const desc = (el) => {
			if (!isEl(el)) return el?.nodeType === 3 ? "#text" : el?.nodeType === 9 ? "#document" : String(el?.nodeName ?? el);
			let s = String(el.localName ?? el.tagName ?? "").toLowerCase();
			const id = el.getAttribute("id");
			if (id) s += `#${id}`;
			const dt = el.getAttribute("data-test");
			if (dt) s += `[data-test="${dt}"]`;
			const cls = String(el.getAttribute("class") ?? "").trim();
			if (cls) s += `.${cls.split(/\s+/).slice(0, 6).join(".")}`;
			return s;
		};
		// The element and up to three ancestors, stopping at one with an id or data-test.
		const sig = (el) => {
			if (!isEl(el)) return desc(el);
			const parts = [];
			for (let c = el, i = 0; isEl(c) && i < 4; c = c.parentElement ?? c.parentNode, i++) {
				parts.unshift(desc(c));
				if (c.getAttribute("id") || c.getAttribute("data-test")) break;
			}
			return parts.join(" > ");
		};
		const hostKey = (el) => (el && el._hostKey !== undefined ? el._hostKey : undefined);
		const effect = (e) => {
			const key = [e.k, e.target, e.name ?? "", e.value ?? ""].join("\u0001");
			const have = effects.get(key);
			if (have) {
				have.n++;
				return;
			}
			if (effects.size >= MAX_EFFECTS) return;
			e.t = Date.now() - t0;
			e.n = 1;
			effects.set(key, e);
			dirty = true;
		};
		const markOwn = (n) => {
			own.add(n);
			for (const c of n.childNodes ?? []) markOwn(c);
		};
		const isOwn = (n) => {
			for (let c = n; c; c = c.parentNode) if (own.has(c)) return true;
			return false;
		};
		const parseStyle = (text) => {
			const m = new Map();
			let depth = 0;
			let cur = "";
			for (const ch of String(text ?? "")) {
				if (ch === "(") depth++;
				if (ch === ")") depth--;
				if (ch === ";" && depth === 0) {
					const i = cur.indexOf(":");
					if (i > 0) m.set(cur.slice(0, i).trim().toLowerCase(), cur.slice(i + 1).trim());
					cur = "";
				} else cur += ch;
			}
			const i = cur.indexOf(":");
			if (i > 0) m.set(cur.slice(0, i).trim().toLowerCase(), cur.slice(i + 1).trim());
			return m;
		};
		const take = (recs) => {
			for (const r of recs) {
				if (opts.hostFlag && r._host) continue;
				const target = r.target;
				if (r.type === "childList") {
					const added = [...(r.addedNodes ?? [])];
					const removed = [...(r.removedNodes ?? [])];
					const inOwn = isOwn(target);
					for (const n of added) markOwn(n);
					if (inOwn) continue;
					const textOnly = (l) => l.every((n) => n.nodeType === 3);
					if (added.length && removed.length && textOnly(added) && textOnly(removed)) {
						effect({ k: "text", target: sig(target), key: hostKey(target), value: squash(target.textContent, 120) });
						continue;
					}
					for (const n of added) {
						if (n.nodeType === 3) {
							if (n.data.trim()) effect({ k: "add-text", target: sig(target), key: hostKey(target), value: squash(n.data, 120) });
						} else if (isEl(n)) effect({ k: "add", target: sig(target), key: hostKey(target), name: desc(n), value: squash(n.textContent, 80) });
					}
					for (const n of removed) {
						if (own.has(n)) continue;
						if (isEl(n)) effect({ k: "remove", target: sig(target), key: hostKey(target), name: desc(n) });
						else if (n.nodeType === 3 && n.data.trim()) effect({ k: "remove-text", target: sig(target), key: hostKey(target), value: squash(n.data, 80) });
					}
				} else if (r.type === "attributes") {
					if (isOwn(target)) continue;
					const name = r.attributeName;
					const now = target.getAttribute(name);
					if (name === "style") {
						const before = parseStyle(r.oldValue);
						const after = parseStyle(now);
						for (const [p, v] of after) if (before.get(p) !== v) effect({ k: "style", target: sig(target), key: hostKey(target), name: p, value: clip(v, 200) });
						for (const p of before.keys()) if (!after.has(p)) effect({ k: "style", target: sig(target), key: hostKey(target), name: p, value: "" });
					} else if (name === "class") {
						const before = new Set(String(r.oldValue ?? "").split(/\s+/).filter(Boolean));
						const after = new Set(String(now ?? "").split(/\s+/).filter(Boolean));
						for (const c of after) if (!before.has(c)) effect({ k: "class+", target: sig(target), key: hostKey(target), name: c });
						for (const c of before) if (!after.has(c)) effect({ k: "class-", target: sig(target), key: hostKey(target), name: c });
					} else if (now !== r.oldValue) effect({ k: "attr", target: sig(target), key: hostKey(target), name, value: now === null ? null : clip(now, 200) });
				} else if (r.type === "characterData") {
					if (isOwn(target)) continue;
					const p = target.parentNode;
					effect({ k: "text", target: sig(p), key: hostKey(p), value: squash(p?.textContent ?? target.data, 120) });
				}
			}
		};
		// Changes that are neither the host tree's nor made inside one of the plugin's wrapped calls (a way of changing
		// the page the wrappers do not cover, or the page's reply to the plugin): counted, the first few described.
		const missedSeen = [];
		const miss = (recs) => {
			if (!opts.hostFlag) return;
			for (const r of recs) {
				// On its own elements every change is its own: the snapshots carry their look.
				if (r._host || isOwn(r.target)) continue;
				missed++;
				if (missedSeen.length < 20) missedSeen.push(`${r.type} ${r.attributeName ?? ""} ${sig(r.target)}`.slice(0, 200));
			}
		};
		const obs = new g.MutationObserver(miss);
		obs.observe(g.document, { subtree: true, childList: true, attributes: true, attributeOldValue: true, characterData: true, characterDataOldValue: true });
		const flushNoise = () => miss(obs.takeRecords());
		// Calls the plugin makes: its own DOM changes are taken right after.
		const around = (orig, self, args, after) => {
			if (busy) return orig.apply(self, args);
			busy++;
			try {
				if (secondary || !owner()) return orig.apply(self, args);
				flushNoise();
				const out = orig.apply(self, args);
				take(obs.takeRecords());
				after?.(self, args);
				return out;
			} finally {
				busy--;
			}
		};
		// The prototype on `proto`'s chain that defines `name`.
		const defining = (proto, name) => {
			for (let p = proto; p && p !== Object.prototype; p = Object.getPrototypeOf(p)) if (Object.prototype.hasOwnProperty.call(p, name)) return p;
			return null;
		};
		const wrap = (start, name, after) => {
			const proto = defining(start, name);
			if (!proto) return;
			const d = Object.getOwnPropertyDescriptor(proto, name);
			if (!d || !d.configurable) return;
			if (typeof d.value === "function") {
				if (d.value.__plugrec) return;
				const orig = d.value;
				const w = function (...a) {
					return around(orig, this, a, after);
				};
				w.__plugrec = true;
				Object.defineProperty(proto, name, { ...d, value: w });
			} else if (typeof d.set === "function") {
				if (d.set.__plugrec) return;
				const set = d.set;
				const w = function (v) {
					around(set, this, [v], after);
				};
				w.__plugrec = true;
				Object.defineProperty(proto, name, { ...d, set: w });
			}
		};
		const P = (n) => g[n]?.prototype;
		for (const n of ["appendChild", "insertBefore", "removeChild", "replaceChild", "textContent", "nodeValue", "normalize"]) wrap(P("Node"), n);
		for (const n of ["data", "appendData", "insertData", "deleteData", "replaceData", "remove", "before", "after", "replaceWith"]) wrap(P("CharacterData"), n);
		for (const n of ["append", "prepend", "before", "after", "replaceWith", "remove", "replaceChildren", "insertAdjacentElement", "insertAdjacentHTML", "insertAdjacentText", "setAttribute", "setAttributeNS", "removeAttribute", "removeAttributeNS", "toggleAttribute", "setAttributeNode", "innerHTML", "outerHTML", "className", "id", "slot", "setHTMLUnsafe"])
			wrap(P("Element"), n);
		for (const n of ["innerText", "outerText", "hidden", "title", "lang", "dir", "style", "draggable", "contentEditable", "tabIndex"]) wrap(P("HTMLElement"), n);
		for (const n of ["src", "srcset", "alt"]) wrap(P("HTMLImageElement"), n);
		wrap(P("HTMLAnchorElement"), "href");
		for (const n of ["add", "remove", "toggle", "replace", "value"]) wrap(P("DOMTokenList"), n);
		// Browsers put the property setters on CSSStyleProperties (newer) or CSSStyleDeclaration.
		for (const decl of [P("CSSStyleDeclaration"), P("CSSStyleProperties"), P("CSS2Properties")]) {
			if (!decl) continue;
			for (const n of Object.getOwnPropertyNames(decl)) {
				const d = Object.getOwnPropertyDescriptor(decl, n);
				if (n === "setProperty" || n === "removeProperty" || typeof d?.set === "function") wrap(decl, n);
			}
		}
		wrap(P("SVGElement"), "style");
		// Chromium has no setter per property (`el.style.color = x` goes to a named-property handler): an element's
		// style object is handed out behind a proxy whose writes count as calls.
		const hasSetters = [P("CSSStyleDeclaration"), P("CSSStyleProperties"), P("CSS2Properties")].some((p) => typeof (p && Object.getOwnPropertyDescriptor(p, "color"))?.set === "function");
		if (!hasSetters) {
			const proxies = new WeakMap();
			for (const proto of [P("HTMLElement"), P("SVGElement")]) {
				const d = proto && Object.getOwnPropertyDescriptor(proto, "style");
				if (!d?.get || !d.configurable) continue;
				const get = d.get;
				Object.defineProperty(proto, "style", {
					...d,
					get() {
						const real = get.call(this);
						if (!real || busy) return real;
						let p = proxies.get(real);
						if (!p) {
							p = new Proxy(real, {
								get: (t, k) => {
									const v = Reflect.get(t, k, t);
									return typeof v === "function" ? v.bind(t) : v;
								},
								set: (t, k, v) => {
									around((val) => Reflect.set(t, k, val, t), null, [v]);
									return true;
								},
							});
							proxies.set(real, p);
						}
						return p;
					},
				});
			}
		}
		// Rules added to a sheet are not DOM mutations: recorded as they are made.
		const sheetRule = (sheet, a) => effect({ k: "sheet", target: sig(sheet?.ownerNode ?? null), value: clip(a[0], 400) });
		for (const n of ["insertRule", "replaceSync", "replace"]) wrap(P("CSSStyleSheet"), n, sheetRule);
		// What the page does in reply is not the plugin's own change.
		const replying = (start, name, kind) => {
			const proto = defining(start, name);
			const d = proto && Object.getOwnPropertyDescriptor(proto, name);
			if (!d || typeof d.value !== "function" || d.value.__plugrecReply) return;
			const orig = d.value;
			Object.defineProperty(proto, name, {
				...d,
				value: function (...a) {
					// Its own elements' handlers are its own code.
					if (busy || secondary || isOwn(this)) return orig.apply(this, a);
					let mine = false;
					busy++;
					try {
						mine = owner();
					} finally {
						busy--;
					}
					if (!mine) return orig.apply(this, a);
					effect({ k: "event", target: sig(this), key: hostKey(this), name: kind === "dispatch" ? String(a[0]?.type ?? "") : kind });
					secondary++;
					try {
						return orig.apply(this, a);
					} finally {
						secondary--;
					}
				},
			});
		};
		replying(P("HTMLElement"), "click", "click");
		replying(P("HTMLElement"), "focus", "focus");
		replying(P("HTMLElement"), "blur", "blur");
		replying(P("EventTarget"), "dispatchEvent", "dispatch");
		// Redux actions.
		const store = opts.store?.();
		if (store && typeof store.dispatch === "function") {
			const orig = store.dispatch;
			store.dispatch = function (action, ...rest) {
				let mine = false;
				if (!busy && !secondary && action && typeof action.type === "string") {
					busy++;
					try {
						mine = owner();
					} finally {
						busy--;
					}
					if (mine) {
						let payload;
						try {
							payload = clip(JSON.stringify(action.payload) ?? "", 300);
						} catch {
							payload = "?";
						}
						actions.push({ t: Date.now() - t0, type: action.type, payload });
						dirty = true;
					}
				}
				if (!mine) return orig.call(this, action, ...rest);
				secondary++;
				try {
					return orig.call(this, action, ...rest);
				} finally {
					secondary--;
				}
			};
		}
		// Requests.
		const blocked = (method) => opts.blockWrites && !/^(GET|HEAD|OPTIONS)$/i.test(method);
		// The address without its query values (a token can be one).
		const bare = (url) => {
			const s = String(url ?? "");
			const q = s.indexOf("?");
			if (q < 0) return s;
			const keys = s
				.slice(q + 1)
				.split("&")
				.map((kv) => kv.split("=")[0])
				.filter(Boolean);
			return `${s.slice(0, q)}?${keys.join("&")}`;
		};
		const request = (method, url) => {
			const r = { t: Date.now() - t0, method: String(method || "GET").toUpperCase(), url: clip(bare(url), 300) };
			if (blocked(r.method)) r.blocked = true;
			// Made by a plugin's Node-side code here (`native:` frames); TIDAL runs that code in its main process,
			// where a page cannot see its requests.
			if (/\((plugin|native):/.exec(new Error().stack ?? "")?.[1] === "native") r.native = true;
			requests.push(r);
			dirty = true;
			return r;
		};
		const mineNow = () => {
			busy++;
			try {
				return owner();
			} finally {
				busy--;
			}
		};
		if (typeof g.fetch === "function") {
			const orig = g.fetch;
			g.fetch = function (input, init) {
				if (!busy && mineNow()) {
					const url = typeof input === "string" ? input : (input?.url ?? String(input));
					const r = request(init?.method ?? input?.method ?? "GET", url);
					if (r.blocked) return Promise.reject(new TypeError("Failed to fetch"));
				}
				return orig.apply(this, arguments);
			};
		}
		const xhr = P("XMLHttpRequest");
		if (xhr?.open && xhr?.send) {
			const open = xhr.open;
			const send = xhr.send;
			xhr.open = function (method, url, ...rest) {
				this.__plugrecReq = [method, url];
				return open.call(this, method, url, ...rest);
			};
			xhr.send = function (...a) {
				if (!busy && this.__plugrecReq && mineNow()) {
					const r = request(this.__plugrecReq[0], this.__plugrecReq[1]);
					if (r.blocked) throw new TypeError("Failed to send");
				}
				return send.apply(this, a);
			};
		}
		// Errors and warnings it logs.
		for (const level of ["error", "warn"]) {
			const orig = g.console?.[level];
			if (typeof orig !== "function") continue;
			g.console[level] = function (...a) {
				if (!busy && errors.length < 100 && mineNow()) {
					errors.push({ t: Date.now() - t0, level, msg: squash(a.map((x) => (x instanceof Error ? x.message : typeof x === "string" ? x : (() => { try { return JSON.stringify(x); } catch { return String(x); } })())).join(" "), 300) });
					dirty = true;
				}
				return orig.apply(this, a);
			};
		}
		// What its elements look like now.
		const styleOf = (el) => {
			const o = {};
			let cs;
			try {
				cs = g.getComputedStyle(el);
			} catch {
				return o;
			}
			for (const k of CS_KEYS) {
				const v = cs?.getPropertyValue?.(k);
				if (v !== undefined && v !== null && v !== "") o[k] = clip(v, 160);
			}
			return o;
		};
		const vidOf = (n) => (n._vid !== undefined ? n._vid : undefined);
		const snapshot = () => {
			const roots = [];
			const css = [];
			const walk = (n) => {
				for (const c of n.childNodes ?? []) {
					if (own.has(c) && isEl(c)) {
						const tag = String(c.localName ?? "").toLowerCase();
						if (tag === "style") {
							css.push({ parent: sig(c.parentNode), id: c.getAttribute("id") ?? "", text: clip(c.textContent, 20000) });
							continue;
						}
						if (tag === "script" || tag === "link") continue;
						const kids = [];
						const kidVids = [];
						const under = (p) => {
							for (const k of p.childNodes ?? []) {
								if (!isEl(k)) continue;
								if (vidOf(k) !== undefined) kidVids.push(vidOf(k));
								if (kids.length < 25) kids.push({ d: desc(k), text: squash(k.textContent, 60), style: styleOf(k) });
								under(k);
							}
						};
						under(c);
						roots.push({ parent: sig(c.parentNode), key: hostKey(c.parentNode), d: desc(c), text: squash(c.textContent, 160), style: styleOf(c), kids, vid: vidOf(c), kidVids });
						continue;
					}
					if (isEl(c)) walk(c);
				}
			};
			if (g.document?.documentElement) walk(g.document.documentElement);
			return { roots, css };
		};
		const snap = () => {
			busy++;
			try {
				const s = snapshot();
				const text = JSON.stringify(s);
				if (text !== lastSnap) {
					lastSnap = text;
					snaps.push({ t: Date.now() - t0, ...s });
					if (snaps.length > 120) snaps.splice(1, 1);
					dirty = true;
				}
			} catch (err) {
				errors.push({ t: Date.now() - t0, level: "recorder", msg: squash(err?.message ?? err, 200) });
			} finally {
				busy--;
			}
		};
		const dump = () => ({ t0, effects: [...effects.values()], actions, requests, errors, snaps, missed, missedSeen });
		let ticks = 0;
		const tick = () => {
			snap();
			// Every fifth second regardless: what the host adds to the record (the elements it drew) changes too.
			if (opts.write && (dirty || ++ticks % 5 === 0)) {
				dirty = false;
				try {
					opts.write(JSON.stringify(dump()));
				} catch {}
			}
		};
		const timer = g.setInterval(tick, 1000);
		g.__plugrec = { dump, snap, tick, stop: () => g.clearInterval(timer) };
		return g.__plugrec;
	};
})(globalThis);
