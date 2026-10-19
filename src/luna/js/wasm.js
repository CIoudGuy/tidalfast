// The WebAssembly JS API for plugins, on the wasmi engine in src/luna/wasm.rs
// (the `__wasm` natives). Values are coerced here (ToInt32, ToBigInt64, ...)
// so the natives only see well-typed numbers, BigInts and handles:
// `{s, i}` names a wasm object (store, index), `{x}` an externref.
"use strict";
(() => {
	const g = globalThis;
	const W = g.__wasm;
	delete g.__wasm;
	const internal = g.__lunaInternal;
	const MARK = "\u0001wasm:";
	const REF = Symbol("wasm.ref");
	const TYPE = Symbol("wasm.type");
	const MOD = Symbol("wasm.module");

	class CompileError extends Error {
		constructor(m) {
			super(m);
			this.name = "CompileError";
		}
	}
	class LinkError extends Error {
		constructor(m) {
			super(m);
			this.name = "LinkError";
		}
	}
	class RuntimeError extends Error {
		constructor(m) {
			super(m);
			this.name = "RuntimeError";
		}
	}
	const ERRORS = { CompileError, LinkError, RuntimeError, TypeError, RangeError, Error };
	// Native errors carry their class in the message; real JS exceptions
	// (thrown by imported functions) pass through untouched.
	const rethrow = (e) => {
		if (e instanceof Error && typeof e.message === "string" && e.message.startsWith(MARK)) {
			const rest = e.message.slice(MARK.length);
			const i = rest.indexOf(":");
			const kind = rest.slice(0, i);
			if (kind === "InternalError") throw e;
			throw new (ERRORS[kind] ?? Error)(rest.slice(i + 1));
		}
		throw e;
	};
	const guard = (f) => {
		try {
			return f();
		} catch (e) {
			rethrow(e);
		}
	};
	const owner = () => internal.pluginFromStack() ?? "";

	const bytesOf = (src) => {
		if (src instanceof ArrayBuffer) return new Uint8Array(src);
		if (ArrayBuffer.isView(src)) return new Uint8Array(src.buffer, src.byteOffset, src.byteLength);
		throw new TypeError("WebAssembly: argument must be an ArrayBuffer or a typed array");
	};

	// externref values: JS keeps them, wasm holds ids.
	const externs = new Map();
	const externIds = new Map();
	const externId = (v) => {
		let id = externIds.get(v);
		if (id === undefined) {
			id = externIds.size + 1;
			externIds.set(v, id);
			externs.set(id, v);
		}
		return id;
	};

	const toWasm = (t, v) => {
		switch (t) {
			case "i32":
				return v | 0;
			case "i64":
				if (typeof v === "number") throw new TypeError("WebAssembly: cannot convert a Number to a BigInt (i64)");
				return BigInt.asIntN(64, BigInt(v));
			case "f32":
				return Math.fround(+v);
			case "f64":
				return +v;
			case "anyfunc":
			case "funcref": {
				if (v === null) return null;
				const r = typeof v === "function" ? v[REF] : undefined;
				if (r === undefined) throw new TypeError("WebAssembly: expected an exported WebAssembly function or null");
				return r;
			}
			case "externref":
				return v === null ? null : externId(v);
			default:
				throw new TypeError(`WebAssembly: ${t} values cannot cross into JavaScript`);
		}
	};
	const fromWasm = (v) => {
		if (v === null || typeof v !== "object") return v;
		if ("x" in v) return externs.get(v.x);
		return wrapFunc(v.s, v.i);
	};

	const defaultFor = (t) => (t === "i64" ? 0n : t === "externref" ? undefined : t === "anyfunc" || t === "funcref" ? null : 0);

	// Exported functions: one JS function per wasm function handle.
	const funcs = new Map();
	const wrapFunc = (s, i, type) => {
		const key = `${s}:${i}`;
		let f = funcs.get(key);
		if (f !== undefined) return f;
		const { params, results } = type ?? guard(() => W.funcType(s, i));
		f = function (...args) {
			const a = params.map((t, k) => toWasm(t, args[k]));
			const r = guard(() => W.call(s, i, a));
			return results.length > 1 ? r.map(fromWasm) : fromWasm(r);
		};
		Object.defineProperty(f, "name", { value: String(i) });
		Object.defineProperty(f, "length", { value: params.length });
		Object.defineProperty(f, REF, { value: { s, i } });
		funcs.set(key, f);
		return f;
	};

	// Imported JS functions, by id.
	const hostFns = [];
	const hostIds = new Map();
	const hostId = (fn) => {
		let id = hostIds.get(fn);
		if (id === undefined) {
			id = hostFns.push(fn) - 1;
			hostIds.set(fn, id);
		}
		return id;
	};
	W.setHost((id, args, results) => {
		const r = hostFns[id](...args.map(fromWasm));
		if (results.length === 0) return [];
		if (results.length === 1) return [toWasm(results[0], r)];
		const all = Array.from(r);
		if (all.length !== results.length) throw new TypeError("WebAssembly: imported function returned the wrong number of results");
		return all.map((v, k) => toWasm(results[k], v));
	});

	const objects = new Map();
	const wrapObject = (C, h, type) => {
		const key = `${h.s}:${h.i}`;
		let o = objects.get(key);
		if (o === undefined) {
			o = Object.create(C.prototype);
			Object.defineProperty(o, REF, { value: { s: h.s, i: h.i } });
			Object.defineProperty(o, TYPE, { value: type });
			objects.set(key, o);
		}
		return o;
	};
	const index = (v, what) => {
		const n = Number(v);
		if (!Number.isFinite(n) || n < 0 || n > 0xffffffff) throw new TypeError(`WebAssembly: ${what} must be a non-negative integer`);
		return Math.floor(n);
	};
	const refOf = (o, C) => {
		const r = o?.[REF];
		if (r === undefined || !(o instanceof C)) throw new TypeError(`WebAssembly: receiver is not a WebAssembly.${C.name}`);
		return r;
	};

	class Module {
		constructor(bytes) {
			if (new.target === undefined) throw new TypeError("WebAssembly.Module must be called with new");
			newModule(this, bytes, owner());
		}
		static imports(m) {
			return guard(() => W.imports(modId(m))).map(({ module, name, kind }) => ({ module, name, kind }));
		}
		static exports(m) {
			return guard(() => W.exports(modId(m))).map(({ name, kind }) => ({ name, kind }));
		}
		static customSections(m, name) {
			if (name === undefined) throw new TypeError("WebAssembly.Module.customSections: name is required");
			return guard(() => W.customSections(modId(m), String(name)));
		}
		get [Symbol.toStringTag]() {
			return "WebAssembly.Module";
		}
	}
	const newModule = (m, bytes, own) => {
		const u = bytesOf(bytes);
		Object.defineProperty(m, MOD, { value: guard(() => W.compile(own, u.buffer, u.byteOffset, u.byteLength)) });
		return m;
	};
	const modId = (m) => {
		const id = m?.[MOD];
		if (id === undefined) throw new TypeError("WebAssembly: argument is not a WebAssembly.Module");
		return id;
	};

	const makeExtern = (e) => {
		switch (e.kind) {
			case "function":
				return wrapFunc(e.s, e.i, e.type);
			case "memory":
				return wrapObject(Memory, e, e.type);
			case "table":
				return wrapObject(Table, e, e.type);
			default:
				return wrapObject(Global, e, e.type);
		}
	};

	class Instance {
		constructor(module, importObject) {
			if (new.target === undefined) throw new TypeError("WebAssembly.Instance must be called with new");
			newInstance(this, module, importObject, owner());
		}
		get [Symbol.toStringTag]() {
			return "WebAssembly.Instance";
		}
	}
	const newInstance = (inst, module, importObject, own) => {
		const id = modId(module);
		const desc = guard(() => W.imports(id));
		if (desc.length > 0 && (importObject === null || typeof importObject !== "object")) {
			throw new TypeError("WebAssembly.Instance: imports argument must be present and must be an object");
		}
		const list = desc.map(({ module: m, name, kind, type }) => {
			const ns = importObject[m];
			if (ns === null || (typeof ns !== "object" && typeof ns !== "function")) {
				throw new TypeError(`WebAssembly.Instance: import module "${m}" is not an object`);
			}
			const v = ns[name];
			const where = `import ${m}.${name}`;
			switch (kind) {
				case "function":
					if (typeof v !== "function") throw new LinkError(`${where} must be a function`);
					return v[REF] ?? { h: hostId(v) };
				case "memory":
					if (!(v instanceof Memory)) throw new LinkError(`${where} must be a WebAssembly.Memory`);
					return v[REF];
				case "table":
					if (!(v instanceof Table)) throw new LinkError(`${where} must be a WebAssembly.Table`);
					return v[REF];
				default:
					if (v instanceof Global) return v[REF];
					if (type.value === "i64" ? typeof v === "bigint" : typeof v === "number" || type.value === "externref" || type.value === "anyfunc") {
						return { v: toWasm(type.value, v) };
					}
					throw new LinkError(`${where} must be a WebAssembly.Global or a ${type.value === "i64" ? "BigInt" : "Number"}`);
			}
		});
		const r = guard(() => W.instantiate(own, id, list));
		const exports = Object.create(null);
		for (const e of r.exports) exports[e.name] = makeExtern(e);
		Object.freeze(exports);
		Object.defineProperty(inst, "exports", { value: exports, enumerable: true });
		return inst;
	};

	class Memory {
		constructor(desc) {
			if (new.target === undefined) throw new TypeError("WebAssembly.Memory must be called with new");
			if (desc === null || typeof desc !== "object") throw new TypeError("WebAssembly.Memory: descriptor must be an object");
			if (desc.shared) throw new TypeError("WebAssembly.Memory: shared memory is not supported");
			const initial = index(desc.initial ?? desc.minimum, "initial");
			const maximum = desc.maximum === undefined ? undefined : index(desc.maximum, "maximum");
			const h = guard(() => W.memNew(owner(), initial, maximum));
			Object.defineProperty(this, REF, { value: h });
			Object.defineProperty(this, TYPE, { value: { minimum: initial, maximum } });
			objects.set(`${h.s}:${h.i}`, this);
		}
		get buffer() {
			const { s, i } = refOf(this, Memory);
			return guard(() => W.memBuffer(s, i));
		}
		grow(delta) {
			const { s, i } = refOf(this, Memory);
			return guard(() => W.memGrow(s, i, index(delta, "delta")));
		}
		type() {
			return { ...this[TYPE] };
		}
		get [Symbol.toStringTag]() {
			return "WebAssembly.Memory";
		}
	}

	const elementOf = (t) => (t.element === "externref" ? "externref" : "anyfunc");
	class Table {
		constructor(desc, value) {
			if (new.target === undefined) throw new TypeError("WebAssembly.Table must be called with new");
			if (desc === null || typeof desc !== "object") throw new TypeError("WebAssembly.Table: descriptor must be an object");
			const element = desc.element === "funcref" ? "anyfunc" : desc.element;
			if (element !== "anyfunc" && element !== "externref") throw new TypeError('WebAssembly.Table: element must be "anyfunc" or "externref"');
			const initial = index(desc.initial ?? desc.minimum, "initial");
			const maximum = desc.maximum === undefined ? undefined : index(desc.maximum, "maximum");
			const init = toWasm(element, value === undefined ? defaultFor(element) : value);
			const h = guard(() => W.tableNew(owner(), element, initial, maximum, init));
			Object.defineProperty(this, REF, { value: h });
			Object.defineProperty(this, TYPE, { value: { element, minimum: initial, maximum } });
			objects.set(`${h.s}:${h.i}`, this);
		}
		get length() {
			const { s, i } = refOf(this, Table);
			return guard(() => W.tableSize(s, i));
		}
		get(n) {
			const { s, i } = refOf(this, Table);
			return fromWasm(guard(() => W.tableGet(s, i, index(n, "index"))));
		}
		set(n, value) {
			const { s, i } = refOf(this, Table);
			const t = elementOf(this[TYPE]);
			const v = toWasm(t, arguments.length < 2 ? defaultFor(t) : value);
			guard(() => W.tableSet(s, i, index(n, "index"), v));
		}
		grow(delta, value) {
			const { s, i } = refOf(this, Table);
			const t = elementOf(this[TYPE]);
			const v = toWasm(t, value === undefined ? defaultFor(t) : value);
			return guard(() => W.tableGrow(s, i, index(delta, "delta"), v));
		}
		type() {
			return { ...this[TYPE] };
		}
		get [Symbol.toStringTag]() {
			return "WebAssembly.Table";
		}
	}

	const VALUE_TYPES = ["i32", "i64", "f32", "f64", "anyfunc", "funcref", "externref"];
	class Global {
		constructor(desc, value) {
			if (new.target === undefined) throw new TypeError("WebAssembly.Global must be called with new");
			if (desc === null || typeof desc !== "object") throw new TypeError("WebAssembly.Global: descriptor must be an object");
			const t = desc.value === "funcref" ? "anyfunc" : desc.value;
			if (!VALUE_TYPES.includes(t)) throw new TypeError("WebAssembly.Global: value must be a WebAssembly value type");
			const mutable = !!desc.mutable;
			const v = toWasm(t, value === undefined ? defaultFor(t) : value);
			const h = guard(() => W.globalNew(owner(), t, mutable, v));
			Object.defineProperty(this, REF, { value: h });
			Object.defineProperty(this, TYPE, { value: { value: t, mutable } });
			objects.set(`${h.s}:${h.i}`, this);
		}
		get value() {
			const { s, i } = refOf(this, Global);
			return fromWasm(guard(() => W.globalGet(s, i)));
		}
		set value(v) {
			const { s, i } = refOf(this, Global);
			const t = this[TYPE];
			if (!t.mutable) throw new TypeError("WebAssembly.Global: can't set the value of an immutable global");
			const w = toWasm(t.value, v);
			guard(() => W.globalSet(s, i, w));
		}
		valueOf() {
			return this.value;
		}
		type() {
			return { ...this[TYPE] };
		}
		get [Symbol.toStringTag]() {
			return "WebAssembly.Global";
		}
	}

	// The plugin is read from the stack now, before any await loses it.
	const instantiateAs = (own, source, importObject) =>
		new Promise((resolve) => {
			if (source instanceof Module) {
				resolve(newInstance(Object.create(Instance.prototype), source, importObject, own));
			} else {
				const module = newModule(Object.create(Module.prototype), source, own);
				resolve({ module, instance: newInstance(Object.create(Instance.prototype), module, importObject, own) });
			}
		});
	const bytesFromResponse = async (source) => {
		const r = await source;
		if (r === null || typeof r !== "object" || typeof r.arrayBuffer !== "function") {
			throw new TypeError("WebAssembly: streaming source must be a Response");
		}
		if (r.ok === false) throw new TypeError(`WebAssembly: HTTP status ${r.status} for ${r.url ?? "response"}`);
		return r.arrayBuffer();
	};

	const WebAssembly = {
		Module,
		Instance,
		Memory,
		Table,
		Global,
		CompileError,
		LinkError,
		RuntimeError,
		validate(bytes) {
			const u = bytesOf(bytes);
			return W.validate(u.buffer, u.byteOffset, u.byteLength);
		},
		compile(bytes) {
			const own = owner();
			return new Promise((resolve) => resolve(newModule(Object.create(Module.prototype), bytes, own)));
		},
		instantiate(source, importObject) {
			return instantiateAs(owner(), source, importObject);
		},
		compileStreaming(source) {
			const own = owner();
			return bytesFromResponse(source).then((b) => newModule(Object.create(Module.prototype), b, own));
		},
		instantiateStreaming(source, importObject) {
			const own = owner();
			return bytesFromResponse(source).then((b) => instantiateAs(own, b, importObject));
		},
	};
	Object.defineProperty(WebAssembly, Symbol.toStringTag, { value: "WebAssembly" });
	Object.defineProperty(g, "WebAssembly", { value: WebAssembly, writable: true, configurable: true, enumerable: false });
})();
