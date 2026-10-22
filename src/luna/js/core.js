// @luna/core: module registry, plugin loading, storage, tracing, the
// emulated redux store, and the IPC bridge to `.native` modules.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const { callOp, callOpAsync } = L;
	const PLATFORM = callOp("platform", null);
	g.__platform = PLATFORM;

	// ---------------------------------------------------------------- unloads
	const unloadSet = async (unloads) => {
		if (unloads === undefined || unloads === null || unloads.size === 0) return;
		const list = [...unloads];
		await Promise.all(
			list.map(async (unload) => {
				try {
					let timer;
					await Promise.race([
						Promise.resolve().then(() => unload()),
						new Promise((_, rej) => (timer = L.internalTimeout(() => rej(new Error("Unload took longer than 5s to run...")), 5000))),
					]).finally(() => clearTimeout(timer));
				} catch (err) {
					L.reportError(err);
				}
			}),
		);
		unloads.clear();
	};

	// ---------------------------------------------------------------- tracing
	// Luna's sanitizeData: objects as JSON, errors as their message.
	const sanitizeOne = (arg) => {
		if (arg === undefined) return;
		if (arg instanceof Error) return arg.message;
		if (typeof arg === "object") {
			try {
				return JSON.stringify(arg);
			} catch {}
		}
		return String(arg);
	};
	const sanitize = (...data) => (data.length === 1 ? sanitizeOne(data[0]) : data.map(sanitizeOne).join(" ")) ?? "";
	// As Luna: Messager dispatches `message/MESSAGE_*` through the store, so
	// other plugins' interceptors see (and can cancel) it; the host's reducer
	// turns what is left into the toast.
	let messageDepth = 0;
	const sendMessage = (type, severity, data) => {
		const action = buildActions[type]({ message: sanitize(...data), category: "OTHER", severity });
		// Every interceptor is offered the message, the sender's own too. One
		// that sends a message of its own from inside would loop for ever.
		if (messageDepth >= 3) return;
		messageDepth++;
		try {
			L.reduxStore.dispatch(action, true);
		} catch (err) {
			L.reportError(err);
		} finally {
			messageDepth--;
		}
	};
	const Messager = {
		Info: (...d) => sendMessage("message/MESSAGE_INFO", "INFO", d),
		Warn: (...d) => sendMessage("message/MESSAGE_WARN", "MESSAGE_WARN", d),
		Error: (...d) => sendMessage("message/MESSAGE_ERROR", "ERROR", d),
	};
	// Same shape as @inrixia/helpers Signal, used for errSignal.
	class Signal {
		constructor(value, onSet) {
			this._v = value;
			this._l = new Set();
			if (onSet) this._l.add(onSet);
		}
		get _() {
			return this._v;
		}
		set _(v) {
			this._v = v;
			for (const l of [...this._l]) {
				try {
					l(v);
				} catch (err) {
					L.reportError(err);
				}
			}
		}
		onValue(cb) {
			this._l.add(cb);
			const unload = () => this._l.delete(cb);
			return unload;
		}
	}
	const createLogger = (source, consumer) => {
		const logger = (...data) => consumer(source, ...data);
		logger.withContext = (...context) => {
			const withCtx = (...data) => consumer(source, ...context, ...data);
			withCtx.throw = (...data) => {
				withCtx(...data);
				if (data?.[0] instanceof Error) {
					data[0].cause = sanitize(source, ...context);
					throw data[0];
				}
				throw new Error(sanitize(source, ...context, ...data), { cause: source });
			};
			return withCtx;
		};
		logger.throw = (...data) => {
			logger(...data);
			if (data?.[0] instanceof Error) throw data[0];
			throw new Error(sanitize(source, ...data), { cause: source });
		};
		return logger;
	};
	const Tracer = (source, errSignal = new Signal(undefined)) => {
		const errConsumer =
			errSignal !== undefined && errSignal !== null
				? (src, ...args) => {
						console.error(src, ...args);
						errSignal._ = args.map((a) => (a instanceof Error ? a.message : typeof a === "object" ? "" : String(a))).join(" ");
					}
				: console.error;
		return {
			trace: {
				withSource: (s) => Tracer(source + s, errSignal),
				log: createLogger(source, console.log),
				warn: createLogger(source, console.warn),
				err: createLogger(source, errConsumer),
				debug: createLogger(source, console.debug),
				msg: {
					log: createLogger(source, (...d) => (Messager.Info(...d), console.log(...d))),
					warn: createLogger(source, (...d) => (Messager.Warn(...d), console.warn(...d))),
					err: createLogger(source, (...d) => (Messager.Error(...d), errConsumer(...d))),
				},
			},
			errSignal,
		};
	};
	const coreTrace = Tracer("[@luna/core]", null).trace;

	// ---------------------------------------------------------------- storage
	// ReactiveStore: idb-keyval + oby in Luna; here JSON files written by Rust.
	const proxies = new WeakMap();
	const reactive = (target, save) => {
		if (target === null || typeof target !== "object") return target;
		if (proxies.has(target)) return proxies.get(target);
		const p = new Proxy(target, {
			get(t, k, r) {
				const v = Reflect.get(t, k, r);
				return typeof k === "symbol" ? v : reactive(v, save);
			},
			set(t, k, v) {
				const raw = v !== null && typeof v === "object" && v.__lunaRaw ? v.__lunaRaw : v;
				const ok = Reflect.set(t, k, raw);
				save();
				return ok;
			},
			deleteProperty(t, k) {
				const ok = Reflect.deleteProperty(t, k);
				save();
				return ok;
			},
		});
		Object.defineProperty(target, "__lunaRaw", { value: target, enumerable: false, configurable: true });
		proxies.set(target, p);
		return p;
	};
	const unwrap = (v) => (v !== null && typeof v === "object" && v.__lunaRaw ? v.__lunaRaw : v);

	// ---------------------------------------------------------------- IndexedDB facade
	// `ReactiveStore.idbStore` is idb-keyval's `UseStore`: `store(mode, (objectStore)
	// => ...)`, which is what `get(key, rs.idbStore)`, `set`, `update`, `keys`,
	// `entries`, `getMany`, `setMany`, `del` and `clear` call. The object store
	// here is a facade over the same files `ReactiveStore.get/set` use
	// (`storage/<store>.json`: atomic writes, 16 MB a store), so both see the same
	// data. Values that are not plain JSON (undefined, null, NaN, Date, Map, Set,
	// typed arrays, circular references, ...) are kept as structured-clone trees;
	// keys that are not strings (numbers, dates, arrays, binary) are kept
	// under an encoded name and come back as they went in, in IndexedDB's order.
	const CLONE_TAG = "__lunaClone";
	const isPlain = (v, depth = 0) => {
		if (depth > 64) return false;
		switch (typeof v) {
			case "string":
			case "boolean":
				return true;
			case "number":
				return Number.isFinite(v) && !Object.is(v, -0);
		}
		if (v === null || typeof v !== "object") return false;
		if (Array.isArray(v)) {
			if (Object.keys(v).length !== v.length) return false;
			for (let i = 0; i < v.length; i++) if (!isPlain(v[i], depth + 1)) return false;
			return true;
		}
		const proto = Object.getPrototypeOf(v);
		if (proto !== Object.prototype && proto !== null) return false;
		if (Object.getOwnPropertySymbols(v).length !== 0 || v === g) return false;
		for (const k of Object.keys(v)) if (k === CLONE_TAG || !isPlain(v[k], depth + 1)) return false;
		return true;
	};
	const storeValue = (v) => (isPlain(v) ? JSON.parse(JSON.stringify(v)) : { [CLONE_TAG]: L.clone.toTree(v) });
	const loadValue = (v) => (v !== null && typeof v === "object" && !Array.isArray(v) && CLONE_TAG in v && Object.keys(v).length === 1 ? L.clone.fromTree(v[CLONE_TAG]) : (v ?? undefined));
	const dataError = (msg, name = "DataError") => new DOMException(msg, name);
	// 0 number, 1 date, 2 string, 3 binary, 4 array: IndexedDB's order of key types.
	const keyRank = (k) => (typeof k === "number" ? 0 : k instanceof Date ? 1 : typeof k === "string" ? 2 : Array.isArray(k) ? 4 : 3);
	const toBytes = (k) => (k instanceof ArrayBuffer ? new Uint8Array(k) : new Uint8Array(k.buffer, k.byteOffset, k.byteLength));
	const validKey = (k, depth = 0) => {
		if (typeof k === "string") return true;
		if (typeof k === "number") return !Number.isNaN(k);
		if (k instanceof Date) return !Number.isNaN(k.getTime());
		if (k instanceof ArrayBuffer || ArrayBuffer.isView(k)) return true;
		return Array.isArray(k) && depth < 16 && k.every((x) => validKey(x, depth + 1));
	};
	const compareKeys = (a, b) => {
		const ra = keyRank(a), rb = keyRank(b);
		if (ra !== rb) return ra < rb ? -1 : 1;
		switch (ra) {
			case 0:
				return a < b ? -1 : a > b ? 1 : 0;
			case 1:
				return a.getTime() - b.getTime();
			case 2:
				return a < b ? -1 : a > b ? 1 : 0;
			case 3: {
				const x = toBytes(a), y = toBytes(b);
				for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
				return x.length - y.length;
			}
			default:
				for (let i = 0; i < Math.min(a.length, b.length); i++) {
					const c = compareKeys(a[i], b[i]);
					if (c !== 0) return c;
				}
				return a.length - b.length;
		}
	};
	// A key as the name it has in the file: strings stay as they are.
	const encodeKey = (k) => {
		if (!validKey(k)) throw dataError("The parameter is not a valid key.");
		if (typeof k === "string") return k.startsWith("\u0000") ? `\u0000s${k}` : k;
		if (typeof k === "number") return `\u0000n${Object.is(k, -0) ? "-0" : String(k)}`;
		if (k instanceof Date) return `\u0000d${k.getTime()}`;
		if (Array.isArray(k)) return `\u0000a${JSON.stringify(k.map(encodeKey))}`;
		return `\u0000b${Buffer.from(toBytes(k)).toString("base64")}`;
	};
	const decodeKey = (s) => {
		if (!s.startsWith("\u0000")) return s;
		const rest = s.slice(2);
		switch (s[1]) {
			case "s":
				return rest;
			case "n":
				return rest === "-0" ? -0 : Number(rest);
			case "d":
				return new Date(Number(rest));
			case "a":
				return JSON.parse(rest).map(decodeKey);
			default: {
				const b = Buffer.from(rest, "base64");
				return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
			}
		}
	};
	// A key range: a bare key is the range of just itself; { lower, upper,
	// lowerOpen, upperOpen } (what IDBKeyRange carries) bounds one.
	const isRange = (r) => r !== null && typeof r === "object" && ("lower" in r || "upper" in r) && !Array.isArray(r) && !(r instanceof Date) && !ArrayBuffer.isView(r) && !(r instanceof ArrayBuffer);
	const inRange = (range, key) => {
		if (range === undefined || range === null) return true;
		if (isRange(range)) {
			if (range.lower !== undefined) {
				const c = compareKeys(key, range.lower);
				if (c < 0 || (c === 0 && range.lowerOpen)) return false;
			}
			if (range.upper !== undefined) {
				const c = compareKeys(key, range.upper);
				if (c > 0 || (c === 0 && range.upperOpen)) return false;
			}
			return true;
		}
		return compareKeys(key, range) === 0;
	};
	const idbEvent = (type, target) => ({ type, target, currentTarget: target, bubbles: false, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, stopPropagation() {} });
	class IdbRequest {
		constructor(source, transaction) {
			this.source = source;
			this.transaction = transaction;
			this.readyState = "pending";
			this.result = undefined;
			this.error = null;
			this.onsuccess = null;
			this.onerror = null;
			this._l = { success: new Set(), error: new Set() };
		}
		addEventListener(type, fn) {
			this._l[type]?.add(fn);
		}
		removeEventListener(type, fn) {
			this._l[type]?.delete(fn);
		}
		_fire(type) {
			const ev = idbEvent(type, this);
			const handler = type === "success" ? this.onsuccess : this.onerror;
			if (typeof handler === "function") handler.call(this, ev);
			for (const fn of [...this._l[type]]) fn.call(this, ev);
			return ev;
		}
	}
	class IdbTransaction {
		constructor(db, mode) {
			this.db = db;
			this.mode = mode;
			this.error = null;
			this.durability = "default";
			this.objectStoreNames = ["_"];
			this.oncomplete = this.onerror = this.onabort = null;
			this._l = { complete: new Set(), error: new Set(), abort: new Set() };
			this._queue = [];
			this._undo = new Map();
			this._scheduled = false;
			this._done = false;
		}
		addEventListener(type, fn) {
			this._l[type]?.add(fn);
		}
		removeEventListener(type, fn) {
			this._l[type]?.delete(fn);
		}
		objectStore() {
			return this.db._objectStore(this);
		}
		commit() {}
		_emit(type) {
			const ev = idbEvent(type, this);
			const handler = this[`on${type}`];
			if (typeof handler === "function") handler.call(this, ev);
			for (const fn of [...this._l[type]]) fn.call(this, ev);
			return ev;
		}
		// Queue one operation; it runs, with its callbacks, on a timer, in order.
		_request(source, run, write) {
			if (this._done) throw new DOMException("The transaction has finished.", "TransactionInactiveError");
			if (write && this.mode === "readonly") throw new DOMException("The transaction is read-only.", "ReadOnlyError");
			const req = new IdbRequest(source, this);
			this._enqueue(req, run);
			return req;
		}
		_enqueue(req, run) {
			req.readyState = "pending";
			this._queue.push({ req, run });
			this._schedule();
		}
		_schedule() {
			if (this._scheduled) return;
			this._scheduled = true;
			L.internalTimeout(() => {
				this._scheduled = false;
				this._drain();
			}, 0);
		}
		_drain() {
			while (this._queue.length !== 0 && !this._done) {
				const { req, run } = this._queue.shift();
				let ev;
				try {
					req.result = run(req);
					req.readyState = "done";
					ev = req._fire("success");
				} catch (err) {
					req.readyState = "done";
					req.error = err instanceof Error && err.name !== "Error" ? err : new DOMException(String(err?.message ?? err), "UnknownError");
					req.result = undefined;
					try {
						ev = req._fire("error");
					} catch (e) {
						L.reportError(e);
					}
					// An error nobody handled aborts the transaction.
					if (!ev?.defaultPrevented) return void this._abort(req.error);
				}
			}
			if (this._queue.length === 0 && !this._done) {
				// A callback may still add a request; complete on the next turn if it did not.
				L.internalTimeout(() => {
					if (this._queue.length === 0 && !this._done) {
						this._done = true;
						this._emit("complete");
					} else this._schedule();
				}, 0);
			}
		}
		_abort(error) {
			if (this._done) return;
			this._done = true;
			this.error = error ?? null;
			for (const [store, keys] of this._undo) for (const [key, before] of keys) before === undefined ? callOp("storage.del", { store, key }) : callOp("storage.set", { store, key, value: before });
			for (const { req } of this._queue.splice(0)) {
				req.readyState = "done";
				req.error = new DOMException("The transaction was aborted.", "AbortError");
				try {
					req._fire("error");
				} catch (e) {
					L.reportError(e);
				}
			}
			if (error) this._emit("error");
			this._emit("abort");
		}
		abort() {
			this._abort(null);
		}
	}
	class IdbObjectStore {
		constructor(db, transaction) {
			this._db = db;
			this.transaction = transaction;
			this.name = "_";
			this.keyPath = null;
			this.autoIncrement = false;
			this.indexNames = [];
		}
		get _name() {
			return this._db.idbName;
		}
		_keys() {
			return (callOp("storage.keys", { store: this._name }) ?? []).map(decodeKey).sort(compareKeys);
		}
		_read(key) {
			return loadValue(callOp("storage.get", { store: this._name, key: encodeKey(key) }));
		}
		_remember(name) {
			const undo = this.transaction._undo;
			if (!undo.has(this._name)) undo.set(this._name, new Map());
			const m = undo.get(this._name);
			if (!m.has(name)) m.set(name, callOp("storage.get", { store: this._name, key: name }) ?? undefined);
		}
		_write(key, value) {
			const name = encodeKey(key);
			this._remember(name);
			try {
				callOp("storage.set", { store: this._name, key: name, value: storeValue(value) });
			} catch (e) {
				const m = /^QuotaExceededError/.exec(String(e?.message));
				throw m ? new DOMException(String(e.message), "QuotaExceededError") : e;
			}
		}
		_put(value, key, add) {
			if (key === undefined) throw dataError("Evaluating the object store's key path did not yield a value.");
			return this.transaction._request(this, () => {
				if (add && (callOp("storage.keys", { store: this._name }) ?? []).includes(encodeKey(key))) throw new DOMException("A mutation operation in the transaction failed because a key constraint was not satisfied.", "ConstraintError");
				this._write(key, value);
				return key;
			}, true);
		}
		put(value, key) {
			return this._put(value, key, false);
		}
		add(value, key) {
			return this._put(value, key, true);
		}
		delete(range) {
			if (range === undefined) throw dataError("The parameter is not a valid key.");
			return this.transaction._request(this, () => {
				for (const k of isRange(range) ? this._keys().filter((x) => inRange(range, x)) : [range]) {
					const name = encodeKey(k);
					this._remember(name);
					callOp("storage.del", { store: this._name, key: name });
				}
			}, true);
		}
		clear() {
			return this.transaction._request(this, () => {
				for (const k of this._keys()) {
					const name = encodeKey(k);
					this._remember(name);
					callOp("storage.del", { store: this._name, key: name });
				}
			}, true);
		}
		get(range) {
			if (range === undefined) throw dataError("The parameter is not a valid key.");
			return this.transaction._request(this, () => {
				if (!isRange(range)) return this._read(range);
				const k = this._keys().find((x) => inRange(range, x));
				return k === undefined ? undefined : this._read(k);
			});
		}
		getKey(range) {
			return this.transaction._request(this, () => this._keys().find((k) => inRange(range, k)));
		}
		getAll(range, count) {
			return this.transaction._request(this, () => this._keys().filter((k) => inRange(range, k)).slice(0, count ?? undefined).map((k) => this._read(k)));
		}
		getAllKeys(range, count) {
			return this.transaction._request(this, () => this._keys().filter((k) => inRange(range, k)).slice(0, count ?? undefined));
		}
		count(range) {
			return this.transaction._request(this, () => this._keys().filter((k) => inRange(range, k)).length);
		}
		_cursor(range, direction, withValue) {
			let keys = this._keys().filter((k) => inRange(range, k));
			if (direction === "prev" || direction === "prevunique") keys = keys.reverse();
			let at = -1;
			const store = this;
			const req = this.transaction._request(this, (r) => step(r));
			const step = (r) => {
				at++;
				if (at >= keys.length) return null;
				const key = keys[at];
				const cursor = {
					source: store,
					direction: direction ?? "next",
					request: r,
					key,
					primaryKey: key,
					...(withValue ? { value: store._read(key) } : {}),
					continue() {
						store.transaction._enqueue(r, () => step(r));
					},
					advance(n) {
						at += n - 1;
						cursor.continue();
					},
					delete() {
						return store.delete(key);
					},
					update(v) {
						return store.put(v, key);
					},
				};
				return cursor;
			};
			return req;
		}
		openCursor(range, direction) {
			return this._cursor(range, direction, true);
		}
		openKeyCursor(range, direction) {
			return this._cursor(range, direction, false);
		}
		createIndex() {
			throw new DOMException("Indexes are not available in Tidalfast's plugin stores.", "InvalidStateError");
		}
		index() {
			throw new DOMException("Indexes are not available in Tidalfast's plugin stores.", "NotFoundError");
		}
	}
	// What `ReactiveStore#idbStore` is: idb-keyval's `UseStore` for one store.
	const makeIdbStore = (rs) => {
		const db = { idbName: rs.idbName, _objectStore: (tx) => new IdbObjectStore(db, tx) };
		return (mode, callback) => {
			if (mode !== "readonly" && mode !== "readwrite") return Promise.reject(new TypeError(`The provided value '${mode}' is not a valid enum value of type IDBTransactionMode.`));
			return Promise.resolve().then(() => callback(new IdbTransaction(db, mode).objectStore()));
		};
	};
	class ReactiveStore {
		static Storages = {};
		static async getPluginStorage(pluginName, defaultValue) {
			const storage = await this.getStore("@luna/pluginStorage").getReactive(pluginName);
			if (defaultValue !== undefined) for (const k of Object.keys(defaultValue)) storage[k] ??= defaultValue[k];
			return storage;
		}
		static getStore(name) {
			return (this.Storages[name] ??= new this(name));
		}
		constructor(name) {
			this.idbName = name;
			// idb-keyval's store handle upstream; here the same call shape over the
			// store's file (see the IndexedDB facade above).
			this.idbStore = makeIdbStore(this);
			this.trace = coreTrace.withSource(`.ReactiveStore[${name}]`).trace;
			this.reactiveCache = {};
			this.saveTimers = {};
		}
		async getReactive(key, defaultValue = {}) {
			if (key in this.reactiveCache) return this.reactiveCache[key];
			const stored = callOp("storage.get", { store: this.idbName, key });
			const raw = stored ?? structuredClone(defaultValue);
			const save = () => {
				if (this.saveTimers[key]) return;
				this.saveTimers[key] = L.internalTimeout(() => {
					delete this.saveTimers[key];
					callOp("storage.set", { store: this.idbName, key, value: raw });
				}, 0);
			};
			if (stored === null || stored === undefined) save();
			return (this.reactiveCache[key] = reactive(raw, save));
		}
		async ensure(key, defaultValue, awaitSet = false) {
			const value = await this.get(key);
			if (value === undefined) {
				const v = typeof defaultValue === "function" ? await defaultValue() : defaultValue;
				await this.set(key, v);
				return v;
			}
			return value;
		}
		async get(key) {
			if (key in this.reactiveCache) return structuredClone(unwrap(this.reactiveCache[key]));
			return loadValue(callOp("storage.get", { store: this.idbName, key }));
		}
		async set(key, value) {
			callOp("storage.set", { store: this.idbName, key, value: unwrap(value) });
			if (key in this.reactiveCache) {
				const raw = unwrap(this.reactiveCache[key]);
				if (raw && typeof raw === "object" && value && typeof value === "object") {
					for (const k of Object.keys(raw)) if (!(k in value)) delete raw[k];
					Object.assign(raw, structuredClone(unwrap(value)));
				} else delete this.reactiveCache[key];
			}
			return value;
		}
		async del(key) {
			delete this.reactiveCache[key];
			callOp("storage.del", { store: this.idbName, key });
		}
		async keys() {
			return callOp("storage.keys", { store: this.idbName }) ?? [];
		}
		async dump() {
			const out = {};
			for (const k of await this.keys()) out[k] = await this.get(k);
			return out;
		}
		async clear() {
			for (const k of await this.keys()) await this.del(k);
		}
	}

	// ---------------------------------------------------------------- tidal "modules"
	// Luna searches TIDAL's webpack modules. Tidalfast offers a small set of
	// properties the host chooses to share (e.g. the Last.fm API key).
	const hostProps = callOp("module_props", null) ?? {};
	const tidalModules = {
		tidalfast: {
			...hostProps,
			activePlayer: {
				get currentTime() {
					return L.reduxStore?.getState().playbackControls.latestCurrentTime ?? 0;
				},
			},
		},
	};
	function* recursiveSearch(obj, selector, seen = new Set(), path = []) {
		if (obj === g || obj === null || typeof obj !== "object" || seen.has(obj)) return;
		seen.add(obj);
		// An object with no own keys is searched through its prototype's
		// (a class instance whose members are methods).
		const own = Object.keys(obj);
		for (const key of own.length !== 0 ? own : Reflect.ownKeys(Object.getPrototypeOf(obj) ?? obj)) {
			try {
				const prop = obj[key];
				const cur = [...path, key];
				if (typeof prop === "object" && prop !== null) yield* recursiveSearch(prop, selector, seen, cur);
				if (selector(key, prop)) yield { value: prop, path: cur };
			} catch {}
		}
	}
	const missedSelectors = new Set();
	const findModuleProperty = (selector) => {
		const found = recursiveSearch(tidalModules, selector).next().value;
		// TIDAL's own web modules do not exist here: say so (once per selector)
		// instead of handing back undefined without a word.
		if (found === undefined && !missedSelectors.has(String(selector))) {
			missedSelectors.add(String(selector));
			coreTrace.warn("findModuleProperty", "No module matches selector (Tidalfast has no TIDAL web bundle), returning undefined:", String(selector));
		}
		return found;
	};
	const findModuleByProperty = (selector) => {
		const found = recursiveSearch(tidalModules, selector).next().value;
		if (found === undefined) return void coreTrace.warn("findModuleByProperty", "Unable to find module using selector:", String(selector));
		let m = tidalModules;
		found.path.pop();
		for (const k of found.path) m = m[k];
		return m;
	};

	// ---------------------------------------------------------------- redux
	const interceptors = {};
	const subscribers = new Set();
	// subscriber -> the plugin that subscribed it.
	const subscriberOwners = new Map();
	const content = { mediaItems: {}, albums: {}, artists: {}, playlists: {}, mixes: {}, videos: {} };
	// The maps of TIDAL's `content` slice (`Content` in @luna/lib's types).
	const CONTENT_MAPS = ["albumCredits", "albumLists", "albumReviews", "articleLists", "articles", "artistBios", "artistContributions", "artistLists", "artistRoleCategories", "dateAddedMaps", "dynamicPages", "folders", "lyrics", "mediaItemProxies", "mixLists", "mixedTypesLists", "playlistLists", "trackContributors", "trackLists", "videoContributors"];
	let cached = null;
	let cachedAt = 0;
	let lastState = null;
	const invalidate = () => (cached = null);
	// TIDAL UI state Tidalfast does not own (device mode, app settings,
	// feature flags, router, search, ...). Plugins read it, and the UI actions
	// they dispatch update it here, so code that "corrects" a setting on a
	// timer sees its change take effect instead of dispatching forever.
	// Every slice of TIDAL's store state (`TidalStoreState` in @luna/lib's
	// redux types) is here with its fields, so a plugin reading
	// `state.network.hasNetworkConnection` gets a value, not a TypeError.
	const systemDevice = { id: "default", name: "System default", type: "default", controllableVolume: true, nativeDeviceId: "default", webDeviceId: "default" };
	const local = {
		accumulatedPlaybackTime: { lastPlayingTimestamp: 0, playbackState: "NOT_PLAYING", productId: "", storedTimes: {} },
		activePlayer: { activePlayer: "PLAYER_SDK", currentStreamingSessionId: null },
		artistPicker: { alreadyFetchedArtists: null, categories: [], isLoading: false, searchAsideShowing: false, searchPhrase: "", searchResults: [], selected: [] },
		auth: { verificationUri: "" },
		cloudQueue: { currentItemId: "", etag: "", headPosition: 0, historyMediaItemIds: [], itemsEtag: "", properties: null, queueId: "", repeatMode: 0, shuffled: false, tailItemId: "", tailPosition: 0 },
		etag: {},
		experimentationPlatform: { activeExperiments: {}, anonymousExperiments: {}, authenticatedExperiments: {}, experimentKeys: {} },
		feed: { hasNewFeedItems: false, status: "", updates: [] },
		network: { failedStartup: false, hasNetworkConnection: true },
		selections: { indices: [], isDragging: false, items: [], loadedFolders: [], openFolders: [] },
		userProfiles: {},
		speech: { command: null, recognitionActive: false, recognitionSupported: false },
		player: { activeDeviceId: "default", activeDeviceMode: "shared", activeDeviceType: "default", desiredDeviceMode: {}, forceVolume: {}, hasPreloadedNextProduct: false, devices: [systemDevice], availableDevices: [systemDevice], preloadedItem: null, isConnected: true },
		settings: {
			autoPlay: true,
			audioNormalization: "ALBUM",
			audioSpectrumEnabled: false,
			explicitContentEnabled: true,
			streamingQuality: "HI_RES_LOSSLESS",
			quality: { streaming: "HI_RES_LOSSLESS" },
			downloadQuality: "HI_RES_LOSSLESS",
			videoQuality: "HIGH",
			language: "en",
			crossfade: 0,
			startMinimized: false,
			closeToTray: false,
			desktop: { autoStartMode: 0, closeToTray: false },
			openLinksInDesktopApp: false,
			updateAvailable: false,
			urls: { accessibility: "https://tidal.com/accessibility", betaParticipationAgreement: "https://tidal.com/beta-participation-agreement", earlyAccess: "https://tidal.com/early-access", privacy: "https://tidal.com/privacy", support: "https://support.tidal.com", terms: "https://tidal.com/terms" },
		},
		featureFlags: { ready: true, flags: {}, userOverrides: {} },
		router: { currentPath: "/", currentParams: {}, currentSearch: "", search: "", previousPath: null, history: ["/"] },
		search: {
			searchPhrase: "",
			results: null,
			didYouMeanByQueryMap: {},
			genres: null,
			isLoading: false,
			queryId: "",
			recentSearches: [],
			searchResultFilterOrder: ["TRACKS", "VIDEOS", "ARTISTS", "ALBUMS", "PLAYLISTS", "UPLOADS", "USERPROFILES"],
			searchSession: { lastUpdated: 0, triggerNewSession: false, uuid: "" },
			suggestionUuid: "",
			topHit: null,
		},
		entities: { lyrics: { entities: {}, ids: [] } },
		locale: { ready: true, currentLocale: "en_US", locale: "en_US", bundles: {}, currentBundleName: "en-us" },
		view: {
			appReady: true,
			isDragging: false,
			isFeedSidebarShowing: false,
			isFullscreen: false,
			isNativeFullscreen: false,
			isNowPlaying: false,
			isNowPlayingVisible: false,
			isPortal: false,
			isSearchPopoverShowing: false,
			modules: {},
			showNowPlaying: false,
			showPlayQueueAside: false,
			triggerSearchFocus: 0,
			fullscreenAllowed: true,
		},
		modal: { activeModal: null },
		notifications: { items: [], notifications: [] },
		remotePlayback: { connectedDevice: null, deviceToConnectTo: null, devices: { chromeCast: [], cloudConnect: [], tidalConnect: [] }, isConnected: false, remotePlaybackReceiverState: 0, session: null, sessionStatus: "nosession", type: null },
		user: {
			clients: [],
			isLoaded: true,
			isLoading: false,
			meta: {},
			onboarding: { error: null, loaded: true, stepsShown: [] },
			subscription: { status: "ACTIVE", subscription: { type: "PREMIUM", offlineGracePeriod: 30 }, highestSoundQuality: "HI_RES_LOSSLESS", premiumAccess: true, canGetTrial: false, paymentType: "", paymentOverdue: false, startDate: "", validUntil: "" },
			temporaryDisableArtistPicker: false,
		},
		session: { clientId: "", clientUniqueKey: "", facebookAccessToken: null, isInitialized: true, isLoading: false, isPolling: false, migrated: true, utmParameters: { banner: "", campaign: "", content: "", medium: "", source: "" } },
	};
	// Which songs TIDAL has lyrics for, as the app announced them (the
	// playing song's lyrics load when it starts); kept for the last few.
	const lyricsKnown = new Map();
	const localReducer = (type, p) => {
		const set = (slice, patch) => (local[slice] = { ...local[slice], ...patch });
		switch (type) {
			case "content/LOAD_ITEM_LYRICS":
			case "content/LOAD_ITEM_LYRICS_SUCCESS":
			case "content/LOAD_ITEM_LYRICS_FAIL": {
				const id = String(p?.trackId ?? "");
				if (id === "") return;
				lyricsKnown.delete(id);
				// The words come along: TidalLyricsTranslate reads the playing
				// song's lyrics from `entities.lyrics`.
				if (type.endsWith("SUCCESS")) lyricsKnown.set(id, { text: String(p?.lyrics ?? ""), lrcText: String(p?.subtitles ?? ""), direction: p?.isRightToLeft ? "rtl" : "ltr" });
				for (const old of [...lyricsKnown.keys()].slice(0, Math.max(0, lyricsKnown.size - 20))) lyricsKnown.delete(old);
				return;
			}
			case "settings/TOGGLE_AUTOPLAY":
				return set("settings", { autoPlay: !local.settings.autoPlay });
			case "settings/SET_AUTOPLAY":
				return set("settings", { autoPlay: !!(p?.autoPlay ?? p) });
			case "settings/TOGGLE_NORMALIZATION":
				return set("settings", { audioNormalization: local.settings.audioNormalization === "NONE" ? "ALBUM" : "NONE" });
			case "settings/SET_AUDIO_NORMALIZATION":
				return set("settings", { audioNormalization: String(p?.audioNormalization ?? p ?? "NONE") });
			case "settings/SET_EXPLICIT_CONTENT_TOGGLE":
				return set("settings", { explicitContentEnabled: !!(p?.isEnabled ?? p) });
			case "settings/SET_STREAMING_QUALITY":
				return set("settings", { streamingQuality: p?.quality ?? p, quality: { ...local.settings.quality, streaming: p?.quality ?? p } });
			case "player/SET_DEVICE_MODE":
				return set("player", { activeDeviceMode: String(p?.mode ?? p) });
			case "player/SET_FORCE_VOLUME":
				return set("player", { forceVolume: { ...local.player.forceVolume, [p?.deviceId ?? local.player.activeDeviceId]: !!p?.on } });
			case "player/SET_ACTIVE_DEVICE":
				return set("player", { activeDeviceId: p?.id ?? p?.deviceId ?? p });
			case "featureFlags/READY":
				return set("featureFlags", { ready: true });
			case "featureFlags/TOGGLE_USER_OVERRIDE":
			case "featureFlags/SET_USER_OVERRIDE":
				if (p?.name !== undefined) set("featureFlags", { userOverrides: { ...local.featureFlags.userOverrides, [p.name]: p.value ?? !local.featureFlags.userOverrides[p.name] } });
				return;
			case "router/PUSH":
			case "router/REPLACE":
			case "router/NAVIGATED": {
				const path = typeof p === "string" ? p : (p?.pathname ?? p?.path ?? p?.location?.pathname);
				if (typeof path === "string") set("router", { previousPath: local.router.currentPath, currentPath: path, currentSearch: p?.search ?? "", search: p?.search ?? "", history: [...local.router.history.slice(-49), path] });
				return;
			}
			case "search/SET_SEARCH_PHRASE":
				return set("search", { searchPhrase: String(p?.searchPhrase ?? p ?? "") });
			case "view/REQUEST_FULLSCREEN":
				return set("view", { isFullscreen: true });
			case "view/EXIT_FULLSCREEN":
				return set("view", { isFullscreen: false });
			case "view/ENTERED_NOWPLAYING":
				return set("view", { isNowPlayingVisible: true, isNowPlaying: true, showNowPlaying: true });
			case "view/EXIT_NOWPLAYING":
				return set("view", { isNowPlayingVisible: false, isNowPlaying: false, showNowPlaying: false });
			case "view/FULLSCREEN_ALLOWED":
				return set("view", { fullscreenAllowed: p !== false });
		}
	};
	L.localState = local;
	// Queue edits a plugin made that the app has not applied yet. A plugin
	// that removes a row and reads the queue back must see the row gone, so
	// the edit shows until the real queue changes (or a moment passes).
	let queueOverlay = null;
	let realQueueSig = "";
	const queueSig = (q) => `${q?.currentIndex}|${(q?.elements ?? []).map((e) => e.uid).join(",")}`;
	const editQueue = (type, p) => {
		const q = getState().playQueue;
		if (!q || !Array.isArray(q.elements)) return;
		let els = q.elements;
		let ci = q.currentIndex;
		// Only what the app takes out: a plugin sees nothing removed that
		// would be back a moment later, when its edit expires. Clear keeps
		// the playlist or album (the songs the user queued go).
		const room = typeof q.tidalfastRemovableRows === "number" ? q.tidalfastRemovableRows : els.length;
		const clearRoom = typeof q.tidalfastClearableRows === "number" ? q.tidalfastClearableRows : els.length;
		if (type === "playQueue/REMOVE_ELEMENT" || type === "playQueue/REMOVE_AT_INDEX") {
			const i = type === "playQueue/REMOVE_ELEMENT" ? els.findIndex((e) => e.uid === p?.uid) : Number(p?.index ?? p);
			if (!(i >= 0 && i < els.length) || i === ci) return;
			if (i > ci && i > ci + room) return;
			els = els.filter((_, j) => j !== i);
			if (i < ci) ci--;
		} else if (type === "playQueue/CLEAR_ACTIVE_ITEMS" || type === "playQueue/CLEAR_QUEUE") {
			els = [...els.slice(0, ci + 1), ...els.slice(ci + 1 + clearRoom)];
		} else return;
		queueOverlay = { at: Date.now(), sig: realQueueSig, patch: { elements: els, currentIndex: ci } };
	};
	// `fresh`, or the object `old` already is when the two are equal all the
	// way down (children are kept one by one, too).
	const stabilize = (old, fresh) => {
		if (old === fresh) return old;
		if (old === null || fresh === null || typeof old !== "object" || typeof fresh !== "object") return fresh;
		if (Array.isArray(old) !== Array.isArray(fresh)) return fresh;
		let same;
		let out;
		if (Array.isArray(fresh)) {
			same = old.length === fresh.length;
			out = new Array(fresh.length);
			for (let i = 0; i < fresh.length; i++) {
				out[i] = stabilize(old[i], fresh[i]);
				if (out[i] !== old[i]) same = false;
			}
		} else {
			const keys = Object.keys(fresh);
			same = Object.keys(old).length === keys.length;
			out = {};
			for (const k of keys) {
				out[k] = stabilize(old[k], fresh[k]);
				if (out[k] !== old[k] || !(k in old)) same = false;
			}
		}
		return same ? old : out;
	};
	const getState = () => {
		const now = Date.now();
		if (cached === null || now - cachedAt > 50) {
			const s = callOp("state", null) ?? {};
			realQueueSig = queueSig(s.playQueue);
			if (queueOverlay && (queueOverlay.sig !== realQueueSig || now - queueOverlay.at > 2500)) queueOverlay = null;
			if (queueOverlay && s.playQueue) s.playQueue = { ...s.playQueue, ...queueOverlay.patch };
			const c = s.content ?? {};
			// Redux keeps one object until it changes, and plugins key their
			// caches on it (a WeakMap per stored track): the host hands over
			// a fresh copy on every read, so what did not change keeps the
			// object the plugin saw before.
			for (const k of Object.keys(c)) {
				const old = content[k] ?? {};
				let merged = null;
				for (const id of Object.keys(c[k] ?? {})) {
					const kept = stabilize(old[id], c[k][id]);
					if (kept !== old[id]) (merged ??= { ...old })[id] = kept;
				}
				content[k] = merged ?? old;
			}
			for (const k of CONTENT_MAPS) content[k] ??= {};
			s.content = content;
			for (const [k, v] of Object.entries(local)) s[k] = s[k] && typeof s[k] === "object" && !Array.isArray(s[k]) ? { ...v, ...s[k] } : (s[k] ?? v);
			// TIDAL's `entities.tracks` holds the song being played, with a
			// `lyrics` relationship when it has lyrics (radiant-lyrics reads
			// it to decide whether to register its own).
			const playing = s.playbackControls?.mediaProduct?.productId;
			if (playing !== undefined && playing !== null && playing !== "") {
				const id = String(playing);
				const words = lyricsKnown.get(id);
				const lyrics = words ? [{ id, type: "lyrics" }] : [];
				s.entities = { ...s.entities, tracks: { ids: [id], entities: { [id]: { id, type: "tracks", attributes: {}, relationships: { lyrics: { data: lyrics } } } } } };
				if (words) s.entities.lyrics = { ids: [id], entities: { [id]: { id, type: "lyrics", attributes: words } } };
			}
			cached = lastState = stabilize(lastState, s);
			cachedAt = now;
		}
		return cached;
	};
	const runInterceptors = (type, payload, skipPlugin) => {
		let dispatch = true;
		const set = interceptors[type];
		if (set && set.size > 0) {
			for (const cb of [...set]) {
				// A plugin's own dispatch is not offered back to it: a plugin
				// that cancels an action and dispatches its replacement
				// would otherwise cancel that again, forever.
				if (skipPlugin && cb.__lunaPlugin === skipPlugin) continue;
				try {
					const r = cb(payload, type);
					if (r === true) dispatch = false;
					else if (r instanceof Promise) r.catch(L.reportError);
				} catch (err) {
					L.reportError(err);
				}
			}
		}
		return dispatch;
	};
	const notifySubscribers = () => {
		for (const s of [...subscribers]) {
			try {
				s();
			} catch (err) {
				L.reportError(err);
			}
		}
	};
	// Plugin -> host: run interceptors, then let Rust translate the action.
	// @luna/dev's logging (Settings > Luna core plugins); null/false when off.
	const devLog = { redux: null, native: false, render: false };
	const devText = (v) => {
		let t;
		try {
			t = v === undefined ? "" : JSON.stringify(v);
		} catch {
			t = String(v);
		}
		return t && t.length > 400 ? `${t.slice(0, 400)}…` : (t ?? "");
	};
	const logRedux = (type, payload) => {
		if (devLog.redux !== null && devLog.redux.test(type)) callOp("log", { plugin: null, level: "info", msg: `(Redux) ${type} ${devText(payload)}` });
	};
	const dispatch = (action, offerBack = false) => {
		if (!action || typeof action.type !== "string") return action;
		if (devLog.redux !== null) logRedux(action.type, action.payload);
		const who = L.pluginFromStack();
		if (!runInterceptors(action.type, action.payload, offerBack ? undefined : who)) return { type: "NOOP" };
		try {
			editQueue(action.type, action.payload);
		} catch {}
		try {
			localReducer(action.type, action.payload);
		} catch {}
		try {
			const reply = callOp("redux.dispatch", { type: action.type, payload: action.payload === undefined ? null : action.payload, plugin: who });
			// The host refused a store write the reducer made (an exclusive
			// output the platform lacks): put the slice's value back.
			if (reply && reply.patch) for (const [slice, patch] of Object.entries(reply.patch)) local[slice] = { ...local[slice], ...patch };
		} catch (err) {
			L.reportError(err);
		}
		invalidate();
		notifySubscribers();
		return action;
	};
	// Host -> plugins: something happened in Tidalfast. Returns false when an
	// interceptor cancelled it.
	// Answers whose payload a plugin may edit in place and the host shows.
	const editableAnswer = (type) => type === "search/SEARCH_RESULT_SUCCESS" && interceptors[type]?.size > 0;
	// TIDAL's reducer keeps the payload object the interceptors saw, so an
	// edit made to it (Hunminjeongeum retitles a song when its MusicBrainz
	// answer arrives, seconds on, and in place at once for a cached one)
	// changes what is shown: the edited payload goes back to the host when
	// it differs, now and for a while after. Only while a plugin intercepts.
	let watched = null;
	const sendEdited = (w) => {
		const now = JSON.stringify(w.payload);
		if (now === w.json) return;
		w.json = now;
		try {
			callOp("redux.dispatch", { type: w.type, payload: w.payload, plugin: null });
		} catch (err) {
			L.reportError(err);
		}
	};
	const watchEdits = (w) => {
		watched = w;
		const again = () => {
			if (watched !== w) return;
			sendEdited(w);
			if (--w.left > 0) setTimeout(again, 700);
			else watched = null;
		};
		setTimeout(again, 700);
	};
	g.__luna_dispatch = (type, json) => {
		invalidate();
		const payload = json === undefined || json === "" ? undefined : JSON.parse(json);
		if (devLog.redux !== null) logRedux(type, payload);
		const watch = editableAnswer(type) && payload !== undefined ? { type, payload, json: JSON.stringify(payload), left: 120 } : null;
		const go = runInterceptors(type, payload);
		if (go)
			try {
				localReducer(type, payload);
			} catch {}
		if (go && watch !== null) {
			sendEdited(watch);
			watchEdits(watch);
		}
		notifySubscribers();
		return go;
	};
	const buildActions = new Proxy(
		{},
		{
			get: (_, type) => (typeof type === "string" ? (payload) => ({ type, payload }) : undefined),
			has: (_, type) => typeof type === "string",
		},
	);
	const reduxStore = {
		getState,
		dispatch,
		subscribe(fn) {
			subscribers.add(fn);
			const plugin = L.pluginFromStack();
			if (plugin !== undefined) subscriberOwners.set(fn, plugin);
			return () => {
				subscriberOwners.delete(fn);
				return subscribers.delete(fn);
			};
		},
		replaceReducer() {},
		[Symbol.observable ?? "@@observable"]() {
			return { subscribe: (o) => ({ unsubscribe: reduxStore.subscribe(() => o.next?.(getState())) }) };
		},
	};
	L.reduxStore = reduxStore;
	L.content = content;
	L.interceptors = interceptors;
	L.invalidateState = invalidate;

	// ---------------------------------------------------------------- oby (minimal)
	const obyStore = (o) => o;
	obyStore.unwrap = unwrap;
	obyStore.on = () => () => {};
	obyStore.reconcile = (a, b) => Object.assign(a, b);
	const $ = (v) => {
		const f = (...a) => (a.length ? (v = typeof a[0] === "function" ? a[0](v) : a[0]) : v);
		return f;
	};
	const oby = { store: obyStore, $, effect: () => () => {}, memo: (f) => f, untrack: (f) => f(), batch: (f) => f(), root: (f) => f(() => {}), default: undefined };
	oby.default = oby;

	// ---------------------------------------------------------------- ipc / native modules
	const ipcListeners = new Map();
	// listener -> [channel, plugin], for the plugins' own listeners.
	const ipcOwners = new Map();
	const ipcOn = (channel, listener, once) => {
		const wrapped = once
			? (...a) => {
					unload();
					return listener(...a);
				}
			: listener;
		if (!ipcListeners.has(channel)) ipcListeners.set(channel, new Set());
		ipcListeners.get(channel).add(wrapped);
		// A listener for the player's signals hears where the song is now,
		// as TIDAL's next signal would tell it (media.js).
		if (channel === "client.playback.playersignal") queueMicrotask(() => L.media?.playerSignalNow?.());
		const plugin = L.pluginFromStack();
		if (plugin !== undefined) ipcOwners.set(wrapped, [channel, plugin]);
		const unload = () => {
			ipcOwners.delete(wrapped);
			return ipcListeners.get(channel)?.delete(wrapped);
		};
		return unload;
	};
	// A stopped plugin's store subscriptions and IPC listeners go with it.
	const dropListenersOf = (plugin) => {
		for (const [fn, p] of [...subscriberOwners])
			if (p === plugin) {
				subscriberOwners.delete(fn);
				subscribers.delete(fn);
			}
		for (const [fn, [channel, p]] of [...ipcOwners])
			if (p === plugin) {
				ipcOwners.delete(fn);
				ipcListeners.get(channel)?.delete(fn);
			}
	};
	const ipcEmit = (channel, ...args) => {
		if (devLog.native) callOp("log", { plugin: null, level: "info", msg: `(IPC Native > Render) ${channel} ${devText(args)}` });
		for (const l of [...(ipcListeners.get(channel) ?? [])]) {
			try {
				l(...args);
			} catch (err) {
				L.reportError(err);
			}
		}
	};
	g.__luna_ipc = (channel, json) => ipcEmit(channel, ...(json ? JSON.parse(json) : []));
	// Whether anything listens on a channel, so signals nobody hears cost nothing.
	L.ipcListening = (channel) => (ipcListeners.get(channel)?.size ?? 0) > 0;
	// The shims' own handles on these entry points (Rust removes the globals).
	L.lunaIpc = g.__luna_ipc;
	L.hostDispatch = g.__luna_dispatch;
	// Message boxes (`confirm`, `showMessageBox`): the app shows them over
	// everything and answers with the button clicked (null: Esc), which the
	// `dialog` host call hands to the waiting promise.
	const dialogWaiters = new Map();
	// A permission the user gives (the TIDAL sign-in, system-wide
	// shortcuts, another program's pipe, saving TIDAL audio): resolves once
	// allowed (at once when it already is), rejects with EACCES when
	// denied. The question is asked by the app (permits.rs); `plugin` is
	// taken by the caller before any await, and Rust checks it.
	L.permit = (access) => callOpAsync("permit", { ...access, plugin: access?.plugin ?? null });
	L.askUser = ({ title = "", message = "", detail = "", buttons, defaultId = 0, cancelId }) =>
		new Promise((resolve) => {
			const list = Array.isArray(buttons) && buttons.length ? buttons.map(String) : ["OK"];
			// Esc: the button asked for, else the first one called "cancel" or "no", else the first.
			const named = list.findIndex((b) => /^(cancel|no)$/i.test(b.trim()));
			const cancel = Number.isInteger(cancelId) ? cancelId : named === -1 ? 0 : named;
			const id = callOp("dialog.ask", { title: String(title), message: String(message), detail: String(detail), buttons: list, defaultId });
			// Too many dialogs open: this one is answered as cancelled.
			if (id === null || id === undefined) return resolve({ response: cancel, escaped: true });
			dialogWaiters.set(id, (button) => resolve({ response: button ?? cancel, escaped: button === null || button === undefined }));
		});
	// Asked before a plugin's account write (see writes.rs): "ask" opens a
	// dialog (one at a time for a plugin; the writes that follow wait for the
	// answer) and the answer is kept for a minute, or for good.
	const writeAsks = new Map();
	const describeWrite = (method, url) => {
		const p = new URL(url).pathname.replace(/^\/v1\//, "").split("/");
		if (p[0] === "playlists") {
			if (p[2] === "items") return method === "DELETE" ? "remove songs from a playlist" : "add songs to a playlist";
			return method === "DELETE" ? "delete a playlist" : "change a playlist";
		}
		if (p[0] === "users" && p[2] === "favorites") return method === "DELETE" ? "remove songs, albums or artists from your favourites" : "add to your favourites";
		if (p[0] === "users" && p[2] === "playlists") return "create or change a playlist";
		return "change your account";
	};
	L.allowAccountWrite = (plugin, method, url) => {
		if (!plugin) return Promise.resolve(false);
		const state = callOp("account_write.state", { plugin });
		if (state === "allowed") return Promise.resolve(true);
		if (state === "denied") return Promise.resolve(false);
		let ask = writeAsks.get(plugin);
		if (!ask) {
			ask = L.askUser({
				title: "Change your TIDAL account?",
				message: `${plugin} wants to ${describeWrite(method, url)}.`,
				detail: "Allow for a minute covers what it does next, Always allow is kept until you take it back on the Plugins page.",
				buttons: ["Deny", "Allow for a minute", "Always allow"],
				defaultId: 0,
				cancelId: 0,
			}).then(({ response }) => {
				writeAsks.delete(plugin);
				callOp("account_write.answer", { plugin, allow: response > 0, always: response === 2 });
				return response > 0;
			});
			writeAsks.set(plugin, ask);
		}
		return ask;
	};
	L.dialogAnswer = ({ id, button }) => {
		const waiter = dialogWaiters.get(id);
		dialogWaiters.delete(id);
		waiter?.(button);
	};
	// Tidalfast has no system file dialog: the plugin is told so once, and the
	// dialog counts as cancelled.
	const fileDialogWarned = new Set();
	const noFileDialog = (name) => {
		const plugin = L.pluginFromStack() ?? "";
		if (fileDialogWarned.has(plugin)) return;
		fileDialogWarned.add(plugin);
		coreTrace.warn(name, "file dialogs are not available in Tidalfast; the dialog is reported as cancelled");
	};
	L.noFileDialog = noFileDialog;
	// Electron's dialog.showOpenDialog([window,] options) / showSaveDialog:
	// the host's native dialog; the plugin's `fs` can then reach the pick.
	L.fileDialog = async (kind, a, b) => {
		const o = (b !== undefined ? b : a) ?? {};
		const r = await callOpAsync("file_dialog", { kind, title: o.title, defaultPath: o.defaultPath, filters: o.filters, properties: o.properties, content: typeof o.__content === "string" ? o.__content : undefined });
		return r ?? (kind === "save" ? { canceled: true, filePath: "" } : { canceled: true, filePaths: [] });
	};
	const nativeExports = {};
	const libNative = {
		pkg: { name: "luna", version: g.__lunaVersions.luna },
		update: async () => {},
		relaunch: async () => {},
		needsElevation: async () => false,
		runElevatedInstall: async () => {},
		sendToRender: (ch, ...a) => ipcEmit(ch, ...a),
		showOpenDialog: (a, b) => L.fileDialog("open", a, b),
		showSaveDialog: (a, b) => L.fileDialog("save", a, b),
		// Electron's dialog.showMessageBox([window,] options): a dialog over the app.
		showMessageBox: async (a, b) => {
			const o = b !== undefined ? b : a;
			const { response } = await L.askUser({ title: o?.title, message: o?.message, detail: o?.detail, buttons: o?.buttons, defaultId: o?.defaultId, cancelId: o?.cancelId });
			return { response, checkboxChecked: false };
		},
		// Electron's dialog.showErrorBox(title, content): a modal with one OK
		// button, over the app; it returns nothing and the plugin goes on.
		showErrorBox: (title, content) => void L.askUser({ title: String(title ?? "Error"), message: String(content ?? ""), buttons: ["OK"] }),
		clipboardWriteText: async (text) => callOp("clipboard", { text: String(text) }),
		openExternal: async (url) => callOp("open_url", { url: String(url) }),
		// A track's TIDAL audio as a Node Readable. Its bytes go only to a
		// plugin the user allowed to save TIDAL audio (asked when it first
		// reads them); a stream consumed only by sinks that drop every chunk
		// is read to its end by the app instead, and asks nothing (see
		// `AudioStream` in node.js). Only an unencrypted stream; the plugin
		// never sees a stream URL.
		fetchMediaItemStream: async (playbackInfo, opts) => {
			const plugin = L.pluginFromStack() ?? L.running ?? null;
			const id = playbackInfo?.trackId ?? playbackInfo?.id ?? playbackInfo?.mediaItemId;
			if (id === undefined || id === null) throw new Error("fetchMediaItemStream needs a track's playback info");
			const track = { plugin, id: String(id), quality: playbackInfo?.audioQuality ?? null };
			const source = {
				open: async () => {
					await L.permit({ plugin, kind: "save_audio" });
					return await callOpAsync("audio.open", track);
				},
				drain: () => callOpAsync("audio.void", track),
			};
			return L.node.audioStream(plugin, source, opts);
		},
	};
	nativeExports["@luna/lib.native"] = libNative;
	// The native file name is the plugin's source path, which is not always
	// its package name ("@scope/name/x.native.ts", "plugins/Utils/..."); the
	// owner is the plugin asking to register it, read from the live frames
	// (or the plugin whose timer is running). Never the file name: code
	// would run, files and all, as whichever plugin the name says.
	const nativeOwner = () => {
		const caller = L.pluginFromStack() ?? L.running;
		if (caller) return caller;
		throw new Error("Native code can only be registered by a plugin's own code");
	};
	const registerNative = async (fileName, code) => {
		const plugin = nativeOwner();
		const exports = await L.node.loadNative(plugin, fileName, code, nativeExports);
		nativeExports[fileName] = exports;
		const channel = `__LunaNative.${fileName}`;
		return channel;
	};
	g.__ipcRenderer = {
		async invoke(channel, ...args) {
			if (devLog.render) callOp("log", { plugin: null, level: "info", msg: `(IPC Render > Native) ${channel} ${channel === "__Luna.registerNative" ? String(args[0]) : devText(args)}` });
			if (channel === "__Luna.registerNative") return registerNative(args[0], args[1]);
			if (typeof channel === "string" && channel.startsWith("__LunaNative.")) {
				const fileName = channel.slice("__LunaNative.".length);
				const [exportName, ...rest] = args;
				const exp = nativeExports[fileName];
				if (exp === undefined) throw new Error(`Native module ${fileName} is not registered`);
				const fn = exp[exportName];
				if (typeof fn !== "function") throw new Error(`[Luna.native] (${fileName}).${exportName} is not a function`);
				return await fn(...rest);
			}
			coreTrace.warn("ipcRenderer.invoke", `Unsupported channel ${channel}`);
			return undefined;
		},
		send(channel, ...args) {
			if (devLog.render) callOp("log", { plugin: null, level: "info", msg: `(IPC Render > Native) ${channel} ${devText(args)}` });
			coreTrace.debug("ipcRenderer.send", channel);
		},
		on: (channel, listener) => ipcOn(channel, listener, false),
		once: (channel, listener) => ipcOn(channel, listener, true),
	};

	// ---------------------------------------------------------------- unknown APIs
	// A plugin built against a newer @luna/* may read an export this shim does
	// not have yet. Instead of `undefined` (a TypeError at the first call), it
	// gets an inert stand-in and the plugin's log says what is missing, once.
	// `in` checks, Object.keys and the shim's own lookups are unchanged.
	const passThrough = new Set(["then", "default", "__esModule", "toJSON", "constructor", "prototype", "$$typeof", "displayName", "name", "length", "nodeType", "inspect", "asymmetricMatch"]);
	const warned = new Set();
	const warnMissing = (plugin, path) => {
		const key = `${plugin}|${path}`;
		if (warned.has(key)) return;
		warned.add(key);
		callOp("log", { plugin, level: "warn", msg: `${path} is not available in Tidalfast's Luna (API ${g.__lunaVersions.luna}); it does nothing here` });
	};
	const standIn = (plugin, path) =>
		new Proxy(function () {}, {
			get: (_t, k) => (typeof k !== "string" || passThrough.has(k) ? undefined : standIn(plugin, `${path}.${k}`)),
			apply: () => void warnMissing(plugin, `${path}()`),
			construct: () => (warnMissing(plugin, `new ${path}()`), {}),
		});
	L.guardModule = (name, obj) =>
		new Proxy(obj, {
			get(t, k, r) {
				if (typeof k !== "string" || k in t || passThrough.has(k)) return Reflect.get(t, k, r);
				const plugin = L.pluginFromStack();
				if (plugin === undefined) return undefined;
				warnMissing(plugin, `${name}.${k}`);
				return standIn(plugin, `${name}.${k}`);
			},
		});
	L.missingApis = () => [...warned];

	// ---------------------------------------------------------------- plugins
	const modules = {};
	g.require = (name) => {
		if (Object.prototype.hasOwnProperty.call(modules, name)) return modules[name];
		throw new Error(`Dynamic require called for '${name}' does not exist in core.modules!`);
	};
	g.require.cache = modules;
	g.require.main = undefined;
	class LunaPlugin {
		static plugins = {};
		static corePlugins = new Set(["@luna/lib", "@luna/lib.native", "@luna/ui", "@luna/dev", "@luna/linux"]);
		static trace = coreTrace.withSource(".LunaPlugin").trace;
		// Prefers the installed copy of a name (a store build and a dev
		// build can both be listed).
		static getByName(name) {
			const all = this.getAllByName(name);
			return all.find((p) => p.installed) ?? all[0];
		}
		static getAllByName(name) {
			return Object.values(this.plugins).filter((p) => p.name === name);
		}
		// Luna's plugin storage. Tidalfast keeps its plugin records itself;
		// this store is the plugin-visible one, as a ReactiveStore.
		static get pluginStorage() {
			return ReactiveStore.getStore("@luna/plugins");
		}
		static fetchPackage(url) {
			return fetch(`${url}.json`).then((r) => {
				if (!r.ok) throw new Error(`Failed to fetch package.json for ${url}: ${r.status} ${r.statusText}`);
				return r.json();
			});
		}
		static fetchCode(url) {
			return fetch(`${url}.mjs`).then((r) => {
				if (!r.ok) throw new Error(`Failed to fetch code for ${url}: ${r.status} ${r.statusText}`);
				return r.text();
			});
		}
		static async fromName(name) {
			return this.getByName(name);
		}
		// An installed plugin for that URL, or one shown but not installed
		// (installing goes through Tidalfast's Plugin Store, where the user
		// sees the plugin's permissions first).
		static async fromStorage(init) {
			const url = String(init?.url ?? "").replace(/(\.mjs|\.json|\.mjs\.map)$/, "");
			if (this.plugins[url]) return this.plugins[url];
			const pkg = init.package ?? (await this.fetchPackage(url));
			const plugin = new this(pkg.name, url, pkg);
			plugin.store.installed = false;
			plugin.store.enabled = false;
			return (this.plugins[url] = plugin);
		}
		static async loadStoredPlugins() {
			return Object.values(this.plugins).filter((p) => p.installed);
		}
		static async dumpStorage(stripCode = true) {
			const out = {};
			for (const p of Object.values(this.plugins)) {
				if (!p.installed) continue;
				const store = structuredClone({ ...p.store, package: p.store.package ?? undefined });
				if (stripCode) delete store.package?.code;
				out[p.name] = store;
			}
			return out;
		}
		constructor(name, url, pkg) {
			this.name = name;
			this.store = { url, package: pkg, enabled: true, installed: true, liveReload: false, hideSettings: false };
			this.trace = LunaPlugin.trace.withSource(`[${name}]`).trace;
			this.dependants = new Set();
			this.loadError = new Signal(undefined);
			this.loading = new Signal(false);
			this.fetching = new Signal(false);
			this._liveReload = new Signal(false);
		}
		get code() {
			return this.store.package?.code;
		}
		get isDev() {
			return /^(file:|https?:\/\/(127\.0\.0\.1|localhost|\[::1\])([:/]|$))/.test(this.url ?? "") || !/^https?:/.test(this.url ?? "");
		}
		get liveReload() {
			return this._liveReload._;
		}
		set liveReload(v) {
			this._liveReload._ = !!v;
		}
		// Enable, disable, reload and uninstall go to Tidalfast's plugin
		// manager, as the Plugins page's switches do. A plugin may uninstall
		// only itself.
		control(action) {
			if (!this.installed) throw new Error(`${this.name} is not installed`);
			// Who asks is read by Rust from the live frames.
			callOp("plugin.control", { name: this.name, action });
		}
		async load() {
			return this;
		}
		async enable() {
			this.store.enabled = true;
			this.control("enable");
		}
		async disable() {
			this.store.enabled = false;
			this.control("disable");
		}
		async reload() {
			this.control("reload");
		}
		// Asks the user (a dialog over the app that names the plugin that
		// asks, the plugin and its address); the app installs it itself when
		// they agree. Resolves once the plugin is installed and loading.
		async install() {
			if (this.installed) return this;
			const id = callOp("plugin.install", { url: this.url, name: this.name });
			if (id === null || id === undefined) return this;
			const answer = await new Promise((resolve) => dialogWaiters.set(id, resolve));
			if (answer !== 0) throw new Error(`Installing ${this.name} was not allowed`);
			if (this.installed) return this;
			await new Promise((resolve, reject) => {
				const timer = setTimeout(() => {
					this._installWait = undefined;
					reject(new Error(`${this.name} did not finish installing; see the Plugins page`));
				}, 120000);
				this._installWait = () => {
					clearTimeout(timer);
					this._installWait = undefined;
					resolve();
				};
			});
			return this;
		}
		async uninstall() {
			this.control("uninstall");
		}
		get url() {
			return this.store.url;
		}
		get package() {
			return this.store.package;
		}
		get installed() {
			return this.store.installed !== false;
		}
		get enabled() {
			return this.store.enabled;
		}
		get exports() {
			return modules[this.name];
		}
		addDependant(plugin) {
			if (!(plugin instanceof LunaPlugin)) throw new Error("Cannot add dependancy to non plugin!");
			this.dependants.add(plugin);
		}
		onSetEnabled() {
			return () => {};
		}
		onSetLiveReload() {
			return () => {};
		}
	}
	// Luna's settings export format (`ExportData`, version 1): the plugin
	// settings, the plugin records and Luna's own storage.
	class SettingsTransfer {
		static exportableStores() {
			return [ReactiveStore.getStore("@luna/pluginStorage"), LunaPlugin.pluginStorage, ReactiveStore.getStore("@luna/storage")];
		}
		static async dump(stripCode = true, featureFlags = null) {
			const stores = {};
			for (const store of this.exportableStores()) stores[store.idbName] = store === LunaPlugin.pluginStorage ? await LunaPlugin.dumpStorage(stripCode) : await store.dump();
			return { version: 1, timestamp: new Date().toISOString(), stores, featureFlags };
		}
		static async restore(data) {
			for (const store of this.exportableStores()) {
				const storeData = data.stores[store.idbName];
				if (!storeData) continue;
				// Plugins are installed from Tidalfast's Plugin Store and
				// Settings Transfer, where their permissions are shown.
				if (store === LunaPlugin.pluginStorage) {
					coreTrace.warn("SettingsTransfer.restore", "Plugin records are restored from Settings > Settings Transfer, not by a plugin; skipped");
					continue;
				}
				await store.clear();
				for (const [key, value] of Object.entries(storeData)) await store.set(key, value);
			}
		}
		static validate(data) {
			if (typeof data !== "object" || data === null) return false;
			if (data.version !== 1) return false;
			if (typeof data.stores !== "object" || data.stores === null) return false;
			return true;
		}
	}
	const status = (name, state, extra = {}) => callOp("plugin.status", { ...extra, name, state, plugin: name });
	const settingsRoots = {};
	const renderSettings = (name) => {
		const root = settingsRoots[name];
		if (!root) return;
		try {
			root.render();
		} catch (err) {
			callOp("plugin.settings", { name, plugin: name, tree: { kind: "group", children: [{ kind: "text", text: `Settings failed to render: ${err?.message ?? err}` }] } });
		}
	};
	// The load each plugin is on: its module specifier ("plugin:<name>@<gen>").
	// A newer load (reload, update) replaces it; disabling or uninstalling
	// deletes it. A load that finds it changed once its import settles is
	// stale, and that instance must not run.
	const loadTokens = {};
	// A stale instance: its own unloads run; when nothing replaced it (it was
	// disabled or uninstalled while loading), what it left behind goes too.
	const discardStale = async (name, ns, cancelled) => {
		const unloads = ns?.unloads instanceof Set ? ns.unloads : new Set();
		if (typeof ns?.onUnload === "function") unloads.add(ns.onUnload);
		await unloadSet(unloads).catch(L.reportError);
		if (cancelled && loadTokens[name] === undefined) sweepPlugin(name);
	};
	const loadPlugin = async ({ name, spec, url, pkg }) => {
		loadTokens[name] = spec;
		const current = () => loadTokens[name] === spec;
		const plugin = (LunaPlugin.plugins[url] ??= new LunaPlugin(name, url, pkg));
		plugin.store.package = pkg;
		plugin.store.enabled = true;
		plugin.store.installed = true;
		plugin._installWait?.();
		plugin.loadError._ = undefined;
		try {
			await unloadPlugin({ name, quiet: true });
			if (!current()) return;
			const ns = await import(spec);
			if (!current()) return void (await discardStale(name, ns, loadTokens[name] === undefined));
			modules[name] = ns;
			const unloads = ns.unloads instanceof Set ? ns.unloads : (plugin._unloads = new Set());
			if (typeof ns.onUnload === "function") unloads.add(ns.onUnload);
			if (ns.errSignal && typeof ns.errSignal.onValue === "function") {
				unloads.add(
					ns.errSignal.onValue((next) => {
						plugin.loadError._ = next;
						status(name, "message", { error: next === undefined ? null : String(next) });
					}),
				);
				if (ns.errSignal._ !== undefined) status(name, "message", { error: String(ns.errSignal._) });
			}
			plugin.unloads = unloads;
			if (typeof ns.Settings === "function" || (ns.Settings && typeof ns.Settings === "object")) {
				const root = new L.SettingsRoot(name, ns.Settings);
				root.onTree = (tree) => callOp("plugin.settings", { name, plugin: name, tree });
				settingsRoots[name] = root;
			}
			status(name, "loaded", { hasSettings: !!settingsRoots[name] });
			renderSettings(name);
		} catch (err) {
			// A stale load's failure (its code was replaced meanwhile) is no
			// failure of the plugin.
			if (!current()) return;
			const msg = err?.message ?? String(err);
			plugin.loadError._ = msg;
			await unloadPlugin({ name, quiet: true });
			status(name, "error", { error: msg, stack: err?.stack ?? null });
		}
	};
	const unloadPlugin = async ({ name, quiet }) => {
		const plugin = LunaPlugin.getByName(name);
		const ns = modules[name];
		settingsRoots[name]?.unmount();
		delete settingsRoots[name];
		for (const dep of plugin?.dependants ?? []) if (dep.name !== name && modules[dep.name]) await unloadPlugin({ name: dep.name });
		if (ns || plugin?.unloads) await unloadSet(plugin?.unloads ?? ns?.unloads);
		sweepPlugin(name);
		delete modules[name];
		if (plugin) plugin.unloads = undefined;
		if (!quiet) status(name, "unloaded");
	};
	// Safety net: anything the plugin left behind.
	const sweepPlugin = (name) => {
		for (const type of Object.keys(interceptors)) {
			for (const cb of [...interceptors[type]]) if (cb.__lunaPlugin === name) interceptors[type].delete(cb);
			if (interceptors[type].size === 0) delete interceptors[type];
		}
		L.clearTimersOf(name);
		dropListenersOf(name);
		L.contextMenuDrop?.(name);
		L.node?.closeAll?.(name);
		L.node?.dropShortcuts?.(name);
		L.domFlagReset(name);
	};
	const themeTags = {};
	g.__luna_host = (kind, json) => {
		const arg = json ? JSON.parse(json) : {};
		switch (kind) {
			case "load":
				return void loadPlugin(arg);
			case "unload":
				// A load still in flight is cancelled with it.
				delete loadTokens[arg.name];
				return void unloadPlugin(arg).catch(L.reportError);
			case "forget_storage": {
				// A removed plugin's settings and stores are deleted on disk:
				// the objects opened from them (and a save waiting to run)
				// must not bring them back.
				for (const [name, keys] of [["@luna/pluginStorage", arg.settings ?? []], ...(arg.stores ?? []).map((s) => [s, null])]) {
					const store = ReactiveStore.Storages[name];
					if (!store) continue;
					for (const key of keys ?? Object.keys(store.reactiveCache)) {
						delete store.reactiveCache[key];
						if (store.saveTimers[key]) {
							clearTimeout(store.saveTimers[key]);
							delete store.saveTimers[key];
						}
					}
				}
				return;
			}
			case "render_settings":
				return void renderSettings(arg.name);
			case "settings_input": {
				const root = settingsRoots[arg.name];
				if (!root) throw new Error(`${arg.name} has no settings`);
				const r = root.input(arg.id, arg.value);
				if (r instanceof Promise) r.catch(L.reportError).finally(() => root.invalidate());
				root.invalidate();
				return;
			}
			case "theme": {
				// Luna's ThemesTab: one <style> per theme in the document.
				let el = themeTags[arg.url];
				if (arg.css === null || arg.css === undefined || String(arg.css).trim() === "") {
					el?.remove();
					delete themeTags[arg.url];
					return;
				}
				if (!el) {
					el = themeTags[arg.url] = document.createElement("style");
					el.setAttribute("data-luna-theme", String(arg.url));
				}
				el.innerHTML = String(arg.css);
				if (!el.isConnected) document.head.appendChild(el);
				return;
			}
			case "dev_log": {
				devLog.native = !!arg.native;
				devLog.render = !!arg.render;
				devLog.redux = null;
				if (typeof arg.redux === "string")
					try {
						devLog.redux = new RegExp(arg.redux || ".*");
					} catch (err) {
						callOp("log", { plugin: null, level: "error", msg: `Failed to start redux event logging: ${err?.message ?? err}` });
					}
				return;
			}
			case "visibility":
				return void L.dom.setHidden(arg.hidden);
			case "editor":
				return void L.node.editorEvent(arg);
			case "focus":
				L.setTimersFocused(arg.focused);
				return void L.dom.setFocused(arg.focused);
			case "context_menu":
				return void L.contextMenuOpen(arg);
			case "context_click":
				return void L.contextMenuClick(arg);
			case "dialog":
				return void L.dialogAnswer(arg);
			case "shortcut":
				return void L.node.globalShortcutFired(arg);
			default:
				throw new Error(`Unknown host call ${kind}`);
		}
	};

	const core = {
		modules,
		LunaPlugin,
		ReactiveStore,
		Tracer,
		Messager,
		Signal,
		ftch: {
			json: (...a) =>
				fetch(...a).then((r) => {
					if (!r.ok) throw new Error(`Status code is ${r.status}`);
					return r.json();
				}),
			text: (...a) =>
				fetch(...a).then((r) => {
					if (!r.ok) throw new Error(`Status code is ${r.status}`);
					return r.text();
				}),
		},
		unloadSet,
		findModuleByProperty,
		findModuleProperty,
		recursiveSearch,
		reduxStore,
		buildActions,
		interceptors,
		tidalModules,
		SettingsTransfer,
	};
	core.default = core;
	modules["@luna/core"] = L.guardModule("@luna/core", core);
	modules["oby"] = oby;
	modules["react"] = L.react.React;
	modules["react/jsx-runtime"] = L.react.jsxRuntime;
	modules["react/jsx-dev-runtime"] = L.react.jsxRuntime;
	modules["react-dom"] = L.react.reactDom;
	modules["react-dom/client"] = L.react.reactDomClient;
	modules["@luna/ui"] = L.guardModule("@luna/ui", L.ui);
	modules["@mui/material"] = L.mui;
	modules["@luna/lib.native"] = L.guardModule("@luna/lib.native", libNative);
	g.luna = { core, modules };
	// What `.native` code finds on `globalThis.luna` in Luna's main process.
	Object.defineProperty(g.luna, "tidalWindow", { get: () => L.node?.tidalWindow, enumerable: false });
	for (const k of ["lib", "ui", "native", "dev"]) {
		const name = k === "native" ? "@luna/lib.native" : `@luna/${k}`;
		Object.defineProperty(g.luna, k, { get: () => modules[name], enumerable: true });
	}
	L.core = core;
	L.Signal = Signal;
	L.Tracer = Tracer;
	L.unloadSet = unloadSet;
})();
