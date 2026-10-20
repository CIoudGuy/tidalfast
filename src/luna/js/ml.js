// Luna compatibility runtime: module blobs, Web Workers, and transformers.js.
//
// - `import(blobUrl)` of JavaScript a plugin made runs as that plugin
//   (Rust's resolver asks `__luna_module` whose blob it is and for its
//   source).
// - `Worker` runs a module script on the Luna thread with a scope of its
//   own: `self` is a DedicatedWorkerGlobalScope whose `postMessage` and
//   `onmessage` talk to the Worker object, asynchronously, as two threads
//   would. Each worker gets its own instance of the modules it imports.
// - transformers.js, when a plugin loads it as a module blob, is answered
//   by a stand-in whose depth-estimation pipeline runs natively
//   (src/luna/depth.rs) on the model files fetched through the plugin's own
//   `env.fetch`. Other pipelines say they are not available.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const D = g.__depth;
	const { callOpAsync } = L;

	// One value handed to a module's first line through a global that line
	// deletes: a module's source can name nothing else of the runtime.
	const handoff = (value) => {
		const key = `__lunaHandoff_${g.crypto.randomUUID().replace(/-/g, "")}`;
		Object.defineProperty(g, key, { value, configurable: true, enumerable: false, writable: false });
		return `const __luna_h = globalThis[${JSON.stringify(key)}]; delete globalThis[${JSON.stringify(key)}]; `;
	};

	// ---------------------------------------------------------------- workers
	let workerSeq = 1;
	const scopes = new Map();
	// What a message carries over: transferred buffers and image bitmaps
	// as they are, everything else copied as structured clone would.
	const cloneMessage = (value, transfer) => {
		const moved = new Set(transfer ?? []);
		const seen = new Map();
		const clone = (x) => {
			if (x === null || typeof x !== "object") return x;
			if (seen.has(x)) return seen.get(x);
			if (moved.has(x) || x instanceof g.ImageBitmap || x instanceof g.Blob || (g.MessagePort && x instanceof g.MessagePort)) return x;
			if (x instanceof ArrayBuffer) return x.slice(0);
			if (ArrayBuffer.isView(x)) return moved.has(x.buffer) ? x : x.slice();
			if (x instanceof Date) return new Date(x.getTime());
			if (x instanceof RegExp) return new RegExp(x.source, x.flags);
			if (x instanceof Map) {
				const m = new Map();
				seen.set(x, m);
				for (const [k, v] of x) m.set(clone(k), clone(v));
				return m;
			}
			if (x instanceof Set) {
				const s = new Set();
				seen.set(x, s);
				for (const v of x) s.add(clone(v));
				return s;
			}
			if (x instanceof Error) {
				const e = new Error(x.message);
				e.name = x.name;
				return e;
			}
			if (typeof x === "function") throw new g.DOMException("A function could not be cloned.", "DataCloneError");
			const out = Array.isArray(x) ? [] : {};
			seen.set(x, out);
			for (const k of Object.keys(x)) out[k] = clone(x[k]);
			return out;
		};
		return clone(value);
	};
	const transferList = (t) => (Array.isArray(t) ? t : Array.isArray(t?.transfer) ? t.transfer : []);
	// Delivers `data` to `target` (its `onmessage`, then its listeners) as
	// a task of its own.
	const deliver = (target, data, isDead, onError) =>
		g.setTimeout(() => {
			if (isDead()) return;
			const ev = new g.MessageEvent("message", { data });
			try {
				if (typeof target.onmessage === "function") target.onmessage.call(target, ev);
			} catch (err) {
				onError(err);
			}
			target.dispatchEvent(ev);
		}, 0);

	class WorkerGlobalScope extends g.EventTarget {}
	class DedicatedWorkerGlobalScope extends WorkerGlobalScope {}
	// The names a worker module sees as its scope's own (when it does not
	// declare them itself).
	const SCOPE_NAMES = ["self", "postMessage", "close", "addEventListener", "removeEventListener", "dispatchEvent", "importScripts", "WorkerGlobalScope", "DedicatedWorkerGlobalScope"];

	const makeScope = (worker, name) => {
		const scope = new DedicatedWorkerGlobalScope();
		scope.onmessage = null;
		scope.onmessageerror = null;
		scope.onerror = null;
		scope.name = name ?? "";
		const raise = (err) => {
			L.reportError(err);
			if (!worker._dead) worker.dispatchEvent(new g.ErrorEvent("error", { message: String(err?.message ?? err), error: err }));
			if (typeof worker.onerror === "function") worker.onerror(new g.ErrorEvent("error", { message: String(err?.message ?? err), error: err }));
		};
		scope._raise = raise;
		scope.postMessage = (data, transfer) => {
			if (worker._dead) return;
			deliver(worker, cloneMessage(data, transferList(transfer)), () => worker._dead, L.reportError);
		};
		scope.close = () => worker.terminate();
		scope.importScripts = () => {
			throw new g.DOMException("importScripts is not available in Tidalfast's workers; use a module worker", "NotSupportedError");
		};
		// Everything else a worker reads from `self` is the shared global's
		// (fetch, timers, OffscreenCanvas, navigator without a GPU).
		const self = new Proxy(scope, {
			get: (t, k) => (k in t ? Reflect.get(t, k, t) : g[k]),
			has: (t, k) => k in t || k in g,
		});
		const own = { self, WorkerGlobalScope, DedicatedWorkerGlobalScope };
		for (const k of ["postMessage", "close", "importScripts"]) own[k] = scope[k];
		for (const k of ["addEventListener", "removeEventListener", "dispatchEvent"]) own[k] = scope[k].bind(scope);
		scope._bindings = own;
		return scope;
	};

	class Worker extends g.EventTarget {
		constructor(url, options = {}) {
			super();
			this.onmessage = null;
			this.onmessageerror = null;
			this.onerror = null;
			const id = workerSeq++;
			this._dead = false;
			this._queue = [];
			this._ready = false;
			const href = String(url instanceof g.URL ? url.href : url);
			const scope = makeScope(this, options?.name);
			this._scope = scope;
			scopes.set(id, scope);
			this._id = id;
			const fail = (msg, err) => {
				if (this._dead) return;
				const ev = new g.ErrorEvent("error", { message: msg, error: err });
				if (typeof this.onerror === "function") this.onerror(ev);
				this.dispatchEvent(ev);
				L.callOp("log", { plugin: L.pluginFromStack(), level: "warn", msg: `Worker ${href}: ${msg}` });
			};
			if (!L.blobModules.has(href)) {
				g.setTimeout(() => fail("Tidalfast runs workers from blob: URLs of JavaScript the plugin made"), 0);
				return;
			}
			import(`${href}#worker=${id}`).then(
				() => {
					this._ready = true;
					for (const data of this._queue.splice(0)) this._send(data);
				},
				(err) => fail(String(err?.message ?? err), err),
			);
		}
		_send(data) {
			const scope = this._scope;
			deliver(scope, data, () => this._dead, scope._raise);
		}
		postMessage(data, transfer) {
			if (this._dead) return;
			const msg = cloneMessage(data, transferList(transfer));
			if (this._ready) this._send(msg);
			else this._queue.push(msg);
		}
		terminate() {
			this._dead = true;
			this._queue = [];
			scopes.delete(this._id);
		}
	}
	g.Worker ??= Worker;

	// ---------------------------------------------------------------- modules
	// transformers.js's bundle, from what every build of it carries.
	const isTransformers = (src) => src.length > 100000 && src.includes("transformers.js_config") && src.includes("huggingface/transformers.js") && /\bas pipeline\b/.test(src);
	const declares = (src, name) => new RegExp(`\\b(?:var|let|const|function|class)\\s+${name}\\b`).test(src);

	g.__luna_module = (kind, url, worker) => {
		const blob = L.blobModules.get(String(url));
		if (kind === "owner") return blob?.owner ?? null;
		if (!blob) return null;
		const src = L.utf8dec(blob.bytes);
		if (isTransformers(src)) {
			const version = /"(\d+\.\d+\.\d+(?:-[\w.]+)?)"/.exec(src.slice(0, 4000))?.[1] ?? "";
			const lib = makeTransformers(version);
			return `${handoff(lib)}${Object.keys(lib)
				.map((k) => `export const ${k} = __luna_h.${k};`)
				.join(" ")} export default __luna_h;`;
		}
		if (!worker) return src;
		const scope = scopes.get(Number(worker));
		if (!scope) throw new Error("The worker was terminated");
		const names = SCOPE_NAMES.filter((n) => !declares(src, n));
		const bind = names.map((n) => `${n} = __luna_h.${n}`).join(", ");
		return `${handoff(scope._bindings)}${bind ? `const ${bind}; ` : ""}${src}`;
	};

	// ---------------------------------------------------------------- transformers.js
	// dtype -> model file suffix, as transformers.js names them.
	const SUFFIX = { fp32: "", fp16: "_fp16", int8: "_int8", uint8: "_uint8", q8: "_quantized", q4: "_q4", q4f16: "_q4f16", bnb4: "_bnb4" };
	const UNLOADED = "depth model unloaded";
	const constrain = (val, multiple, min = 0, max = null) => {
		let x = Math.round(val / multiple) * multiple;
		if (max !== null && x > max) x = Math.floor(val / multiple) * multiple;
		if (x < min) x = Math.ceil(val / multiple) * multiple;
		return x;
	};
	const three = (v) => (Array.isArray(v) ? v.map(Number) : typeof v === "number" ? [v, v, v] : null);

	const defaultFetch = (...a) => g.fetch(...a);
	const makeTransformers = (version) => {
		const env = {
			version,
			backends: { onnx: { wasm: { numThreads: 1, proxy: false }, webgpu: {}, logLevel: "warning" } },
			allowRemoteModels: true,
			remoteHost: "https://huggingface.co/",
			remotePathTemplate: "{model}/resolve/{revision}/",
			allowLocalModels: false,
			localModelPath: "/models/",
			useFS: false,
			useBrowserCache: true,
			useFSCache: false,
			useCustomCache: false,
			customCache: null,
			useWasmCache: true,
			cacheKey: "transformers-cache",
			fetch: defaultFetch,
		};
		class Tensor {
			constructor(type, data, dims) {
				this.type = type;
				this.data = data;
				this.dims = dims;
			}
			get size() {
				return this.dims.reduce((a, b) => a * b, 1);
			}
			tolist() {
				return Array.from(this.data);
			}
		}
		class RawImage {
			constructor(data, width, height, channels) {
				this.data = data;
				this.width = width;
				this.height = height;
				this.channels = channels;
			}
			get size() {
				return [this.width, this.height];
			}
			static async fromBlob(blob) {
				const bitmap = await g.createImageBitmap(blob);
				const canvas = new g.OffscreenCanvas(bitmap.width, bitmap.height);
				const ctx = canvas.getContext("2d");
				ctx.drawImage(bitmap, 0, 0);
				bitmap.close();
				return new RawImage(ctx.getImageData(0, 0, canvas.width, canvas.height).data, canvas.width, canvas.height, 4);
			}
			static async fromURL(url) {
				const res = await env.fetch(String(url));
				if (!res.ok) throw new Error(`Unable to read image from "${url}" (${res.status} ${res.statusText})`);
				return RawImage.fromBlob(await res.blob());
			}
			static async read(input) {
				if (input instanceof RawImage) return input;
				if (input instanceof g.Blob) return RawImage.fromBlob(input);
				return RawImage.fromURL(input);
			}
		}
		const fileUrl = (model, file, revision) => env.remoteHost + env.remotePathTemplate.replaceAll("{model}", model).replaceAll("{revision}", encodeURIComponent(revision)) + file;
		// The plugin's own `env.fetch` when it set one (3d-cover's goes to its
		// cache); else a plain GET made for the plugin that asked.
		const get = async (plugin, model, file, revision) => {
			const url = fileUrl(model, file, revision);
			let res;
			if (env.fetch === defaultFetch) {
				const r = await callOpAsync("fetch", { plugin, url, method: "GET", headers: {}, body: null, redirect: "follow" });
				res = new g.Response(L.b64dec(r.body), { status: r.status, statusText: r.statusText, headers: r.headers, url: r.url });
			} else res = await env.fetch(url);
			if (!res?.ok) throw new Error(`Could not locate file: "${url}".`);
			return res;
		};
		const pipeline = async (task, model, options = {}) => {
			// The plugin asking, while its code is on the stack: the native work
			// below runs after awaits, when only the runtime's is.
			const plugin = L.pluginFromStack() ?? L.running;
			if (task !== "depth-estimation") throw new Error(`Tidalfast runs only the "depth-estimation" pipeline of transformers.js, not "${task}"`);
			model ??= "onnx-community/depth-anything-v2-small";
			const { device, dtype, revision = "main" } = options ?? {};
			const dev = String(device ?? "wasm").toLowerCase();
			if (!["wasm", "cpu", "auto"].includes(dev)) throw new Error(`Unsupported device: "${dev}". Tidalfast runs this model on the CPU (device "wasm").`);
			const dt = String((dtype && typeof dtype === "object" ? dtype.model : dtype) ?? "q8");
			if (!(dt in SUFFIX)) throw new Error(`Invalid dtype: ${dt}. Should be one of: ${Object.keys(SUFFIX).join(", ")}`);
			const config = await (await get(plugin, model, "config.json", revision)).json();
			const pre = await (await get(plugin, model, "preprocessor_config.json", revision)).json();
			const file = `onnx/model${SUFFIX[dt]}.onnx`;
			const load = async () => {
				const res = await get(plugin, model, file, revision);
				const bytes = new Uint8Array(await res.arrayBuffer());
				const id = D.stage(bytes.buffer, bytes.byteOffset, bytes.byteLength);
				return (await callOpAsync("depth.load", { plugin, bytes: id })).model;
			};
			let handle = await load();
			const image_processor = { config: pre, size: pre.size, do_resize: pre.do_resize ?? true, keep_aspect_ratio: pre.keep_aspect_ratio, ensure_multiple_of: pre.ensure_multiple_of };
			// transformers.js's get_resize_output_image_size, read at each
			// call (3d-cover sets `processor.image_processor.size`).
			const target = (w, h) => {
				const size = image_processor.size;
				if (image_processor.do_resize === false || !size) return [w, h];
				if (size.width !== undefined && size.height !== undefined) {
					let [nw, nh] = [size.width, size.height];
					if (pre.keep_aspect_ratio && pre.ensure_multiple_of) {
						let sh = nh / h;
						let sw = nw / w;
						if (Math.abs(1 - sw) < Math.abs(1 - sh)) sh = sw;
						else sw = sh;
						nh = constrain(sh * h, pre.ensure_multiple_of);
						nw = constrain(sw * w, pre.ensure_multiple_of);
					}
					return [nw, nh];
				}
				if (size.shortest_edge !== undefined) {
					const s = size.shortest_edge / Math.min(w, h);
					return [Math.round(w * s), Math.round(h * s)];
				}
				return [w, h];
			};
			const one = async (input) => {
				const image = await RawImage.read(input);
				const { width, height, channels } = image;
				const px = image.data instanceof Uint8Array || image.data instanceof Uint8ClampedArray ? image.data : Uint8Array.from(image.data);
				const ask = () => ({
					plugin,
					model: handle,
					image: D.stage(px.buffer, px.byteOffset, px.byteLength),
					width,
					height,
					channels,
					size: target(width, height),
					rescale: pre.do_rescale === false ? null : (pre.rescale_factor ?? 1 / 255),
					mean: pre.do_normalize === false ? null : three(pre.image_mean),
					std: pre.do_normalize === false ? null : three(pre.image_std),
				});
				let r;
				try {
					r = await callOpAsync("depth.run", ask());
				} catch (err) {
					// The idle model went: load it again (from the plugin's cache).
					if (String(err?.message) !== UNLOADED) throw err;
					handle = await load();
					r = await callOpAsync("depth.run", ask());
				}
				const depth = new Uint8Array(D.take(r.depth) ?? new ArrayBuffer(0));
				const predicted = new Float32Array(D.take(r.predicted) ?? new ArrayBuffer(0));
				return { predicted_depth: new Tensor("float32", predicted, [r.height, r.width]), depth: new RawImage(depth, r.width, r.height, 1) };
			};
			const pipe = async function (images) {
				if (!Array.isArray(images)) return one(images);
				const out = [];
				for (const im of images) out.push(await one(im));
				return out;
			};
			Object.assign(pipe, {
				task,
				model: { config },
				processor: { image_processor, config: pre },
				tokenizer: null,
				dispose: async () => {
					await callOpAsync("depth.free", { plugin, model: handle }).catch(() => {});
				},
			});
			return pipe;
		};
		return { env, pipeline, RawImage, Tensor };
	};
})();
