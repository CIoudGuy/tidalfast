// Luna compatibility runtime: the process a plugin's `child_process.fork`
// started (src/luna/fork.rs). Runs after prelude.js and node.js in the
// fork's own runtime and makes `process` the forked script's, as Node's:
// its argv, cwd and env, the IPC channel (`process.send`, 'message',
// `disconnect`), `process.exit` with 'beforeExit' and 'exit', an uncaught
// error ending the process with code 1, and its output going to the parent
// when the parent reads it (`silent`). The script is loaded from the
// plugin's files as a CommonJS module (or an ES module, rewritten as
// node.js does for `.native` code), with `require` of Node's modules and of
// its own files beside it.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const N = g.__fork;
	const process = g.process;
	const { callOp } = L;
	const path = L.node.core.path;
	let plugin;
	let advanced = false;
	let connected = false;
	let exiting = false;

	const ipcError = (code, message) => Object.assign(new Error(message), { code });

	// ---------------------------------------------------------------- the channel
	process.send = function send(message, sendHandle, options, callback) {
		if (typeof sendHandle === "function") [callback, sendHandle] = [sendHandle, undefined];
		else if (typeof options === "function") [callback, options] = [options, undefined];
		if (message === undefined) throw ipcError("ERR_MISSING_ARGS", 'The "message" argument must be specified');
		if (!connected) {
			const err = ipcError("ERR_IPC_CHANNEL_CLOSED", "Channel closed");
			if (typeof callback === "function") queueMicrotask(() => callback(err));
			else queueMicrotask(() => process.emit("error", err));
			return false;
		}
		const ok = N.send(L.node.ipcEncode(message, advanced));
		if (typeof callback === "function") queueMicrotask(() => callback(ok ? null : ipcError("ERR_IPC_CHANNEL_CLOSED", "Channel closed")));
		return ok;
	};
	const closed = () => {
		connected = false;
		process.connected = false;
		process.channel = undefined;
	};
	process.disconnect = function disconnect() {
		if (!connected) {
			process.emit("error", ipcError("ERR_IPC_DISCONNECTED", "IPC channel is already disconnected"));
			return;
		}
		closed();
		N.disconnect();
		queueMicrotask(() => process.emit("disconnect"));
	};

	// ---------------------------------------------------------------- exit
	const exitCode = () => Number(process.exitCode ?? 0) | 0;
	const exit = (code) => {
		if (code !== undefined && code !== null) process.exitCode = Number(code) | 0;
		const c = exitCode();
		if (!exiting) {
			exiting = true;
			try {
				process.emit("exit", c);
			} catch {}
		}
		// The runtime stops this script at once (nothing after this runs).
		N.exit(c);
		throw new Error("process.exit");
	};
	process.exit = exit;
	process.reallyExit = exit;
	process.abort = () => exit(134);

	// An uncaught error ends the process with code 1, as in Node, unless it
	// listens for 'uncaughtException'.
	L.reportError = (err) => {
		if (exiting) return;
		if (process.listenerCount("uncaughtException") > 0) {
			try {
				process.emit("uncaughtException", err, "uncaughtException");
				return;
			} catch (e) {
				err = e;
			}
		}
		try {
			callOp("log", { plugin, level: "error", msg: `Uncaught ${err?.stack ? `${err.message}\n${err.stack}` : String(err)}` });
		} catch {}
		exit(1);
	};

	// Changing the signed-in account is asked about from the plugin's own
	// code; a forked script's request to do it is refused.
	L.allowAccountWrite ??= async () => false;

	// ---------------------------------------------------------------- modules
	const esm = L.node.esm;
	let fs;
	let coreRequire;
	const cache = Object.create(null);
	const isPath = (name) => /^(\.{1,2}([\\/]|$)|[\\/]|[a-zA-Z]:[\\/])/.test(name);
	const resolveFile = (p) => {
		for (const c of [p, `${p}.js`, `${p}.cjs`, `${p}.mjs`, `${p}.json`, path.join(p, "index.js")]) {
			try {
				if (fs.statSync(c).isFile()) return c;
			} catch {}
		}
		throw Object.assign(new Error(`Cannot find module '${p}'`), { code: "MODULE_NOT_FOUND" });
	};
	let mainModule;
	const makeRequire = (from) => {
		const req = (name) => {
			name = String(name);
			if (!isPath(name)) return coreRequire(name);
			const file = resolveFile(path.resolve(path.dirname(from), name));
			const known = cache[file];
			if (known) return known.exports;
			if (/\.json$/i.test(file)) {
				const module = { exports: JSON.parse(fs.readFileSync(file, "utf8")), id: file, filename: file, loaded: true };
				cache[file] = module;
				return module.exports;
			}
			return compile(file, fs.readFileSync(file, "utf8")).exports;
		};
		req.resolve = (name) => (isPath(String(name)) ? resolveFile(path.resolve(path.dirname(from), String(name))) : String(name));
		req.cache = cache;
		req.main = mainModule;
		return req;
	};
	// Returns the module; an ES module's body may still be running.
	const compile = (file, code) => {
		let src = code.startsWith("#!") ? `//${code}` : code;
		const isEsm = /\.mjs$/i.test(file) || L.node.isEsm(src);
		if (isEsm) src = L.node.esmToCjs(src);
		const wrapper = L.evalAs(
			`(${isEsm ? "async " : ""}function (exports, require, module, __filename, __dirname, __esmDefault, __esmNamed, __importMetaUrl) {${src}\n})`,
			`native:${encodeURIComponent(plugin)}@${path.basename(file)}`,
		);
		const module = { exports: {}, id: file, filename: file, loaded: false, children: [], paths: [], path: path.dirname(file) };
		mainModule ??= module;
		cache[file] = module;
		const req = makeRequire(file);
		module.require = req;
		const ret = wrapper.call(module.exports, module.exports, req, module, file, path.dirname(file), esm.esmDefault, esm.esmNamed, `file:///${file.replace(/\\/g, "/").replace(/^\/+/, "")}`);
		if (isEsm) {
			Promise.resolve(ret).then(
				() => (module.loaded = true),
				(err) => L.reportError(err),
			);
		} else module.loaded = true;
		return module;
	};

	// ---------------------------------------------------------------- entry points
	g.__fork_start = (json) => {
		const spec = JSON.parse(json);
		plugin = spec.plugin;
		advanced = spec.serialization === "advanced";
		connected = !!spec.ipc;
		process.argv = [process.execPath, spec.file, ...spec.args];
		process.argv0 = "node";
		process.execArgv = spec.execArgv;
		process.pid = spec.pid;
		process.ppid = 1;
		if (spec.env && typeof spec.env === "object") process.env = { ...spec.env };
		const cwd = spec.cwd;
		process.cwd = () => cwd;
		process.connected = connected;
		process.channel = connected ? { ref() {}, unref() {}, hasRef: () => true } : undefined;
		// Output the parent reads (`silent`, or pipes in `stdio`); otherwise
		// it goes to the plugin's log, as the parent's own would.
		const out = (kind) => (s) => {
			N.output(kind, L.b64enc(typeof s === "string" ? L.utf8enc(s) : L.toU8(s)));
			return true;
		};
		const line = (kind) => (...a) => out(kind)(`${L.format(...a)}\n`);
		if (spec.pipes[0]) {
			process.stdout.write = out("stdout");
			for (const k of ["log", "info", "debug", "dir", "table"]) console[k] = line("stdout");
		}
		if (spec.pipes[1]) {
			process.stderr.write = out("stderr");
			for (const k of ["error", "warn", "trace"]) console[k] = line("stderr");
		}
		coreRequire = L.node.makeRequire(plugin, {});
		fs = coreRequire("fs");
		compile(spec.file, spec.code);
	};
	g.__fork_event = (kind, data) => {
		if (kind === "message") {
			if (!connected) return;
			let message;
			try {
				message = L.node.ipcDecode(data, advanced);
			} catch (err) {
				L.reportError(err);
				return;
			}
			process.emit("message", message, undefined);
		} else if (kind === "disconnect") {
			if (!connected) return;
			closed();
			process.emit("disconnect");
		}
	};
	// Nothing is scheduled: 1 while something still keeps the process alive
	// (a channel with a listener, an op, program or socket in flight);
	// otherwise 'beforeExit', and the second time in a row, the exit.
	g.__fork_idle = (final) => {
		const listening = connected && (process.listenerCount("message") > 0 || process.listenerCount("disconnect") > 0);
		if (listening || L.pendingOps() > 0 || L.node.openHandles() > 0) return 1;
		if (!final) {
			try {
				process.emit("beforeExit", exitCode());
			} catch (err) {
				L.reportError(err);
			}
			return 0;
		}
		exit();
	};
})();
