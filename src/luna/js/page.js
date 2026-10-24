// Luna compatibility runtime: the page a plugin's Electron `BrowserWindow`
// loads (src/luna/page.rs). Runs after prelude.js, dom.js and css.js in the
// window's own runtime, as Electron runs a window's page in a renderer of
// its own: its document, stylesheets and scripts never mix with the
// plugins' or with TIDAL's page. What it holds is drawn natively by the
// same renderer as plugin elements in the app (src/ui/plugin_windows.rs).
//
// It provides what the page and its preload script see in Electron:
// the browser's defaults for an HTML document, `location`, storage, inline
// `on*` handlers, the page's own scripts run in document order, and the
// renderer's `electron` module for the preload (`contextBridge`,
// `ipcRenderer`, `webFrame`). Messages between the window and the plugin
// are copied as Electron's structured clone copies them.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const D = L.dom;
	const { callOp, callOpAsync } = L;
	const doc = g.document;
	const html = doc.documentElement;
	const head = doc.head;
	const body = doc.body;

	// ---------------------------------------------------------------- the browser's defaults
	// Chromium's own stylesheet for what pages rely on, at no specificity
	// (`:where`), so any rule of the page wins over it as an author rule
	// wins over the browser's.
	const UA = `
:where(html){display:block;color:#000;font-family:"Times New Roman",serif;font-size:16px;line-height:normal}
:where(head,script,style,title,meta,link,template,noscript){display:none}
:where(body){display:block;margin:8px}
:where(div,p,section,article,header,footer,main,nav,aside,ul,ol,li,h1,h2,h3,h4,h5,h6,form,fieldset,figure,blockquote,pre,dl,dd,dt){display:block}
:where(p,blockquote,figure,dl,pre){margin:16px 0}
:where(h1){font-size:32px;font-weight:700;margin:21px 0}
:where(h2){font-size:24px;font-weight:700;margin:20px 0}
:where(h3){font-size:19px;font-weight:700;margin:19px 0}
:where(h4){font-size:16px;font-weight:700;margin:21px 0}
:where(h5){font-size:13px;font-weight:700;margin:22px 0}
:where(h6){font-size:11px;font-weight:700;margin:25px 0}
:where(ul,ol){margin:16px 0;padding-left:40px}
:where(b,strong){font-weight:700}
:where(i,em){font-style:italic}
:where(button){font-family:system-ui;font-size:13px}
:where(a){color:#0000ee;text-decoration:underline;cursor:pointer}
:where([hidden]){display:none}
`;
	const ua = doc.createElement("style");
	ua.textContent = UA;
	head.appendChild(ua);
	doc.title = "";

	// ---------------------------------------------------------------- location
	const setLocation = (url) => {
		const loc = g.location;
		let parts = null;
		try {
			parts = new URL(url);
		} catch {}
		const opaque = /^(data|about|blob):/i.test(url);
		Object.assign(loc, {
			href: url,
			origin: opaque || !parts ? "null" : parts.origin === "null" ? `${parts.protocol}//` : parts.origin,
			protocol: parts?.protocol ?? "about:",
			host: opaque ? "" : (parts?.host ?? ""),
			hostname: opaque ? "" : (parts?.hostname ?? ""),
			port: opaque ? "" : (parts?.port ?? ""),
			pathname: opaque ? url.replace(/^[a-z]+:/i, "") : (parts?.pathname ?? ""),
			search: opaque ? "" : (parts?.search ?? ""),
			hash: opaque ? "" : (parts?.hash ?? ""),
		});
		Object.defineProperty(doc, "URL", { value: url, configurable: true });
		Object.defineProperty(doc, "documentURI", { value: url, configurable: true });
	};

	// ---------------------------------------------------------------- storage
	// A page's storage belongs to its origin. Chromium has none for pages
	// with an opaque origin (`data:`, `about:`): reading `localStorage`
	// there throws a SecurityError. Others keep it in memory while the
	// window is open.
	const memoryStorage = () => {
		const m = new Map();
		const s = {
			getItem: (k) => (m.has(String(k)) ? m.get(String(k)) : null),
			setItem: (k, v) => void m.set(String(k), String(v)),
			removeItem: (k) => void m.delete(String(k)),
			clear: () => m.clear(),
			key: (i) => [...m.keys()][i] ?? null,
			get length() {
				return m.size;
			},
		};
		return s;
	};
	const storageFor = (kind) => {
		let store = null;
		return () => {
			if (g.location.origin === "null") {
				const e = new Error(`Failed to read the '${kind}' property from 'Window': Storage is disabled inside 'data:' URLs.`);
				e.name = "SecurityError";
				throw e;
			}
			return (store ??= memoryStorage());
		};
	};
	Object.defineProperty(g, "localStorage", { get: storageFor("localStorage"), configurable: true });
	Object.defineProperty(g, "sessionStorage", { get: storageFor("sessionStorage"), configurable: true });

	// ---------------------------------------------------------------- structured clone
	// As node.js's `serialization: "advanced"` writes it (the plugin side
	// sends with it); a Buffer arrives as a Uint8Array, as in a renderer.
	const TAG = "\u0000luna";
	const VIEWS = { Uint8Array, Int8Array, Uint8ClampedArray, Uint16Array, Int16Array, Uint32Array, Int32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array, DataView };
	const cloneError = (m) => Object.assign(new Error(m), { name: "DataCloneError" });
	const encode = (value) => {
		const seen = new Set();
		const enc = (x) => {
			if (x === undefined) return { [TAG]: "u" };
			if (typeof x === "bigint") return { [TAG]: "n", v: String(x) };
			if (typeof x === "number") return Number.isFinite(x) && !Object.is(x, -0) ? x : { [TAG]: "f", v: Object.is(x, -0) ? "-0" : String(x) };
			if (typeof x === "function" || typeof x === "symbol") throw cloneError(`${String(x)} could not be cloned.`);
			if (x === null || typeof x !== "object") return x;
			if (seen.has(x)) throw cloneError("A circular value could not be cloned.");
			seen.add(x);
			try {
				if (x instanceof Date) return { [TAG]: "d", v: x.getTime() };
				if (x instanceof RegExp) return { [TAG]: "r", v: x.source, f: x.flags };
				if (x instanceof Map) return { [TAG]: "m", v: [...x].map(([k, v]) => [enc(k), enc(v)]) };
				if (x instanceof Set) return { [TAG]: "s", v: [...x].map(enc) };
				if (x instanceof ArrayBuffer) return { [TAG]: "ab", v: L.b64enc(new Uint8Array(x)) };
				if (ArrayBuffer.isView(x)) return { [TAG]: "ta", c: x instanceof DataView ? "DataView" : (x.constructor?.name ?? "Uint8Array"), v: L.b64enc(new Uint8Array(x.buffer, x.byteOffset, x.byteLength)) };
				if (x instanceof Error) return { [TAG]: "e", n: x.name, m: x.message, s: x.stack };
				if (Array.isArray(x)) return x.map(enc);
				const out = {};
				for (const k of Object.keys(x)) out[k] = enc(x[k]);
				return out;
			} finally {
				seen.delete(x);
			}
		};
		return JSON.stringify(enc(value));
	};
	const decode = (text) => {
		const dec = (x) => {
			if (x === null || typeof x !== "object") return x;
			if (Array.isArray(x)) return x.map(dec);
			switch (Object.prototype.hasOwnProperty.call(x, TAG) ? x[TAG] : undefined) {
				case "u":
					return undefined;
				case "n":
					return BigInt(x.v);
				case "f":
					return x.v === "-0" ? -0 : Number(x.v);
				case "d":
					return new Date(x.v);
				case "r":
					return new RegExp(x.v, x.f);
				case "m":
					return new Map(x.v.map(([k, v]) => [dec(k), dec(v)]));
				case "s":
					return new Set(x.v.map(dec));
				case "ab":
					return L.b64dec(x.v).buffer;
				case "ta": {
					const bytes = L.b64dec(x.v);
					const View = VIEWS[x.c] ?? Uint8Array;
					if (View === DataView) return new DataView(bytes.buffer);
					return new View(bytes.buffer, 0, bytes.byteLength / (View.BYTES_PER_ELEMENT ?? 1));
				}
				case "e": {
					const e = new Error(x.m);
					if (x.n) e.name = x.n;
					return e;
				}
				default: {
					const out = {};
					for (const k of Object.keys(x)) out[k] = dec(x[k]);
					return out;
				}
			}
		};
		return dec(JSON.parse(text));
	};

	// ---------------------------------------------------------------- electron (renderer)
	const listeners = new Map();
	const ipcRenderer = {
		send(channel, ...args) {
			callOp("page.ipc", { channel: String(channel), args: encode(args) });
		},
		invoke(channel, ...args) {
			let data;
			try {
				data = encode(args);
			} catch (e) {
				return Promise.reject(e);
			}
			return callOpAsync("page.invoke", { channel: String(channel), args: data }).then(
				(r) => (typeof r === "string" ? decode(r) : undefined),
				(e) => {
					throw new Error(`Error invoking remote method '${channel}': ${e?.message ?? e}`);
				},
			);
		},
		sendSync(channel) {
			throw new Error(`ipcRenderer.sendSync('${channel}') is not available in Tidalfast's plugin windows`);
		},
		sendToHost(channel, ...args) {
			this.send(channel, ...args);
		},
		postMessage(channel, message) {
			this.send(channel, message);
		},
		on(channel, fn) {
			if (typeof fn !== "function") return this;
			const list = listeners.get(String(channel)) ?? [];
			list.push({ fn, once: false });
			listeners.set(String(channel), list);
			return this;
		},
		addListener(channel, fn) {
			return this.on(channel, fn);
		},
		once(channel, fn) {
			if (typeof fn !== "function") return this;
			const list = listeners.get(String(channel)) ?? [];
			list.push({ fn, once: true });
			listeners.set(String(channel), list);
			return this;
		},
		off(channel, fn) {
			const list = listeners.get(String(channel));
			if (!list) return this;
			const i = list.findIndex((l) => l.fn === fn);
			if (i >= 0) list.splice(i, 1);
			if (!list.length) listeners.delete(String(channel));
			return this;
		},
		removeListener(channel, fn) {
			return this.off(channel, fn);
		},
		removeAllListeners(channel) {
			if (channel === undefined) listeners.clear();
			else listeners.delete(String(channel));
			return this;
		},
		listenerCount(channel) {
			return listeners.get(String(channel))?.length ?? 0;
		},
	};
	// What the plugin sent with `webContents.send`.
	g.__page_ipc = (channel, args) => {
		const list = listeners.get(String(channel));
		if (!list?.length) return;
		let values;
		try {
			values = decode(args);
		} catch (err) {
			L.reportError(err);
			return;
		}
		const event = { sender: ipcRenderer, senderId: 0, ports: [] };
		for (const l of [...list]) {
			if (l.once) ipcRenderer.off(channel, l.fn);
			try {
				l.fn(event, ...(Array.isArray(values) ? values : []));
			} catch (err) {
				L.reportError(err);
			}
		}
	};
	// `contextBridge` hands the page a copy of what the preload exposes, as
	// Electron does: values are copied, functions stay callable.
	const bridgeCopy = (x, seen = new Map()) => {
		if (x === null || (typeof x !== "object" && typeof x !== "function")) return x;
		if (typeof x === "function") return (...a) => x(...a);
		if (seen.has(x)) return seen.get(x);
		if (x instanceof Date) return new Date(x.getTime());
		if (x instanceof RegExp || x instanceof Error || ArrayBuffer.isView(x) || x instanceof ArrayBuffer || x instanceof Promise) return x;
		const out = Array.isArray(x) ? [] : {};
		seen.set(x, out);
		for (const k of Object.keys(x)) out[k] = bridgeCopy(x[k], seen);
		return Object.freeze(out);
	};
	const contextBridge = {
		exposeInMainWorld(key, api) {
			Object.defineProperty(g, String(key), { value: bridgeCopy(api), configurable: false, enumerable: true, writable: false });
		},
		exposeInIsolatedWorld(_world, key, api) {
			this.exposeInMainWorld(key, api);
		},
		executeInMainWorld({ func, args = [] } = {}) {
			return typeof func === "function" ? func(...args) : undefined;
		},
	};
	let zoom = 1;
	const webFrame = {
		setZoomFactor: (f) => void (zoom = Number(f) || 1),
		getZoomFactor: () => zoom,
		setZoomLevel() {},
		getZoomLevel: () => 0,
		setVisualZoomLevelLimits() {},
		insertCSS(css) {
			const s = doc.createElement("style");
			s.textContent = String(css ?? "");
			head.appendChild(s);
			return `css-${Math.random().toString(36).slice(2)}`;
		},
		removeInsertedCSS() {},
		executeJavaScript: async (code) => (0, eval)(String(code)),
		routingId: 1,
	};
	const electron = {
		ipcRenderer,
		contextBridge,
		webFrame,
		webUtils: { getPathForFile: () => "" },
		crashReporter: { start() {}, addExtraParameter() {}, removeExtraParameter() {}, getParameters: () => ({}) },
	};
	// A tiny EventEmitter for a preload's `require("events")`.
	class EventEmitter {
		#l = new Map();
		on(n, f) {
			(this.#l.get(n) ?? this.#l.set(n, []).get(n)).push(f);
			return this;
		}
		addListener(n, f) {
			return this.on(n, f);
		}
		once(n, f) {
			const w = (...a) => {
				this.off(n, w);
				f(...a);
			};
			return this.on(n, w);
		}
		off(n, f) {
			const l = this.#l.get(n);
			if (l) l.splice(l.indexOf(f) >>> 0, 1);
			return this;
		}
		removeListener(n, f) {
			return this.off(n, f);
		}
		removeAllListeners(n) {
			if (n === undefined) this.#l.clear();
			else this.#l.delete(n);
			return this;
		}
		emit(n, ...a) {
			const l = this.#l.get(n);
			if (!l?.length) return false;
			for (const f of [...l]) f(...a);
			return true;
		}
		listenerCount(n) {
			return this.#l.get(n)?.length ?? 0;
		}
	}
	EventEmitter.EventEmitter = EventEmitter;
	const platform = callOp("platform", null);
	const preloadProcess = Object.freeze({
		platform,
		type: "renderer",
		sandboxed: true,
		contextIsolated: true,
		env: Object.freeze({}),
		argv: Object.freeze([]),
		versions: Object.freeze({ ...(g.__lunaVersions ?? {}) }),
	});
	// A sandboxed preload's `require`, as Electron gives it: the renderer's
	// `electron` module and a few of Node's.
	const preloadRequire = (name) => {
		switch (String(name)) {
			case "electron":
			case "electron/renderer":
				return electron;
			case "events":
			case "node:events":
				return EventEmitter;
			case "timers":
			case "node:timers":
				return { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate: g.setImmediate, clearImmediate: g.clearImmediate };
			case "url":
			case "node:url":
				return { URL, URLSearchParams };
			default: {
				const e = new Error(`module not found: ${name}`);
				e.code = "MODULE_NOT_FOUND";
				throw e;
			}
		}
	};
	const runPreload = (src, file) => {
		if (!src) return;
		const module = { exports: {} };
		try {
			const fn = new Function("require", "module", "exports", "process", "__filename", "__dirname", src);
			fn.call(module.exports, preloadRequire, module, module.exports, preloadProcess, file, file.replace(/[\\/][^\\/]*$/, ""));
		} catch (err) {
			callOp("log", { plugin: null, level: "error", msg: `Unable to load preload script: ${file}\n${L.format?.(err) ?? err}` });
		}
	};

	// ---------------------------------------------------------------- the window
	g.close = () => callOp("page.close", {});
	g.open = () => null;
	g.moveTo = g.moveBy = g.resizeTo = g.resizeBy = () => {};
	g.focus = g.blur = () => {};
	g.print = () => {};
	g.name = "";
	g.opener = null;
	g.closed = false;
	g.devicePixelRatio = 1;

	// ---------------------------------------------------------------- inline handlers
	// `onclick="..."` and the other `on*` attributes are compiled as a
	// browser compiles them: a function of `event` whose scope reaches the
	// element and its document first, `this` the element, and `return
	// false` cancelling the event.
	const HANDLER = /^on[a-z]+$/;
	const compile = (el, name, code) => {
		let fn = null;
		try {
			fn = new Function("event", `with (this.ownerDocument ?? document) { with (this) { return (() => {\n${code}\n}).call(this); } }`);
		} catch (err) {
			L.reportError(err);
			return;
		}
		el[name] = function (event) {
			const r = fn.call(this, event);
			if (r === false) event?.preventDefault?.();
			return r;
		};
	};
	const compileAll = (root) => {
		const visit = (n) => {
			if (!(n instanceof g.Element)) return;
			for (const k of n._attrs?.keys?.() ?? []) if (HANDLER.test(k) && `${k}` in n) compile(n, k, n.getAttribute(k));
			for (const c of n.childNodes) visit(c);
		};
		visit(root);
	};
	const setAttribute = g.Element.prototype.setAttribute;
	g.Element.prototype.setAttribute = function (name, value) {
		const r = setAttribute.call(this, name, value);
		const k = String(name).toLowerCase();
		if (HANDLER.test(k) && k in this) compile(this, k, String(value));
		return r;
	};

	// ---------------------------------------------------------------- scripts
	const runScript = (el) => {
		const type = (el.getAttribute("type") ?? "").trim().toLowerCase();
		if (type && !/^(text|application)\/(javascript|ecmascript)$|^module$/.test(type)) return;
		if (el.hasAttribute("src")) {
			callOp("log", { plugin: null, level: "warn", msg: `the page's script ${el.getAttribute("src")} was not loaded: plugin windows run only the scripts written in their page` });
			return;
		}
		const code = el.textContent ?? "";
		if (!code.trim()) return;
		const prev = doc.currentScript;
		try {
			Object.defineProperty(doc, "currentScript", { value: el, configurable: true });
			(0, eval)(code);
		} catch (err) {
			L.reportError(err);
		} finally {
			Object.defineProperty(doc, "currentScript", { value: prev ?? null, configurable: true });
		}
	};
	// The parsed page goes into the document node by node, and each script
	// runs when it is inserted, as a browser runs it while parsing: what
	// comes after it is not there yet.
	const insert = (parent, node) => {
		const kids = node instanceof g.Element && node.localName !== "template" ? [...node.childNodes] : [];
		for (const k of kids) node.removeChild(k);
		parent.appendChild(node);
		if (node instanceof g.Element) {
			compileAll(node);
			if (node.localName === "script") {
				for (const k of kids) node.appendChild(k);
				runScript(node);
				return;
			}
		}
		for (const k of kids) insert(node, k);
	};
	const setReady = (state) => Object.defineProperty(doc, "readyState", { value: state, configurable: true, writable: true });

	// `{ url, html, preload, preloadFile }` from the plugin's `loadURL`.
	g.__page_load = (json) => {
		const { url, html: source, preload, preloadFile } = JSON.parse(json);
		setLocation(String(url ?? "about:blank"));
		setReady("loading");
		runPreload(preload, String(preloadFile ?? ""));
		const nodes = D.parseHTML(String(source ?? ""), false, "div");
		const into = (n) => {
			if (!(n instanceof g.Element)) return n instanceof g.Text && !n.data.trim() ? null : body;
			if (n.localName === "head") return head;
			if (n.localName === "body") return body;
			return body;
		};
		const place = (n) => {
			if (n instanceof g.Element && n.localName === "html") {
				for (const [k, v] of n._attrs) html.setAttribute(k, v);
				for (const c of [...n.childNodes]) place(c);
				return;
			}
			if (n instanceof g.Element && (n.localName === "head" || n.localName === "body")) {
				const target = into(n);
				for (const [k, v] of n._attrs) target.setAttribute(k, v);
				for (const c of [...n.childNodes]) {
					n.removeChild(c);
					if (n.localName === "head" && c instanceof g.Element && c.localName === "title") doc.title = c.textContent ?? "";
					if (c instanceof g.Text && !c.data.trim()) continue;
					insert(target, c);
				}
				return;
			}
			const target = into(n);
			if (!target) return;
			if (n.parentNode) n.parentNode.removeChild(n);
			if (n instanceof g.Element && n.localName === "title") doc.title = n.textContent ?? "";
			insert(target, n);
		};
		for (const n of nodes) place(n);
		setReady("interactive");
		try {
			doc.dispatchEvent(new g.Event("DOMContentLoaded", { bubbles: true }));
		} catch (err) {
			L.reportError(err);
		}
		setReady("complete");
		try {
			g.dispatchEvent(new g.Event("load"));
		} catch (err) {
			L.reportError(err);
		}
		callOp("page.loaded", { title: String(doc.title ?? "") });
	};
})();
