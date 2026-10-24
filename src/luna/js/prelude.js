// Luna compatibility runtime: browser-ish globals for QuickJS.
// Runs once per runtime before any other shim. Rust provides:
//   __op(name, json, plugin) -> json    synchronous host operations
//   __op_async(name, json, id, plugin)  asynchronous host operations, resolved through __luna_resolve
//   __caller() -> plugin name           the plugin whose code is innermost on the live stack
//   __eval(code, filename)              evaluate shim-made code under a module name
//   __utf8_enc(str) -> ArrayBuffer, __utf8_dec(ArrayBuffer, offset, length) -> string
//   __b64_enc(ArrayBuffer, offset, length) -> string, __b64_dec(string) -> ArrayBuffer
//   __fetch_take(token) -> ArrayBuffer   a large fetched body, read once from its file
//   __fs_bytes(json, plugin, ArrayBuffer?) -> ArrayBuffer | undefined   fs read/write without base64
// The shims keep them (and `__lunaInternal`) in closures; Rust deletes the
// globals before any plugin code runs, so plugins reach the host only
// through the shims, and Rust checks every identity a shim passes against
// the live frames.
"use strict";
(() => {
	const g = globalThis;
	g.window = g;
	g.self = g;
	g.global = g;
	// QuickJS has no Intl: its Number#toLocaleString is toString. TIDAL's
	// Chromium groups thousands ("1,530", Tidal Tags' "1,530kb/s") and
	// keeps at most three decimals unless the options say otherwise.
	Object.defineProperty(Number.prototype, "toLocaleString", {
		configurable: true,
		writable: true,
		value: function toLocaleString(_locales, options) {
			const n = Number(this);
			if (Number.isNaN(n)) return "NaN";
			if (!Number.isFinite(n)) return n > 0 ? "∞" : "-∞";
			const o = options ?? {};
			const minF = Math.min(20, Math.max(0, Number(o.minimumFractionDigits ?? 0) || 0));
			const maxF = Math.min(20, Math.max(minF, Number(o.maximumFractionDigits ?? Math.max(minF, 3)) || 0));
			const fixed = Math.abs(n).toFixed(maxF);
			let [whole, frac = ""] = fixed.split(".");
			frac = frac.replace(/0+$/, "");
			while (frac.length < minF) frac += "0";
			if (o.useGrouping !== false) whole = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
			return (n < 0 && Number(fixed) !== 0 ? "-" : "") + whole + (frac ? `.${frac}` : "");
		},
	});
	// Nor are its dates' toLocale*String options read: TIDAL's Chromium
	// (en-US) writes `{ hour: "2-digit", minute: "2-digit" }` as "02:29 PM"
	// where QuickJS gave "14:29:07" with seconds (DownloadManager's taskbar
	// clock). The en-US forms of the options plugins use: hour, minute,
	// second, hour12/hourCycle, weekday, day, month, year, timeZone UTC.
	{
		const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
		const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
		const parts = (d, o) => {
			const utc = /^(utc|gmt|etc\/utc)$/i.test(String(o?.timeZone ?? ""));
			return utc
				? { y: d.getUTCFullYear(), mo: d.getUTCMonth(), da: d.getUTCDate(), wd: d.getUTCDay(), h: d.getUTCHours(), mi: d.getUTCMinutes(), s: d.getUTCSeconds() }
				: { y: d.getFullYear(), mo: d.getMonth(), da: d.getDate(), wd: d.getDay(), h: d.getHours(), mi: d.getMinutes(), s: d.getSeconds() };
		};
		const pad = (n) => String(n).padStart(2, "0");
		const num = (n, style) => (style === "2-digit" ? pad(n % 100) : String(n));
		const timeOf = (p, o) => {
			let { hour, minute, second } = o;
			if (hour === undefined && minute === undefined && second === undefined) [hour, minute, second] = ["numeric", "2-digit", "2-digit"];
			const twelve = o.hour12 !== undefined ? !!o.hour12 : !/^h(23|24)$/.test(String(o.hourCycle ?? ""));
			const out = [];
			if (hour !== undefined) {
				const h = twelve ? p.h % 12 || 12 : p.h;
				out.push(twelve ? num(h, hour) : pad(h));
			}
			if (minute !== undefined) out.push(hour !== undefined || second !== undefined ? pad(p.mi) : num(p.mi, minute));
			if (second !== undefined) out.push(out.length ? pad(p.s) : num(p.s, second));
			let text = out.join(":");
			if (hour !== undefined && twelve) text += p.h < 12 ? " AM" : " PM";
			return text;
		};
		const dateOf = (p, o) => {
			let { weekday, day, month, year } = o;
			if (weekday === undefined && day === undefined && month === undefined && year === undefined) [day, month, year] = ["numeric", "numeric", "numeric"];
			const y = year === undefined ? "" : year === "2-digit" ? pad(p.y % 100) : String(p.y);
			let text;
			if (month === "long" || month === "short" || month === "narrow") {
				const name = month === "long" ? MONTHS[p.mo] : month === "short" ? MONTHS[p.mo].slice(0, 3) : MONTHS[p.mo][0];
				text = [name, day === undefined ? "" : ` ${num(p.da, day)}`, y === "" ? "" : day === undefined ? ` ${y}` : `, ${y}`].join("");
			} else {
				text = [month === undefined ? null : num(p.mo + 1, month), day === undefined ? null : num(p.da, day), y === "" ? null : y].filter((v) => v !== null).join("/");
			}
			if (weekday !== undefined) {
				const w = weekday === "long" ? DAYS[p.wd] : weekday === "short" ? DAYS[p.wd].slice(0, 3) : DAYS[p.wd][0];
				text = text ? `${w}, ${text}` : w;
			}
			return text;
		};
		const define = (name, fn) =>
			Object.defineProperty(Date.prototype, name, {
				configurable: true,
				writable: true,
				value: {
					[name](_locales, options) {
						if (Number.isNaN(this.getTime())) return "Invalid Date";
						return fn(parts(this, options), options ?? {});
					},
				}[name],
			});
		const hasTime = (o) => o.hour !== undefined || o.minute !== undefined || o.second !== undefined;
		const hasDate = (o) => o.weekday !== undefined || o.day !== undefined || o.month !== undefined || o.year !== undefined;
		define("toLocaleTimeString", (p, o) => (hasDate(o) ? `${dateOf(p, o)}, ` : "") + timeOf(p, o));
		define("toLocaleDateString", (p, o) => dateOf(p, o) + (hasTime(o) ? `, ${timeOf(p, o)}` : ""));
		define("toLocaleString", (p, o) => {
			if (!hasTime(o) && !hasDate(o)) return `${dateOf(p, o)}, ${timeOf(p, o)}`;
			const parts = [];
			if (hasDate(o)) parts.push(dateOf(p, o));
			if (hasTime(o)) parts.push(timeOf(p, o));
			return parts.join(", ");
		});
	}
	const native = {
		op: g.__op,
		opAsync: g.__op_async,
		caller: g.__caller,
		eval: g.__eval,
		utf8enc: g.__utf8_enc,
		utf8dec: g.__utf8_dec,
		b64enc: g.__b64_enc,
		b64dec: g.__b64_dec,
		fetchTake: g.__fetch_take,
		fsBytes: g.__fs_bytes,
	};
	const stringify = JSON.stringify;
	const parse = JSON.parse;
	const hasOwn = Object.prototype.hasOwnProperty;

	// ---------------------------------------------------------------- ops
	// The plugin an op acts for goes as its own argument (read from the
	// shim's own object, never from the serialised JSON), and Rust checks it.
	const actor = (arg) => (arg !== null && typeof arg === "object" && typeof arg.plugin === "string" ? arg.plugin : null);
	const callOp = (name, arg) => {
		const out = parse(native.op(name, stringify(arg ?? null), actor(arg)));
		if (out && hasOwn.call(out, "err")) throw new Error(out.err);
		return out?.ok;
	};
	const pending = new Map();
	let opSeq = 1;
	const callOpAsync = (name, arg) =>
		new Promise((resolve, reject) => {
			const id = opSeq++;
			pending.set(id, { resolve, reject });
			native.opAsync(name, stringify(arg ?? null), id, actor(arg));
		});
	g.__luna_resolve = (id, ok, json) => {
		const p = pending.get(id);
		if (p === undefined) return;
		pending.delete(id);
		const value = json === undefined || json === "" ? undefined : parse(json);
		if (ok) p.resolve(value);
		else p.reject(new Error(typeof value === "string" ? value : stringify(value)));
	};
	const lunaInternal = (g.__lunaInternal = { callOp, callOpAsync, blobUrls: new Map(), blobModules: new Map() });
	// Async ops still in flight (a forked process lives while it waits for one).
	lunaInternal.pendingOps = () => pending.size;
	lunaInternal.evalAs = native.eval;
	lunaInternal.b64encRaw = native.b64enc;
	lunaInternal.b64decRaw = native.b64dec;
	// fs reads and writes with the bytes as they are (node.js's fs shim).
	lunaInternal.fsBytes = (arg, data) => native.fsBytes(stringify(arg), actor(arg), data);

	// Which plugin is running the current code. Without a stack: the
	// innermost plugin on the live call stack, which Rust reads from the
	// engine's frames (module "plugin:<name>@<gen>" for plugin code,
	// "native:<name>@<file>" for .native code); no plugin can shape it. With
	// a stack string (an error's): the first plugin frame in it, for logs.
	lunaInternal.pluginFromStack = (stack) => {
		if (stack === undefined || stack === null) return native.caller() ?? undefined;
		for (const line of String(stack).split("\n")) {
			const m = / \((?:plugin|native):([^@\s():]+)@[^()]*\)\s*$/.exec(line);
			if (m) return decodeURIComponent(m[1]);
		}
		return undefined;
	};

	// ---------------------------------------------------------------- bytes
	const toU8 = (v) => {
		if (v instanceof Uint8Array) return v;
		if (v instanceof ArrayBuffer) return new Uint8Array(v);
		if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
		if (Array.isArray(v)) return Uint8Array.from(v);
		throw new TypeError("Expected bytes");
	};
	lunaInternal.toU8 = toU8;
	lunaInternal.b64enc = (bytes) => {
		const u = toU8(bytes);
		return native.b64enc(u.buffer, u.byteOffset, u.byteLength);
	};
	lunaInternal.b64dec = (s) => new Uint8Array(native.b64dec(s ?? ""));
	lunaInternal.utf8enc = (s) => new Uint8Array(native.utf8enc(String(s)));
	lunaInternal.utf8dec = (bytes) => {
		const u = toU8(bytes);
		return native.utf8dec(u.buffer, u.byteOffset, u.byteLength);
	};

	class TextEncoder {
		get encoding() {
			return "utf-8";
		}
		encode(s = "") {
			return lunaInternal.utf8enc(s);
		}
		encodeInto(s, dest) {
			const b = lunaInternal.utf8enc(s);
			const n = Math.min(b.length, dest.length);
			dest.set(b.subarray(0, n));
			return { read: s.length, written: n };
		}
	}
	class TextDecoder {
		constructor(encoding = "utf-8") {
			this.encoding = String(encoding).toLowerCase();
		}
		decode(bytes) {
			if (bytes === undefined) return "";
			if (this.encoding === "latin1" || this.encoding === "ascii" || this.encoding === "iso-8859-1") {
				let s = "";
				for (const c of toU8(bytes)) s += String.fromCharCode(c);
				return s;
			}
			return lunaInternal.utf8dec(bytes);
		}
	}
	g.TextEncoder = TextEncoder;
	g.TextDecoder = TextDecoder;
	g.btoa = (s) => {
		s = String(s);
		const u = new Uint8Array(s.length);
		for (let i = 0; i < s.length; i++) {
			const c = s.charCodeAt(i);
			if (c > 255) throw new Error("btoa: character out of range");
			u[i] = c;
		}
		return lunaInternal.b64enc(u);
	};
	g.atob = (s) => {
		let out = "";
		for (const c of lunaInternal.b64dec(String(s).replace(/\s+/g, ""))) out += String.fromCharCode(c);
		return out;
	};

	// ---------------------------------------------------------------- console
	const fmt = (a, depth = 0) => {
		if (typeof a === "string") return a;
		if (a instanceof Error) return a.stack ? `${a.name}: ${a.message}\n${a.stack}` : `${a.name}: ${a.message}`;
		if (typeof a === "function") return `[Function ${a.name || "anonymous"}]`;
		if (typeof a === "bigint") return `${a}n`;
		if (typeof a === "symbol") return a.toString();
		if (a === undefined) return "undefined";
		if (a === null || typeof a !== "object") return String(a);
		if (depth > 2) return Array.isArray(a) ? "[Array]" : "[Object]";
		try {
			const seen = new WeakSet();
			return JSON.stringify(a, (k, v) => {
				if (typeof v === "bigint") return `${v}n`;
				if (typeof v === "function") return `[Function ${v.name || "anonymous"}]`;
				if (v && typeof v === "object") {
					if (seen.has(v)) return "[Circular]";
					seen.add(v);
				}
				return v;
			});
		} catch {
			return Object.prototype.toString.call(a);
		}
	};
	lunaInternal.format = (...args) => {
		if (typeof args[0] === "string" && /%[sdifjoO%]/.test(args[0])) {
			let i = 1;
			const first = args[0].replace(/%([sdifjoO%])/g, (m, c) => {
				if (c === "%") return "%";
				if (i >= args.length) return m;
				const v = args[i++];
				switch (c) {
					case "s":
						return typeof v === "string" ? v : fmt(v);
					case "d":
					case "i":
						return String(parseInt(v));
					case "f":
						return String(parseFloat(v));
					default:
						return fmt(v);
				}
			});
			return [first, ...args.slice(i).map((a) => fmt(a))].join(" ");
		}
		return args.map((a) => fmt(a)).join(" ");
	};
	const log =
		(level) =>
		(...args) => {
			try {
				callOp("log", { plugin: lunaInternal.pluginFromStack(), level, msg: lunaInternal.format(...args) });
			} catch {}
		};
	const counts = {};
	const timersLabel = {};
	g.console = {
		log: log("info"),
		info: log("info"),
		warn: log("warn"),
		error: log("error"),
		debug: log("debug"),
		trace: log("debug"),
		dir: log("info"),
		table: log("info"),
		group: () => {},
		groupCollapsed: () => {},
		groupEnd: () => {},
		assert: (cond, ...a) => {
			if (!cond) log("error")("Assertion failed", ...a);
		},
		count: (l = "default") => log("info")(`${l}: ${(counts[l] = (counts[l] ?? 0) + 1)}`),
		countReset: (l = "default") => delete counts[l],
		time: (l = "default") => (timersLabel[l] = Date.now()),
		timeEnd: (l = "default") => log("info")(`${l}: ${Date.now() - (timersLabel[l] ?? Date.now())}ms`),
		timeLog: (l = "default") => log("info")(`${l}: ${Date.now() - (timersLabel[l] ?? Date.now())}ms`),
	};

	// ---------------------------------------------------------------- timers
	// All timers live here; Rust calls __luna_tick() and sleeps until the next due time.
	const timers = new Map();
	let timerSeq = 1;
	// The plugin whose timer or animation-frame callback is running: timers it
	// sets belong to it without reading a stack trace (which costs far more
	// than the timer itself for a 60 Hz animation loop).
	lunaInternal.running = undefined;
	// Nested timers are clamped as in browsers: a timer set by a timer task
	// (or by the promise jobs that follow it) more than 5 levels deep waits
	// at least 4 ms, and intervals repeat at most every 4 ms, so a
	// `setTimeout(f, 0)` / `setImmediate` chain cannot spin a core. While the
	// window is not focused the floor is one frame (16 ms).
	const NEST_LEVELS = 5;
	let nestNow = 0;
	lunaInternal.focused = true;
	lunaInternal.setTimersFocused = (focused) => {
		lunaInternal.focused = !!focused;
	};
	const clampFloor = () => (lunaInternal.focused ? 4 : 16);
	const addTimer = (cb, ms, args, repeat, internal) => {
		const id = timerSeq++;
		let delay = Math.max(0, Number(ms) || 0);
		const nesting = internal ? 0 : nestNow + 1;
		if (!internal && (repeat || nesting > NEST_LEVELS)) delay = Math.max(delay, clampFloor());
		const plugin = internal ? undefined : (lunaInternal.running ?? lunaInternal.pluginFromStack());
		timers.set(id, { cb, args, due: Date.now() + delay, every: repeat ? Math.max(1, delay) : 0, plugin, internal: !!internal, nesting });
		return id;
	};
	// While the window is hidden (minimized, closed to the tray) plugin timers
	// run at most once a second, together, as in a browser's background tab;
	// the runtime's own timers keep their time.
	const HIDDEN_STEP = 1000;
	let hiddenRun = 0;
	lunaInternal.hidden = false;
	const dueOf = (t) => (lunaInternal.hidden && !t.internal ? Math.max(t.due, hiddenRun + HIDDEN_STEP) : t.due);
	lunaInternal.setTimersHidden = (hidden) => {
		lunaInternal.hidden = hidden;
		hiddenRun = Date.now();
	};
	// Runtime bookkeeping timers: never cleared with a plugin's own timers.
	lunaInternal.internalTimeout = (cb, ms) => addTimer(cb, ms, [], false, true);
	const timerId = (t) => (typeof t === "object" && t !== null ? t[Symbol.toPrimitive]?.() ?? t.id : t);
	g.setTimeout = (cb, ms, ...args) => addTimer(cb, ms, args, false);
	g.setInterval = (cb, ms, ...args) => addTimer(cb, ms, args, true);
	g.setImmediate = (cb, ...args) => addTimer(cb, 0, args, false);
	g.clearTimeout = g.clearInterval = g.clearImmediate = (t) => timers.delete(timerId(t));
	g.requestAnimationFrame = (cb) => addTimer(() => cb(Date.now()), 16, [], false);
	g.cancelAnimationFrame = g.clearTimeout;
	g.requestIdleCallback = (cb) => addTimer(() => cb({ didTimeout: false, timeRemaining: () => 10 }), 1, [], false);
	g.cancelIdleCallback = g.clearTimeout;
	lunaInternal.clearTimersOf = (plugin) => {
		for (const [id, t] of timers) if (t.plugin === plugin) timers.delete(id);
	};
	// `TIDALFAST_LUNA_STATS`: what ran, summed per 10 s (see LoopStats in runtime.rs).
	const stats = new Map();
	let statsSince = Date.now();
	const stat = g.__lunaStatsOn
		? (key, t0) => {
				const s = stats.get(key) ?? { n: 0, ms: 0 };
				s.n++;
				s.ms += Date.now() - t0;
				stats.set(key, s);
			}
		: null;
	lunaInternal.stat = stat;
	// The time to the next timer (-1: none).
	const nextDue = () => {
		let next = -1;
		for (const t of timers.values()) {
			const d = Math.max(0, dueOf(t) - Date.now());
			if (next < 0 || d < next) next = d;
		}
		return next;
	};
	// `peek`: only the time to the next timer, nothing run.
	g.__luna_tick = (mode) => {
		if (mode === "peek") return nextDue();
		const now = Date.now();
		const due = [];
		for (const [id, t] of timers) if (dueOf(t) <= now) due.push([id, t]);
		due.sort((a, b) => a[1].due - b[1].due || a[0] - b[0]);
		if (lunaInternal.hidden && due.some(([, t]) => !t.internal)) hiddenRun = now;
		// The deepest nesting that runs now carries over to the promise jobs
		// that follow this tick (they belong to the same timer task).
		let deepest = 0;
		for (const [id, t] of due) {
			if (!timers.has(id)) continue;
			if (t.every) {
				if (!t.internal) t.every = Math.max(t.every, clampFloor());
				t.due = now + t.every;
			} else timers.delete(id);
			const t0 = stat ? Date.now() : 0;
			lunaInternal.running = t.plugin;
			nestNow = t.nesting ?? 0;
			if (nestNow > deepest) deepest = nestNow;
			try {
				if (typeof t.cb === "function") t.cb(...t.args);
			} catch (err) {
				lunaInternal.reportError(err);
			} finally {
				lunaInternal.running = undefined;
			}
			if (stat) stat(`timer ${t.plugin ?? "luna"}${t.every ? ` every ${t.every}ms` : ""}`, t0);
		}
		nestNow = deepest;
		if (stat && now - statsSince >= 10000) {
			const parts = [...stats].sort((a, b) => b[1].ms - a[1].ms || b[1].n - a[1].n).map(([k, s]) => `${k}: ${s.n}x ${s.ms}ms`);
			callOp("log", { plugin: null, level: "info", msg: `js stats ${Math.round((now - statsSince) / 1000)}s: ${parts.join("; ")}` });
			stats.clear();
			statsSince = now;
		}
		return nextDue();
	};
	lunaInternal.reportError = (err) => {
		const stack = err?.stack ?? "";
		try {
			callOp("log", { plugin: lunaInternal.pluginFromStack(stack), level: "error", msg: `Uncaught ${fmt(err)}` });
		} catch {}
	};

	// V8 stack API (`Error.captureStackTrace` + `Error.prepareStackTrace`
	// receiving CallSites), which libraries such as depd use while loading.
	// QuickJS's CallSites lack some of V8's methods; fill them in.
	{
		const callSiteDefaults = {
			getFileName: () => undefined,
			getLineNumber: () => null,
			getColumnNumber: () => null,
			getFunctionName: () => null,
			getFunction: () => undefined,
			getThis: () => undefined,
			getTypeName: () => null,
			getMethodName: () => null,
			getEvalOrigin: () => undefined,
			getScriptNameOrSourceURL() {
				return this.getFileName();
			},
			getScriptHash: () => "",
			getEnclosingLineNumber: () => null,
			getEnclosingColumnNumber: () => null,
			getPosition: () => 0,
			getPromiseIndex: () => null,
			isEval: () => false,
			isNative: () => false,
			isToplevel: () => true,
			isConstructor: () => false,
			isAsync: () => false,
			isPromiseAll: () => false,
			toString() {
				return `${this.getFunctionName() ?? "<anonymous>"} (${this.getFileName() ?? "<anonymous>"}:${this.getLineNumber()}:${this.getColumnNumber()})`;
			},
		};
		class CallSite {
			constructor(fn, file, line, col) {
				Object.assign(this, { fn, file, line, col });
			}
			getFileName() {
				return this.file;
			}
			getLineNumber() {
				return this.line;
			}
			getColumnNumber() {
				return this.col;
			}
			getFunctionName() {
				return this.fn;
			}
		}
		for (const [k, v] of Object.entries(callSiteDefaults)) if (!(k in CallSite.prototype)) CallSite.prototype[k] = v;
		const parseSites = (stack) =>
			String(stack ?? "")
				.split("\n")
				.map((l) => /^\s*at (?:(.*?) \()?(.*?)(?::(\d+):(\d+))?\)?$/.exec(l))
				.filter(Boolean)
				.map((m) => new CallSite(m[1] ?? null, m[2], m[3] ? +m[3] : null, m[4] ? +m[4] : null));
		const native = Error.captureStackTrace;
		let patched = false;
		const patchNativeSites = (sites) => {
			if (patched || !Array.isArray(sites) || !sites[0]) return;
			patched = true;
			const proto = Object.getPrototypeOf(sites[0]);
			if (proto && proto !== Object.prototype) for (const [k, v] of Object.entries(callSiteDefaults)) if (typeof proto[k] !== "function") Object.defineProperty(proto, k, { value: v, configurable: true, writable: true });
		};
		Error.stackTraceLimit ??= 10;
		Error.captureStackTrace = function captureStackTrace(obj, fn) {
			if (typeof native === "function") {
				const prep = Error.prepareStackTrace;
				if (typeof prep === "function") {
					// Let the engine build its CallSites, complete them, then hand over.
					let sites;
					Error.prepareStackTrace = (e, s) => ((sites = s), "");
					try {
						native.call(this, obj, fn);
						void obj.stack;
					} finally {
						Error.prepareStackTrace = prep;
					}
					if (Array.isArray(sites)) {
						patchNativeSites(sites);
						const v = prep(obj, sites);
						Object.defineProperty(obj, "stack", { value: v, configurable: true, writable: true });
						return;
					}
				}
				try {
					return native.call(this, obj, fn);
				} catch {}
			}
			const raw = new Error().stack ?? "";
			const prep = Error.prepareStackTrace;
			const value = typeof prep === "function" ? prep(obj, parseSites(raw).slice(1)) : raw.split("\n").slice(1).join("\n");
			Object.defineProperty(obj, "stack", { value, configurable: true, writable: true });
		};
	}

	g.queueMicrotask ??= (cb) => Promise.resolve().then(cb).catch(lunaInternal.reportError);
	Promise.withResolvers ??= function () {
		let resolve, reject;
		const promise = new this((a, b) => ((resolve = a), (reject = b)));
		return { promise, resolve, reject };
	};
	g.structuredClone ??= (v) => {
		const seen = new Map();
		const clone = (x) => {
			if (x === null || typeof x !== "object") return x;
			if (seen.has(x)) return seen.get(x);
			if (x instanceof Date) return new Date(x.getTime());
			if (x instanceof RegExp) return new RegExp(x.source, x.flags);
			if (ArrayBuffer.isView(x)) return x.slice();
			if (x instanceof ArrayBuffer) return x.slice(0);
			if (x instanceof Map) {
				const m = new Map();
				seen.set(x, m);
				for (const [k, val] of x) m.set(clone(k), clone(val));
				return m;
			}
			if (x instanceof Set) {
				const s = new Set();
				seen.set(x, s);
				for (const val of x) s.add(clone(val));
				return s;
			}
			const out = Array.isArray(x) ? [] : {};
			seen.set(x, out);
			for (const k of Object.keys(x)) out[k] = clone(x[k]);
			return out;
		};
		return clone(v);
	};

	// ---------------------------------------------------------------- performance / crypto
	const t0 = Date.now();
	g.performance = { now: () => Date.now() - t0, timeOrigin: t0, mark: () => {}, measure: () => {}, getEntriesByName: () => [] };
	const randomBytes = (n) => lunaInternal.b64dec(callOp("crypto.random", { n }));
	lunaInternal.randomBytes = randomBytes;
	g.crypto = {
		getRandomValues(arr) {
			const u = toU8(arr);
			u.set(randomBytes(u.length));
			return arr;
		},
		randomUUID() {
			const b = randomBytes(16);
			b[6] = (b[6] & 0x0f) | 0x40;
			b[8] = (b[8] & 0x3f) | 0x80;
			const h = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
			return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
		},
		subtle: {
			async digest(alg, data) {
				const name = (typeof alg === "string" ? alg : alg.name).toLowerCase().replace("-", "");
				return lunaInternal.b64dec(callOp("crypto.hash", { alg: name, data: lunaInternal.b64enc(data) })).buffer;
			},
		},
	};

	// ---------------------------------------------------------------- events
	class Event {
		constructor(type, init = {}) {
			this.type = type;
			this.bubbles = !!init.bubbles;
			this.cancelable = !!init.cancelable;
			this.defaultPrevented = false;
			this.timeStamp = Date.now();
			this.target = null;
			this.currentTarget = null;
		}
		preventDefault() {
			if (this.cancelable) this.defaultPrevented = true;
		}
		stopPropagation() {}
		stopImmediatePropagation() {
			this._stop = true;
		}
	}
	class CustomEvent extends Event {
		constructor(type, init = {}) {
			super(type, init);
			this.detail = init.detail ?? null;
		}
	}
	class EventTarget {
		#l = new Map();
		addEventListener(type, cb, opts) {
			if (!cb) return;
			if (!this.#l.has(type)) this.#l.set(type, []);
			this.#l.get(type).push({ cb, once: typeof opts === "object" && !!opts?.once });
		}
		removeEventListener(type, cb) {
			const l = this.#l.get(type);
			if (l) this.#l.set(type, l.filter((x) => x.cb !== cb));
		}
		dispatchEvent(ev) {
			ev.target ??= this;
			ev.currentTarget = this;
			for (const x of [...(this.#l.get(ev.type) ?? [])]) {
				if (x.once) this.removeEventListener(ev.type, x.cb);
				try {
					typeof x.cb === "function" ? x.cb.call(this, ev) : x.cb.handleEvent(ev);
				} catch (err) {
					lunaInternal.reportError(err);
				}
				if (ev._stop) break;
			}
			return !ev.defaultPrevented;
		}
	}
	g.Event = Event;
	g.CustomEvent = CustomEvent;
	g.EventTarget = EventTarget;
	// `code`: the legacy number browsers still give each named exception
	// (a full store is QuotaExceededError, 22, which code checks for).
	const DOM_CODES = { IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5, NoModificationAllowedError: 7, NotFoundError: 8, NotSupportedError: 9, InvalidStateError: 11, SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14, InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20, URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24, DataCloneError: 25 };
	g.DOMException ??= class DOMException extends Error {
		constructor(message = "", name = "Error") {
			super(message);
			this.name = name;
		}
		get code() {
			return DOM_CODES[this.name] ?? 0;
		}
	};
	class AbortSignal extends EventTarget {
		constructor() {
			super();
			this.aborted = false;
			this.reason = undefined;
			this.onabort = null;
		}
		throwIfAborted() {
			if (this.aborted) throw this.reason;
		}
		static abort(reason) {
			const c = new AbortController();
			c.abort(reason);
			return c.signal;
		}
		static timeout(ms) {
			const c = new AbortController();
			setTimeout(() => c.abort(new DOMException("The operation timed out.", "TimeoutError")), ms);
			return c.signal;
		}
		static any(signals) {
			const c = new AbortController();
			for (const s of signals) {
				if (s.aborted) c.abort(s.reason);
				else s.addEventListener("abort", () => c.abort(s.reason));
			}
			return c.signal;
		}
	}
	class AbortController {
		constructor() {
			this.signal = new AbortSignal();
		}
		abort(reason) {
			const s = this.signal;
			if (s.aborted) return;
			s.aborted = true;
			s.reason = reason ?? new DOMException("This operation was aborted", "AbortError");
			const ev = new Event("abort");
			s.onabort?.(ev);
			s.dispatchEvent(ev);
		}
	}
	g.AbortController = AbortController;
	g.AbortSignal = AbortSignal;

	// ---------------------------------------------------------------- URL
	class URLSearchParams {
		#p = [];
		constructor(init) {
			if (init === undefined || init === null) return;
			if (typeof init === "string") {
				const s = init.startsWith("?") ? init.slice(1) : init;
				for (const part of s.split("&")) {
					if (part === "") continue;
					const i = part.indexOf("=");
					const dec = (x) => decodeURIComponent(x.replace(/\+/g, " "));
					this.#p.push(i < 0 ? [dec(part), ""] : [dec(part.slice(0, i)), dec(part.slice(i + 1))]);
				}
			} else if (init instanceof URLSearchParams || Array.isArray(init) || typeof init[Symbol.iterator] === "function") {
				for (const [k, v] of init) this.#p.push([String(k), String(v)]);
			} else for (const k of Object.keys(init)) if (init[k] !== undefined) this.#p.push([k, String(init[k])]);
		}
		append(k, v) {
			this.#p.push([String(k), String(v)]);
			this._changed?.();
		}
		delete(k) {
			this.#p = this.#p.filter(([x]) => x !== k);
			this._changed?.();
		}
		get(k) {
			return this.#p.find(([x]) => x === k)?.[1] ?? null;
		}
		getAll(k) {
			return this.#p.filter(([x]) => x === k).map(([, v]) => v);
		}
		has(k) {
			return this.#p.some(([x]) => x === k);
		}
		set(k, v) {
			const i = this.#p.findIndex(([x]) => x === k);
			if (i < 0) this.#p.push([String(k), String(v)]);
			else {
				this.#p[i] = [String(k), String(v)];
				this.#p = this.#p.filter(([x], j) => x !== k || j === i);
			}
			this._changed?.();
		}
		sort() {
			this.#p.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
			this._changed?.();
		}
		forEach(cb, thisArg) {
			for (const [k, v] of this.#p) cb.call(thisArg, v, k, this);
		}
		keys() {
			return this.#p.map(([k]) => k)[Symbol.iterator]();
		}
		values() {
			return this.#p.map(([, v]) => v)[Symbol.iterator]();
		}
		entries() {
			return this.#p.map((x) => [...x])[Symbol.iterator]();
		}
		[Symbol.iterator]() {
			return this.entries();
		}
		get size() {
			return this.#p.length;
		}
		toString() {
			const enc = (s) => encodeURIComponent(s).replace(/%20/g, "+").replace(/[!'()~]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());
			return this.#p.map(([k, v]) => `${enc(k)}=${enc(v)}`).join("&");
		}
	}
	g.URLSearchParams = URLSearchParams;
	class URL {
		#c;
		#sp;
		constructor(href, base) {
			this.#set(callOp("url.parse", { href: String(href), base: base === undefined ? null : String(base) }));
		}
		#set(c) {
			this.#c = c;
			this.#sp = new URLSearchParams(c.search);
			this.#sp._changed = () => {
				const s = this.#sp.toString();
				this.#c = callOp("url.set", { href: this.#c.href, part: "search", value: s });
			};
		}
		#upd(part, value) {
			this.#set(callOp("url.set", { href: this.#c.href, part, value: String(value) }));
		}
		static canParse(h, b) {
			try {
				new URL(h, b);
				return true;
			} catch {
				return false;
			}
		}
		static parse(h, b) {
			try {
				return new URL(h, b);
			} catch {
				return null;
			}
		}
		// Blob URLs remember their blob, so an <img> or Image can load one.
		// A blob of JavaScript can be imported (a module, a Worker's script)
		// as the plugin that made the URL: its bytes are kept as they were
		// then (js/ml.js).
		static createObjectURL(blob) {
			const url = "blob:luna/" + crypto.randomUUID();
			if (blob && typeof blob === "object") {
				lunaInternal.blobUrls.set(url, blob);
				if (blob._bytes instanceof Uint8Array && /^(text|application)\/(x-)?(java|ecma)script\b/i.test(String(blob.type ?? ""))) {
					lunaInternal.blobModules.set(url, { owner: lunaInternal.pluginFromStack() ?? null, bytes: blob._bytes.slice() });
				}
			}
			return url;
		}
		static revokeObjectURL(url) {
			lunaInternal.blobUrls.delete(String(url));
			lunaInternal.blobModules.delete(String(url));
		}
		get href() {
			return this.#c.href;
		}
		set href(v) {
			this.#set(callOp("url.parse", { href: String(v), base: null }));
		}
		get origin() {
			return this.#c.origin;
		}
		get protocol() {
			return this.#c.protocol;
		}
		set protocol(v) {
			this.#upd("protocol", v);
		}
		get username() {
			return this.#c.username;
		}
		set username(v) {
			this.#upd("username", v);
		}
		get password() {
			return this.#c.password;
		}
		set password(v) {
			this.#upd("password", v);
		}
		get host() {
			return this.#c.host;
		}
		set host(v) {
			this.#upd("host", v);
		}
		get hostname() {
			return this.#c.hostname;
		}
		set hostname(v) {
			this.#upd("hostname", v);
		}
		get port() {
			return this.#c.port;
		}
		set port(v) {
			this.#upd("port", v);
		}
		get pathname() {
			return this.#c.pathname;
		}
		set pathname(v) {
			this.#upd("pathname", v);
		}
		get search() {
			return this.#c.search;
		}
		set search(v) {
			this.#upd("search", v);
		}
		get searchParams() {
			return this.#sp;
		}
		get hash() {
			return this.#c.hash;
		}
		set hash(v) {
			this.#upd("hash", v);
		}
		toString() {
			return this.href;
		}
		toJSON() {
			return this.href;
		}
	}
	g.URL = URL;

	// ---------------------------------------------------------------- Blob / fetch
	class Blob {
		constructor(parts = [], opts = {}) {
			const chunks = parts.map((p) => (typeof p === "string" ? lunaInternal.utf8enc(p) : p instanceof Blob ? p._bytes : toU8(p)));
			const len = chunks.reduce((n, c) => n + c.length, 0);
			this._bytes = new Uint8Array(len);
			let o = 0;
			for (const c of chunks) {
				this._bytes.set(c, o);
				o += c.length;
			}
			this.type = opts.type ?? "";
		}
		get size() {
			return this._bytes.length;
		}
		async text() {
			return lunaInternal.utf8dec(this._bytes);
		}
		async arrayBuffer() {
			return this._bytes.slice().buffer;
		}
		async bytes() {
			return this._bytes.slice();
		}
		slice(a, b, type) {
			return new Blob([this._bytes.slice(a, b)], { type });
		}
	}
	g.Blob = Blob;
	g.File ??= class File extends Blob {
		constructor(parts, name, opts) {
			super(parts, opts);
			this.name = name;
			this.lastModified = Date.now();
		}
	};
	class Headers {
		#h = new Map();
		constructor(init) {
			if (!init) return;
			if (init instanceof Headers || Array.isArray(init)) for (const [k, v] of init) this.append(k, v);
			else for (const k of Object.keys(init)) this.append(k, init[k]);
		}
		append(k, v) {
			k = String(k).toLowerCase();
			const old = this.#h.get(k);
			this.#h.set(k, old === undefined ? String(v) : `${old}, ${v}`);
		}
		set(k, v) {
			this.#h.set(String(k).toLowerCase(), String(v));
		}
		get(k) {
			return this.#h.get(String(k).toLowerCase()) ?? null;
		}
		has(k) {
			return this.#h.has(String(k).toLowerCase());
		}
		delete(k) {
			this.#h.delete(String(k).toLowerCase());
		}
		forEach(cb, t) {
			for (const [k, v] of this.#h) cb.call(t, v, k, this);
		}
		keys() {
			return this.#h.keys();
		}
		values() {
			return this.#h.values();
		}
		entries() {
			return this.#h.entries();
		}
		getSetCookie() {
			return [];
		}
		[Symbol.iterator]() {
			return this.#h.entries();
		}
	}
	g.Headers = Headers;
	const bodyBytes = (body) => {
		if (body === undefined || body === null) return undefined;
		if (typeof body === "string") return lunaInternal.utf8enc(body);
		if (body instanceof URLSearchParams) return lunaInternal.utf8enc(body.toString());
		if (body instanceof Blob) return body._bytes;
		return toU8(body);
	};
	class Response {
		constructor(body, init = {}) {
			this._bytes = bodyBytes(body) ?? new Uint8Array(0);
			// A large fetched body waits in a file until it is read.
			this._file = init._file;
			this.status = init.status ?? 200;
			this.statusText = init.statusText ?? "";
			this.headers = new Headers(init.headers);
			this.url = init.url ?? "";
			this.redirected = false;
			this.type = "basic";
			this.bodyUsed = false;
		}
		get ok() {
			return this.status >= 200 && this.status < 300;
		}
		#use() {
			if (this.bodyUsed) throw new TypeError("Body has already been consumed.");
			this.bodyUsed = true;
			return this.#load();
		}
		#load() {
			if (this._file !== undefined) {
				const token = this._file;
				this._file = undefined;
				this._bytes = new Uint8Array(native.fetchTake(token));
			}
			return this._bytes;
		}
		async text() {
			return lunaInternal.utf8dec(this.#use());
		}
		async json() {
			return JSON.parse(await this.text());
		}
		async arrayBuffer() {
			const b = this.#use();
			// A whole buffer read from its file is the plugin's alone: no copy.
			return this._fromFile && b.byteOffset === 0 && b.byteLength === b.buffer.byteLength ? b.buffer : b.slice().buffer;
		}
		async bytes() {
			return this.#use().slice();
		}
		async blob() {
			return new Blob([this.#use()], { type: this.headers.get("content-type") ?? "" });
		}
		clone() {
			return new Response(this.#load().slice(), { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url });
		}
		static json(data, init = {}) {
			const h = new Headers(init.headers);
			h.set("content-type", "application/json");
			return new Response(JSON.stringify(data), { ...init, headers: h });
		}
		static error() {
			return new Response(null, { status: 0 });
		}
	}
	g.Response = Response;
	class Request {
		constructor(input, init = {}) {
			const base = input instanceof Request ? input : {};
			this.url = input instanceof Request ? input.url : String(input);
			this.method = (init.method ?? base.method ?? "GET").toUpperCase();
			this.headers = new Headers(init.headers ?? base.headers);
			this._body = init.body ?? base._body;
			this.signal = init.signal ?? base.signal;
		}
	}
	g.Request = Request;
	// The requests that change the signed-in account: not a read, to TIDAL's
	// API, as the account (`TidalApi.getAuthHeaders()`).
	lunaInternal.isAccountWrite = (method, url, headers) => {
		if (/^(GET|HEAD|OPTIONS)$/.test(method)) return false;
		let u;
		try {
			u = new URL(url);
		} catch {
			return false;
		}
		// v1, and v2's playlist folders (`/v2/my-collection/...`).
		if (u.protocol !== "https:" || !/^(desktop|api|listen)\.tidal\.com$/.test(u.hostname) || !(u.pathname.startsWith("/v1/") || u.pathname.startsWith("/v2/my-collection/"))) return false;
		return Object.entries(headers).some(([k, v]) => k.toLowerCase() === "authorization" && v === "Bearer tidalfast-account");
	};
	g.fetch = async (input, init = {}) => {
		const req = new Request(input, init);
		const url = new URL(req.url).href;
		const signal = req.signal;
		if (signal?.aborted) throw signal.reason;
		const headers = {};
		for (const [k, v] of req.headers) headers[k] = v;
		// L.resolveLocalStream asking a plugin's proxy what it fetches.
		if (lunaInternal.captureFetch) {
			lunaInternal.captureFetch.push({ url, method: req.method });
			throw Object.assign(new Error("The operation was aborted."), { name: "AbortError" });
		}
		const plugin = lunaInternal.pluginFromStack();
		// A write to the signed-in account (playlist-tools removing
		// duplicates) goes ahead only after the user said yes to that plugin;
		// the runtime checks the answer again before the host carries it out.
		if (lunaInternal.isAccountWrite(req.method, url, headers) && !(await lunaInternal.allowAccountWrite(plugin, req.method, url))) {
			throw new TypeError("fetch failed: you did not allow this plugin to change your TIDAL account");
		}
		const body = bodyBytes(req._body);
		if (body !== undefined && !req.headers.has("content-type") && typeof req._body === "string") headers["content-type"] = "text/plain;charset=UTF-8";
		if (req._body instanceof URLSearchParams && !req.headers.has("content-type")) headers["content-type"] = "application/x-www-form-urlencoded;charset=UTF-8";
		const work = callOpAsync("fetch", {
			plugin,
			url,
			method: req.method,
			headers,
			body: body === undefined ? null : lunaInternal.b64enc(body),
			redirect: init.redirect ?? "follow",
		});
		const res = await (signal
			? Promise.race([work, new Promise((_, rej) => signal.addEventListener("abort", () => rej(signal.reason)))])
			: work);
		if (typeof res.file === "string") {
			const r = new Response(null, { status: res.status, statusText: res.statusText, headers: res.headers, url: res.url, _file: res.file });
			r._fromFile = true;
			return r;
		}
		return new Response(lunaInternal.b64dec(res.body), { status: res.status, statusText: res.statusText, headers: res.headers, url: res.url });
	};

	// ---------------------------------------------------------------- web streams (minimal)
	// Enough for libraries that build on them at load time (undici); not a
	// full implementation.
	class ReadableStream {
		constructor(source = {}, strategy) {
			this._queue = [];
			this._closed = false;
			this._waiters = [];
			this.locked = false;
			const controller = {
				enqueue: (c) => {
					const w = this._waiters.shift();
					w ? w({ value: c, done: false }) : this._queue.push(c);
				},
				close: () => {
					this._closed = true;
					for (const w of this._waiters.splice(0)) w({ value: undefined, done: true });
				},
				error: (e) => {
					this._error = e;
				},
				desiredSize: 1,
			};
			try {
				source.start?.(controller);
			} catch {}
			this._pull = () => source.pull?.(controller);
		}
		getReader() {
			this.locked = true;
			return {
				read: async () => {
					if (this._error) throw this._error;
					if (this._queue.length) return { value: this._queue.shift(), done: false };
					if (this._closed) return { value: undefined, done: true };
					await this._pull();
					if (this._queue.length) return { value: this._queue.shift(), done: false };
					if (this._closed) return { value: undefined, done: true };
					return new Promise((r) => this._waiters.push(r));
				},
				releaseLock: () => (this.locked = false),
				cancel: async () => (this._closed = true),
				closed: Promise.resolve(),
			};
		}
		cancel() {
			this._closed = true;
			return Promise.resolve();
		}
		tee() {
			return [this, this];
		}
		async *[Symbol.asyncIterator]() {
			const r = this.getReader();
			while (true) {
				const { value, done } = await r.read();
				if (done) return;
				yield value;
			}
		}
		static from(it) {
			return new ReadableStream({
				async start(c) {
					for await (const x of it) c.enqueue(x);
					c.close();
				},
			});
		}
	}
	class WritableStream {
		constructor(sink = {}) {
			this._sink = sink;
			this.locked = false;
		}
		getWriter() {
			return { write: async (c) => this._sink.write?.(c), close: async () => this._sink.close?.(), abort: async (e) => this._sink.abort?.(e), releaseLock() {}, ready: Promise.resolve(), closed: Promise.resolve() };
		}
	}
	class TransformStream {
		constructor() {
			this.readable = new ReadableStream();
			this.writable = new WritableStream();
		}
	}
	// Response bodies are fully buffered; `body` streams them as one chunk.
	Object.defineProperty(Response.prototype, "body", {
		get() {
			if (this.status === 204 || this.status === 304) return null;
			if (!this._stream) {
				const bytes = this._bytes;
				this._stream = new ReadableStream({
					start(c) {
						if (bytes.length) c.enqueue(bytes);
						c.close();
					},
				});
			}
			return this._stream;
		},
		configurable: true,
	});
	g.ReadableStream ??= ReadableStream;
	g.WritableStream ??= WritableStream;
	g.TransformStream ??= TransformStream;
	g.ReadableStreamDefaultReader ??= class ReadableStreamDefaultReader {};
	g.ByteLengthQueuingStrategy ??= class ByteLengthQueuingStrategy {};
	g.CountQueuingStrategy ??= class CountQueuingStrategy {};
	g.FormData ??= class FormData {
		constructor() {
			this._e = [];
		}
		append(k, v) {
			this._e.push([k, v]);
		}
		get(k) {
			return this._e.find((e) => e[0] === k)?.[1] ?? null;
		}
		entries() {
			return this._e[Symbol.iterator]();
		}
		[Symbol.iterator]() {
			return this.entries();
		}
	};
	g.MessageChannel ??= class MessageChannel {
		constructor() {
			this.port1 = new EventTarget();
			this.port2 = new EventTarget();
		}
	};
	// ---------------------------------------------------------------- DOMMatrix
	// 4x4 matrices in CSS's column order (m11, m12, m13, m14, m21, ...), built
	// from arrays or CSS transform lists, as `getComputedStyle(el).transform`
	// gives them (3d-cover reads TIDAL's cover tilt this way).
	const IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
	const mul4 = (a, b) => {
		const out = new Array(16);
		for (let c = 0; c < 4; c++)
			for (let r = 0; r < 4; r++) {
				let s = 0;
				for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
				out[c * 4 + r] = s;
			}
		return out;
	};
	const ANGLE = { deg: Math.PI / 180, rad: 1, grad: Math.PI / 200, turn: 2 * Math.PI };
	// One CSS transform list as a matrix and whether it is 2-D; `box` sizes
	// percentages (none: a percentage is a syntax error, as for DOMMatrix).
	const parseTransform = (text, box) => {
		const s = String(text ?? "").trim();
		if (s === "" || s === "none") return { m: IDENTITY.slice(), is2D: true };
		let m = IDENTITY.slice();
		let is2D = true;
		const re = /([a-zA-Z0-9]+)\(([^)]*)\)/gy;
		let at = 0;
		const bad = () => {
			throw new DOMException(`Failed to parse '${s}' as a transform list`, "SyntaxError");
		};
		while (at < s.length) {
			while (s[at] === " " || s[at] === "\t" || s[at] === "\n") at++;
			if (at >= s.length) break;
			re.lastIndex = at;
			const hit = re.exec(s);
			if (!hit) bad();
			at = re.lastIndex;
			const fn = hit[1].toLowerCase();
			const args = hit[2].split(",").map((a) => a.trim()).filter((a) => a !== "");
			const len = (a, axis) => {
				const v = /^(-?[\d.]+(?:e[+-]?\d+)?)(px|%)?$/i.exec(a);
				if (!v) bad();
				if (v[2] === "%") return box ? (parseFloat(v[1]) / 100) * (axis === 1 ? box[1] : box[0]) : bad();
				if (!v[2] && parseFloat(v[1]) !== 0 && fn !== "matrix" && fn !== "matrix3d") bad();
				return parseFloat(v[1]);
			};
			const num = (a) => {
				const v = Number(a);
				if (!Number.isFinite(v)) bad();
				return v;
			};
			const ang = (a) => {
				const v = /^(-?[\d.]+(?:e[+-]?\d+)?)(deg|rad|grad|turn)?$/i.exec(a);
				if (!v || (!v[2] && parseFloat(v[1]) !== 0)) bad();
				return parseFloat(v[1]) * ANGLE[(v[2] ?? "rad").toLowerCase()];
			};
			const f = IDENTITY.slice();
			const rot = (x, y, z, a) => {
				const n = Math.hypot(x, y, z) || 1;
				[x, y, z] = [x / n, y / n, z / n];
				const c = Math.cos(a);
				const sn = Math.sin(a);
				const t = 1 - c;
				f.splice(0, 16, t * x * x + c, t * x * y + sn * z, t * x * z - sn * y, 0, t * x * y - sn * z, t * y * y + c, t * y * z + sn * x, 0, t * x * z + sn * y, t * y * z - sn * x, t * z * z + c, 0, 0, 0, 0, 1);
			};
			switch (fn) {
				case "matrix":
					if (args.length !== 6) bad();
					[f[0], f[1], f[4], f[5], f[12], f[13]] = args.map(num);
					break;
				case "matrix3d":
					if (args.length !== 16) bad();
					f.splice(0, 16, ...args.map(num));
					is2D = false;
					break;
				case "translate":
					f[12] = len(args[0] ?? "0", 0);
					f[13] = len(args[1] ?? "0", 1);
					break;
				case "translatex":
					f[12] = len(args[0], 0);
					break;
				case "translatey":
					f[13] = len(args[0], 1);
					break;
				case "translatez":
					f[14] = len(args[0], 2);
					is2D = false;
					break;
				case "translate3d":
					[f[12], f[13], f[14]] = [len(args[0], 0), len(args[1], 1), len(args[2], 2)];
					is2D = false;
					break;
				case "scale":
					f[0] = num(args[0]);
					f[5] = num(args[1] ?? args[0]);
					break;
				case "scalex":
					f[0] = num(args[0]);
					break;
				case "scaley":
					f[5] = num(args[0]);
					break;
				case "scalez":
					f[10] = num(args[0]);
					is2D = false;
					break;
				case "scale3d":
					[f[0], f[5], f[10]] = args.map(num);
					is2D = false;
					break;
				case "rotate":
				case "rotatez":
					rot(0, 0, 1, ang(args[0]));
					if (fn === "rotatez") is2D = false;
					break;
				case "rotatex":
					rot(1, 0, 0, ang(args[0]));
					is2D = false;
					break;
				case "rotatey":
					rot(0, 1, 0, ang(args[0]));
					is2D = false;
					break;
				case "rotate3d":
					rot(num(args[0]), num(args[1]), num(args[2]), ang(args[3]));
					is2D = false;
					break;
				case "skew":
					f[4] = Math.tan(ang(args[0]));
					f[1] = Math.tan(ang(args[1] ?? "0"));
					break;
				case "skewx":
					f[4] = Math.tan(ang(args[0]));
					break;
				case "skewy":
					f[1] = Math.tan(ang(args[0]));
					break;
				case "perspective": {
					const d = args[0] === "none" ? 0 : len(args[0], 2);
					if (d > 0) f[11] = -1 / d;
					is2D = false;
					break;
				}
				default:
					bad();
			}
			m = mul4(m, f);
		}
		return { m, is2D };
	};
	lunaInternal.parseTransform = parseTransform;
	const NAMES = [];
	for (let c = 1; c <= 4; c++) for (let r = 1; r <= 4; r++) NAMES.push(`m${c}${r}`);
	const ALIAS = { a: 0, b: 1, c: 4, d: 5, e: 12, f: 13 };
	const inverse4 = (m) => {
		const inv = new Array(16);
		inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
		inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
		inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
		inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
		inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
		inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
		inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
		inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
		inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
		inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
		inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
		inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
		inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
		inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
		inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
		inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
		const det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
		return det === 0 ? null : inv.map((v) => v / det);
	};
	class DOMMatrixReadOnly {
		constructor(init) {
			let m = IDENTITY.slice();
			let is2D = true;
			if (typeof init === "string") ({ m, is2D } = parseTransform(init));
			else if (init != null && typeof init.length === "number") {
				const a = Array.from(init, Number);
				if (a.length === 6) m = [a[0], a[1], 0, 0, a[2], a[3], 0, 0, 0, 0, 1, 0, a[4], a[5], 0, 1];
				else if (a.length === 16) {
					m = a;
					is2D = false;
				} else throw new TypeError("A DOMMatrix needs 6 or 16 values");
			} else if (init !== undefined) throw new TypeError("Unsupported DOMMatrix init");
			Object.defineProperty(this, "_m", { value: m, writable: true });
			Object.defineProperty(this, "_2d", { value: is2D, writable: true });
		}
		static fromMatrix(o) {
			if (o instanceof DOMMatrixReadOnly) return new this(o._m.slice());
			const v = o ?? {};
			return new this([v.m11 ?? v.a ?? 1, v.m12 ?? v.b ?? 0, v.m13 ?? 0, v.m14 ?? 0, v.m21 ?? v.c ?? 0, v.m22 ?? v.d ?? 1, v.m23 ?? 0, v.m24 ?? 0, v.m31 ?? 0, v.m32 ?? 0, v.m33 ?? 1, v.m34 ?? 0, v.m41 ?? v.e ?? 0, v.m42 ?? v.f ?? 0, v.m43 ?? 0, v.m44 ?? 1]);
		}
		static fromFloat32Array(a) {
			return new this(a);
		}
		static fromFloat64Array(a) {
			return new this(a);
		}
		get is2D() {
			return this._2d;
		}
		get isIdentity() {
			return this._m.every((v, i) => v === IDENTITY[i]);
		}
		multiply(o) {
			return new DOMMatrix(this._m.slice()).multiplySelf(o);
		}
		translate(x = 0, y = 0, z = 0) {
			return new DOMMatrix(this._m.slice()).translateSelf(x, y, z);
		}
		scale(x = 1, y = x, z = 1) {
			return new DOMMatrix(this._m.slice()).scaleSelf(x, y, z);
		}
		rotate(x = 0, y, z) {
			return new DOMMatrix(this._m.slice()).rotateSelf(x, y, z);
		}
		rotateAxisAngle(x = 0, y = 0, z = 0, a = 0) {
			return new DOMMatrix(this._m.slice()).multiplySelf(new DOMMatrix(`rotate3d(${x}, ${y}, ${z}, ${a}deg)`));
		}
		skewX(a = 0) {
			return this.multiply(new DOMMatrix(`skewX(${a}deg)`));
		}
		skewY(a = 0) {
			return this.multiply(new DOMMatrix(`skewY(${a}deg)`));
		}
		inverse() {
			return new DOMMatrix(this._m.slice()).invertSelf();
		}
		flipX() {
			return this.multiply(new DOMMatrix([-1, 0, 0, 1, 0, 0]));
		}
		flipY() {
			return this.multiply(new DOMMatrix([1, 0, 0, -1, 0, 0]));
		}
		transformPoint(p = {}) {
			const [x, y, z, w] = [p.x ?? 0, p.y ?? 0, p.z ?? 0, p.w ?? 1];
			const m = this._m;
			return { x: m[0] * x + m[4] * y + m[8] * z + m[12] * w, y: m[1] * x + m[5] * y + m[9] * z + m[13] * w, z: m[2] * x + m[6] * y + m[10] * z + m[14] * w, w: m[3] * x + m[7] * y + m[11] * z + m[15] * w };
		}
		toFloat32Array() {
			return Float32Array.from(this._m);
		}
		toFloat64Array() {
			return Float64Array.from(this._m);
		}
		toString() {
			const m = this._m;
			return this._2d ? `matrix(${[m[0], m[1], m[4], m[5], m[12], m[13]].join(", ")})` : `matrix3d(${m.join(", ")})`;
		}
		toJSON() {
			const o = { is2D: this.is2D, isIdentity: this.isIdentity };
			for (const [k, i] of Object.entries(ALIAS)) o[k] = this._m[i];
			NAMES.forEach((k, i) => (o[k] = this._m[i]));
			return o;
		}
	}
	for (const [i, k] of NAMES.entries()) Object.defineProperty(DOMMatrixReadOnly.prototype, k, { get() { return this._m[i]; }, configurable: true });
	for (const [k, i] of Object.entries(ALIAS)) Object.defineProperty(DOMMatrixReadOnly.prototype, k, { get() { return this._m[i]; }, configurable: true });
	class DOMMatrix extends DOMMatrixReadOnly {
		multiplySelf(o) {
			const b = o instanceof DOMMatrixReadOnly ? o : DOMMatrixReadOnly.fromMatrix(o);
			this._m = mul4(this._m, b._m);
			this._2d = this._2d && b._2d;
			return this;
		}
		preMultiplySelf(o) {
			const b = o instanceof DOMMatrixReadOnly ? o : DOMMatrixReadOnly.fromMatrix(o);
			this._m = mul4(b._m, this._m);
			this._2d = this._2d && b._2d;
			return this;
		}
		translateSelf(x = 0, y = 0, z = 0) {
			return this.multiplySelf(new DOMMatrixReadOnly(z ? [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1] : [1, 0, 0, 1, x, y]));
		}
		scaleSelf(x = 1, y = x, z = 1) {
			return this.multiplySelf(new DOMMatrixReadOnly(z !== 1 ? [x, 0, 0, 0, 0, y, 0, 0, 0, 0, z, 0, 0, 0, 0, 1] : [x, 0, 0, y, 0, 0]));
		}
		rotateSelf(x = 0, y, z) {
			// One angle turns about z; three are x, then y, then z.
			if (y === undefined && z === undefined) return this.multiplySelf(new DOMMatrixReadOnly(`rotate(${x}deg)`));
			return this.multiplySelf(new DOMMatrixReadOnly(`rotateZ(${z ?? 0}deg) rotateY(${y ?? 0}deg) rotateX(${x}deg)`));
		}
		invertSelf() {
			const inv = inverse4(this._m);
			this._m = inv ?? new Array(16).fill(NaN);
			if (!inv) this._2d = false;
			return this;
		}
		setMatrixValue(text) {
			const { m, is2D } = parseTransform(text);
			this._m = m;
			this._2d = is2D;
			return this;
		}
	}
	for (const [i, k] of NAMES.entries())
		Object.defineProperty(DOMMatrix.prototype, k, {
			get() {
				return this._m[i];
			},
			set(v) {
				this._m[i] = Number(v);
				if (![0, 1, 4, 5, 12, 13].includes(i)) this._2d = false;
			},
			configurable: true,
		});
	for (const [k, i] of Object.entries(ALIAS))
		Object.defineProperty(DOMMatrix.prototype, k, {
			get() {
				return this._m[i];
			},
			set(v) {
				this._m[i] = Number(v);
			},
			configurable: true,
		});
	g.DOMMatrixReadOnly ??= DOMMatrixReadOnly;
	g.DOMMatrix ??= DOMMatrix;
	g.WebKitCSSMatrix ??= DOMMatrix;

	// WebAssembly: js/wasm.js (on src/luna/wasm.rs).

	// ---------------------------------------------------------------- misc
	// The browser identity TIDAL's Electron gives on the same system: its
	// default user agent (the system as Chromium names it, the app's name
	// and version, Chrome's and Electron's versions from `process.versions`)
	// and the user-agent client hints.
	const os = callOp("platform", null);
	const versions = g.__lunaVersions ?? {};
	const chrome = String(versions.chrome);
	const uaSystem = os === "win32" ? "Windows NT 10.0; Win64; x64" : os === "darwin" ? "Macintosh; Intel Mac OS X 10_15_7" : "X11; Linux x86_64";
	const userAgent = `Mozilla/5.0 (${uaSystem}) AppleWebKit/537.36 (KHTML, like Gecko) Tidalfast/${versions.tidal} Chrome/${chrome} Electron/${versions.electron} Safari/537.36`;
	const hintBrands = (full) => [
		{ brand: "Not/A)Brand", version: full ? "8.0.0.0" : "8" },
		{ brand: "Chromium", version: full ? chrome : chrome.split(".")[0] },
	];
	const hintPlatform = os === "win32" ? "Windows" : os === "darwin" ? "macOS" : "Linux";
	const userAgentData = {
		brands: hintBrands(false),
		mobile: false,
		platform: hintPlatform,
		getHighEntropyValues: async (hints) => {
			const arch = callOp("arch", null);
			const all = {
				architecture: arch === "arm64" ? "arm" : "x86",
				bitness: arch === "ia32" ? "32" : "64",
				fullVersionList: hintBrands(true),
				model: "",
				uaFullVersion: chrome,
				wow64: false,
			};
			const out = { brands: hintBrands(false), mobile: false, platform: hintPlatform };
			for (const h of Array.isArray(hints) ? hints : []) if (h in all) out[h] = all[h];
			return out;
		},
		toJSON() {
			return { brands: this.brands, mobile: this.mobile, platform: this.platform };
		},
	};
	g.navigator = {
		userAgent,
		appVersion: userAgent.slice("Mozilla/".length),
		appName: "Netscape",
		appCodeName: "Mozilla",
		product: "Gecko",
		productSub: "20030107",
		vendor: "Google Inc.",
		vendorSub: "",
		userAgentData,
		language: "en-US",
		languages: ["en-US", "en"],
		platform: os === "win32" ? "Win32" : os === "darwin" ? "MacIntel" : "Linux x86_64",
		maxTouchPoints: 0,
		cookieEnabled: true,
		webdriver: false,
		onLine: true,
		clipboard: { writeText: async (text) => callOp("clipboard", { text: String(text) }), readText: async () => callOp("clipboard.read", null) ?? "" },
		mediaSession: { metadata: null, setActionHandler: () => {}, playbackState: "none", setPositionState: () => {} },
		sendBeacon: () => true,
		hardwareConcurrency: 4,
	};
	g.alert = (msg) => callOp("notify", { plugin: lunaInternal.pluginFromStack(), level: "info", msg: String(msg) });
	g.confirm = () => false;
	g.prompt = () => null;
	g.open = (url) => {
		callOp("open_url", { url: String(url) });
		return null;
	};
	g.reportError = lunaInternal.reportError;
})();
