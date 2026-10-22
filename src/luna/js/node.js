// Node-like modules for plugins' `.native` code (which in Luna runs in
// Electron's main process). Everything is scoped to the plugin: `fs` only
// reaches the plugin's own data directory, `net` reaches local IPC pipes and
// TCP, `child_process` starts programs only after the user said yes, and
// there is no native addon loading.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const { callOp } = L;
	const PLATFORM = callOp("platform", null);
	const ENV = callOp("env", null) ?? {};

	// ---------------------------------------------------------------- events
	class EventEmitter {
		static defaultMaxListeners = 10;
		static errorMonitor = Symbol("events.errorMonitor");
		static captureRejectionSymbol = Symbol.for("nodejs.rejection");
		static captureRejections = false;
		static once(emitter, name) {
			return new Promise((res, rej) => {
				const onErr = (e) => {
					emitter.removeListener?.(name, onOk);
					rej(e);
				};
				const onOk = (...a) => {
					if (name !== "error") emitter.removeListener?.("error", onErr);
					res(a);
				};
				emitter.once(name, onOk);
				if (name !== "error") emitter.once("error", onErr);
			});
		}
		static listenerCount(e, n) {
			return e.listenerCount(n);
		}
		static setMaxListeners() {}
		static getEventListeners(e, n) {
			return e.listeners?.(n) ?? [];
		}
		constructor() {
			EventEmitter.init.call(this);
		}
		static init() {
			if (!this._events || this._events === Object.getPrototypeOf(this)._events) {
				this._events = Object.create(null);
				this._eventsCount = 0;
			}
			this._maxListeners ??= undefined;
		}
		_ev() {
			if (!this._events) EventEmitter.init.call(this);
			return this._events;
		}
		setMaxListeners(n) {
			this._maxListeners = n;
			return this;
		}
		getMaxListeners() {
			return this._maxListeners ?? EventEmitter.defaultMaxListeners;
		}
		_add(name, fn, prepend, once) {
			if (typeof fn !== "function") throw new TypeError('The "listener" argument must be of type Function');
			const ev = this._ev();
			if (ev.newListener) this.emit("newListener", name, fn.listener ?? fn);
			const entry = once ? Object.assign((...a) => (this.removeListener(name, entry), fn.apply(this, a)), { listener: fn }) : fn;
			const list = (ev[name] ??= []);
			prepend ? list.unshift(entry) : list.push(entry);
			return this;
		}
		on(n, f) {
			return this._add(n, f, false, false);
		}
		addListener(n, f) {
			return this._add(n, f, false, false);
		}
		prependListener(n, f) {
			return this._add(n, f, true, false);
		}
		once(n, f) {
			return this._add(n, f, false, true);
		}
		prependOnceListener(n, f) {
			return this._add(n, f, true, true);
		}
		removeListener(n, f) {
			const ev = this._ev();
			const list = ev[n];
			if (!list) return this;
			const i = list.findIndex((x) => x === f || x.listener === f);
			if (i >= 0) {
				list.splice(i, 1);
				if (list.length === 0) delete ev[n];
				if (ev.removeListener) this.emit("removeListener", n, f);
			}
			return this;
		}
		off(n, f) {
			return this.removeListener(n, f);
		}
		removeAllListeners(n) {
			const ev = this._ev();
			if (n === undefined) this._events = Object.create(null);
			else delete ev[n];
			return this;
		}
		emit(n, ...args) {
			const list = this._ev()[n];
			if (n === "error" && (!list || list.length === 0)) {
				const err = args[0] instanceof Error ? args[0] : new Error(`Unhandled error. (${String(args[0])})`);
				throw err;
			}
			if (!list) return false;
			for (const fn of [...list]) {
				const r = fn.apply(this, args);
				if (r && typeof r.then === "function" && typeof r.catch === "function") r.catch(L.reportError);
			}
			return true;
		}
		listeners(n) {
			return (this._ev()[n] ?? []).map((x) => x.listener ?? x);
		}
		rawListeners(n) {
			return [...(this._ev()[n] ?? [])];
		}
		listenerCount(n) {
			return (this._ev()[n] ?? []).length;
		}
		eventNames() {
			return Reflect.ownKeys(this._ev());
		}
	}
	EventEmitter.EventEmitter = EventEmitter;
	EventEmitter.on = (emitter, name) => {
		const queue = [];
		const waiters = [];
		emitter.on(name, (...a) => (waiters.length ? waiters.shift()({ value: a, done: false }) : queue.push(a)));
		return { [Symbol.asyncIterator]: () => ({ next: () => (queue.length ? Promise.resolve({ value: queue.shift(), done: false }) : new Promise((r) => waiters.push(r))), return: async () => ({ done: true }) }) };
	};
	EventEmitter.EventTarget = g.EventTarget;
	EventEmitter.Event = g.Event;
	EventEmitter.default = EventEmitter;

	// ---------------------------------------------------------------- Buffer
	const hexOf = (u) => {
		let s = "";
		for (const b of u) s += b.toString(16).padStart(2, "0");
		return s;
	};
	const normEnc = (e) => String(e ?? "utf8").toLowerCase().replace("-", "");
	const BufferClass = class Buffer extends Uint8Array {
		static poolSize = 8192;
		static from(v, encOrOffset, length) {
			if (typeof v === "string") {
				const enc = normEnc(encOrOffset);
				let u;
				if (enc === "hex") {
					const clean = v.length % 2 ? v.slice(0, -1) : v;
					u = new Uint8Array(clean.length / 2);
					for (let i = 0; i < u.length; i++) {
						const b = parseInt(clean.substr(i * 2, 2), 16);
						if (isNaN(b)) {
							u = u.subarray(0, i);
							break;
						}
						u[i] = b;
					}
				} else if (enc === "base64" || enc === "base64url") u = L.b64dec(v.replace(/-/g, "+").replace(/_/g, "/").replace(/[^A-Za-z0-9+/=]/g, ""));
				else if (enc === "latin1" || enc === "binary" || enc === "ascii") {
					u = new Uint8Array(v.length);
					for (let i = 0; i < v.length; i++) u[i] = v.charCodeAt(i) & 0xff;
				} else if (enc === "utf16le" || enc === "ucs2") {
					u = new Uint8Array(v.length * 2);
					for (let i = 0; i < v.length; i++) {
						const c = v.charCodeAt(i);
						u[i * 2] = c & 0xff;
						u[i * 2 + 1] = c >> 8;
					}
				} else u = L.utf8enc(v);
				const b = new Buffer(u.length);
				b.set(u);
				return b;
			}
			if (v instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer)) return new Buffer(v, encOrOffset ?? 0, length ?? v.byteLength - (encOrOffset ?? 0));
			if (ArrayBuffer.isView(v)) {
				const b = new Buffer(v.byteLength);
				b.set(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
				return b;
			}
			if (Array.isArray(v)) {
				const b = new Buffer(v.length);
				for (let i = 0; i < v.length; i++) b[i] = v[i] & 0xff;
				return b;
			}
			if (v && v.type === "Buffer" && Array.isArray(v.data)) return Buffer.from(v.data);
			if (v && typeof v.valueOf === "function" && v.valueOf() !== v) return Buffer.from(v.valueOf(), encOrOffset, length);
			throw new TypeError("The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object.");
		}
		static alloc(n, fill, enc) {
			const b = new Buffer(n);
			if (fill !== undefined) b.fill(fill, 0, n, enc);
			return b;
		}
		static allocUnsafe(n) {
			return new Buffer(n);
		}
		static allocUnsafeSlow(n) {
			return new Buffer(n);
		}
		static isBuffer(b) {
			return b instanceof Buffer;
		}
		static isEncoding(e) {
			return ["utf8", "hex", "base64", "base64url", "latin1", "binary", "ascii", "utf16le", "ucs2"].includes(normEnc(e));
		}
		static byteLength(v, enc) {
			if (typeof v !== "string") return v.byteLength;
			return Buffer.from(v, enc).length;
		}
		static concat(list, total) {
			total ??= list.reduce((n, b) => n + b.length, 0);
			const out = Buffer.alloc(total);
			let o = 0;
			for (const b of list) {
				if (o >= total) break;
				const part = b.length + o > total ? b.subarray(0, total - o) : b;
				out.set(part, o);
				o += part.length;
			}
			return out;
		}
		static compare(a, b) {
			return a.compare(b);
		}
		toString(enc, start = 0, end = this.length) {
			const u = this.subarray(start, end);
			switch (normEnc(enc)) {
				case "hex":
					return hexOf(u);
				case "base64":
					return L.b64enc(u);
				case "base64url":
					return L.b64enc(u).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
				case "latin1":
				case "binary":
				case "ascii": {
					let s = "";
					for (const c of u) s += String.fromCharCode(c);
					return s;
				}
				case "utf16le":
				case "ucs2": {
					let s = "";
					for (let i = 0; i + 1 < u.length; i += 2) s += String.fromCharCode(u[i] | (u[i + 1] << 8));
					return s;
				}
				default:
					return L.utf8dec(u);
			}
		}
		toJSON() {
			return { type: "Buffer", data: [...this] };
		}
		equals(o) {
			if (this.length !== o.length) return false;
			for (let i = 0; i < this.length; i++) if (this[i] !== o[i]) return false;
			return true;
		}
		compare(o) {
			const n = Math.min(this.length, o.length);
			for (let i = 0; i < n; i++) if (this[i] !== o[i]) return this[i] < o[i] ? -1 : 1;
			return this.length === o.length ? 0 : this.length < o.length ? -1 : 1;
		}
		copy(target, tStart = 0, sStart = 0, sEnd = this.length) {
			const part = this.subarray(sStart, Math.min(sEnd, sStart + target.length - tStart));
			target.set(part, tStart);
			return part.length;
		}
		slice(s, e) {
			return this.subarray(s, e);
		}
		subarray(s, e) {
			const u = Uint8Array.prototype.subarray.call(this, s, e);
			return new Buffer(u.buffer, u.byteOffset, u.byteLength);
		}
		write(str, offset = 0, length, enc) {
			if (typeof offset === "string") [enc, offset] = [offset, 0];
			if (typeof length === "string") [enc, length] = [length, undefined];
			const b = Buffer.from(str, enc);
			const n = Math.min(b.length, length ?? this.length - offset, this.length - offset);
			this.set(b.subarray(0, n), offset);
			return n;
		}
		fill(v, start = 0, end = this.length, enc) {
			if (typeof start === "string") [enc, start, end] = [start, 0, this.length];
			if (typeof v === "string") {
				const b = Buffer.from(v, enc);
				if (b.length === 0) return Uint8Array.prototype.fill.call(this, 0, start, end), this;
				for (let i = start, j = 0; i < end; i++, j = (j + 1) % b.length) this[i] = b[j];
				return this;
			}
			Uint8Array.prototype.fill.call(this, typeof v === "number" ? v & 0xff : 0, start, end);
			return this;
		}
		indexOf(v, from = 0, enc) {
			if (typeof v === "number") return Uint8Array.prototype.indexOf.call(this, v, from);
			const n = typeof v === "string" ? Buffer.from(v, enc) : v;
			outer: for (let i = from; i <= this.length - n.length; i++) {
				for (let j = 0; j < n.length; j++) if (this[i + j] !== n[j]) continue outer;
				return i;
			}
			return -1;
		}
		includes(v, from, enc) {
			return this.indexOf(v, from, enc) !== -1;
		}
		_dv() {
			return new DataView(this.buffer, this.byteOffset, this.byteLength);
		}
		readUInt8(o = 0) {
			return this._dv().getUint8(o);
		}
		readInt8(o = 0) {
			return this._dv().getInt8(o);
		}
		readUInt16LE(o = 0) {
			return this._dv().getUint16(o, true);
		}
		readUInt16BE(o = 0) {
			return this._dv().getUint16(o, false);
		}
		readInt16LE(o = 0) {
			return this._dv().getInt16(o, true);
		}
		readInt16BE(o = 0) {
			return this._dv().getInt16(o, false);
		}
		readUInt32LE(o = 0) {
			return this._dv().getUint32(o, true);
		}
		readUInt32BE(o = 0) {
			return this._dv().getUint32(o, false);
		}
		readInt32LE(o = 0) {
			return this._dv().getInt32(o, true);
		}
		readInt32BE(o = 0) {
			return this._dv().getInt32(o, false);
		}
		readFloatLE(o = 0) {
			return this._dv().getFloat32(o, true);
		}
		readFloatBE(o = 0) {
			return this._dv().getFloat32(o, false);
		}
		readDoubleLE(o = 0) {
			return this._dv().getFloat64(o, true);
		}
		readDoubleBE(o = 0) {
			return this._dv().getFloat64(o, false);
		}
		readBigUInt64LE(o = 0) {
			return this._dv().getBigUint64(o, true);
		}
		readBigUInt64BE(o = 0) {
			return this._dv().getBigUint64(o, false);
		}
		readBigInt64LE(o = 0) {
			return this._dv().getBigInt64(o, true);
		}
		readBigInt64BE(o = 0) {
			return this._dv().getBigInt64(o, false);
		}
		writeUInt8(v, o = 0) {
			this._dv().setUint8(o, v);
			return o + 1;
		}
		writeInt8(v, o = 0) {
			this._dv().setInt8(o, v);
			return o + 1;
		}
		writeUInt16LE(v, o = 0) {
			this._dv().setUint16(o, v, true);
			return o + 2;
		}
		writeUInt16BE(v, o = 0) {
			this._dv().setUint16(o, v, false);
			return o + 2;
		}
		writeInt16LE(v, o = 0) {
			this._dv().setInt16(o, v, true);
			return o + 2;
		}
		writeInt16BE(v, o = 0) {
			this._dv().setInt16(o, v, false);
			return o + 2;
		}
		writeUInt32LE(v, o = 0) {
			this._dv().setUint32(o, v, true);
			return o + 4;
		}
		writeUInt32BE(v, o = 0) {
			this._dv().setUint32(o, v, false);
			return o + 4;
		}
		writeInt32LE(v, o = 0) {
			this._dv().setInt32(o, v, true);
			return o + 4;
		}
		writeInt32BE(v, o = 0) {
			this._dv().setInt32(o, v, false);
			return o + 4;
		}
		writeFloatLE(v, o = 0) {
			this._dv().setFloat32(o, v, true);
			return o + 4;
		}
		writeDoubleLE(v, o = 0) {
			this._dv().setFloat64(o, v, true);
			return o + 8;
		}
		writeBigUInt64LE(v, o = 0) {
			this._dv().setBigUint64(o, BigInt(v), true);
			return o + 8;
		}
		writeBigInt64LE(v, o = 0) {
			this._dv().setBigInt64(o, BigInt(v), true);
			return o + 8;
		}
		swap16() {
			for (let i = 0; i + 1 < this.length; i += 2) [this[i], this[i + 1]] = [this[i + 1], this[i]];
			return this;
		}
	};
	// Node's `Buffer` is callable without `new` too (deprecated, still used:
	// `Buffer(n)`, `Buffer(string, enc)`, `Buffer(array)`), and `new
	// Buffer(string)` copies the string as `Buffer.from` does.
	function Buffer(v, encOrOffset, length) {
		const target = new.target ?? Buffer;
		if (typeof v === "number") return Reflect.construct(BufferClass, [v], target);
		if (v instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer)) {
			const offset = encOrOffset ?? 0;
			return Reflect.construct(BufferClass, [v, offset, length ?? v.byteLength - offset], target);
		}
		return BufferClass.from(v, encOrOffset, length);
	}
	Buffer.prototype = BufferClass.prototype;
	Object.defineProperty(BufferClass.prototype, "constructor", { value: Buffer, writable: true, configurable: true });
	Object.setPrototypeOf(Buffer, BufferClass);
	for (const [a, b] of [
		["readUint8", "readUInt8"],
		["readUint16LE", "readUInt16LE"],
		["readUint16BE", "readUInt16BE"],
		["readUint32LE", "readUInt32LE"],
		["readUint32BE", "readUInt32BE"],
		["writeUint8", "writeUInt8"],
		["writeUint16LE", "writeUInt16LE"],
		["writeUint32LE", "writeUInt32LE"],
		["writeUint32BE", "writeUInt32BE"],
		["readBigUint64LE", "readBigUInt64LE"],
	])
		Buffer.prototype[a] = Buffer.prototype[b];
	g.Buffer = Buffer;
	const bufferModule = { Buffer, kMaxLength: 2 ** 31 - 1, constants: { MAX_LENGTH: 2 ** 31 - 1, MAX_STRING_LENGTH: 2 ** 29 }, Blob: g.Blob, File: g.File, atob: g.atob, btoa: g.btoa, SlowBuffer: Buffer, isUtf8: () => true, transcode: (b) => b };

	// ---------------------------------------------------------------- util
	const inspect = (v, opts) => L.format(v);
	inspect.custom = Symbol.for("nodejs.util.inspect.custom");
	inspect.defaultOptions = {};
	const promisify = (fn) => {
		if (fn[promisify.custom]) return fn[promisify.custom];
		return function (...args) {
			return new Promise((res, rej) => fn.call(this, ...args, (err, ...v) => (err ? rej(err) : res(v.length > 1 ? v : v[0]))));
		};
	};
	promisify.custom = Symbol.for("nodejs.util.promisify.custom");
	const util = {
		inspect,
		promisify,
		format: (...a) => L.format(...a),
		formatWithOptions: (_, ...a) => L.format(...a),
		inherits(ctor, superCtor) {
			Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
			Object.setPrototypeOf(ctor, superCtor);
			ctor.super_ = superCtor;
		},
		callbackify: (fn) =>
			function (...a) {
				const cb = a.pop();
				fn.apply(this, a).then((v) => cb(null, v), (e) => cb(e));
			},
		deprecate: (fn) => fn,
		debuglog: () => Object.assign(() => {}, { enabled: false }),
		debug: () => () => {},
		isDeepStrictEqual: (a, b) => {
			try {
				return JSON.stringify(a) === JSON.stringify(b);
			} catch {
				return a === b;
			}
		},
		isArray: Array.isArray,
		isBuffer: (b) => b instanceof Buffer,
		isString: (s) => typeof s === "string",
		isFunction: (f) => typeof f === "function",
		isObject: (o) => o !== null && typeof o === "object",
		isError: (e) => e instanceof Error,
		toUSVString: (s) => String(s),
		stripVTControlCharacters: (s) => String(s).replace(/\u001b\[[0-9;]*m/g, ""),
		styleText: (_, s) => s,
		getSystemErrorName: (n) => `E${n}`,
		TextEncoder: g.TextEncoder,
		TextDecoder: g.TextDecoder,
		types: {
			isUint8Array: (v) => v instanceof Uint8Array,
			isTypedArray: (v) => ArrayBuffer.isView(v) && !(v instanceof DataView),
			isArrayBuffer: (v) => v instanceof ArrayBuffer,
			isAnyArrayBuffer: (v) => v instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && v instanceof SharedArrayBuffer),
			isArrayBufferView: (v) => ArrayBuffer.isView(v),
			isPromise: (v) => v instanceof Promise,
			isDate: (v) => v instanceof Date,
			isRegExp: (v) => v instanceof RegExp,
			isNativeError: (v) => v instanceof Error,
			isProxy: () => false,
			isMap: (v) => v instanceof Map,
			isSet: (v) => v instanceof Set,
			isAsyncFunction: (f) => f?.constructor?.name === "AsyncFunction",
			isGeneratorFunction: (f) => /GeneratorFunction/.test(f?.constructor?.name ?? ""),
			isBoxedPrimitive: () => false,
			isDataView: (v) => v instanceof DataView,
			isExternal: () => false,
			isKeyObject: () => false,
			isCryptoKey: () => false,
		},
	};
	util.default = util;

	// ---------------------------------------------------------------- path
	const makePath = (sep) => {
		const isAbs = (p) => (sep === "\\" ? /^([a-zA-Z]:)?[\\/]/.test(p) || p.startsWith("\\\\") : p.startsWith("/"));
		const norm = (p) => {
			if (p === "") return ".";
			const unc = sep === "\\" && /^[\\/]{2}/.test(p);
			const drive = sep === "\\" ? (/^[a-zA-Z]:/.exec(p)?.[0] ?? "") : "";
			const rest = p.slice(drive.length);
			const abs = /^[\\/]/.test(rest);
			const parts = [];
			for (const s of rest.split(/[\\/]+/)) {
				if (s === "" || s === ".") continue;
				if (s === "..") {
					if (parts.length && parts.at(-1) !== "..") parts.pop();
					else if (!abs) parts.push("..");
				} else parts.push(s);
			}
			let out = parts.join(sep);
			if (abs) out = (unc ? sep + sep : sep) + out;
			out = drive + out;
			if (/[\\/]$/.test(p) && parts.length) out += sep;
			return out || (abs ? sep : ".");
		};
		const P = {
			sep,
			delimiter: sep === "\\" ? ";" : ":",
			isAbsolute: isAbs,
			normalize: norm,
			join: (...a) => norm(a.filter((x) => x !== "").join(sep)),
			resolve: (...a) => {
				let r = "";
				for (let i = a.length - 1; i >= 0 && !isAbs(r); i--) r = r ? `${a[i]}${sep}${r}` : a[i];
				if (!isAbs(r)) r = `${process.cwd()}${sep}${r}`;
				const n = norm(r);
				return n.length > 1 && n.endsWith(sep) && !/^[a-zA-Z]:\\$/.test(n) ? n.slice(0, -1) : n;
			},
			dirname: (p) => {
				const m = /^(.*)[\\/][^\\/]*[\\/]?$/.exec(p);
				if (!m) return ".";
				return m[1] === "" ? sep : /^[a-zA-Z]:$/.test(m[1]) ? m[1] + sep : m[1];
			},
			basename: (p, ext) => {
				const b = p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
				return ext && b.endsWith(ext) && b !== ext ? b.slice(0, -ext.length) : b;
			},
			extname: (p) => {
				const b = P.basename(p);
				const i = b.lastIndexOf(".");
				return i <= 0 ? "" : b.slice(i);
			},
			relative: (from, to) => {
				const f = P.resolve(from).split(/[\\/]/).filter(Boolean);
				const t = P.resolve(to).split(/[\\/]/).filter(Boolean);
				let i = 0;
				while (i < f.length && i < t.length && f[i] === t[i]) i++;
				return [...Array(f.length - i).fill(".."), ...t.slice(i)].join(sep);
			},
			parse: (p) => {
				const base = P.basename(p);
				const ext = P.extname(p);
				const dir = P.dirname(p);
				const root = isAbs(p) ? (sep === "\\" ? (/^[a-zA-Z]:[\\/]/.exec(p)?.[0] ?? sep) : "/") : "";
				return { root, dir, base, ext, name: ext ? base.slice(0, -ext.length) : base };
			},
			format: (o) => (o.dir ? o.dir + sep : (o.root ?? "")) + (o.base ?? (o.name ?? "") + (o.ext ?? "")),
			toNamespacedPath: (p) => p,
		};
		return P;
	};
	const posix = makePath("/");
	const win32 = makePath("\\");
	const path = PLATFORM === "win32" ? { ...win32, posix, win32 } : { ...posix, posix, win32 };
	path.default = path;

	// ---------------------------------------------------------------- process
	const procEmitter = new EventEmitter();
	const process = {
		platform: PLATFORM,
		arch: callOp("arch", null) ?? "x64",
		env: { ...ENV },
		argv: ["tidalfast"],
		execArgv: [],
		execPath: "tidalfast",
		pid: 1,
		ppid: 0,
		title: "tidalfast",
		version: `v${g.__lunaVersions.node}`,
		versions: { node: g.__lunaVersions.node, v8: g.__lunaVersions.v8, uv: g.__lunaVersions.uv, modules: g.__lunaVersions.modules, electron: g.__lunaVersions.electron, chrome: g.__lunaVersions.chrome, quickjs: "luna" },
		release: { name: "node" },
		features: {},
		config: { variables: {} },
		resourcesPath: "",
		type: "browser",
		exitCode: undefined,
		cwd: () => L.node.homeOf(L.pluginFromStack()) ?? "/",
		chdir: () => {},
		umask: () => 0o22,
		uptime: () => performance.now() / 1000,
		hrtime: Object.assign(
			(prev) => {
				const ns = Math.round(performance.now() * 1e6);
				let s = Math.floor(ns / 1e9);
				let n = ns % 1e9;
				if (prev) {
					s -= prev[0];
					n -= prev[1];
					if (n < 0) {
						s--;
						n += 1e9;
					}
				}
				return [s, n];
			},
			{ bigint: () => BigInt(Math.round(performance.now() * 1e6)) },
		),
		memoryUsage: Object.assign(() => ({ rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0 }), { rss: () => 0 }),
		cpuUsage: () => ({ user: 0, system: 0 }),
		resourceUsage: () => ({}),
		nextTick: (cb, ...a) => queueMicrotask(() => cb(...a)),
		emitWarning: (w) => console.warn("Warning:", w?.message ?? w),
		exit: () => {
			throw new Error("process.exit is not available to plugins");
		},
		abort: () => {
			throw new Error("process.abort is not available to plugins");
		},
		dlopen: (_module, path) => {
			throw L.node.nativeAddonError(L.pluginFromStack(), path ?? "addon.node");
		},
		kill: () => {
			throw new Error("process.kill is not available to plugins");
		},
		on: (...a) => (procEmitter.on(...a), process),
		once: (...a) => (procEmitter.once(...a), process),
		off: (...a) => (procEmitter.off(...a), process),
		addListener: (...a) => (procEmitter.on(...a), process),
		removeListener: (...a) => (procEmitter.removeListener(...a), process),
		removeAllListeners: (...a) => (procEmitter.removeAllListeners(...a), process),
		emit: (...a) => procEmitter.emit(...a),
		listeners: (n) => procEmitter.listeners(n),
		listenerCount: (n) => procEmitter.listenerCount(n),
		stdout: { write: (s) => (console.log(String(s).replace(/\n$/, "")), true), isTTY: false, columns: 80 },
		stderr: { write: (s) => (console.error(String(s).replace(/\n$/, "")), true), isTTY: false, columns: 80 },
		stdin: { on() {}, isTTY: false },
		getuid: () => 1000,
		getgid: () => 1000,
		binding: () => {
			throw new Error("process.binding is not available");
		},
	};
	g.process ??= process;

	// ---------------------------------------------------------------- timers
	class Timeout {
		constructor(id, interval) {
			this.id = id;
			this._interval = interval;
		}
		ref() {
			return this;
		}
		unref() {
			return this;
		}
		hasRef() {
			return true;
		}
		refresh() {
			return this;
		}
		close() {
			clearTimeout(this.id);
			return this;
		}
		[Symbol.toPrimitive]() {
			return this.id;
		}
	}
	const timers = {
		setTimeout: (cb, ms, ...a) => new Timeout(g.setTimeout(cb, ms, ...a)),
		setInterval: (cb, ms, ...a) => new Timeout(g.setInterval(cb, ms, ...a), true),
		setImmediate: (cb, ...a) => new Timeout(g.setImmediate(cb, ...a)),
		clearTimeout: (t) => g.clearTimeout(t),
		clearInterval: (t) => g.clearInterval(t),
		clearImmediate: (t) => g.clearImmediate(t),
	};
	const timersPromises = {
		setTimeout: (ms, v, o) =>
			new Promise((res, rej) => {
				const id = g.setTimeout(() => res(v), ms);
				o?.signal?.addEventListener("abort", () => (g.clearTimeout(id), rej(o.signal.reason)));
			}),
		setImmediate: (v) => new Promise((res) => g.setImmediate(() => res(v))),
		setInterval: async function* (ms, v) {
			while (true) {
				await new Promise((r) => g.setTimeout(r, ms));
				yield v;
			}
		},
		scheduler: { wait: (ms) => new Promise((r) => g.setTimeout(r, ms)), yield: () => new Promise((r) => g.setImmediate(r)) },
	};
	timers.promises = timersPromises;

	// ---------------------------------------------------------------- stream (minimal)
	class Stream extends EventEmitter {
		pipe(dest, opts) {
			const onData = (c) => {
				if (dest.write(c) === false && this.pause) this.pause();
			};
			const onDrain = () => this.resume?.();
			const onEnd = () => dest.end?.();
			this.on("data", onData);
			dest.on?.("drain", onDrain);
			if (opts?.end !== false) this.on("end", onEnd);
			(this._pipes ??= []).push({ dest, onData, onDrain, onEnd });
			dest.emit?.("pipe", this);
			return dest;
		}
		// Express's final handler unpipes the request before it waits for it.
		unpipe(dest) {
			const keep = [];
			for (const p of this._pipes ?? []) {
				if (dest && p.dest !== dest) {
					keep.push(p);
					continue;
				}
				this.off("data", p.onData);
				this.off("end", p.onEnd);
				p.dest.off?.("drain", p.onDrain);
				p.dest.emit?.("unpipe", this);
			}
			this._pipes = keep;
			return this;
		}
	}
	class Readable extends Stream {
		constructor(opts = {}) {
			super();
			this._buf = [];
			this._ended = false;
			this._flowing = null;
			this.readable = true;
			this.destroyed = false;
			this.readableEnded = false;
			this._objectMode = !!(opts.objectMode || opts.readableObjectMode);
			this._enc = opts.encoding;
			if (opts.read) this._read = opts.read.bind(this);
			if (opts.destroy) this._destroy = opts.destroy.bind(this);
		}
		static from(iter, opts) {
			const r = new Readable({ objectMode: true, ...opts });
			(async () => {
				try {
					for await (const c of iter) r.push(c);
					r.push(null);
				} catch (e) {
					r.destroy(e);
				}
			})();
			return r;
		}
		_read() {}
		push(chunk) {
			if (chunk === null) {
				this._ended = true;
				queueMicrotask(() => this.__lunaPump());
				return false;
			}
			if (typeof chunk === "string" && !this._objectMode) chunk = Buffer.from(chunk);
			this._buf.push(chunk);
			queueMicrotask(() => this.__lunaPump());
			return true;
		}
		unshift(chunk) {
			this._buf.unshift(chunk);
		}
		__lunaPump() {
			if (this._flowing === false) return;
			if (this._flowing === null && this.listenerCount("data") === 0) {
				if (this._buf.length && this.listenerCount("readable")) this.emit("readable");
				if (this._ended && !this._buf.length && !this.readableEnded) this._end();
				return;
			}
			while (this._buf.length && this._flowing !== false) this.emit("data", this._enc ? this._buf.shift().toString(this._enc) : this._buf.shift());
			if (this._ended && !this._buf.length && !this.readableEnded) this._end();
		}
		_end() {
			this.readableEnded = true;
			this.emit("end");
			this.emit("close");
		}
		read() {
			if (!this._buf.length) {
				if (this._ended && !this.readableEnded) queueMicrotask(() => this._end());
				return null;
			}
			const all = this._objectMode ? this._buf.shift() : Buffer.concat(this._buf.splice(0));
			return this._enc && !this._objectMode ? all.toString(this._enc) : all;
		}
		on(n, f) {
			super.on(n, f);
			if (n === "data" && this._flowing !== false) {
				this._flowing = true;
				queueMicrotask(() => this.__lunaPump());
			}
			if (n === "readable") queueMicrotask(() => this.__lunaPump());
			return this;
		}
		pause() {
			this._flowing = false;
			return this;
		}
		resume() {
			this._flowing = true;
			queueMicrotask(() => this.__lunaPump());
			return this;
		}
		isPaused() {
			return this._flowing === false;
		}
		setEncoding(e) {
			this._enc = e;
			return this;
		}
		destroy(err) {
			if (this.destroyed) return this;
			this.destroyed = true;
			const done = (e) => {
				if (e) this.emit("error", e);
				this.emit("close");
			};
			this._destroy ? this._destroy(err ?? null, done) : done(err);
			return this;
		}
		async *[Symbol.asyncIterator]() {
			while (true) {
				const c = this.read();
				if (c !== null) {
					yield c;
					continue;
				}
				if (this.readableEnded || this._ended) return;
				await new Promise((r) => {
					const done = () => (this.off("readable", done), this.off("end", done), r());
					this.on("readable", done);
					this.on("end", done);
				});
			}
		}
	}
	// Whether `fn(chunk, encoding, callback)` throws the chunk away: its
	// whole body calls back and nothing else (`(_c, _e, cb) => cb()`,
	// `function (c, e, done) { done(null); }`, `write(c, e, next) {
	// setImmediate(next) }`). Read from its source; anything else, or a
	// function whose source is not there, keeps the chunk.
	const discardsChunk = (fn) => {
		if (typeof fn !== "function") return false;
		let src;
		try {
			src = Function.prototype.toString.call(fn);
		} catch {
			return false;
		}
		const m = /^\s*(?:async\s+)?(?:function\b\s*[\w$]*|[\w$]+)?\s*\(([^()]*)\)\s*(=>)?\s*([\s\S]*)$/.exec(src);
		if (!m) return false;
		const params = m[1].split(",").map((p) => p.trim());
		if (params.length !== 3 || !params.every((p) => /^[A-Za-z_$][\w$]*$/.test(p))) return false;
		let body = m[3].trim();
		if (body.startsWith("{")) {
			if (!body.endsWith("}")) return false;
			body = body.slice(1, -1).trim().replace(/;+$/, "").trim().replace(/^return\b/, "");
		} else if (!m[2]) return false;
		const cb = params[2];
		const calls = [`${cb}()`, `${cb}(null)`, `${cb}(undefined)`, `${cb}(void0)`, `void${cb}()`, `setImmediate(${cb})`, `process.nextTick(${cb})`, `queueMicrotask(${cb})`];
		return calls.includes(body.replace(/\s+/g, ""));
	};
	// A sink that drops everything written to it: one of these Writables
	// (not a Duplex or Transform, which pass data on) whose `write` is its
	// own and whose `_write` throws the chunk away, given as the `write`
	// option or by a subclass.
	const discards = (dest) => {
		if (!(dest instanceof Writable) || dest.write !== Writable.prototype.write || typeof dest._writev === "function") return false;
		if (Object.prototype.hasOwnProperty.call(dest, "_write")) return dest._write === dest.__lunaDiscards;
		return dest._write !== Writable.prototype._write && discardsChunk(dest._write);
	};
	class Writable extends Stream {
		constructor(opts = {}) {
			super();
			this.writable = true;
			this.destroyed = false;
			this.writableEnded = false;
			this.writableFinished = false;
			if (opts.write) this._write = opts.write.bind(this);
			// `discards` checks the bound copy is still the one made here.
			if (opts.write && discardsChunk(opts.write)) Object.defineProperty(this, "__lunaDiscards", { value: this._write, configurable: true });
			if (opts.writev) this._writev = opts.writev.bind(this);
			if (opts.final) this._final = opts.final.bind(this);
			if (opts.destroy) this._destroy = opts.destroy.bind(this);
			this._decode = opts.decodeStrings !== false;
		}
		_write(c, e, cb) {
			cb();
		}
		write(chunk, enc, cb) {
			if (typeof enc === "function") [cb, enc] = [enc, undefined];
			if (typeof chunk === "string" && this._decode) chunk = Buffer.from(chunk, enc);
			this._write(chunk, enc ?? "buffer", (err) => {
				if (err) this.emit("error", err);
				cb?.(err);
			});
			return true;
		}
		end(chunk, enc, cb) {
			if (typeof chunk === "function") [cb, chunk] = [chunk, undefined];
			if (typeof enc === "function") [cb, enc] = [enc, undefined];
			if (chunk !== undefined && chunk !== null) this.write(chunk, enc);
			this.writableEnded = true;
			const fin = () => {
				this.writableFinished = true;
				this.emit("finish");
				this.emit("close");
				cb?.();
			};
			this._final ? this._final((e) => (e ? this.emit("error", e) : fin())) : queueMicrotask(fin);
			return this;
		}
		cork() {}
		uncork() {}
		setDefaultEncoding() {
			return this;
		}
		destroy(err) {
			if (this.destroyed) return this;
			this.destroyed = true;
			if (err) this.emit("error", err);
			this.emit("close");
			return this;
		}
	}
	class Duplex extends Readable {
		constructor(opts = {}) {
			super(opts);
			this.writable = true;
			this.writableEnded = false;
			if (opts.write) this._write = opts.write.bind(this);
			if (opts.final) this._final = opts.final.bind(this);
		}
		_write(c, e, cb) {
			cb();
		}
	}
	for (const k of ["write", "end", "cork", "uncork", "setDefaultEncoding"]) Duplex.prototype[k] = Writable.prototype[k];
	class Transform extends Duplex {
		constructor(opts = {}) {
			super(opts);
			if (opts.transform) this._transform = opts.transform.bind(this);
			if (opts.flush) this._flushT = opts.flush.bind(this);
		}
		_transform(c, e, cb) {
			cb(null, c);
		}
		_write(c, e, cb) {
			this._transform(c, e, (err, out) => {
				if (out !== undefined && out !== null) this.push(out);
				cb(err);
			});
		}
		_final(cb) {
			const done = () => {
				this.push(null);
				cb();
			};
			const flush = this._flushT ?? this._flush;
			flush ? flush.call(this, (e, out) => (e ? this.emit("error", e) : out != null && this.push(out), done())) : done();
		}
	}
	class PassThrough extends Transform {}
	const finished = (s, opts, cb) => {
		if (typeof opts === "function") cb = opts;
		let called = false;
		const done = (e) => !called && ((called = true), cb?.(e));
		s.on("end", () => done());
		s.on("finish", () => done());
		s.on("close", () => done());
		s.on("error", (e) => done(e));
		return () => {};
	};
	const pipeline = (...streams) => {
		const cb = typeof streams.at(-1) === "function" ? streams.pop() : undefined;
		let cur = streams[0];
		for (const s of streams.slice(1)) cur = cur.pipe(s);
		finished(cur, (e) => cb?.(e));
		return cur;
	};
	const streamPromises = {
		pipeline: (...s) => new Promise((res, rej) => pipeline(...s, (e) => (e ? rej(e) : res()))),
		finished: (s) => new Promise((res, rej) => finished(s, (e) => (e ? rej(e) : res()))),
	};
	// Pre-class libraries call constructors as functions (`EventEmitter.call(this)`
	// with util.inherits); classes refuse that, so the module exports accept
	// it: called on an object, the instance fields are copied onto it.
	const callable = (C) =>
		new Proxy(C, {
			apply(t, thisArg, args) {
				if (thisArg === null || thisArg === undefined || thisArg === g || typeof thisArg !== "object") return Reflect.construct(t, args);
				const tmp = Reflect.construct(t, args);
				for (const k of Reflect.ownKeys(tmp)) Object.defineProperty(thisArg, k, Object.getOwnPropertyDescriptor(tmp, k));
				return thisArg;
			},
		});
	const stream = callable(Object.assign(Stream, { Stream: callable(Stream), Readable: callable(Readable), Writable: callable(Writable), Duplex: callable(Duplex), Transform: callable(Transform), PassThrough: callable(PassThrough), pipeline, finished, promises: streamPromises, addAbortSignal: (_, s) => s, isReadable: (s) => !!s?.readable, isErrored: () => false, isDisturbed: () => false, getDefaultHighWaterMark: () => 16384, setDefaultHighWaterMark: () => {} }));
	stream.default = stream;
	class StubWebStream {
		constructor() {
			throw new Error("Web streams are not available to plugins");
		}
	}
	const streamWeb = { ReadableStream: g.ReadableStream ?? StubWebStream, WritableStream: g.WritableStream ?? StubWebStream, TransformStream: g.TransformStream ?? StubWebStream, TextEncoderStream: StubWebStream, TextDecoderStream: StubWebStream, ByteLengthQueuingStrategy: class {}, CountQueuingStrategy: class {} };

	// ---------------------------------------------------------------- net
	// Who owns each socket, server and child process: kept here, never on
	// the object. Rust can read the plugin off the stack only while plugin
	// code is on it; a method called from a promise job is taken at the
	// shim's word, so a plugin that could write another plugin's name on a
	// socket (or on an object made from the prototype) could use that
	// plugin's connections, programs and permissions. The WeakMap's methods
	// are the ones from before any plugin ran.
	const owners = new WeakMap();
	const ownersGet = WeakMap.prototype.get;
	const ownersSet = WeakMap.prototype.set;
	const applyFn = Reflect.apply;
	const setOwner = (o, plugin) => void applyFn(ownersSet, owners, [o, plugin ?? null]);
	const ownerOf = (o) => (o !== null && typeof o === "object" ? (applyFn(ownersGet, owners, [o]) ?? null) : null);
	const sockets = new Map();
	class Socket extends Duplex {
		constructor(opts = {}) {
			super(opts);
			setOwner(this, L.pluginFromStack());
			this.readyState = "closed";
			this.connecting = false;
			this.handle = undefined;
			this.remoteAddress = undefined;
			this.remotePort = undefined;
			this.bytesWritten = 0;
			this.bytesRead = 0;
			this.pending = true;
		}
		get plugin() {
			return ownerOf(this);
		}
		connect(...args) {
			let opts = {};
			let cb;
			if (typeof args.at(-1) === "function") cb = args.pop();
			if (typeof args[0] === "object" && args[0] !== null) opts = args[0];
			else if (typeof args[0] === "string" && isNaN(Number(args[0]))) opts = { path: args[0] };
			else opts = { port: Number(args[0]), host: args[1] ?? "localhost" };
			if (cb) this.once("connect", cb);
			this.readyState = "opening";
			this.connecting = true;
			const open = () => {
				this.handle = callOp("net.connect", { plugin: ownerOf(this), path: opts.path ?? null, host: opts.host ?? null, port: opts.port ?? null });
				sockets.set(this.handle, this);
			};
			try {
				open();
			} catch (err) {
				// Another program's pipe or socket: the user is asked first,
				// and the connect goes ahead once they allowed it.
				if (String(err?.message ?? "").startsWith("EASK") && opts.path) {
					L.permit({ plugin: ownerOf(this), kind: "pipe", path: String(opts.path) })
						.then(() => {
							if (this.destroyed || this.readyState !== "opening") return;
							open();
						})
						.catch((e) => {
							this.readyState = "closed";
							this.connecting = false;
							this.emit("error", e);
							this.emit("close", true);
						});
					return this;
				}
				queueMicrotask(() => {
					this.readyState = "closed";
					this.emit("error", err);
					this.emit("close", true);
				});
			}
			return this;
		}
		_event(kind, data) {
			switch (kind) {
				case "connect":
					this.readyState = "open";
					this.connecting = false;
					this.pending = false;
					this.emit("connect");
					this.emit("ready");
					break;
				case "data": {
					const chunk = Buffer.from(L.b64dec(data));
					this.bytesRead += chunk.length;
					this.push(chunk);
					break;
				}
				case "end":
					this.readyState = this.writable ? "writeOnly" : "closed";
					this.push(null);
					break;
				case "error":
					this.emit("error", Object.assign(new Error(data), { code: /refused|not found|No such|cannot find/i.test(data) ? "ENOENT" : "EIO" }));
					break;
				case "close":
					sockets.delete(this.handle);
					this.readyState = "closed";
					if (!this._ended) this.push(null);
					this.emit("close", false);
					break;
			}
		}
		_write(chunk, enc, cb) {
			if (this.handle === undefined || this.readyState === "closed") return cb(new Error("This socket has been ended by the other party"));
			try {
				// A string is written as Node does: in its encoding, UTF-8 by
				// default (node-mpv sends its JSON commands as strings).
				const bytes = typeof chunk === "string" ? Buffer.from(chunk, typeof enc === "string" && enc !== "buffer" ? enc : "utf8") : chunk;
				callOp("net.write", { handle: this.handle, data: L.b64enc(bytes), plugin: ownerOf(this) });
				this.bytesWritten += bytes.length;
				cb();
			} catch (err) {
				cb(err);
			}
		}
		end(chunk, enc, cb) {
			if (chunk !== undefined && typeof chunk !== "function") this.write(chunk, enc);
			if (this.handle !== undefined) callOp("net.close", { handle: this.handle, plugin: ownerOf(this) });
			this.writable = false;
			this.writableEnded = true;
			if (typeof chunk === "function") chunk();
			else if (typeof enc === "function") enc();
			else cb?.();
			return this;
		}
		destroy(err) {
			if (this.destroyed) return this;
			this.destroyed = true;
			if (this.handle !== undefined) callOp("net.close", { handle: this.handle, plugin: ownerOf(this) });
			sockets.delete(this.handle);
			this.readyState = "closed";
			queueMicrotask(() => {
				if (err) this.emit("error", err);
				this.emit("close", !!err);
			});
			return this;
		}
		destroySoon() {
			this.end();
		}
		resetAndDestroy() {
			return this.destroy();
		}
		setKeepAlive() {
			return this;
		}
		setNoDelay() {
			return this;
		}
		setTimeout(ms, cb) {
			if (cb) this.once("timeout", cb);
			return this;
		}
		ref() {
			return this;
		}
		unref() {
			return this;
		}
		address() {
			return {};
		}
	}
	const isIPv4 = (s) => /^(\d{1,3}\.){3}\d{1,3}$/.test(String(s)) && String(s).split(".").every((x) => +x <= 255);
	const isIPv6 = (s) => /^[0-9a-fA-F:]+$/.test(String(s)) && String(s).includes(":");
	const net = {
		Socket: callable(Socket),
		Stream: callable(Socket),
		createConnection: (...a) => new Socket().connect(...a),
		connect: (...a) => new Socket().connect(...a),
		createServer: () => {
			throw new Error("Plugins cannot open listening sockets in Tidalfast");
		},
		Server: class Server {
			constructor() {
				throw new Error("Plugins cannot open listening sockets in Tidalfast");
			}
		},
		isIP: (s) => (isIPv4(s) ? 4 : isIPv6(s) ? 6 : 0),
		isIPv4,
		isIPv6,
		BlockList: class BlockList {
			addAddress() {}
			check() {
				return false;
			}
		},
		SocketAddress: class SocketAddress {},
		getDefaultAutoSelectFamily: () => true,
		setDefaultAutoSelectFamily: () => {},
	};
	L.node = L.node ?? {};
	L.node.netEvent = (handle, kind, data) => sockets.get(handle)?._event(kind, data);
	g.__luna_net = (handle, kind, data) => L.node.netEvent(handle, kind, data);
	L.node.closeAll = (plugin) => {
		for (const s of [...sockets.values()]) if (ownerOf(s) === plugin) s.destroy();
	};
	// A native Node addon (`.node`) is machine code built against Electron's
	// Node.js ABI; Tidalfast runs plugins in QuickJS, which has no such ABI to
	// load it into. The plugin's card says so even when the plugin catches
	// the error.
	L.node.nativeAddonError = (plugin, path) => {
		const file = String(path).replace(/^.*[\\/]/, "");
		const why = `Needs a native Node addon (${file}). Native addons are compiled for Electron's Node.js, and Tidalfast runs plugins in its own JavaScript engine, which cannot load them: a technical limit, not a setting`;
		if (plugin !== undefined) {
			callOp("plugin.status", { name: plugin, plugin, state: "message", error: why });
			callOp("log", { plugin, level: "warn", msg: why });
		}
		return Object.assign(new Error(`${why}.`), { code: "ERR_DLOPEN_FAILED" });
	};

	// ---------------------------------------------------------------- fs (plugin data dir only)
	const homes = {};
	L.node.homeOf = (plugin) => {
		if (plugin === undefined) return undefined;
		return (homes[plugin] ??= callOp("fs", { plugin, op: "home" }));
	};
	const fsErr = (e, syscall, p) => {
		const m = /^(E[A-Z]+):/.exec(e.message);
		return Object.assign(new Error(e.message), { code: m?.[1] ?? "EIO", syscall, path: p, errno: -1 });
	};
	const makeFs = (plugin) => {
		const op = (name, p, extra = {}) => {
			try {
				return callOp("fs", { op: name, path: p === undefined ? null : String(p instanceof URL ? url.fileURLToPath(p) : p), ...extra, plugin });
			} catch (e) {
				throw fsErr(e, name, p);
			}
		};
		// Reads and writes carry the bytes beside the JSON, not as base64: a
		// 100 MB model's base64 copies would not fit the plugins' memory.
		const bytesOp = (name, p, extra, data) => {
			try {
				return L.fsBytes({ op: name, path: String(p instanceof URL ? url.fileURLToPath(p) : p), ...extra, plugin }, data);
			} catch (e) {
				throw fsErr(e, name, p);
			}
		};
		const bufferOf = (d, enc) => {
			const u8 = typeof d === "string" ? Buffer.from(d, typeof enc === "string" ? enc : enc?.encoding) : d instanceof ArrayBuffer ? new Uint8Array(d) : ArrayBuffer.isView(d) ? new Uint8Array(d.buffer, d.byteOffset, d.byteLength) : Buffer.from(String(d));
			return u8.byteOffset === 0 && u8.byteLength === u8.buffer.byteLength ? u8.buffer : u8.slice().buffer;
		};
		const readBytes = (p, enc) => {
			const b = Buffer.from(bytesOp("read", p, {}));
			const e = typeof enc === "string" ? enc : enc?.encoding;
			return e ? b.toString(e) : b;
		};
		const statOf = (s) => ({
			...s,
			isFile: () => s.file,
			isDirectory: () => s.dir,
			isSymbolicLink: () => false,
			isFIFO: () => false,
			isSocket: () => false,
			isBlockDevice: () => false,
			isCharacterDevice: () => false,
			mtime: new Date(s.mtimeMs),
			atime: new Date(s.mtimeMs),
			ctime: new Date(s.mtimeMs),
			birthtime: new Date(s.mtimeMs),
		});
		const sync = {
			existsSync: (p) => {
				try {
					return !!op("exists", p);
				} catch {
					return false;
				}
			},
			readFileSync: (p, enc) => readBytes(p, enc),
			writeFileSync: (p, d, enc) => void bytesOp("write", p, { append: false }, bufferOf(d, enc)),
			appendFileSync: (p, d, enc) => void bytesOp("write", p, { append: true }, bufferOf(d, enc)),
			mkdirSync: (p, o) => void op("mkdir", p, { recursive: !!o?.recursive }),
			readdirSync: (p, o) => {
				const list = op("readdir", p);
				return o?.withFileTypes ? list.map((e) => ({ name: e.name, isFile: () => !e.dir, isDirectory: () => e.dir, isSymbolicLink: () => false })) : list.map((e) => e.name);
			},
			statSync: (p, o) => {
				try {
					return statOf(op("stat", p));
				} catch (e) {
					if (o?.throwIfNoEntry === false) return undefined;
					throw e;
				}
			},
			lstatSync: (p, o) => sync.statSync(p, o),
			unlinkSync: (p) => void op("remove", p, { recursive: false }),
			rmSync: (p, o) => {
				try {
					op("remove", p, { recursive: !!o?.recursive });
				} catch (e) {
					if (!o?.force) throw e;
				}
			},
			rmdirSync: (p, o) => void op("remove", p, { recursive: !!o?.recursive }),
			renameSync: (a, b) => void op("rename", a, { to: String(b) }),
			copyFileSync: (a, b) => void op("copy", a, { to: String(b) }),
			realpathSync: Object.assign((p) => op("realpath", p), { native: (p) => op("realpath", p) }),
			accessSync: (p) => {
				if (!op("exists", p)) throw Object.assign(new Error(`ENOENT: no such file or directory, access '${p}'`), { code: "ENOENT" });
			},
			chmodSync: () => {},
			utimesSync: () => {},
			mkdtempSync: (prefix) => {
				const p = `${prefix}${Math.random().toString(36).slice(2, 8)}`;
				op("mkdir", p, { recursive: true });
				return op("realpath", p);
			},
		};
		const promises = {};
		const callbacks = {};
		for (const [k, fn] of Object.entries(sync)) {
			if (!k.endsWith("Sync")) continue;
			const base = k.slice(0, -4);
			promises[base] = async (...a) => fn(...a);
			callbacks[base] = (...a) => {
				const cb = typeof a.at(-1) === "function" ? a.pop() : () => {};
				let r;
				try {
					r = fn(...a);
				} catch (e) {
					return queueMicrotask(() => cb(e));
				}
				queueMicrotask(() => (base === "exists" ? cb(r) : cb(null, r)));
			};
		}
		promises.access = async (p) => sync.accessSync(p);
		// A plugin window's preload script (Themer's editor bridge).
		L.node.readText = (p) => sync.readFileSync(p, "utf8");
		promises.open = async () => {
			throw new Error("fs.open is not available to plugins");
		};
		const fs = {
			...sync,
			...callbacks,
			promises,
			constants: { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1, O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64 },
			createReadStream: (p, o) => {
				const r = new Readable();
				queueMicrotask(() => {
					try {
						r.push(sync.readFileSync(p));
						r.push(null);
					} catch (e) {
						r.destroy(e);
					}
				});
				return r;
			},
			createWriteStream: (p, o) => {
				const parts = [];
				return new Writable({
					write: (c, e, cb) => (parts.push(Buffer.from(c)), cb()),
					final: (cb) => {
						try {
							o?.flags === "a" ? sync.appendFileSync(p, Buffer.concat(parts)) : sync.writeFileSync(p, Buffer.concat(parts));
							cb();
						} catch (e) {
							cb(e);
						}
					},
				});
			},
			watch: () => ({ close() {}, on() {} }),
			watchFile: () => {},
			unwatchFile: () => {},
		};
		fs.default = fs;
		promises.default = promises;
		return fs;
	};

	// ---------------------------------------------------------------- crypto
	const hashAlg = (a) => String(a).toLowerCase().replace(/-/g, "");
	class Hash {
		constructor(alg, key) {
			this.alg = hashAlg(alg);
			this.key = key;
			this.parts = [];
		}
		update(data, enc) {
			this.parts.push(typeof data === "string" ? Buffer.from(data, enc) : Buffer.from(data));
			return this;
		}
		digest(enc) {
			const data = L.b64enc(Buffer.concat(this.parts));
			const out = Buffer.from(L.b64dec(this.key === undefined ? callOp("crypto.hash", { alg: this.alg, data }) : callOp("crypto.hmac", { alg: this.alg, key: L.b64enc(this.key), data })));
			return enc ? out.toString(enc) : out;
		}
		copy() {
			const h = new Hash(this.alg, this.key);
			h.parts = [...this.parts];
			return h;
		}
	}
	const cryptoMod = {
		createHash: (alg) => new Hash(alg),
		createHmac: (alg, key) => new Hash(alg, typeof key === "string" ? Buffer.from(key) : Buffer.from(key)),
		hash: (alg, data, enc = "hex") => new Hash(alg).update(data).digest(enc),
		randomBytes: (n, cb) => {
			const b = Buffer.from(L.randomBytes(n));
			if (cb) return void queueMicrotask(() => cb(null, b));
			return b;
		},
		randomFillSync: (b) => (g.crypto.getRandomValues(b), b),
		randomInt: (a, b) => (b === undefined ? Math.floor(Math.random() * a) : a + Math.floor(Math.random() * (b - a))),
		randomUUID: () => g.crypto.randomUUID(),
		getRandomValues: (a) => g.crypto.getRandomValues(a),
		timingSafeEqual: (a, b) => a.length === b.length && Buffer.from(a).equals(Buffer.from(b)),
		getHashes: () => ["md5", "sha1", "sha256", "sha384", "sha512"],
		webcrypto: g.crypto,
		subtle: g.crypto.subtle,
		constants: {},
		createCipheriv: () => {
			throw new Error("Ciphers are not available to plugins");
		},
		createDecipheriv: () => {
			throw new Error("Ciphers are not available to plugins");
		},
	};
	cryptoMod.default = cryptoMod;

	// ---------------------------------------------------------------- the rest
	const os = {
		platform: () => PLATFORM,
		type: () => (PLATFORM === "win32" ? "Windows_NT" : PLATFORM === "darwin" ? "Darwin" : "Linux"),
		arch: () => process.arch,
		release: () => "0.0.0",
		version: () => "",
		machine: () => process.arch,
		hostname: () => "tidalfast",
		homedir: () => process.cwd(),
		tmpdir: () => ENV.TMPDIR ?? ENV.TEMP ?? ENV.TMP ?? (PLATFORM === "win32" ? "C:\\Windows\\Temp" : "/tmp"),
		EOL: PLATFORM === "win32" ? "\r\n" : "\n",
		cpus: () => [],
		totalmem: () => 0,
		freemem: () => 0,
		loadavg: () => [0, 0, 0],
		uptime: () => 0,
		networkInterfaces: () => ({}),
		userInfo: () => ({ username: "user", uid: 1000, gid: 1000, shell: null, homedir: process.cwd() }),
		endianness: () => "LE",
		availableParallelism: () => 1,
		constants: { signals: {}, errno: {}, priority: {} },
		devNull: PLATFORM === "win32" ? "\\\\.\\nul" : "/dev/null",
	};
	const url = {
		URL: g.URL,
		URLSearchParams: g.URLSearchParams,
		fileURLToPath: (u) => {
			const s = typeof u === "string" ? new URL(u) : u;
			const p = decodeURIComponent(s.pathname);
			return PLATFORM === "win32" ? p.replace(/^\/([a-zA-Z]:)/, "$1").replace(/\//g, "\\") : p;
		},
		pathToFileURL: (p) => new URL(`file://${PLATFORM === "win32" ? "/" + String(p).replace(/\\/g, "/") : p}`),
		parse: (s) => {
			try {
				const u = new URL(s);
				return { href: u.href, protocol: u.protocol, host: u.host, hostname: u.hostname, port: u.port || null, pathname: u.pathname, search: u.search || null, query: u.search.slice(1) || null, hash: u.hash || null, path: u.pathname + u.search, auth: u.username ? `${u.username}:${u.password}` : null, slashes: true };
			} catch {
				return { href: s, pathname: s, path: s };
			}
		},
		format: (u) => (typeof u === "string" ? u : u instanceof URL ? u.href : `${u.protocol ?? "http:"}//${u.host ?? u.hostname ?? ""}${u.pathname ?? ""}${u.search ?? ""}`),
		resolve: (from, to) => new URL(to, from).href,
		domainToASCII: (d) => d,
		domainToUnicode: (d) => d,
	};
	class StringDecoder {
		constructor(enc = "utf8") {
			this.enc = normEnc(enc);
			this.rest = Buffer.alloc(0);
		}
		write(b) {
			if (this.enc !== "utf8") return Buffer.from(b).toString(this.enc);
			let buf = Buffer.concat([this.rest, Buffer.from(b)]);
			let cut = buf.length;
			for (let i = 1; i <= Math.min(3, buf.length); i++) {
				const c = buf[buf.length - i];
				if ((c & 0xc0) === 0x80) continue;
				const need = c >= 0xf0 ? 4 : c >= 0xe0 ? 3 : c >= 0xc0 ? 2 : 1;
				if (need > i) cut = buf.length - i;
				break;
			}
			this.rest = buf.subarray(cut);
			return buf.subarray(0, cut).toString("utf8");
		}
		end(b) {
			const s = (b ? this.write(b) : "") + this.rest.toString(this.enc);
			this.rest = Buffer.alloc(0);
			return s;
		}
	}
	class AssertionError extends Error {
		constructor(o = {}) {
			super(o.message ?? "Assertion failed");
			this.name = "AssertionError";
			this.code = "ERR_ASSERTION";
		}
	}
	const fail = (m) => {
		throw m instanceof Error ? m : new AssertionError({ message: m });
	};
	const assert = (v, m) => v || fail(m ?? "The expression evaluated to a falsy value");
	Object.assign(assert, {
		ok: assert,
		AssertionError,
		fail,
		equal: (a, b, m) => a == b || fail(m ?? `${a} == ${b}`),
		notEqual: (a, b, m) => a != b || fail(m),
		strictEqual: (a, b, m) => Object.is(a, b) || fail(m ?? `Expected values to be strictly equal: ${a} !== ${b}`),
		notStrictEqual: (a, b, m) => !Object.is(a, b) || fail(m),
		deepEqual: (a, b, m) => util.isDeepStrictEqual(a, b) || fail(m),
		deepStrictEqual: (a, b, m) => util.isDeepStrictEqual(a, b) || fail(m),
		notDeepStrictEqual: (a, b, m) => !util.isDeepStrictEqual(a, b) || fail(m),
		throws: (fn, _, m) => {
			try {
				fn();
			} catch {
				return;
			}
			fail(m ?? "Missing expected exception.");
		},
		doesNotThrow: (fn) => fn(),
		rejects: async (p, _, m) => {
			try {
				await (typeof p === "function" ? p() : p);
			} catch {
				return;
			}
			fail(m ?? "Missing expected rejection.");
		},
		match: (s, re, m) => re.test(s) || fail(m),
		ifError: (e) => {
			if (e !== null && e !== undefined) throw e;
		},
	});
	assert.strict = assert;
	const querystring = {
		parse: (s, sep = "&", eq = "=") => {
			const o = {};
			for (const [k, v] of new URLSearchParams(String(s).split(sep).join("&").split(eq).join("="))) o[k] = k in o ? [].concat(o[k], v) : v;
			return o;
		},
		stringify: (o, sep = "&", eq = "=") =>
			Object.entries(o ?? {})
				.flatMap(([k, v]) => (Array.isArray(v) ? v : [v]).map((x) => `${encodeURIComponent(k)}${eq}${encodeURIComponent(x ?? "")}`))
				.join(sep),
		escape: encodeURIComponent,
		unescape: decodeURIComponent,
	};
	querystring.decode = querystring.parse;
	querystring.encode = querystring.stringify;
	const channels = {};
	const diagnosticsChannel = {
		channel: (name) => (channels[name] ??= { name, hasSubscribers: false, publish() {}, subscribe() {}, unsubscribe() {}, bindStore() {}, unbindStore() {}, runStores: (d, fn, t, ...a) => fn.apply(t, a) }),
		hasSubscribers: () => false,
		subscribe() {},
		unsubscribe() {},
		tracingChannel: (n) => ({ start: diagnosticsChannel.channel(`${n}:start`), end: diagnosticsChannel.channel(`${n}:end`), asyncStart: diagnosticsChannel.channel(`${n}:asyncStart`), asyncEnd: diagnosticsChannel.channel(`${n}:asyncEnd`), error: diagnosticsChannel.channel(`${n}:error`), hasSubscribers: false, traceSync: (fn, c, t, ...a) => fn.apply(t, a), tracePromise: (fn, c, t, ...a) => fn.apply(t, a), traceCallback: (fn, p, c, t, ...a) => fn.apply(t, a), subscribe() {}, unsubscribe() {} }),
		Channel: class Channel {},
	};
	class AsyncResource {
		constructor(type) {
			this.type = type;
		}
		runInAsyncScope(fn, thisArg, ...a) {
			return fn.apply(thisArg, a);
		}
		bind(fn) {
			return fn.bind(this);
		}
		emitDestroy() {
			return this;
		}
		asyncId() {
			return 1;
		}
		triggerAsyncId() {
			return 0;
		}
		static bind(fn) {
			return fn;
		}
	}
	class AsyncLocalStorage {
		constructor() {
			this._store = undefined;
		}
		getStore() {
			return this._store;
		}
		run(store, fn, ...a) {
			const prev = this._store;
			this._store = store;
			try {
				return fn(...a);
			} finally {
				this._store = prev;
			}
		}
		enterWith(store) {
			this._store = store;
		}
		exit(fn, ...a) {
			return this.run(undefined, fn, ...a);
		}
		disable() {}
		static bind(fn) {
			return fn;
		}
		static snapshot() {
			return (fn, ...a) => fn(...a);
		}
	}
	const asyncHooks = { AsyncResource, AsyncLocalStorage, createHook: () => ({ enable() {}, disable() {} }), executionAsyncId: () => 1, triggerAsyncId: () => 0, executionAsyncResource: () => ({}) };
	// ---------------------------------------------------------------- zlib
	// Whole-buffer (de)compression on the host; the stream classes buffer
	// their input and emit the result when ended.
	const zlibRun = (mode, data, opts) => {
		if (typeof data === "string") data = Buffer.from(data);
		const u = L.toU8(data);
		return Buffer.from(L.b64dec(callOp("zlib", { mode, data: L.b64enc(u), level: opts?.level ?? -1 })));
	};
	const zlibModes = { Inflate: "inflate", Deflate: "deflate", Gzip: "gzip", Gunzip: "gunzip", InflateRaw: "inflateRaw", DeflateRaw: "deflateRaw", Unzip: "unzip" };
	const zlibMod = {
		constants: {
			Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2, Z_FULL_FLUSH: 3, Z_FINISH: 4, Z_BLOCK: 5, Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2, Z_DATA_ERROR: -3, Z_BUF_ERROR: -5,
			Z_NO_COMPRESSION: 0, Z_BEST_SPEED: 1, Z_BEST_COMPRESSION: 9, Z_DEFAULT_COMPRESSION: -1, Z_FILTERED: 1, Z_HUFFMAN_ONLY: 2, Z_RLE: 3, Z_FIXED: 4, Z_DEFAULT_STRATEGY: 0,
			BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1, BROTLI_OPERATION_FINISH: 2, BROTLI_PARAM_MODE: 0, BROTLI_PARAM_QUALITY: 1,
		},
		crc32: (data, value = 0) => {
			const u = L.toU8(typeof data === "string" ? Buffer.from(data) : data);
			let c = ~value >>> 0;
			for (let i = 0; i < u.length; i++) {
				c ^= u[i];
				for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
			}
			return ~c >>> 0;
		},
	};
	for (const [cls, mode] of Object.entries(zlibModes)) {
		const C = class extends Transform {
			constructor(opts = {}) {
				super(opts);
				this._zmode = mode;
				this._zopts = opts;
				this._zchunks = [];
				this.bytesWritten = 0;
			}
			_transform(c, e, cb) {
				const b = typeof c === "string" ? Buffer.from(c, e) : Buffer.from(c);
				this.bytesWritten += b.length;
				this._zchunks.push(b);
				cb();
			}
			_flush(cb) {
				try {
					cb(null, zlibRun(this._zmode, Buffer.concat(this._zchunks), this._zopts));
				} catch (err) {
					cb(err);
				}
			}
			params(level, strategy, cb) {
				this._zopts = { ...this._zopts, level };
				cb?.();
			}
			reset() {
				this._zchunks = [];
			}
			close(cb) {
				if (cb) setTimeout(cb);
				this.destroy?.();
			}
		};
		Object.defineProperty(C, "name", { value: cls });
		const lower = mode;
		zlibMod[cls] = C;
		zlibMod[`create${cls}`] = (opts) => new C(opts);
		zlibMod[`${lower}Sync`] = (buf, opts) => zlibRun(mode, buf, opts);
		zlibMod[lower] = (buf, opts, cb) => {
			if (typeof opts === "function") [opts, cb] = [{}, opts];
			let out, err;
			try {
				out = zlibRun(mode, buf, opts);
			} catch (e) {
				err = e;
			}
			setTimeout(() => cb?.(err ?? null, out));
		};
		zlibMod[lower][Symbol.for("nodejs.util.promisify.custom")] = (buf, opts) => new Promise((res, rej) => zlibMod[lower](buf, opts, (e, o) => (e ? rej(e) : res(o))));
	}
	// Brotli: compression writes a valid stream of stored (uncompressed)
	// meta-blocks (RFC 7932 9.2), which every decoder reads: a plugin's web
	// server answering `Accept-Encoding: br` (streamer-tools for OBS) works,
	// without the size win. Decompression reads such streams; compressed
	// meta-blocks need a real decoder, which Tidalfast does not ship.
	const brotliCompress = (input) => {
		const u = L.toU8(typeof input === "string" ? Buffer.from(input) : input);
		const out = [];
		let acc = 0,
			n = 0;
		const bits = (v, count) => {
			for (let i = 0; i < count; i++) {
				acc |= ((v >>> i) & 1) << n;
				if (++n === 8) {
					out.push(acc);
					acc = n = 0;
				}
			}
		};
		const align = () => {
			if (n > 0) {
				out.push(acc);
				acc = n = 0;
			}
		};
		bits(0, 1); // WBITS = 16
		for (let at = 0; at < u.length; at += 65536) {
			const len = Math.min(65536, u.length - at);
			bits(0, 1); // ISLAST
			bits(0, 2); // MNIBBLES = 4
			bits(len - 1, 16); // MLEN - 1
			bits(1, 1); // ISUNCOMPRESSED
			align();
			for (let i = 0; i < len; i++) out.push(u[at + i]);
		}
		bits(1, 1); // ISLAST
		bits(1, 1); // ISLASTEMPTY
		align();
		return Buffer.from(Uint8Array.from(out));
	};
	const brotliDecompress = (input) => {
		const u = L.toU8(typeof input === "string" ? Buffer.from(input) : input);
		let pos = 0,
			bit = 0;
		const read = (count) => {
			let v = 0;
			for (let i = 0; i < count; i++) {
				if (pos >= u.length) throw Object.assign(new Error("unexpected end of file"), { code: "Z_BUF_ERROR" });
				v |= ((u[pos] >>> bit) & 1) << i;
				if (++bit === 8) {
					bit = 0;
					pos++;
				}
			}
			return v >>> 0;
		};
		const unsupported = () => {
			throw Object.assign(new Error("Brotli-compressed data can't be decoded by Tidalfast's Luna (it reads only stored Brotli blocks)"), { code: "ERR_BROTLI_DECOMPRESSION_FAILED" });
		};
		// WBITS: the window size does not matter for stored blocks.
		if (read(1) === 1 && read(3) === 0) read(3);
		const parts = [];
		for (;;) {
			const last = read(1);
			if (last && read(1)) break;
			const nibbles = read(2);
			if (nibbles === 3) unsupported(); // metadata block
			const len = read((nibbles + 4) * 4) + 1;
			if (last || !read(1)) unsupported();
			if (bit) {
				bit = 0;
				pos++;
			}
			if (pos + len > u.length) throw Object.assign(new Error("unexpected end of file"), { code: "Z_BUF_ERROR" });
			parts.push(u.subarray(pos, pos + len));
			pos += len;
		}
		return Buffer.concat(parts.map((p) => Buffer.from(p)));
	};
	for (const [cls, run] of [
		["BrotliCompress", brotliCompress],
		["BrotliDecompress", brotliDecompress],
	]) {
		const C = class extends Transform {
			constructor(opts = {}) {
				super(opts);
				this._chunks = [];
				this.bytesWritten = 0;
			}
			_transform(c, e, cb) {
				const b = typeof c === "string" ? Buffer.from(c, e) : Buffer.from(c);
				this.bytesWritten += b.length;
				this._chunks.push(b);
				cb();
			}
			_flush(cb) {
				try {
					cb(null, run(Buffer.concat(this._chunks)));
				} catch (err) {
					cb(err);
				}
			}
			params(_p, cb) {
				cb?.();
			}
			reset() {
				this._chunks = [];
			}
			close(cb) {
				if (cb) setTimeout(cb);
				this.destroy?.();
			}
		};
		Object.defineProperty(C, "name", { value: cls });
		const lower = cls[0].toLowerCase() + cls.slice(1);
		zlibMod[cls] = C;
		zlibMod[`create${cls}`] = (opts) => new C(opts);
		zlibMod[`${lower}Sync`] = (buf) => run(buf);
		zlibMod[lower] = (buf, opts, cb) => {
			if (typeof opts === "function") [opts, cb] = [{}, opts];
			let out, err;
			try {
				out = run(buf);
			} catch (e) {
				err = e;
			}
			setTimeout(() => cb?.(err ?? null, out));
		};
		zlibMod[lower][Symbol.for("nodejs.util.promisify.custom")] = (buf, opts) => new Promise((res, rej) => zlibMod[lower](buf, opts, (e, o) => (e ? rej(e) : res(o))));
	}

	// ---------------------------------------------------------------- dgram
	// UDP is not available; sockets exist (so modules load) but error on use.
	class UdpSocket extends EventEmitter {
		constructor(type) {
			super();
			this.type = typeof type === "object" ? type?.type : type;
		}
		_fail(what, cb) {
			const err = Object.assign(new Error(`dgram.${what}: UDP is not available to plugins`), { code: "EACCES" });
			setTimeout(() => (typeof cb === "function" ? cb(err) : this.emit("error", err)));
			return this;
		}
		bind(...a) {
			return this._fail("bind", a.find((x) => typeof x === "function") && undefined);
		}
		send(...a) {
			return this._fail("send", a.find((x) => typeof x === "function"));
		}
		connect(...a) {
			return this._fail("connect", a.find((x) => typeof x === "function"));
		}
		close(cb) {
			if (cb) setTimeout(cb);
			setTimeout(() => this.emit("close"));
			return this;
		}
		address() {
			return { address: "0.0.0.0", family: "IPv4", port: 0 };
		}
		addMembership() {}
		dropMembership() {}
		setBroadcast() {}
		setMulticastTTL() {}
		setMulticastLoopback() {}
		setMulticastInterface() {}
		setTTL() {}
		setRecvBufferSize() {}
		setSendBufferSize() {}
		ref() {
			return this;
		}
		unref() {
			return this;
		}
	}
	const dgram = { createSocket: (type, listener) => {
		const s = new UdpSocket(type);
		if (listener) s.on("message", listener);
		return s;
	}, Socket: UdpSocket };

	const notAvailable = (what) => () => {
		throw new Error(`${what} is not available to plugins in Tidalfast`);
	};

	// ---------------------------------------------------------------- child_process
	// Every program is started by the host, which first asks the user (Allow
	// once / Always / Deny) and tells the plugin the answer as events. Nothing
	// is spawned from here, and `fork` (a Node process) does not exist.
	const children = new Map();
	const cpError = (message, extra) => {
		const m = /^([A-Z][A-Z0-9_]+): ([\s\S]*)$/.exec(String(message));
		const err = new Error(m ? m[2] : String(message));
		if (m) err.code = m[1];
		if (m && m[1] === "ENOENT") Object.assign(err, { errno: -2 });
		return Object.assign(err, extra);
	};
	const envOf = (env) => {
		if (env === undefined || env === null || typeof env !== "object") return undefined;
		const out = {};
		for (const [k, v] of Object.entries(env)) if (v !== undefined && v !== null) out[k] = String(v);
		return out;
	};
	const pipesOf = (stdio) => {
		const one = (v) => v === undefined || v === null || v === "pipe" || v === "overlapped";
		if (Array.isArray(stdio)) return [one(stdio[0]), one(stdio[1]), one(stdio[2])];
		if (typeof stdio === "string") return [one(stdio), one(stdio), one(stdio)];
		return [true, true, true];
	};
	const parseExit = (data) => {
		try {
			return JSON.parse(data);
		} catch {
			return { code: null, signal: null };
		}
	};
	class ChildProcess extends EventEmitter {
		constructor(plugin, file, args, pipes) {
			super();
			setOwner(this, plugin);
			this.handle = undefined;
			this.pid = undefined;
			this.exitCode = null;
			this.signalCode = null;
			this.killed = false;
			this.connected = false;
			this.spawnfile = file;
			this.spawnargs = [file, ...args];
			this._over = false;
			this._failed = false;
			this.stdin = pipes[0]
				? new Writable({
						write: (chunk, enc, cb) => {
							if (this._over) return cb();
							try {
								callOp("child.stdin", { plugin: ownerOf(this), handle: this.handle, data: L.b64enc(chunk) });
								cb();
							} catch (err) {
								cb(cpError(err.message));
							}
						},
						final: (cb) => {
							if (!this._over && this.handle !== undefined) {
								try {
									callOp("child.stdinEnd", { plugin: ownerOf(this), handle: this.handle });
								} catch {}
							}
							cb();
						},
					})
				: null;
			this.stdout = pipes[1] ? new Readable() : null;
			this.stderr = pipes[2] ? new Readable() : null;
			this.stdio = [this.stdin, this.stdout, this.stderr];
		}
		get plugin() {
			return ownerOf(this);
		}
		kill(signal) {
			if (this._over || this.handle === undefined) return false;
			let sent = false;
			try {
				sent = callOp("child.kill", { plugin: ownerOf(this), handle: this.handle, signal: typeof signal === "string" ? signal : "SIGTERM" });
			} catch {}
			if (sent) this.killed = true;
			return sent;
		}
		ref() {}
		unref() {}
		disconnect() {}
		send() {
			throw cpError("ERR_IPC_CHANNEL_CLOSED: Channel closed");
		}
		[Symbol.dispose]() {
			this.kill();
		}
		_event(kind, data) {
			switch (kind) {
				case "spawn":
					this.pid = Number(data);
					this.emit("spawn");
					break;
				case "stdout":
				case "stderr": {
					const s = this[kind];
					if (s) s.push(Buffer.from(L.b64dec(data)));
					break;
				}
				case "error":
					this._failed = true;
					this.emit(
						"error",
						cpError(data, { syscall: `spawn ${this.spawnfile}`, path: this.spawnfile, spawnargs: this.spawnargs.slice(1) }),
					);
					break;
				case "exit": {
					const { code, signal } = parseExit(data);
					this._over = true;
					this.exitCode = code;
					this.signalCode = signal;
					this.emit("exit", code, signal);
					break;
				}
				case "close": {
					const { code, signal } = parseExit(data);
					this._over = true;
					if (this.exitCode === null && this.signalCode === null) {
						this.exitCode = this._failed ? -2 : code;
						this.signalCode = signal;
					}
					children.delete(this.handle);
					this.stdout?.push(null);
					this.stderr?.push(null);
					this.stdin?.destroy?.();
					// After the streams' own end events.
					queueMicrotask(() => queueMicrotask(() => this.emit("close", this.exitCode, this.signalCode)));
					break;
				}
			}
		}
	}
	L.node.childEvent = (handle, kind, data) => children.get(handle)?._event(kind, data);
	g.__luna_child = (handle, kind, data) => L.node.childEvent(handle, kind, data);
	const startChild = ({ file, args, command, opts }) => {
		const plugin = L.pluginFromStack();
		const pipes = pipesOf(opts.stdio);
		const child = new ChildProcess(plugin, command ?? file, args, pipes);
		try {
			child.handle = callOp("child.spawn", {
				plugin: plugin ?? null,
				file: command === undefined ? file : "",
				args,
				command,
				shell: !!opts.shell,
				cwd: opts.cwd === undefined || opts.cwd === null ? undefined : String(opts.cwd),
				env: envOf(opts.env),
				pipes,
			});
			children.set(child.handle, child);
		} catch (err) {
			queueMicrotask(() => {
				child._failed = true;
				child.emit("error", cpError(err.message, { syscall: `spawn ${child.spawnfile}`, path: child.spawnfile }));
				child._event("close", JSON.stringify({ code: null, signal: null }));
			});
			return child;
		}
		if (opts.timeout > 0) {
			const timer = setTimeout(() => child.kill(typeof opts.killSignal === "string" ? opts.killSignal : "SIGTERM"), opts.timeout);
			child.once("close", () => clearTimeout(timer));
		}
		return child;
	};
	const argsOpts = (args, opts) => {
		if (!Array.isArray(args)) return [[], args ?? {}];
		return [args.map(String), opts ?? {}];
	};
	const cpSpawn = (file, args, opts) => {
		if (typeof file !== "string" || file === "") throw cpError("ERR_INVALID_ARG_TYPE: The \"file\" argument must be of type string and non-empty");
		const [a, o] = argsOpts(args, opts);
		return startChild({ file, args: a, opts: o });
	};
	// exec / execFile: buffer the output and call back when the program ends.
	const buffered = (child, opts, cmd, cb) => {
		const encoding = opts.encoding === undefined ? "utf8" : opts.encoding;
		const max = opts.maxBuffer ?? 1024 * 1024;
		const out = { stdout: [], stderr: [] };
		let size = 0;
		let overflow = false;
		for (const k of ["stdout", "stderr"]) {
			child[k]?.on("data", (d) => {
				if (overflow) return;
				size += d.length;
				if (size > max) {
					overflow = true;
					child.kill(typeof opts.killSignal === "string" ? opts.killSignal : "SIGTERM");
					return;
				}
				out[k].push(d);
			});
		}
		let finished = false;
		const text = (chunks) => {
			const all = Buffer.concat(chunks);
			return encoding === "buffer" || encoding === null ? all : all.toString(encoding);
		};
		const finish = (err, code, signal) => {
			if (finished) return;
			finished = true;
			if (!cb) return;
			const stdout = text(out.stdout);
			const stderr = text(out.stderr);
			if (!err && overflow) err = cpError("ERR_CHILD_PROCESS_STDIO_MAXBUFFER: stdout maxBuffer length exceeded");
			if (!err && (code !== 0 || signal)) {
				err = Object.assign(new Error(`Command failed: ${cmd}\n${typeof stderr === "string" ? stderr : stderr.toString()}`), {
					code,
					killed: child.killed,
					signal,
				});
			}
			if (err) Object.assign(err, { cmd });
			cb(err ?? null, stdout, stderr);
		};
		child.on("error", (e) => finish(e));
		child.on("close", (code, signal) => finish(null, code, signal));
		return child;
	};
	const cpExec = (command, opts, cb) => {
		if (typeof opts === "function") [cb, opts] = [opts, undefined];
		opts = opts ?? {};
		if (typeof command !== "string") throw cpError("ERR_INVALID_ARG_TYPE: The \"command\" argument must be of type string");
		return buffered(startChild({ file: command, args: [], command, opts }), opts, command, cb);
	};
	const cpExecFile = (file, args, opts, cb) => {
		if (typeof args === "function") [cb, args, opts] = [args, undefined, undefined];
		else if (typeof opts === "function") [cb, opts] = [opts, undefined];
		if (typeof file !== "string" || file === "") throw cpError("ERR_INVALID_ARG_TYPE: The \"file\" argument must be of type string and non-empty");
		const [a, o] = argsOpts(args, opts);
		return buffered(startChild({ file, args: a, opts: o }), o, [file, ...a].join(" "), cb);
	};
	const promisified = (fn) => (...a) =>
		new Promise((res, rej) =>
			fn(...a, (e, stdout, stderr) => {
				if (e) rej(Object.assign(e, { stdout, stderr }));
				else res({ stdout, stderr });
			}),
		);
	cpExec[Symbol.for("nodejs.util.promisify.custom")] = promisified(cpExec);
	cpExecFile[Symbol.for("nodejs.util.promisify.custom")] = promisified(cpExecFile);
	// The synchronous forms wait for the program here, so only a program the
	// user already allowed for good can run (there is nobody to ask meanwhile);
	// otherwise the host asks and the call fails, to be retried.
	const runSync = ({ file, args, command, opts }) => {
		const plugin = L.pluginFromStack();
		const pipes = pipesOf(opts.stdio);
		const empty = { pid: 0, output: null, stdout: null, stderr: null, status: null, signal: null };
		let r;
		try {
			r = callOp("child.spawnSync", {
				plugin: plugin ?? null,
				file: command === undefined ? file : "",
				args,
				command,
				shell: !!opts.shell,
				cwd: opts.cwd === undefined || opts.cwd === null ? undefined : String(opts.cwd),
				env: envOf(opts.env),
				pipes,
				input: opts.input === undefined ? undefined : L.b64enc(typeof opts.input === "string" ? Buffer.from(opts.input) : opts.input),
				timeout: opts.timeout > 0 ? opts.timeout : undefined,
				maxBuffer: opts.maxBuffer,
			});
		} catch (err) {
			return { ...empty, error: cpError(err.message, { syscall: `spawnSync ${command ?? file}`, path: command ?? file }) };
		}
		const enc = opts.encoding === undefined || opts.encoding === null || opts.encoding === "buffer" ? null : opts.encoding;
		const conv = (b64) => {
			const b = Buffer.from(L.b64dec(b64));
			return enc ? b.toString(enc) : b;
		};
		const stdout = pipes[1] ? conv(r.stdout) : null;
		const stderr = pipes[2] ? conv(r.stderr) : null;
		return {
			pid: r.pid,
			output: [null, stdout, stderr],
			stdout,
			stderr,
			status: r.status,
			signal: r.signal,
			...(r.error ? { error: cpError(r.error, { syscall: `spawnSync ${command ?? file}` }) } : {}),
		};
	};
	const failedSync = (r, cmd) => {
		const text = (v) => (v === null || v === undefined ? "" : typeof v === "string" ? v : v.toString());
		const err = r.error ?? Object.assign(new Error(`Command failed: ${cmd}${text(r.stderr) ? "\n" + text(r.stderr) : ""}`), {});
		return Object.assign(err, { status: r.status, signal: r.signal, output: r.output, pid: r.pid, stdout: r.stdout, stderr: r.stderr });
	};
	const cpSpawnSync = (file, args, opts) => {
		if (typeof file !== "string" || file === "") throw cpError("ERR_INVALID_ARG_TYPE: The \"file\" argument must be of type string and non-empty");
		const [a, o] = argsOpts(args, opts);
		return runSync({ file, args: a, opts: o });
	};
	const cpExecFileSync = (file, args, opts) => {
		if (typeof file !== "string" || file === "") throw cpError("ERR_INVALID_ARG_TYPE: The \"file\" argument must be of type string and non-empty");
		const [a, o] = argsOpts(args, opts);
		const r = runSync({ file, args: a, opts: o });
		if (r.error || r.status !== 0 || r.signal) throw failedSync(r, [file, ...a].join(" "));
		return r.stdout;
	};
	const cpExecSync = (command, opts) => {
		if (typeof command !== "string") throw cpError("ERR_INVALID_ARG_TYPE: The \"command\" argument must be of type string");
		opts = opts ?? {};
		const r = runSync({ file: command, args: [], command, opts });
		if (r.error || r.status !== 0 || r.signal) throw failedSync(r, command);
		return r.stdout;
	};
	const childProcess = {
		ChildProcess,
		spawn: cpSpawn,
		exec: cpExec,
		execFile: cpExecFile,
		spawnSync: cpSpawnSync,
		execSync: cpExecSync,
		execFileSync: cpExecFileSync,
		fork: notAvailable("child_process.fork (there is no Node here to run a script)"),
	};
	// http/https clients on top of fetch (so the host's network policy and
	// limits apply). Bodies are buffered; servers are not available.
	class IncomingMessage extends Readable {
		constructor() {
			super();
			Object.assign(this, { headers: {}, rawHeaders: [], trailers: {}, rawTrailers: [], statusCode: 0, statusMessage: "", httpVersion: "1.1", httpVersionMajor: 1, httpVersionMinor: 1, complete: false, aborted: false, url: "", method: null });
			this.socket = this.connection = new EventEmitter();
		}
		setTimeout() {
			return this;
		}
	}
	const httpRequest = (scheme) =>
		function request(a, b, c) {
			let url, opts, cb;
			if (typeof a === "string" || a instanceof URL) {
				url = new URL(String(a));
				if (typeof b === "function") cb = b;
				else [opts, cb] = [b ?? {}, c];
			} else [opts, cb] = [a ?? {}, b];
			opts ??= {};
			if (!url) {
				const proto = opts.protocol ?? `${scheme}:`;
				const host = opts.hostname ?? (opts.host ?? "localhost").replace(/:\d+$/, "");
				const port = opts.port ?? (opts.host ?? "").match(/:(\d+)$/)?.[1];
				url = new URL(`${proto}//${host.includes(":") ? `[${host}]` : host}${port ? `:${port}` : ""}${opts.path ?? "/"}`);
			} else if (opts.path) url = new URL(opts.path, url);
			return new ClientRequest(url, opts, cb);
		};
	class ClientRequest extends Writable {
		constructor(url, opts, cb) {
			super();
			this._url = url;
			this.method = String(opts.method ?? "GET").toUpperCase();
			this.path = url.pathname + url.search;
			this.host = url.hostname;
			this.protocol = url.protocol;
			this._headers = {};
			for (const [k, v] of Object.entries(opts.headers ?? {})) if (v !== undefined) this._headers[k.toLowerCase()] = v;
			if (opts.auth && !this._headers.authorization) this._headers.authorization = `Basic ${Buffer.from(opts.auth).toString("base64")}`;
			this._chunks = [];
			this._ended = false;
			this.aborted = false;
			this.finished = false;
			this.reusedSocket = false;
			this.socket = this.connection = Object.assign(new EventEmitter(), { setTimeout() {}, setNoDelay() {}, setKeepAlive() {}, destroy() {}, remoteAddress: url.hostname, encrypted: url.protocol === "https:" });
			if (opts.signal) opts.signal.addEventListener?.("abort", () => this.destroy(opts.signal.reason));
			if (cb) this.once("response", cb);
			setTimeout(() => this.emit("socket", this.socket));
		}
		setHeader(k, v) {
			this._headers[String(k).toLowerCase()] = v;
			return this;
		}
		getHeader(k) {
			return this._headers[String(k).toLowerCase()];
		}
		getHeaders() {
			return { ...this._headers };
		}
		getHeaderNames() {
			return Object.keys(this._headers);
		}
		hasHeader(k) {
			return String(k).toLowerCase() in this._headers;
		}
		removeHeader(k) {
			delete this._headers[String(k).toLowerCase()];
		}
		get headersSent() {
			return this._ended;
		}
		write(c, e, cb) {
			if (typeof e === "function") [cb, e] = [e, undefined];
			if (c !== undefined && c !== null) this._chunks.push(typeof c === "string" ? Buffer.from(c, e) : Buffer.from(c));
			if (cb) queueMicrotask(cb);
			return true;
		}
		end(c, e, cb) {
			if (typeof c === "function") [cb, c] = [c, undefined];
			else if (typeof e === "function") [cb, e] = [e, undefined];
			if (c !== undefined && c !== null) this.write(c, e);
			if (this._ended) return this;
			this._ended = this.finished = this.writableEnded = true;
			this._send();
			if (cb) queueMicrotask(cb);
			this.emit("finish");
			return this;
		}
		_send() {
			const body = this._chunks.length ? Buffer.concat(this._chunks) : undefined;
			const headers = {};
			for (const [k, v] of Object.entries(this._headers)) headers[k] = Array.isArray(v) ? v.join(", ") : String(v);
			this._ctl = new AbortController();
			g.fetch(this._url.href, { method: this.method, headers, body: this.method === "GET" || this.method === "HEAD" ? undefined : body, signal: this._ctl.signal })
				.then(async (res) => {
					const buf = Buffer.from(await res.arrayBuffer());
					if (this.aborted) return;
					const m = new IncomingMessage();
					m.statusCode = res.status;
					m.statusMessage = res.statusText;
					m.url = res.url;
					for (const [k, v] of res.headers) {
						m.headers[k] = k === "set-cookie" ? String(v).split(/,(?=[^;]+=)/) : v;
						m.rawHeaders.push(k, v);
					}
					this.res = m;
					this.emit("response", m);
					if (buf.length) m.push(buf);
					m.complete = true;
					m.push(null);
					this.emit("close");
				})
				.catch((err) => {
					if (this.aborted) return;
					this.emit("error", Object.assign(err instanceof Error ? err : new Error(String(err)), { code: err?.code ?? "ECONNRESET" }));
					this.emit("close");
				});
		}
		abort() {
			if (this.aborted) return;
			this.aborted = true;
			this._ctl?.abort();
			this.emit("abort");
			this.emit("close");
		}
		destroy(err) {
			this.abort();
			if (err) this.emit("error", err);
			return this;
		}
		setTimeout(ms, cb) {
			if (cb) this.once("timeout", cb);
			return this;
		}
		setNoDelay() {}
		setSocketKeepAlive() {}
		flushHeaders() {}
	}
	const httpGet = (scheme) =>
		function get(...a) {
			const r = httpRequest(scheme)(...a);
			r.end();
			return r;
		};
	// ---------------------------------------------------------------- servers
	// TCP servers listen on 127.0.0.1 only (whatever host is asked for), so
	// only apps on this computer can reach a plugin's server.
	class NetServer extends EventEmitter {
		constructor(opts, onConnection) {
			super();
			if (typeof opts === "function") onConnection = opts;
			if (onConnection) this.on("connection", onConnection);
			setOwner(this, L.pluginFromStack());
			this.handle = undefined;
			this.listening = false;
			this._address = null;
			this.maxConnections = undefined;
			this._conns = new Set();
		}
		get plugin() {
			return ownerOf(this);
		}
		listen(...args) {
			let cb;
			if (typeof args.at(-1) === "function") cb = args.pop();
			let port = 0;
			if (typeof args[0] === "object" && args[0] !== null) port = Number(args[0].port ?? 0);
			else if (args[0] !== undefined && args[0] !== null) port = Number(args[0]);
			if (cb) this.once("listening", cb);
			try {
				this.handle = callOp("net.listen", { plugin: ownerOf(this), port: Number.isFinite(port) ? port : 0 });
				sockets.set(this.handle, this);
			} catch (err) {
				const m = /^(E[A-Z]+):/.exec(err.message);
				queueMicrotask(() => this.emit("error", Object.assign(new Error(err.message), { code: m?.[1] ?? "EACCES", syscall: "listen", port })));
			}
			return this;
		}
		_event(kind, data) {
			switch (kind) {
				case "listening":
					this.listening = true;
					this._address = { address: "127.0.0.1", family: "IPv4", port: Number(data) };
					this.emit("listening");
					break;
				case "connection": {
					const [h, ip, port] = String(data).split(" ");
					const s = new Socket();
					setOwner(s, ownerOf(this));
					s.handle = Number(h);
					s.readyState = "open";
					s.pending = false;
					s.remoteAddress = ip;
					s.remotePort = Number(port);
					s.remoteFamily = String(ip).includes(":") ? "IPv6" : "IPv4";
					s.localAddress = "127.0.0.1";
					s.localPort = this._address?.port;
					s.server = this;
					sockets.set(s.handle, s);
					this._conns.add(s);
					s.once("close", () => this._conns.delete(s));
					this.emit("connection", s);
					break;
				}
				case "error": {
					const m = /^(E[A-Z]+):/.exec(data);
					this.emit("error", Object.assign(new Error(data), { code: m?.[1] ?? "EIO", syscall: "listen" }));
					break;
				}
				case "close":
					sockets.delete(this.handle);
					this.listening = false;
					this._closing = false;
					this.emit("close");
					break;
			}
		}
		close(cb) {
			if (!this.listening || this._closing) {
				// As in Node: closing a server that is not running only errors the callback.
				if (cb) queueMicrotask(() => cb(Object.assign(new Error("Server is not running."), { code: "ERR_SERVER_NOT_RUNNING" })));
				return this;
			}
			this._closing = true;
			if (cb) this.once("close", () => cb());
			callOp("net.close", { handle: this.handle, plugin: ownerOf(this) });
			return this;
		}
		closeAllConnections() {
			for (const s of this._conns) s.destroy();
		}
		destroy() {
			this.closeAllConnections();
			return this.close();
		}
		closeIdleConnections() {}
		address() {
			return this._address;
		}
		getConnections(cb) {
			cb?.(null, this._conns.size);
		}
		ref() {
			return this;
		}
		unref() {
			return this;
		}
		setTimeout() {
			return this;
		}
		[Symbol.asyncDispose]() {
			return new Promise((r) => this.close(r));
		}
	}
	// HTTP/1.1 on top of NetServer: requests are parsed once their body has
	// arrived; responses stream (chunked) or go out in one piece on end().
	class ServerResponse extends Writable {
		constructor(req, sock) {
			super();
			this.req = req;
			this.socket = this.connection = sock;
			this.statusCode = 200;
			this.statusMessage = undefined;
			this.sendDate = true;
			this.headersSent = false;
			this.finished = false;
			this.writableEnded = false;
			this.writableFinished = false;
			this.chunkedEncoding = false;
			this._h = new Map();
			const conn = String(req.headers.connection ?? "").toLowerCase();
			this.shouldKeepAlive = req.httpVersion === "1.1" ? !conn.includes("close") : conn.includes("keep-alive");
		}
		setHeader(k, v) {
			this._h.set(String(k).toLowerCase(), [String(k), v]);
			return this;
		}
		appendHeader(k, v) {
			const old = this.getHeader(k);
			return this.setHeader(k, old === undefined ? v : [].concat(old, v));
		}
		getHeader(k) {
			return this._h.get(String(k).toLowerCase())?.[1];
		}
		getHeaders() {
			const o = {};
			for (const [k, [, v]] of this._h) o[k] = v;
			return o;
		}
		getHeaderNames() {
			return [...this._h.keys()];
		}
		hasHeader(k) {
			return this._h.has(String(k).toLowerCase());
		}
		removeHeader(k) {
			this._h.delete(String(k).toLowerCase());
		}
		writeHead(code, msg, headers) {
			if (typeof msg === "object" && msg !== null) [headers, msg] = [msg, undefined];
			this.statusCode = code;
			if (msg) this.statusMessage = msg;
			if (Array.isArray(headers)) for (let i = 0; i + 1 < headers.length; i += 2) this.setHeader(headers[i], headers[i + 1]);
			else if (headers) for (const [k, v] of Object.entries(headers)) this.setHeader(k, v);
			return this;
		}
		_head(extra) {
			const lines = [`HTTP/1.1 ${this.statusCode} ${this.statusMessage ?? http.STATUS_CODES[this.statusCode] ?? ""}`];
			if (this.sendDate && !this.hasHeader("date")) lines.push(`Date: ${new Date().toUTCString()}`);
			if (!this.hasHeader("connection")) lines.push(`Connection: ${this.shouldKeepAlive ? "keep-alive" : "close"}`);
			for (const [, [k, v]] of this._h) for (const x of [].concat(v)) lines.push(`${k}: ${x}`);
			for (const [k, v] of extra) lines.push(`${k}: ${v}`);
			this.headersSent = true;
			return Buffer.from(`${lines.join("\r\n")}\r\n\r\n`, "latin1");
		}
		_bodyless() {
			return this.req.method === "HEAD" || this.statusCode === 204 || this.statusCode === 304 || (this.statusCode >= 100 && this.statusCode < 200);
		}
		flushHeaders() {
			if (this.headersSent) return;
			const extra = [];
			if (!this.hasHeader("content-length") && !this._bodyless()) {
				this.chunkedEncoding = true;
				extra.push(["Transfer-Encoding", "chunked"]);
			}
			this.socket.write(this._head(extra));
		}
		write(c, e, cb) {
			if (typeof e === "function") [cb, e] = [e, undefined];
			if (this.writableEnded) {
				cb?.(new Error("write after end"));
				return false;
			}
			const b = c === undefined || c === null ? Buffer.alloc(0) : typeof c === "string" ? Buffer.from(c, e) : Buffer.from(c);
			this.flushHeaders();
			if (b.length && !this._bodyless()) this.socket.write(this.chunkedEncoding ? Buffer.concat([Buffer.from(`${b.length.toString(16)}\r\n`), b, Buffer.from("\r\n")]) : b);
			if (cb) queueMicrotask(cb);
			return true;
		}
		end(c, e, cb) {
			if (typeof c === "function") [cb, c] = [c, undefined];
			else if (typeof e === "function") [cb, e] = [e, undefined];
			if (this.writableEnded) return this;
			const b = c === undefined || c === null ? Buffer.alloc(0) : typeof c === "string" ? Buffer.from(c, e) : Buffer.from(c);
			if (!this.headersSent) {
				const extra = this.hasHeader("content-length") || this._bodyless() ? [] : [["Content-Length", b.length]];
				this.socket.write(Buffer.concat([this._head(extra), this._bodyless() ? Buffer.alloc(0) : b]));
			} else {
				if (b.length) this.write(b);
				if (this.chunkedEncoding) this.socket.write(Buffer.from("0\r\n\r\n"));
			}
			this.finished = this.writableEnded = this.writableFinished = true;
			queueMicrotask(() => {
				this.emit("finish");
				this.emit("close");
				cb?.();
			});
			return this;
		}
		writeContinue() {
			this.socket.write(Buffer.from("HTTP/1.1 100 Continue\r\n\r\n"));
		}
		writeProcessing() {}
		setTimeout() {
			return this;
		}
		addTrailers() {}
		assignSocket() {}
		detachSocket() {}
		cork() {}
		uncork() {}
	}
	// Plugins' HTTP servers by port, for following a plugin's loopback
	// stream proxy to what it proxies (L.resolveLocalStream).
	const httpServers = new Map();
	class HttpServer extends NetServer {
		constructor(opts, handler) {
			if (typeof opts === "function") [handler, opts] = [opts, {}];
			super();
			if (handler) this.on("request", handler);
			this.on("listening", () => httpServers.set(this._address.port, this));
			this.on("close", () => {
				for (const [port, srv] of httpServers) if (srv === this) httpServers.delete(port);
			});
			this.timeout = 0;
			this.keepAliveTimeout = 5000;
			this.headersTimeout = 60000;
			this.requestTimeout = 300000;
			this.maxHeadersCount = null;
			this.on("connection", (s) => this._serve(s));
		}
		_serve(sock) {
			let buf = Buffer.alloc(0);
			let busy = false;
			let detached = false;
			const MAX = 16 * 1024 * 1024;
			const pump = () => {
				while (!busy && !detached) {
					const i = buf.indexOf("\r\n\r\n");
					if (i < 0) {
						if (buf.length > 64 * 1024) sock.destroy();
						return;
					}
					const lines = buf.subarray(0, i).toString("latin1").split("\r\n");
					const [method, target = "/", ver = "HTTP/1.1"] = lines[0].split(" ");
					const req = new IncomingMessage();
					req.method = method;
					req.url = target;
					req.httpVersion = ver.split("/")[1] ?? "1.1";
					[req.httpVersionMajor, req.httpVersionMinor] = req.httpVersion.split(".").map(Number);
					req.socket = req.connection = sock;
					for (const l of lines.slice(1)) {
						const c = l.indexOf(":");
						if (c <= 0) continue;
						const k = l.slice(0, c).trim();
						const v = l.slice(c + 1).trim();
						req.rawHeaders.push(k, v);
						const lk = k.toLowerCase();
						if (lk === "set-cookie") (req.headers[lk] ??= []).push(v);
						else req.headers[lk] = lk in req.headers ? `${req.headers[lk]}, ${v}` : v;
					}
					let body;
					let rest;
					if (/chunked/i.test(req.headers["transfer-encoding"] ?? "")) {
						const parts = [];
						let p = i + 4;
						for (;;) {
							const e = buf.indexOf("\r\n", p);
							if (e < 0) return;
							const n = parseInt(buf.subarray(p, e).toString("latin1"), 16);
							if (Number.isNaN(n)) return sock.destroy();
							if (buf.length < e + 2 + n + 2) return;
							if (n === 0) {
								const end = buf.indexOf("\r\n\r\n", e);
								rest = buf.subarray(end < 0 ? e + 4 : end + 4);
								break;
							}
							parts.push(buf.subarray(e + 2, e + 2 + n));
							p = e + 2 + n + 2;
						}
						body = Buffer.concat(parts);
					} else {
						const len = Number(req.headers["content-length"] ?? 0) || 0;
						if (len > MAX) return sock.destroy();
						if (buf.length < i + 4 + len) return;
						body = buf.subarray(i + 4, i + 4 + len);
						rest = buf.subarray(i + 4 + len);
					}
					buf = Buffer.from(rest);
					if (req.headers.upgrade && this.listenerCount("upgrade") > 0) {
						detached = true;
						sock.removeListener("data", onData);
						this.emit("upgrade", req, sock, buf);
						return;
					}
					if (method === "CONNECT" && this.listenerCount("connect") > 0) {
						detached = true;
						sock.removeListener("data", onData);
						this.emit("connect", req, sock, buf);
						return;
					}
					const res = new ServerResponse(req, sock);
					busy = true;
					res.once("finish", () => {
						busy = false;
						if (!res.shouldKeepAlive) sock.end();
						else pump();
					});
					if (/100-continue/i.test(req.headers.expect ?? "") && this.listenerCount("checkContinue") > 0) this.emit("checkContinue", req, res);
					else this.emit("request", req, res);
					if (body.length) req.push(body);
					req.complete = true;
					req.push(null);
				}
			};
			const onData = (chunk) => {
				buf = buf.length ? Buffer.concat([buf, chunk]) : Buffer.from(chunk);
				if (buf.length > MAX) return sock.destroy();
				pump();
			};
			sock.on("data", onData);
			sock.on("error", (e) => this.emit("clientError", e, sock));
		}
		setTimeout() {
			return this;
		}
	}
	// The address a plugin's own loopback proxy serves `src` from. AltPlay
	// plays the listener's server through a node:http proxy on 127.0.0.1
	// (Chromium stalls on its server); Tidalfast's engine fetches the
	// stream itself, so the proxy is asked, with a request that goes
	// nowhere, which address it would fetch: the one fetch its handler
	// makes at once. Null when `src` is not such a proxy or it does not
	// answer that way.
	L.resolveLocalStream = (src) => {
		let u;
		try {
			u = new URL(String(src));
		} catch {
			return null;
		}
		if (u.protocol !== "http:" || !["localhost", "127.0.0.1", "[::1]"].includes(u.hostname)) return null;
		const srv = httpServers.get(Number(u.port || 80));
		if (!srv || srv.listenerCount("request") === 0) return null;
		const sock = new EventEmitter();
		Object.assign(sock, { write: () => true, end() {}, destroy() {}, setTimeout() {}, setNoDelay() {}, setKeepAlive() {}, remoteAddress: "127.0.0.1", remotePort: 0, localAddress: "127.0.0.1", localPort: Number(u.port || 80), writable: true });
		const req = new IncomingMessage();
		Object.assign(req, { method: "GET", url: u.pathname + u.search, socket: sock, connection: sock });
		req.headers = { host: u.host };
		req.rawHeaders = ["Host", u.host];
		const res = new ServerResponse(req, sock);
		const seen = [];
		const prev = L.captureFetch;
		L.captureFetch = seen;
		try {
			srv.emit("request", req, res);
		} catch {
		} finally {
			L.captureFetch = prev;
		}
		req.complete = true;
		req.push(null);
		queueMicrotask(() => res.emit("close"));
		const [only] = seen;
		return seen.length === 1 && only.method === "GET" && /^https?:\/\//i.test(only.url) ? only.url : null;
	};
	const http = {
		request: httpRequest("http"),
		get: httpGet("http"),
		createServer: (o, h) => new HttpServer(o, h),
		Server: HttpServer,
		Agent: class Agent {
			constructor(o) {
				this.options = o;
			}
			destroy() {}
		},
		globalAgent: {},
		STATUS_CODES: { 200: "OK", 201: "Created", 204: "No Content", 301: "Moved Permanently", 302: "Found", 304: "Not Modified", 400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 429: "Too Many Requests", 500: "Internal Server Error", 502: "Bad Gateway", 503: "Service Unavailable" },
		METHODS: ["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"],
		IncomingMessage,
		ServerResponse,
		OutgoingMessage: class OutgoingMessage extends Writable {},
		ClientRequest,
		validateHeaderName: () => {},
		validateHeaderValue: () => {},
		maxHeaderSize: 16384,
	};
	net.createServer = (o, l) => new NetServer(o, l);
	net.Server = NetServer;
	// Electron main-process API. Tidalfast has no Electron: windows, menus,
	// trays and global shortcuts exist (so plugins load and run) but are never
	// shown; each warns once per plugin.
	const warnedElectron = new Set();
	const electronWarn = (what) => {
		const plugin = L.pluginFromStack();
		const key = `${plugin}:${what}`;
		if (warnedElectron.has(key)) return;
		warnedElectron.add(key);
		console.warn(`[Luna] electron ${what} has no effect in Tidalfast`);
	};
	const windowState = () => {
		try {
			return callOp("window.state", null) ?? {};
		} catch {
			return {};
		}
	};
	const rect = { x: 0, y: 0, width: 1920, height: 1080 };
	const display = { id: 1, bounds: rect, workArea: rect, size: { width: 1920, height: 1080 }, workAreaSize: { width: 1920, height: 1040 }, scaleFactor: 1, rotation: 0, internal: true, touchSupport: "unknown", monochrome: false, accelerometerSupport: "unknown", colorDepth: 24, colorSpace: "", depthPerComponent: 8, displayFrequency: 60, label: "Display" };
	class WebContents extends EventEmitter {
		constructor(id) {
			super();
			this.id = id;
			this.session = electronSession;
			this._url = "";
		}
		send() {}
		postMessage() {}
		insertCSS() {
			return Promise.resolve("");
		}
		removeInsertedCSS() {
			return Promise.resolve();
		}
		setWindowOpenHandler() {}
		getURL() {
			return this._url;
		}
		getTitle() {
			return "";
		}
		isDestroyed() {
			return false;
		}
		isLoading() {
			return false;
		}
		loadURL(u) {
			this._url = String(u);
			this._editor = editorPage(this._url);
			return Promise.resolve();
		}
		loadFile(f) {
			this._url = `file://${f}`;
			return Promise.resolve();
		}
		// A Monaco editor page (Themer's CSS editor) is shown as Tidalfast's
		// own editor: `window.editor.setValue(text)` opens it with the text.
		executeJavaScript(code) {
			const m = this._editor && /^\s*window\.editor\.setValue\(([\s\S]*)\)\s*;?\s*$/.exec(String(code));
			if (m) {
				let text = "";
				try {
					text = String(JSON.parse(m[1]) ?? "");
				} catch {}
				this._editor.open = true;
				// The preload may be a temporary file (Themer removes it once
				// the page is filled): what it exposes is read now.
				const win = windows.get(this.id);
				if (win && !win._exposed) win._exposed = win._preload ? runPreload(win, win._preload) : {};
				callOp("editor.open", { window: this.id, title: this._editor.title, language: this._editor.language, text });
			}
			return Promise.resolve(undefined);
		}
		openDevTools() {}
		closeDevTools() {}
		reload() {}
		setZoomFactor() {}
		getZoomFactor() {
			return 1;
		}
		setAudioMuted() {}
		isAudioMuted() {
			return false;
		}
		focus() {}
		setUserAgent() {}
		getUserAgent() {
			return "Tidalfast";
		}
		capturePage() {
			return Promise.resolve(nativeImage.createEmpty());
		}
	}
	// The page a plugin window loads, when it is a Monaco editor: its title,
	// language and the call its change handler makes with the text
	// (`ipcRenderer.setCSS`), resolved against what the preload exposed.
	const editorPage = (url) => {
		const m = /^data:text\/html(;charset=[\w-]+)?(;base64)?,([\s\S]*)$/i.exec(url);
		if (!m) return null;
		let html = "";
		try {
			html = m[2] ? Buffer.from(m[3], "base64").toString("utf8") : decodeURIComponent(m[3]);
		} catch {
			return null;
		}
		if (!/monaco\.editor\.create/.test(html)) return null;
		const handler = /onDidChangeModelContent\(\s*\(\)\s*=>\s*([\w.$]+)\(\s*[\w$]+\.getValue\(\)\s*\)/.exec(html)?.[1] ?? null;
		return {
			title: /<title>([^<]*)<\/title>/i.exec(html)?.[1] ?? "Editor",
			language: /language\s*:\s*["'](\w+)["']/.exec(html)?.[1] ?? "plaintext",
			handler: handler?.replace(/^window\./, "") ?? null,
			open: false,
		};
	};
	// What a preload script hands the page through `contextBridge`, with
	// `ipcRenderer.send/invoke` reaching `ipcMain` as from that window.
	const runPreload = (win, path) => {
		const exposed = {};
		let src;
		try {
			src = L.node.readText(path);
		} catch {
			return exposed;
		}
		const event = () => ({ sender: win.webContents, reply() {}, returnValue: undefined });
		const electron = {
			contextBridge: { exposeInMainWorld: (k, api) => void (exposed[k] = api), exposeInIsolatedWorld: (_, k, api) => void (exposed[k] = api) },
			ipcRenderer: {
				send: (ch, ...a) => void ipcMain.emit(String(ch), event(), ...a),
				invoke: async (ch, ...a) => ipcMain._handlers.get(String(ch))?.(event(), ...a),
				on() {},
				once() {},
				removeListener() {},
				removeAllListeners() {},
			},
		};
		try {
			const module = { exports: {} };
			new Function("require", "module", "exports", src)((n) => (n === "electron" ? electron : g.require?.(n)), module, module.exports);
		} catch (err) {
			console.warn(`[Luna] preload failed: ${err?.message ?? err}`);
		}
		return exposed;
	};
	// The user typed in, or closed, the editor Tidalfast shows for a window.
	L.node.editorEvent = (arg) => {
		const win = windows.get(Number(arg.window));
		const ed = win?.webContents._editor;
		if (!win || !ed) return;
		if (arg.closed) {
			ed.open = false;
			win.destroy();
			return;
		}
		win._exposed ??= win._preload ? runPreload(win, win._preload) : {};
		let fn = win._exposed;
		let self = null;
		for (const part of (ed.handler ?? "").split(".")) {
			self = fn;
			fn = fn?.[part];
		}
		if (typeof fn === "function") fn.call(self, String(arg.text ?? ""));
	};
	let nextWin = 1;
	const windows = new Map();
	class BrowserWindow extends EventEmitter {
		constructor(opts = {}) {
			super();
			if (!opts.__lunaMain) electronWarn("BrowserWindow");
			this.id = nextWin++;
			this._opts = opts;
			this._bounds = { x: opts.x ?? 0, y: opts.y ?? 0, width: opts.width ?? 800, height: opts.height ?? 600 };
			this._visible = opts.show !== false;
			this.title = opts.title ?? "";
			this.webContents = new WebContents(this.id);
			this._destroyed = false;
			this._main = !!opts.__lunaMain;
			this._preload = opts.webPreferences?.preload ?? null;
			windows.set(this.id, this);
		}
		static getAllWindows() {
			return [...windows.values()];
		}
		static getFocusedWindow() {
			return windows.get(1) ?? null;
		}
		static fromId(id) {
			return windows.get(id) ?? null;
		}
		static fromWebContents(wc) {
			return windows.get(wc?.id) ?? null;
		}
		loadURL(u) {
			return this.webContents.loadURL(u);
		}
		loadFile(f) {
			return this.webContents.loadFile(f);
		}
		close() {
			if (this._main) return callOp("window.control", { op: "close" });
			this.destroy();
		}
		destroy() {
			if (this._destroyed) return;
			this._destroyed = true;
			windows.delete(this.id);
			if (this.webContents._editor?.open) {
				this.webContents._editor.open = false;
				callOp("editor.close", { window: this.id });
			}
			this.emit("close", { preventDefault() {} });
			this.emit("closed");
		}
		isDestroyed() {
			return this._destroyed;
		}
		show() {
			this._visible = true;
		}
		showInactive() {
			this._visible = true;
		}
		hide() {
			this._visible = false;
		}
		isVisible() {
			return this._visible;
		}
		focus() {}
		blur() {}
		isFocused() {
			return this._main ? windowState().focused : false;
		}
		// Only TIDAL's own window (the app's) is real; plugin-made windows
		// keep their state to themselves.
		minimize() {
			if (this._main) callOp("window.control", { op: "minimize" });
		}
		maximize() {
			if (this._main) callOp("window.control", { op: "maximize" });
			this._pend("maximized", true);
		}
		unmaximize() {
			if (this._main) callOp("window.control", { op: "unmaximize" });
			this._pend("maximized", false);
		}
		restore() {
			if (this._main) callOp("window.control", { op: "restore" });
			this._pend("maximized", false);
			this._pend("minimized", false);
		}
		isMinimized() {
			return this._get("minimized");
		}
		isMaximized() {
			return this._get("maximized");
		}
		setFullScreen(v) {
			this._pend("fullscreen", !!v);
			if (this._main) callOp("window.fullscreen", { value: !!v });
		}
		isFullScreen() {
			return this._get("fullscreen");
		}
		// A change asked for shows at once, until the app's own report
		// takes over.
		_pend(key, v) {
			(this._pending ??= {})[key] = { v, at: Date.now() };
		}
		_get(key) {
			const p = this._pending?.[key];
			if (p && Date.now() - p.at < 1500) return p.v;
			return this._main ? !!windowState()[key] : !!p?.v;
		}
		getBounds() {
			return { ...this._bounds };
		}
		setBounds(b) {
			Object.assign(this._bounds, b);
		}
		getContentBounds() {
			return this.getBounds();
		}
		setContentBounds(b) {
			this.setBounds(b);
		}
		getSize() {
			return [this._bounds.width, this._bounds.height];
		}
		setSize(w, h) {
			Object.assign(this._bounds, { width: w, height: h });
		}
		getContentSize() {
			return this.getSize();
		}
		setContentSize(w, h) {
			this.setSize(w, h);
		}
		getPosition() {
			return [this._bounds.x, this._bounds.y];
		}
		setPosition(x, y) {
			Object.assign(this._bounds, { x, y });
		}
		center() {}
		setTitle(t) {
			this.title = String(t);
		}
		getTitle() {
			return this.title;
		}
		setAlwaysOnTop(v) {
			this._top = !!v;
		}
		isAlwaysOnTop() {
			return !!this._top;
		}
		setOpacity() {}
		getOpacity() {
			return 1;
		}
		setResizable() {}
		isResizable() {
			return true;
		}
		setMovable() {}
		// The size limits a plugin sets are remembered, as Electron's getters
		// answer them ([0, 0] is "no limit"), so a plugin that reads the old
		// limit before widening it and restores it later (SmallWindow) works.
		setMinimumSize(w, h) {
			this._min = [Math.max(0, Math.trunc(w) || 0), Math.max(0, Math.trunc(h) || 0)];
			if (this._main) callOp("window.min_size", { w: this._min[0], h: this._min[1] });
		}
		getMinimumSize() {
			if (this._min) return [...this._min];
			if (!this._main) return [0, 0];
			// The main window starts with Tidalfast's own limit.
			try {
				return [...(callOp("window.min_size", null) ?? [0, 0])];
			} catch {
				return [0, 0];
			}
		}
		setMaximumSize(w, h) {
			this._max = [Math.max(0, Math.trunc(w) || 0), Math.max(0, Math.trunc(h) || 0)];
		}
		getMaximumSize() {
			return [...(this._max ?? [0, 0])];
		}
		setAspectRatio() {}
		setSkipTaskbar() {}
		setIgnoreMouseEvents() {}
		setVisibleOnAllWorkspaces() {}
		setBackgroundColor() {}
		setMenuBarVisibility() {}
		setAutoHideMenuBar() {}
		setMenu() {}
		removeMenu() {}
		setIcon() {}
		setProgressBar() {}
		setThumbarButtons() {
			return true;
		}
		setThumbnailToolTip() {}
		flashFrame() {}
		getNativeWindowHandle() {
			return Buffer.alloc(8);
		}
	}
	const nativeImage = {
		createEmpty: () => makeImage(),
		createFromPath: () => makeImage(),
		createFromBuffer: (b) => makeImage(b),
		createFromDataURL: () => makeImage(),
		createFromBitmap: (b) => makeImage(b),
	};
	const makeImage = (buf) => ({
		isEmpty: () => !buf,
		getSize: () => ({ width: 0, height: 0 }),
		toPNG: () => Buffer.from(buf ?? []),
		toJPEG: () => Buffer.from(buf ?? []),
		toBitmap: () => Buffer.alloc(0),
		toDataURL: () => "data:,",
		resize: function () {
			return this;
		},
		crop: function () {
			return this;
		},
		setTemplateImage() {},
		isTemplateImage: () => false,
		getAspectRatio: () => 1,
		addRepresentation() {},
	});
	const electronSession = {
		webRequest: { onBeforeRequest() {}, onBeforeSendHeaders() {}, onHeadersReceived() {}, onCompleted() {}, onErrorOccurred() {}, onResponseStarted() {}, onSendHeaders() {}, onBeforeRedirect() {} },
		setPermissionRequestHandler() {},
		setPermissionCheckHandler() {},
		setDevicePermissionHandler() {},
		setUserAgent() {},
		getUserAgent: () => "Tidalfast",
		setProxy: async () => {},
		clearCache: async () => {},
		clearStorageData: async () => {},
		setPreloads() {},
		getPreloads: () => [],
		registerPreloadScript: () => "",
		cookies: Object.assign(new EventEmitter(), { get: async () => [], set: async () => {}, remove: async () => {}, flushStore: async () => {} }),
		protocol: { handle() {}, unhandle() {}, isProtocolHandled: () => false, registerFileProtocol() {}, interceptBufferProtocol() {} },
		on() {},
		once() {},
		off() {},
		fetch: (...a) => g.fetch(...a),
	};
	// TIDAL's own window (id 1): what native code sends to its webContents
	// reaches the plugin's renderer side (`ipcRenderer.on`), as in Luna.
	const tidalWindow = new BrowserWindow({ __lunaMain: true, title: "TIDAL", width: 1280, height: 800 });
	tidalWindow.webContents.send = (ch, ...a) => L.lunaIpc(String(ch), JSON.stringify(a));
	// `webContents.executeJavaScript` on TIDAL's window runs the code in the
	// plugin renderer's realm, as in Luna (the API plugins call
	// `window.__apiInvokeAction?.(...)` this way for play, pause and next).
	// The answer is awaited, as Electron does.
	tidalWindow.webContents.executeJavaScript = async (code) => (0, eval)(String(code));
	tidalWindow.webContents._url = "https://desktop.tidal.com/";
	tidalWindow.destroy = () => {};
	L.node.tidalWindow = tidalWindow;
	const app = Object.assign(new EventEmitter(), {
		getPath: () => process.cwd(),
		getAppPath: () => process.cwd(),
		setPath() {},
		getVersion: () => g.__lunaVersions.tidal,
		getName: () => "Tidalfast",
		setName() {},
		getLocale: () => "en-US",
		getSystemLocale: () => "en-US",
		getPreferredSystemLanguages: () => ["en-US"],
		getLocaleCountryCode: () => "US",
		whenReady: async () => {},
		isReady: () => true,
		isPackaged: true,
		quit: () => electronWarn("app.quit"),
		exit: () => electronWarn("app.exit"),
		relaunch: () => electronWarn("app.relaunch"),
		focus() {},
		hide() {},
		show() {},
		requestSingleInstanceLock: () => true,
		hasSingleInstanceLock: () => true,
		releaseSingleInstanceLock() {},
		setAppUserModelId() {},
		setAsDefaultProtocolClient: () => false,
		isDefaultProtocolClient: () => false,
		removeAsDefaultProtocolClient: () => false,
		getLoginItemSettings: () => ({ openAtLogin: false }),
		setLoginItemSettings() {},
		getGPUFeatureStatus: () => ({}),
		getAppMetrics: () => [],
		setBadgeCount: () => false,
		getBadgeCount: () => 0,
		disableHardwareAcceleration() {},
		commandLine: { appendSwitch() {}, appendArgument() {}, hasSwitch: () => false, getSwitchValue: () => "", removeSwitch() {} },
	});
	const ipcMain = Object.assign(new EventEmitter(), {
		_handlers: new Map(),
		handle(ch, fn) {
			this._handlers.set(ch, fn);
		},
		handleOnce(ch, fn) {
			this._handlers.set(ch, fn);
		},
		removeHandler(ch) {
			this._handlers.delete(ch);
		},
	});
	// Shortcuts work while Tidalfast's window has focus, from the key events
	// the window delivers. Where the system has them (Windows), a plugin the
	// user allowed also holds them system-wide (asked at the first
	// register; hotkeys.rs): a press then arrives as the "shortcut" host
	// call, and the window never sees that key.
	const shortcuts = new Map();
	let globalSupport;
	const globalSupported = () => (globalSupport ??= !!callOp("shortcut.supported", null));
	const takeGlobally = (acc, plugin) => {
		if (!plugin || !globalSupported()) return;
		L.permit({ plugin, kind: "global_shortcuts", accelerator: acc })
			.then(() => {
				const s = shortcuts.get(acc);
				if (s === undefined || s.plugin !== plugin) return;
				callOp("shortcut.global", { plugin, accelerator: acc });
				s.global = true;
			})
			.catch((err) => callOp("log", { plugin, level: "warn", msg: `globalShortcut ${acc} works only while Tidalfast has focus: ${err?.message ?? err}` }));
	};
	const releaseGlobally = (acc, s) => {
		if (s?.global) callOp("shortcut.release", { plugin: s.plugin, accelerator: acc });
	};
	L.node.globalShortcutFired = ({ plugin, accelerator }) => {
		const s = shortcuts.get(accelerator);
		if (s === undefined || s.plugin !== plugin) return;
		try {
			s.cb();
		} catch (err) {
			L.reportError?.(err);
		}
	};
	// A track's TIDAL audio as a Readable (fetchMediaItemStream). Nothing is
	// opened until something consumes it, and what consumes it decides how:
	// - Every consumer throws the bytes away (it is piped only into sinks
	//   whose `write` never reads the chunk, see `discards`, or `resume()`d
	//   with nobody listening, Node's way to consume a stream without its
	//   data): the app reads the stream to its end and drops it
	//   (`source.drain`). Nothing reaches the plugin and nothing is saved,
	//   so it needs no permission; then "end", and the sinks are ended as
	//   `pipe` ends them. (A CDN warm-up after a stall reads a song this way.)
	// - Anything else gets the bytes from `source.open` (which asks for the
	//   permission first), read from the app chunk by chunk; destroying the
	//   stream closes it there. An open that fails is the stream's "error".
	class AudioStream extends Readable {
		constructor(plugin, source, opts) {
			super();
			this._lunaAudio = { plugin, source, opts, mode: null, scheduled: false, wants: false, resumed: false, pipes: [], handle: null, closed: false };
		}
		_destroy(err, cb) {
			this._lunaClose();
			cb?.(err);
		}
		_lunaClose() {
			const a = this._lunaAudio;
			if (a.closed) return;
			a.closed = true;
			if (a.handle === null) return;
			try {
				callOp("audio.close", { plugin: a.plugin, handle: a.handle });
			} catch {}
		}
		// Decided once, after the code that attached the first consumer has
		// attached the rest.
		_lunaDecide() {
			const a = this._lunaAudio;
			if (a.mode !== null || a.scheduled) return;
			a.scheduled = true;
			queueMicrotask(() => {
				a.scheduled = false;
				if (a.mode !== null || this.destroyed) return;
				const sinks = a.pipes.every((p) => discards(p.dest));
				if (!a.wants && sinks && (a.pipes.length > 0 || a.resumed)) this._lunaDrain();
				else this._lunaRead();
			});
		}
		_lunaDrain() {
			const a = this._lunaAudio;
			a.mode = "drain";
			for (const p of a.pipes) p.dest.emit?.("pipe", this);
			a.source.drain().then(
				() => {
					if (this.destroyed) return;
					a.closed = true;
					this._ended = true;
					this.readableEnded = true;
					this.emit("end");
					for (const p of a.pipes) if (p.opts?.end !== false) p.dest.end?.();
					this.emit("close");
				},
				(err) => this.destroy(err),
			);
		}
		_lunaRead() {
			const a = this._lunaAudio;
			a.mode = "read";
			for (const p of a.pipes) Readable.prototype.pipe.call(this, p.dest, p.opts);
			const signal = a.opts?.signal;
			let downloaded = 0;
			(async () => {
				try {
					const info = await a.source.open();
					a.handle = info.handle;
					Object.assign(this, { mimeType: info.mimeType, codec: info.codec, extension: info.extension });
					for (;;) {
						if (a.closed || this.destroyed) return this._lunaClose();
						if (signal?.aborted) throw signal.reason ?? new Error("aborted");
						const chunk = await L.callOpAsync("audio.read", { plugin: a.plugin, handle: a.handle });
						if (chunk === null || chunk === undefined) break;
						const b = Buffer.from(L.b64dec(chunk));
						downloaded += b.length;
						this.push(b);
						try {
							a.opts?.onProgress?.({ total: undefined, downloaded, percent: undefined });
						} catch {}
					}
					a.closed = true;
					this.push(null);
				} catch (err) {
					this._lunaClose();
					this.destroy(err);
				}
			})();
		}
		_add(name, fn, prepend, once) {
			const a = this._lunaAudio;
			if (a?.mode === null && (name === "data" || name === "readable")) {
				a.wants = true;
				this._lunaDecide();
			}
			return super._add(name, fn, prepend, once);
		}
		pipe(dest, opts) {
			const a = this._lunaAudio;
			if (a.mode !== null) return super.pipe(dest, opts);
			a.pipes.push({ dest, opts });
			this._lunaDecide();
			return dest;
		}
		unpipe(dest) {
			const a = this._lunaAudio;
			if (a.mode !== null) return super.unpipe(dest);
			a.pipes = dest ? a.pipes.filter((p) => p.dest !== dest) : [];
			return this;
		}
		read() {
			const a = this._lunaAudio;
			if (a.mode === null) {
				a.wants = true;
				this._lunaDecide();
				return null;
			}
			return super.read();
		}
		resume() {
			const a = this._lunaAudio;
			if (a.mode === null) {
				a.resumed = true;
				this._lunaDecide();
			}
			return super.resume();
		}
	}
	L.node.audioStream = (plugin, source, opts) => new AudioStream(plugin, source, opts);
	const accelKeys = { commandorcontrol: "ctrl", cmdorctrl: "ctrl", control: "ctrl", ctrl: "ctrl", command: "meta", cmd: "meta", super: "meta", meta: "meta", option: "alt", alt: "alt", altgr: "alt", shift: "shift" };
	const parseAccel = (acc) => {
		const want = { ctrl: false, meta: false, alt: false, shift: false, key: "" };
		for (const part of String(acc).split("+")) {
			const p = part.trim().toLowerCase();
			if (p === "") continue;
			if (accelKeys[p]) want[accelKeys[p]] = true;
			else want.key = p;
		}
		const names = { space: " ", plus: "+", esc: "escape", return: "enter", up: "arrowup", down: "arrowdown", left: "arrowleft", right: "arrowright", medianexttrack: "medianexttrack", mediaplaypause: "mediaplaypause", mediaprevioustrack: "mediaprevioustrack", mediastop: "mediastop", volumeup: "audiovolumeup", volumedown: "audiovolumedown", volumemute: "audiovolumemute" };
		want.key = names[want.key] ?? want.key;
		return want;
	};
	L.node.dropShortcuts = (plugin) => {
		for (const [acc, s] of [...shortcuts]) if (s.plugin === plugin) shortcuts.delete(acc);
	};
	let shortcutHook = false;
	const onShortcutKey = (e) => {
		const key = String(e.key ?? "").toLowerCase();
		for (const [acc, { want, cb }] of [...shortcuts]) {
			if (want.key !== key || want.ctrl !== !!e.ctrlKey || want.alt !== !!e.altKey || want.shift !== !!e.shiftKey || want.meta !== !!e.metaKey) continue;
			try {
				cb();
			} catch (err) {
				L.reportError?.(err);
			}
		}
	};
	const hookShortcuts = () => {
		if (shortcutHook) return;
		const target = g.window ?? g.document;
		if (typeof target?.addEventListener !== "function") return;
		shortcutHook = true;
		target.addEventListener("keydown", onShortcutKey);
	};
	const globalShortcut = {
		register: (acc, cb) => {
			if (typeof acc !== "string" || typeof cb !== "function") return false;
			if (shortcuts.has(acc)) return false;
			const plugin = L.pluginFromStack();
			shortcuts.set(acc, { want: parseAccel(acc), cb, plugin });
			hookShortcuts();
			takeGlobally(acc, plugin);
			return true;
		},
		registerAll: (list, cb) => {
			for (const a of list ?? []) globalShortcut.register(a, cb);
		},
		isRegistered: (acc) => shortcuts.has(acc),
		unregister: (acc) => {
			releaseGlobally(acc, shortcuts.get(acc));
			shortcuts.delete(acc);
		},
		unregisterAll: () => {
			for (const [acc, s] of shortcuts) releaseGlobally(acc, s);
			shortcuts.clear();
		},
	};
	class Menu {
		constructor() {
			this.items = [];
		}
		static buildFromTemplate(t) {
			const m = new Menu();
			m.items = (t ?? []).map((x) => new MenuItem(x));
			return m;
		}
		static setApplicationMenu() {}
		static getApplicationMenu() {
			return null;
		}
		append(i) {
			this.items.push(i);
		}
		insert(n, i) {
			this.items.splice(n, 0, i);
		}
		popup() {
			electronWarn("Menu.popup");
		}
		closePopup() {}
		getMenuItemById(id) {
			return this.items.find((i) => i.id === id) ?? null;
		}
	}
	class MenuItem {
		constructor(o = {}) {
			Object.assign(this, o);
		}
	}
	class Tray extends EventEmitter {
		constructor() {
			super();
			electronWarn("Tray");
		}
		setToolTip() {}
		setContextMenu() {}
		setImage() {}
		setTitle() {}
		destroy() {}
		isDestroyed() {
			return false;
		}
		displayBalloon() {}
	}
	// The web Notification API: shown as an in-app toast.
	if (g.Notification === undefined) {
		g.Notification = class Notification {
			static permission = "granted";
			static maxActions = 0;
			static requestPermission(cb) {
				cb?.("granted");
				return Promise.resolve("granted");
			}
			constructor(title, options = {}) {
				this.title = String(title ?? "");
				this.body = String(options.body ?? "");
				this.tag = options.tag ?? "";
				this.icon = options.icon ?? "";
				this.data = options.data;
				this.onshow = this.onclick = this.onclose = this.onerror = null;
				this._l = {};
				callOp("notify", { plugin: L.pluginFromStack(), level: "info", msg: [this.title, this.body].filter(Boolean).join(": ") });
				queueMicrotask(() => this._fire("show"));
			}
			addEventListener(t, f) {
				(this._l[t] ??= new Set()).add(f);
			}
			removeEventListener(t, f) {
				this._l[t]?.delete(f);
			}
			_fire(t) {
				const ev = { type: t, target: this };
				try {
					this[`on${t}`]?.(ev);
				} catch {}
				for (const f of [...(this._l[t] ?? [])]) {
					try {
						f(ev);
					} catch {}
				}
			}
			close() {
				this._fire("close");
			}
		};
	}
	const electron = {
		shell: { openExternal: async (u) => {
			if (!/^https?:\/\//i.test(String(u))) throw new Error(`Only http and https links can be opened (${u})`);
			callOp("open_url", { url: String(u) });
		}, openPath: async () => "Not available", showItemInFolder() {}, beep() {}, trashItem: async () => {}, writeShortcutLink: () => false },
		clipboard: { writeText: (t) => callOp("clipboard", { text: String(t) }), readText: () => callOp("clipboard.read", null) ?? "", writeHTML: (t) => callOp("clipboard", { text: String(t) }), readHTML: () => "", writeImage() {}, readImage: () => nativeImage.createEmpty(), clear() {}, availableFormats: () => [], has: () => false, write: (d) => d?.text !== undefined && callOp("clipboard", { text: String(d.text) }) },
		app,
		ipcMain,
		BrowserWindow,
		BrowserView: class BrowserView {
			constructor() {
				this.webContents = new WebContents(0);
			}
			setBounds() {}
			setAutoResize() {}
		},
		WebContentsView: class WebContentsView {
			constructor() {
				this.webContents = new WebContents(0);
			}
			setBounds() {}
		},
		webContents: { getAllWebContents: () => [...windows.values()].map((w) => w.webContents), getFocusedWebContents: () => null, fromId: (id) => windows.get(id)?.webContents ?? null },
		dialog: { showOpenDialog: (a, b) => L.fileDialog("open", a, b), showOpenDialogSync: () => (L.noFileDialog("dialog.showOpenDialogSync"), undefined), showSaveDialog: (a, b) => L.fileDialog("save", a, b), showSaveDialogSync: () => (L.noFileDialog("dialog.showSaveDialogSync"), undefined), showMessageBox: async (a, b) => {
				const o = b !== undefined ? b : a;
				const { response } = await L.askUser({ title: o?.title, message: o?.message, detail: o?.detail, buttons: o?.buttons, defaultId: o?.defaultId, cancelId: o?.cancelId });
				return { response, checkboxChecked: false };
			}, showMessageBoxSync: () => 0, showErrorBox: (title, content) => void L.askUser({ title: String(title ?? "Error"), message: String(content ?? ""), buttons: ["OK"] }) },
		nativeTheme: Object.assign(new EventEmitter(), { shouldUseDarkColors: true, themeSource: "dark", shouldUseHighContrastColors: false, shouldUseInvertedColorScheme: false }),
		Notification: class Notification extends EventEmitter {
			constructor(o) {
				super();
				this.o = o;
			}
			static isSupported() {
				return true;
			}
			show() {
				callOp("notify", { plugin: L.pluginFromStack(), level: "info", msg: [this.o?.title, this.o?.body].filter(Boolean).join(": ") });
				queueMicrotask(() => this.emit("show"));
			}
			close() {}
		},
		session: { defaultSession: electronSession, fromPartition: () => electronSession, fromPath: () => electronSession },
		globalShortcut,
		Menu,
		MenuItem,
		Tray,
		nativeImage,
		screen: Object.assign(new EventEmitter(), { getPrimaryDisplay: () => display, getAllDisplays: () => [display], getDisplayNearestPoint: () => display, getDisplayMatching: () => display, getCursorScreenPoint: () => ({ x: 0, y: 0 }), screenToDipPoint: (p) => p, dipToScreenPoint: (p) => p }),
		powerMonitor: Object.assign(new EventEmitter(), { getSystemIdleState: () => "active", getSystemIdleTime: () => 0, isOnBatteryPower: () => false, onBatteryPower: false }),
		powerSaveBlocker: { start: () => 0, stop() {}, isStarted: () => false },
		systemPreferences: Object.assign(new EventEmitter(), { getAccentColor: () => "000000ff", getColor: () => "#000000", isDarkMode: () => true, getMediaAccessStatus: () => "granted", askForMediaAccess: async () => true, getAnimationSettings: () => ({ shouldRenderRichAnimation: true }) }),
		protocol: { handle() {}, unhandle() {}, isProtocolHandled: () => false, registerSchemesAsPrivileged() {}, registerFileProtocol: () => true, registerBufferProtocol: () => true, registerStringProtocol: () => true, registerHttpProtocol: () => true, interceptFileProtocol: () => true, interceptBufferProtocol: () => true, interceptStringProtocol: () => true, interceptHttpProtocol: () => true, uninterceptProtocol: () => true, unregisterProtocol: () => true },
		net: { fetch: (...a) => g.fetch(...a), isOnline: () => true, online: true, request: notAvailable("electron.net.request (use fetch)") },
		safeStorage: { isEncryptionAvailable: () => false, encryptString: notAvailable("safeStorage"), decryptString: notAvailable("safeStorage") },
		crashReporter: { start() {}, addExtraParameter() {}, removeExtraParameter() {}, getParameters: () => ({}) },
		autoUpdater: Object.assign(new EventEmitter(), { setFeedURL() {}, checkForUpdates() {}, quitAndInstall() {} }),
		contentTracing: { startRecording: async () => {}, stopRecording: async () => "" },
		desktopCapturer: { getSources: async () => [] },
		inAppPurchase: {},
		pushNotifications: {},
		utilityProcess: { fork: notAvailable("utilityProcess") },
		MessageChannelMain: class MessageChannelMain {
			constructor() {
				this.port1 = new EventEmitter();
				this.port2 = new EventEmitter();
			}
		},
		ShareMenu: class ShareMenu {
			popup() {}
		},
		TouchBar: class TouchBar {},
		contextBridge: { exposeInMainWorld() {}, exposeInIsolatedWorld() {} },
		ipcRenderer: { invoke: async () => undefined, send() {}, on() {}, once() {}, removeListener() {}, removeAllListeners() {} },
		webFrame: { setZoomFactor() {}, getZoomFactor: () => 1, insertCSS: () => "", executeJavaScript: async () => undefined },
	};
	// A permissive stand-in for core modules we do not emulate, so code that
	// only touches them while loading (undici & co.) still loads.
	const stubCache = new Map();
	const stub = (name) => {
		if (stubCache.has(name)) return stubCache.get(name);
		const fn = function () {};
		const p = new Proxy(fn, {
			get: (t, k) => {
				if (k === "__esModule") return false;
				if (k === "default") return p;
				if (k === Symbol.toPrimitive) return () => `[stub ${name}]`;
				if (typeof k === "symbol") return undefined;
				if (k === "then") return undefined;
				if (k === "prototype") return fn.prototype;
				return stub(`${name}.${k}`);
			},
			apply: () => undefined,
			construct: () => ({}),
		});
		stubCache.set(name, p);
		return p;
	};
	const workerThreads = { isMainThread: true, parentPort: null, workerData: null, threadId: 0, resourceLimits: {}, markAsUntransferable() {}, markAsUncloneable() {}, isMarkedAsUntransferable: () => false, MessageChannel: class MessageChannel {
			constructor() {
				this.port1 = new EventEmitter();
				this.port2 = new EventEmitter();
			}
		}, MessagePort: class MessagePort extends EventEmitter {}, BroadcastChannel: class BroadcastChannel extends EventEmitter {}, Worker: class Worker {
			constructor() {
				throw new Error("Worker threads are not available to plugins");
			}
		}, receiveMessageOnPort: () => undefined, getEnvironmentData: () => undefined, setEnvironmentData() {} };
	// stream/consumers: a stream's (or async iterable's) chunks, read whole.
	const streamConsumers = (() => {
		const chunks = async (source) => {
			const out = [];
			if (typeof source?.getReader === "function") {
				const reader = source.getReader();
				for (;;) {
					const { done, value } = await reader.read();
					if (done) break;
					out.push(value);
				}
			} else for await (const chunk of source) out.push(chunk);
			return out;
		};
		const toBuffer = async (source) => Buffer.concat((await chunks(source)).map((c) => (typeof c === "string" ? Buffer.from(c) : Buffer.from(c.buffer ? new Uint8Array(c.buffer, c.byteOffset, c.byteLength) : c))));
		return {
			buffer: toBuffer,
			text: async (source) => (await toBuffer(source)).toString(),
			json: async (source) => JSON.parse((await toBuffer(source)).toString()),
			arrayBuffer: async (source) => {
				const b = await toBuffer(source);
				return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
			},
			blob: async (source) => new g.Blob([await toBuffer(source)]),
		};
	})();
	// ---------------------------------------------------------------- dns
	// `lookup` is answered by the host's system resolver, after the same
	// question a connection to that name asks. The `resolve*` family and
	// `reverse` ask a DNS server through the host (plugins have no UDP of
	// their own): the servers a plugin gave `setServers` (each asked as a
	// connection to it), else the system's.
	const dnsMod = (() => {
		const fail = (code, syscall, hostname) => {
			const err = new Error(`${syscall} ${code}${hostname !== undefined ? ` ${hostname}` : ""}`);
			err.code = code;
			err.syscall = syscall;
			if (hostname !== undefined) err.hostname = hostname;
			if (code === "ENOTFOUND") err.errno = -3008;
			return err;
		};
		const badArg = (name, expected, v) => Object.assign(new TypeError(`The "${name}" argument must be ${expected}. Received ${v === null ? "null" : typeof v}`), { code: "ERR_INVALID_ARG_TYPE" });
		let order = "verbatim";
		const lookupOptions = (o) => {
			if (o === undefined || o === null) o = {};
			else if (typeof o === "number") o = { family: o };
			const family = o.family === "IPv4" ? 4 : o.family === "IPv6" ? 6 : (o.family ?? 0);
			if (family !== 0 && family !== 4 && family !== 6) throw Object.assign(new TypeError(`The property 'options.family' must be one of: 0, 4, 6. Received ${family}`), { code: "ERR_INVALID_ARG_VALUE" });
			return { family, all: o.all === true, order: o.order ?? (o.verbatim === false ? "ipv4first" : order) };
		};
		const resolveName = async (hostname, family, syscall = "getaddrinfo") => {
			if (hostname !== undefined && hostname !== null && hostname !== "" && typeof hostname !== "string") throw badArg("hostname", "of type string", hostname);
			if (!hostname) return [{ address: null, family: family === 6 ? 6 : 4 }];
			const literal = net.isIP(hostname);
			let list;
			if (literal !== 0) list = [{ address: hostname, family: literal }];
			else {
				try {
					list = await L.callOpAsync("dns", { plugin: L.pluginFromStack(), host: hostname });
				} catch (e) {
					const m = /^([A-Z]+): ([\s\S]*)$/.exec(String(e?.message ?? e));
					if (m && m[1] !== "ENOTFOUND") throw Object.assign(new Error(`${syscall} ${m[1]} ${hostname}: ${m[2]}`), { code: m[1], syscall, hostname });
					throw fail("ENOTFOUND", syscall, hostname);
				}
			}
			if (family !== 0) list = list.filter((a) => a.family === family);
			if (list.length === 0) throw fail("ENOTFOUND", syscall, hostname);
			return list;
		};
		const sortByOrder = (list, ord) => (ord === "ipv4first" ? [...list.filter((a) => a.family === 4), ...list.filter((a) => a.family === 6)] : ord === "ipv6first" ? [...list.filter((a) => a.family === 6), ...list.filter((a) => a.family === 4)] : list);
		const lookupP = async (hostname, options) => {
			const o = lookupOptions(options);
			const list = sortByOrder(await resolveName(hostname, o.family), o.order);
			return o.all ? list : list[0];
		};
		const lookup = (hostname, options, callback) => {
			if (typeof options === "function") [callback, options] = [options, undefined];
			if (typeof callback !== "function") throw Object.assign(new TypeError(`The "callback" argument must be of type function. Received ${typeof callback}`), { code: "ERR_INVALID_ARG_TYPE" });
			const all = lookupOptions(options).all;
			lookupP(hostname, options).then(
				(r) => (all ? callback(null, r) : callback(null, r.address, r.family)),
				(e) => callback(e),
			);
			return {};
		};
		// Each resolver (the module's own, `new Resolver()`) has its own servers.
		const makeMethods = (st) => {
			// One DNS question; the answer is the records as the host read them.
			const ask = async (hostname, type, syscall, about) => {
				if (typeof hostname !== "string") throw badArg("name", "of type string", hostname);
				try {
					return await L.callOpAsync("dns.query", { plugin: L.pluginFromStack(), name: hostname, host: about, type, servers: st.servers });
				} catch (e) {
					const m = /^([A-Z]+): ([\s\S]*)$/.exec(String(e?.message ?? e));
					if (m && m[1] === "EACCES") throw Object.assign(new Error(`${syscall} EACCES ${hostname}: ${m[2]}`), { code: "EACCES", syscall, hostname });
					throw fail(m ? m[1] : "ESERVFAIL", syscall, hostname);
				}
			};
			const ofType = (list, t) => list.filter((r) => r.type === t);
			const addresses = async (hostname, type, options, syscall) => {
				if (net.isIP(hostname) !== 0) return aRecords(hostname, type === "A" ? 4 : 6, options, syscall);
				return ofType(await ask(hostname, type, syscall), type).map((r) => (options?.ttl ? { address: r.data.address, ttl: r.ttl } : r.data.address));
			};
			// A literal answers as itself; a name is asked of the DNS servers.
			const aRecords = async (hostname, family, options, syscall) => {
				const list = await resolveName(hostname, family, syscall);
				return list.map((a) => (options?.ttl ? { address: a.address, ttl: 0 } : a.address));
			};
			const values = (type, syscall) => async (hostname) => ofType(await ask(hostname, type, syscall), type).map((r) => r.data.value);
			const plain = (type, syscall) => async (hostname) => ofType(await ask(hostname, type, syscall), type).map((r) => r.data);
			// `resolveAny` keeps each record's type, as Node's does.
			const anyRecord = (r) => {
				const d = r.data;
				switch (r.type) {
					case "A":
					case "AAAA":
						return { type: r.type, address: d.address, ttl: r.ttl };
					case "NS":
					case "CNAME":
					case "PTR":
						return { type: r.type, value: d.value };
					case "TXT":
						return { type: "TXT", entries: d.entries };
					default:
						return { type: r.type, ...d };
				}
			};
			const byType = {
				A: (h, o) => addresses(h, "A", o, "queryA"),
				AAAA: (h, o) => addresses(h, "AAAA", o, "queryAaaa"),
				MX: plain("MX", "queryMx"),
				TXT: async (h) => ofType(await ask(h, "TXT", "queryTxt"), "TXT").map((r) => r.data.entries),
				SRV: plain("SRV", "querySrv"),
				NS: values("NS", "queryNs"),
				CNAME: values("CNAME", "queryCname"),
				PTR: values("PTR", "queryPtr"),
				SOA: async (h) => {
					const soa = ofType(await ask(h, "SOA", "querySoa"), "SOA")[0];
					if (!soa) throw fail("ENODATA", "querySoa", h);
					return soa.data;
				},
				NAPTR: plain("NAPTR", "queryNaptr"),
				CAA: plain("CAA", "queryCaa"),
				TLSA: async (h) => ofType(await ask(h, "TLSA", "queryTlsa"), "TLSA").map((r) => ({ ...r.data, data: Uint8Array.from(r.data.data.match(/../g) ?? [], (x) => parseInt(x, 16)).buffer })),
				ANY: async (h) => (await ask(h, "ANY", "queryAny")).map(anyRecord),
			};
			const methods = {
				lookup: lookupP,
				resolve4: byType.A,
				resolve6: byType.AAAA,
				resolve: (h, type = "A", o) => (byType[type] ? byType[type](h, o) : Promise.reject(Object.assign(new TypeError(`The argument 'rrtype' is invalid. Received '${type}'`), { code: "ERR_INVALID_ARG_VALUE" }))),
				resolveAny: byType.ANY,
				resolveCname: byType.CNAME,
				resolveCaa: byType.CAA,
				resolveMx: byType.MX,
				resolveNaptr: byType.NAPTR,
				resolveNs: byType.NS,
				resolvePtr: byType.PTR,
				resolveSoa: byType.SOA,
				resolveSrv: byType.SRV,
				resolveTxt: byType.TXT,
				resolveTlsa: byType.TLSA,
				reverse: async (ip) => {
					if (net.isIP(ip) === 0) throw Object.assign(new TypeError(`The argument 'ip' is invalid. Received '${ip}'`), { code: "ERR_INVALID_ARG_VALUE" });
					try {
						return ofType(await L.callOpAsync("dns.query", { plugin: L.pluginFromStack(), name: ip, host: ip, reverse: true, type: "PTR", servers: st.servers }), "PTR").map((r) => r.data.value);
					} catch (e) {
						const m = /^([A-Z]+): /.exec(String(e?.message ?? e));
						throw fail(m ? m[1] : "ESERVFAIL", "getHostByAddr", ip);
					}
				},
				lookupService: async (address, port) => {
					if (net.isIP(address) === 0) throw Object.assign(new TypeError(`The argument 'address' is invalid. Received '${address}'`), { code: "ERR_INVALID_ARG_VALUE" });
					throw fail("ENOTIMP", "getnameinfo", address);
				},
			};
			return methods;
		};
		let systemServers;
		const checkServers = (list) => {
			if (!Array.isArray(list)) throw badArg("servers", "an instance of Array", list);
			return list.map((x) => {
				const text = String(x);
				const bare = text.replace(/^\[(.*)\](?::\d+)?$/, "$1").replace(/^(\d+\.\d+\.\d+\.\d+):\d+$/, "$1");
				if (net.isIP(bare) === 0) throw Object.assign(new TypeError(`Invalid IP address: ${text}`), { code: "ERR_INVALID_IP_ADDRESS" });
				return text;
			});
		};
		const makeCommon = (st) => ({
			getServers: () => (st.servers.length > 0 ? [...st.servers] : [...(systemServers ??= callOp("dns.servers", null) ?? [])]),
			setServers(list) {
				st.servers = checkServers(list);
			},
			setLocalAddress() {},
			cancel() {},
		});
		const state = { servers: [] };
		const methods = makeMethods(state);
		const common = makeCommon(state);
		const callbackStyle = (fn) => (...args) => {
			const cb = args.pop();
			if (typeof cb !== "function") throw Object.assign(new TypeError(`The "callback" argument must be of type function. Received ${typeof cb}`), { code: "ERR_INVALID_ARG_TYPE" });
			fn(...args).then((r) => cb(null, r), (e) => cb(e));
			return {};
		};
		const promises = { ...methods, ...common };
		delete promises.cancel;
		class Resolver {
			constructor() {
				const st = { servers: [] };
				const own = makeMethods(st);
				for (const k of Object.keys(own)) if (k !== "lookup" && k !== "lookupService") this[k] = callbackStyle(own[k]);
				Object.assign(this, makeCommon(st));
			}
		}
		class PromisesResolver {
			constructor() {
				const st = { servers: [] };
				const own = makeMethods(st);
				for (const k of Object.keys(own)) if (k !== "lookup" && k !== "lookupService") this[k] = own[k];
				Object.assign(this, makeCommon(st));
			}
		}
		promises.Resolver = PromisesResolver;
		const mod = {
			lookup,
			lookupService: callbackStyle(methods.lookupService),
			Resolver,
			getServers: common.getServers,
			setServers: common.setServers,
			setDefaultResultOrder(o) {
				if (o !== "verbatim" && o !== "ipv4first" && o !== "ipv6first") throw Object.assign(new TypeError(`The argument 'order' must be one of: 'verbatim', 'ipv4first', 'ipv6first'. Received '${o}'`), { code: "ERR_INVALID_ARG_VALUE" });
				order = o;
			},
			getDefaultResultOrder: () => order,
			promises,
			ADDRCONFIG: 1024,
			V4MAPPED: 2048,
			ALL: 256,
			NODATA: "ENODATA", FORMERR: "EFORMERR", SERVFAIL: "ESERVFAIL", NOTFOUND: "ENOTFOUND", NOTIMP: "ENOTIMP", REFUSED: "EREFUSED", BADQUERY: "EBADQUERY", BADNAME: "EBADNAME", BADFAMILY: "EBADFAMILY", BADRESP: "EBADRESP", CONNREFUSED: "ECONNREFUSED", TIMEOUT: "ETIMEOUT", EOF: "EOF", FILE: "EFILE", NOMEM: "ENOMEM", DESTRUCTION: "EDESTRUCTION", BADSTR: "EBADSTR", BADFLAGS: "EBADFLAGS", NONAME: "ENONAME", BADHINTS: "EBADHINTS", NOTINITIALIZED: "ENOTINITIALIZED", LOADIPHLPAPI: "ELOADIPHLPAPI", ADDRGETNETWORKPARAMS: "EADDRGETNETWORKPARAMS", CANCELLED: "ECANCELLED",
		};
		for (const k of Object.keys(methods)) if (k !== "lookup" && k !== "lookupService") mod[k] = callbackStyle(methods[k]);
		return mod;
	})();

	// ---------------------------------------------------------------- v8
	// serialize/deserialize keep what structured clone keeps (undefined, NaN,
	// BigInt, Date, RegExp, Map, Set, typed arrays, Error, shared and circular
	// references). The bytes are Tidalfast's own, not V8's wire format, so
	// they read back here and nowhere else.
	const v8Mod = (() => {
		const MAGIC = "TFv8\u0001";
		const TYPED = { Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array };
		const toTree = (value) => {
			const seen = new Map();
			const walk = (v) => {
				switch (typeof v) {
					case "undefined":
						return { t: "u" };
					case "bigint":
						return { t: "b", v: v.toString() };
					case "number":
						return Number.isFinite(v) && !Object.is(v, -0) ? v : { t: "n", v: Object.is(v, -0) ? "-0" : String(v) };
					case "string":
					case "boolean":
						return v;
					case "symbol":
					case "function":
						throw Object.assign(new Error(`${typeof v === "symbol" ? "Symbol()" : String(v)} could not be cloned.`), { name: "DataCloneError", code: 25 });
				}
				if (v === null) return null;
				if (seen.has(v)) return { t: "r", i: seen.get(v) };
				const id = seen.size;
				seen.set(v, id);
				if (v instanceof Date) return { t: "d", i: id, v: v.getTime() };
				if (v instanceof RegExp) return { t: "x", i: id, s: v.source, f: v.flags };
				if (v instanceof Map) return { t: "m", i: id, e: [...v].map(([k, x]) => [walk(k), walk(x)]) };
				if (v instanceof Set) return { t: "s", i: id, e: [...v].map(walk) };
				if (v instanceof Error) return { t: "e", i: id, n: v.name, m: v.message, st: v.stack, c: v.cause === undefined ? undefined : walk(v.cause) };
				if (v instanceof ArrayBuffer) return { t: "ab", i: id, v: Buffer.from(v).toString("base64") };
				if (ArrayBuffer.isView(v)) {
					const bytes = Buffer.from(v.buffer, v.byteOffset, v.byteLength);
					return { t: "tv", i: id, k: v instanceof DataView ? "DataView" : Buffer.isBuffer(v) ? "Buffer" : v.constructor.name, v: bytes.toString("base64") };
				}
				if (Array.isArray(v)) {
					const items = [];
					for (let k = 0; k < v.length; k++) items.push(k in v ? walk(v[k]) : { t: "h" });
					return { t: "a", i: id, e: items, x: Object.keys(v).filter((k) => !/^\d+$/.test(k)).map((k) => [k, walk(v[k])]) };
				}
				if (v instanceof Boolean || v instanceof Number || v instanceof String) return { t: "w", i: id, k: v.constructor.name, v: v.valueOf() };
				return { t: "o", i: id, e: Object.keys(v).map((k) => [k, walk(v[k])]) };
			};
			return walk(value);
		};
		const fromTree = (tree) => {
			const table = [];
			const walk = (n) => {
				if (n === null || typeof n !== "object") return n;
				const reg = (o) => ((table[n.i] = o), o);
				switch (n.t) {
					case "u":
						return undefined;
					case "b":
						return BigInt(n.v);
					case "n":
						return n.v === "-0" ? -0 : Number(n.v);
					case "r":
						return table[n.i];
					case "d":
						return reg(new Date(n.v));
					case "x":
						return reg(new RegExp(n.s, n.f));
					case "m": {
						const m = reg(new Map());
						for (const [k, x] of n.e) m.set(walk(k), walk(x));
						return m;
					}
					case "s": {
						const s = reg(new Set());
						for (const x of n.e) s.add(walk(x));
						return s;
					}
					case "e": {
						const E = { EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError }[n.n] ?? Error;
						const e = reg(new E(n.m));
						if (n.n !== e.name) e.name = n.n;
						if (n.st !== undefined) e.stack = n.st;
						if (n.c !== undefined) e.cause = walk(n.c);
						return e;
					}
					case "ab": {
						const b = Buffer.from(n.v, "base64");
						return reg(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
					}
					case "tv": {
						const b = Buffer.from(n.v, "base64");
						const ab = b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
						if (n.k === "Buffer") return reg(Buffer.from(ab));
						if (n.k === "DataView") return reg(new DataView(ab));
						return reg(new TYPED[n.k](ab));
					}
					case "a": {
						const a = reg(new Array(n.e.length));
						n.e.forEach((x, k) => {
							if (!(x && x.t === "h")) a[k] = walk(x);
						});
						for (const [k, x] of n.x) a[k] = walk(x);
						return a;
					}
					case "w":
						return reg(Object(n.v));
					case "o": {
						const o = reg({});
						for (const [k, x] of n.e) o[k] = walk(x);
						return o;
					}
				}
				return undefined;
			};
			return walk(tree);
		};
		const encode = (value) => Buffer.from(MAGIC + JSON.stringify(toTree(value)));
		const decode = (buf) => {
			const text = Buffer.from(buf.buffer ? new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength) : buf).toString();
			if (!text.startsWith(MAGIC)) throw new Error("Unable to deserialize cloned data due to invalid or unsupported version.");
			return fromTree(JSON.parse(text.slice(MAGIC.length)));
		};
		// The same walk keeps values in plugin stores (the IndexedDB facade).
		L.clone = { toTree, fromTree };
		const limit = Number(callOp("heap.limit", null)) || 0;
		// QuickJS's own accounting of the plugins' heap, read when asked.
		const stats = () => {
			const m = callOp("heap.stats", null) || {};
			const used = Number(m.usedSize) || 0;
			const total = Number(m.mallocSize) || used;
			return { total_heap_size: total, total_heap_size_executable: 0, total_physical_size: total, total_available_size: Math.max(0, limit - total), used_heap_size: used, heap_size_limit: limit, malloced_memory: total, peak_malloced_memory: total, does_zap_garbage: 0, number_of_native_contexts: 1, number_of_detached_contexts: 0, total_global_handles_size: 0, used_global_handles_size: 0, external_memory: 0 };
		};
		const refused = (what) => () => {
			throw new Error(`v8.${what} is not available to plugins in Tidalfast (it would hand over the whole process)`);
		};
		class Serializer {
			constructor() {
				this._v = undefined;
			}
			writeHeader() {}
			writeValue(v) {
				this._v = v;
				return true;
			}
			releaseBuffer() {
				return encode(this._v);
			}
		}
		class Deserializer {
			constructor(buffer) {
				this._b = buffer;
			}
			readHeader() {
				return true;
			}
			readValue() {
				return decode(this._b);
			}
			getWireFormatVersion() {
				return 15;
			}
		}
		return {
			serialize: encode,
			deserialize: decode,
			Serializer,
			Deserializer,
			DefaultSerializer: Serializer,
			DefaultDeserializer: Deserializer,
			getHeapStatistics: stats,
			getHeapSpaceStatistics: () => {
				const s = stats();
				return [{ space_name: "quickjs_heap", space_size: s.total_heap_size, space_used_size: s.used_heap_size, space_available_size: s.total_available_size, physical_space_size: s.total_physical_size }];
			},
			getHeapCodeStatistics: () => ({ code_and_metadata_size: 0, bytecode_and_metadata_size: Number((callOp("heap.stats", null) || {}).codeSize) || 0, external_script_source_size: 0, cpu_profiler_metadata_size: 0 }),
			cachedDataVersionTag: () => 0,
			setFlagsFromString() {},
			setHeapSnapshotNearHeapLimit() {},
			isStringOneByteRepresentation: (s) => !/[^\u0000-ÿ]/.test(String(s)),
			getHeapSnapshot: refused("getHeapSnapshot"),
			writeHeapSnapshot: refused("writeHeapSnapshot"),
			takeCoverage() {},
			stopCoverage() {},
			startupSnapshot: { isBuildingSnapshot: () => false },
			promiseHooks: { onInit: () => () => {}, onSettled: () => () => {}, onBefore: () => () => {}, onAfter: () => () => {}, createHook: () => () => {} },
		};
	})();

	const core = {
		events: (EventEmitter.EventEmitter = callable(EventEmitter)),
		buffer: bufferModule,
		util,
		"util/types": util.types,
		path,
		"path/posix": posix,
		"path/win32": win32,
		process,
		timers,
		"timers/promises": timersPromises,
		stream,
		"stream/promises": streamPromises,
		"stream/web": streamWeb,
		"stream/consumers": streamConsumers,
		// Debugging the process: not offered (Luna's DevTools' native side
		// reaches it; a plugin has no process to debug here).
		inspector: { Session: class Session { constructor() { throw new Error("inspector is not available to plugins in Tidalfast"); } }, open: notAvailable("inspector.open"), close() {}, url: () => undefined, waitForDebugger: notAvailable("inspector.waitForDebugger"), console: g.console },
		net,
		os,
		crypto: cryptoMod,
		url,
		string_decoder: { StringDecoder },
		assert,
		"assert/strict": assert,
		querystring,
		diagnostics_channel: diagnosticsChannel,
		async_hooks: asyncHooks,
		perf_hooks: { performance: g.performance, PerformanceObserver: class PerformanceObserver {
				observe() {}
				disconnect() {}
			}, monitorEventLoopDelay: () => ({ enable() {}, disable() {}, percentile: () => 0 }), constants: {} },
		worker_threads: workerThreads,
		console: g.console,
		http,
		https: { ...http, request: httpRequest("https"), get: httpGet("https"), globalAgent: {}, createServer: notAvailable("https.createServer (TLS servers)"), Server: undefined },
		http2: { connect: notAvailable("http2.connect"), constants: {} },
		tls: { connect: notAvailable("tls.connect"), createSecureContext: () => ({}), DEFAULT_MIN_VERSION: "TLSv1.2", DEFAULT_MAX_VERSION: "TLSv1.3", rootCertificates: [], TLSSocket: class TLSSocket extends Socket {}, checkServerIdentity: () => undefined, getCiphers: () => [] },
		dns: dnsMod,
		"dns/promises": dnsMod.promises,
		zlib: zlibMod,
		dgram,
		child_process: childProcess,
		cluster: { isPrimary: true, isMaster: true },
		tty: { isatty: () => false, ReadStream: class ReadStream {}, WriteStream: class WriteStream {} },
		punycode: { toASCII: (s) => s, toUnicode: (s) => s, encode: (s) => s, decode: (s) => s },
		constants: {},
		domain: { create: () => ({ run: (fn) => fn(), on() {} }) },
		vm: { runInNewContext: notAvailable("vm"), Script: class Script {
				constructor() {
					throw new Error("vm is not available to plugins");
				}
			}, isContext: () => false },
		v8: v8Mod,
		readline: { createInterface: notAvailable("readline") },
		electron,
	};
	for (const k of Object.keys(core)) if (core[k] && typeof core[k] === "object" && !("default" in core[k])) Object.defineProperty(core[k], "default", { value: core[k], enumerable: false, configurable: true });
	const warned = new Set();
	const makeRequire = (plugin, nativeExports) => {
		const fsInst = makeFs(plugin);
		const mods = { ...core, fs: fsInst, "fs/promises": fsInst.promises };
		const req = (name) => {
			name = String(name);
			const bare = name.startsWith("node:") ? name.slice(5) : name;
			if (Object.prototype.hasOwnProperty.call(mods, bare)) return mods[bare];
			if (nativeExports && Object.prototype.hasOwnProperty.call(nativeExports, name)) return nativeExports[name];
			if (bare === "module") return { createRequire: () => req, builtinModules: Object.keys(core), isBuiltin: (n) => Object.prototype.hasOwnProperty.call(core, String(n).replace(/^node:/, "")) };
			if (/(^|[\\/])original\.asar([\\/]|$)/.test(name)) {
				// TIDAL's own desktop code (its Electron main process): not
				// part of Tidalfast, and never read from a TIDAL install.
				const what = name.replace(/^.*original\.asar[\\/]?/, "") || "original.asar";
				throw Object.assign(new Error(`${what} is part of TIDAL's desktop app (original.asar), which Tidalfast does not include; features built on it (TIDAL Connect receiving) are not available`), { code: "MODULE_NOT_FOUND" });
			}
			if (/\.node$/i.test(name)) throw L.node.nativeAddonError(plugin, name);
			if (name.startsWith("node:")) {
				if (!warned.has(name)) {
					warned.add(name);
					console.warn(`[Luna] ${plugin}: Node module ${name} is not emulated; using a stand-in`);
				}
				return stub(name);
			}
			throw Object.assign(new Error(`Cannot find module '${name}'`), { code: "MODULE_NOT_FOUND" });
		};
		req.resolve = (n) => n;
		req.cache = {};
		req.main = undefined;
		return req;
	};
	// Some plugins ship their `.native` code as an ES module (esbuild
	// `format: "esm"`), which Luna imports in Node. The top-level
	// import/export statements esbuild emits are rewritten into CommonJS so
	// the code runs in the same wrapper; exports become getters, so bindings
	// stay live.
	const STMT = "(?<=^|[;\\n}])()(\\s*)";
	const SPEC = "(\"(?:[^\"\\\\\\n]|\\\\.)*\"|'(?:[^'\\\\\\n]|\\\\.)*')";
	const ESM_RE = new RegExp(STMT + "(import\\s*[\\w$*{\"']|export\\s*(\\{|\\*|default\\b|(async\\s+)?function\\b|class\\b|const\\b|let\\b|var\\b))");
	const unq = (s) => s.trim().replace(/^["']|["']$/g, "");
	const splitList = (list) =>
		list
			.split(",")
			.map((x) => x.trim())
			.filter(Boolean)
			.map((x) => {
				const [a, b] = x.split(/\s+as\s+/);
				return [unq(a), (b ?? a).trim()];
			});
	const esmToCjs = (code) => {
		let n = 0;
		const out = [];
		code = code.replace(new RegExp(STMT + "import\\s*([^;\"'()]*?)\\s*from\\s*" + SPEC + "\\s*;?", "g"), (_, pre, ws, clause, spec) => {
			const m = `__esm${n++}`;
			let s = `${pre}${ws}var ${m}=require(${spec});`;
			clause = clause.trim();
			const named = clause.match(/\{([^}]*)\}/);
			const rest = clause.replace(/\{[^}]*\}/, "").replace(/,/g, " ").trim();
			const ns = rest.match(/\*\s*as\s+([\w$]+)/);
			const def = rest.replace(/\*\s*as\s+[\w$]+/, "").trim();
			if (def) s += `var ${def}=__esmDefault(${m});`;
			if (ns) s += `var ${ns[1]}=${m};`;
			if (named) {
				const binds = splitList(named[1]).map(([k, v]) => `${JSON.stringify(k)}:${v}`);
				if (binds.length) s += `var {${binds.join(",")}}=__esmNamed(${m});`;
			}
			return s;
		});
		code = code.replace(new RegExp(STMT + "import\\s*" + SPEC + "\\s*;?", "g"), (_, pre, ws, spec) => `${pre}${ws}require(${spec});`);
		code = code.replace(new RegExp(STMT + "export\\s*\\{([^}]*)\\}\\s*from\\s*" + SPEC + "\\s*;?", "g"), (_, pre, ws, list, spec) => {
			const m = `__esm${n++}`;
			for (const [k, v] of splitList(list)) out.push([unq(v), `__esmNamed(${m})[${JSON.stringify(k)}]`]);
			return `${pre}${ws}var ${m}=require(${spec});`;
		});
		code = code.replace(new RegExp(STMT + "export\\s*\\*\\s*from\\s*" + SPEC + "\\s*;?", "g"), (_, pre, ws, spec) => `${pre}${ws}Object.assign(module.exports,require(${spec}));`);
		code = code.replace(new RegExp(STMT + "export\\s*\\{([^}]*)\\}\\s*;?", "g"), (_, pre, ws, list) => {
			for (const [k, v] of splitList(list)) out.push([unq(v), k]);
			return `${pre}${ws}`;
		});
		code = code.replace(new RegExp(STMT + "export\\s+default\\s+", "g"), (_, pre, ws) => `${pre}${ws}module.exports.default=`);
		code = code.replace(new RegExp(STMT + "export\\s+((?:async\\s+)?function\\*?\\s*([\\w$]+)|class\\s+([\\w$]+)|(?:const|let|var)\\s+([\\w$]+))", "g"), (_, pre, ws, decl, f, c, v) => {
			const name = f ?? c ?? v;
			out.push([name, name]);
			return `${pre}${ws}${decl}`;
		});
		code = code.replace(/\bimport\.meta\.url\b/g, "__importMetaUrl").replace(/\bimport\.meta\b/g, "({url:__importMetaUrl})");
		const getters = out.map(([k, v]) => `${JSON.stringify(k)}:{get:()=>${v},enumerable:true,configurable:true}`).join(",");
		return `Object.defineProperty(module.exports,"__esModule",{value:true});Object.defineProperties(module.exports,{${getters}});\n${code}`;
	};
	const esmDefault = (m) => (m && m.__esModule ? m.default : m && m.default !== undefined ? m.default : m);
	const esmNamed = (m) => ((m && typeof m === "object") || typeof m === "function" ? m : {});
	L.node.isEsm = (code) => ESM_RE.test(code);
	L.node.esmToCjs = esmToCjs;
	L.node.loadNative = (plugin, fileName, code, nativeExports) => {
		callOp("plugin.native", { plugin });
		const esm = L.node.isEsm(code);
		if (esm) code = esmToCjs(code);
		const wrapper = L.evalAs(
			`(${esm ? "async " : ""}function (exports, require, module, __filename, __dirname, luna, process, Buffer, global, __esmDefault, __esmNamed, __importMetaUrl) {${code}\n})`,
			`native:${encodeURIComponent(plugin)}@${fileName}`,
		);
		const module = { exports: {}, id: fileName, filename: fileName, loaded: false, children: [], paths: [] };
		const lunaNative = { modules: nativeExports, tidalWindow: L.node.tidalWindow, sendToRender: (ch, ...a) => L.lunaIpc(ch, JSON.stringify(a)), ...nativeExports["@luna/lib.native"] };
		const ret = wrapper.call(module.exports, module.exports, makeRequire(plugin, nativeExports), module, fileName, plugin, lunaNative, process, Buffer, g, esmDefault, esmNamed, `file:///luna/${fileName}`);
		const done = () => {
			module.loaded = true;
			return module.exports;
		};
		return esm ? Promise.resolve(ret).then(done) : done();
	};
	L.node.core = core;
	L.node.makeRequire = makeRequire;
})();
