//! `WebAssembly` for plugins (src/luna/wasm.rs + js/wasm.js): a module
//! compiled from the text below at test time exercises imports/exports both
//! ways, i32/i64/f32/f64, multi-value, memory views and growth, tables,
//! globals, traps, JS exceptions through wasm, re-entry, the error classes,
//! the per-plugin memory cap and the watchdog.

use std::time::Instant;

use super::*;

const MODULE: &str = r#"
(module
  (type $bin (func (param i32 i32) (result i32)))
  (import "env" "log" (func $log (param i32 i32)))
  (import "env" "twice" (func $twice (param i64) (result i64)))
  (import "env" "boom" (func $boom))
  (import "env" "reenter" (func $reenter (param i32) (result i32)))
  (import "env" "base" (global $base i32))
  (memory (export "mem") 1 100)
  (global $counter (export "counter") (mut i64) (i64.const 5))
  (table (export "tbl") 2 funcref)
  (elem (i32.const 0) $add $mul)
  (data (i32.const 16) "hello wasm")
  (func $add (export "add") (param i32 i32) (result i32)
    local.get 0 local.get 1 i32.add global.get $base i32.add)
  (func $mul (param i32 i32) (result i32) local.get 0 local.get 1 i32.mul)
  (func (export "indirect") (param i32 i32 i32) (result i32)
    local.get 1 local.get 2 local.get 0 call_indirect (type $bin))
  (func (export "greet") i32.const 16 i32.const 10 call $log)
  (func (export "sum") (param $p i32) (param $n i32) (result i32) (local $s i32)
    (block (loop
      (br_if 1 (i32.eqz (local.get $n)))
      (local.set $s (i32.add (local.get $s) (i32.load8_u (local.get $p))))
      (local.set $p (i32.add (local.get $p) (i32.const 1)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br 0)))
    local.get $s)
  (func (export "twice64") (param i64) (result i64) local.get 0 call $twice)
  (func (export "bump") (param i64) (result i64)
    (global.set $counter (i64.add (global.get $counter) (local.get 0)))
    global.get $counter)
  (func (export "fl") (param f32 f64) (result f64)
    local.get 0 f64.promote_f32 local.get 1 f64.add)
  (func (export "pair") (result i32 i64) i32.const 7 i64.const -1)
  (func (export "callBoom") call $boom)
  (func (export "trap") unreachable)
  (func (export "div") (param i32 i32) (result i32) local.get 0 local.get 1 i32.div_s)
  (func (export "growAndLog") (result i32)
    (drop (memory.grow (i32.const 2)))
    i32.const 16 i32.const 10 call $log
    memory.size)
  (func (export "reent") (param i32) (result i32) local.get 0 call $reenter)
  (func (export "spin") (loop br 0))
)
"#;

const SMALL: &str = r#"(module (func (export "mul") (param i32 i32) (result i32) local.get 0 local.get 1 i32.mul))"#;

fn js_bytes(wat: &str) -> String {
    let b = wat::parse_str(wat).unwrap();
    format!(
        "new Uint8Array([{}])",
        b.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
    )
}

#[test]
fn webassembly_api() {
    let body = format!(
        r#"
const bytes = {bytes};
const logs = [];
const thrown = new Error("from js");
let inst;
const imports = {{ env: {{
	log: (p, n) => logs.push(new TextDecoder().decode(new Uint8Array(inst.exports.mem.buffer, p, n)) + ":" + inst.exports.mem.buffer.byteLength),
	twice: (x) => x * 2n,
	boom: () => {{ throw thrown; }},
	reenter: (x) => inst.exports.add(x, 1),
	base: 100,
}} }};
const name = (f) => {{ try {{ f(); return "no error"; }} catch (e) {{ return e.name; }} }};
await check("validate", () => [WebAssembly.validate(bytes), WebAssembly.validate(new Uint8Array([0, 1, 2]))].join());
await check("instantiate", async () => {{
	const r = await WebAssembly.instantiate(bytes, imports);
	inst = r.instance;
	return [r.module instanceof WebAssembly.Module, inst instanceof WebAssembly.Instance, WebAssembly.Module.exports(r.module).length,
		WebAssembly.Module.imports(r.module).map((i) => i.kind).join("/"), String(WebAssembly)].join();
}});
await check("add", () => inst.exports.add(2, 3));
await check("indirect", () => inst.exports.indirect(1, 6, 7) + "," + inst.exports.tbl.get(0)(1, 1));
await check("greet", () => {{ inst.exports.greet(); return logs.join("|"); }});
await check("sum", () => {{ new Uint8Array(inst.exports.mem.buffer).set([1, 2, 3, 250], 100); return inst.exports.sum(100, 4); }});
await check("i64", () => [inst.exports.twice64(-21n), inst.exports.bump(10n), inst.exports.counter.value, typeof inst.exports.counter.value].join());
await check("i64num", () => name(() => inst.exports.twice64(1)));
await check("floats", () => inst.exports.fl(1.5, 0.25));
await check("pair", () => inst.exports.pair().map(String).join());
await check("exception", () => {{ try {{ inst.exports.callBoom(); }} catch (e) {{ return e === thrown; }} return "no error"; }});
await check("trap", () => {{ try {{ inst.exports.trap(); }} catch (e) {{ return e instanceof WebAssembly.RuntimeError && e.name; }} }});
await check("div0", () => name(() => inst.exports.div(1, 0)));
await check("grow", () => {{
	const old = inst.exports.mem.buffer;
	logs.length = 0;
	const size = inst.exports.growAndLog();
	return [old.byteLength, size, logs[0], inst.exports.mem.buffer.byteLength, inst.exports.mem.grow(1), inst.exports.mem.buffer.byteLength,
		inst.exports.mem.buffer === inst.exports.mem.buffer].join();
}});
await check("reenter", () => inst.exports.reent(41));
await check("memory", () => {{
	const m = new WebAssembly.Memory({{ initial: 1, maximum: 2 }});
	const b = m.buffer;
	return [b.byteLength, m.grow(1), b.byteLength, m.buffer.byteLength, name(() => m.grow(5))].join();
}});
await check("limit", () => name(() => new WebAssembly.Memory({{ initial: 1 }}).grow(4000)));
await check("global", () => {{
	const g = new WebAssembly.Global({{ value: "i32", mutable: true }}, 4);
	g.value = 9;
	const c = new WebAssembly.Global({{ value: "f64" }}, 1.5);
	return [g.value, c.value, name(() => (c.value = 2))].join();
}});
await check("table", () => {{
	const t = new WebAssembly.Table({{ element: "anyfunc", initial: 1 }});
	t.set(0, inst.exports.add);
	t.grow(2);
	return [t.length, t.get(0) === inst.exports.add, t.get(1), name(() => t.set(0, () => 1))].join();
}});
await check("errors", async () => {{
	const out = [name(() => new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 9])))];
	try {{ await WebAssembly.instantiate(bytes, {{ env: {{}} }}); }} catch (e) {{ out.push(e.name); }}
	try {{ await WebAssembly.compile(new Uint8Array(4)); }} catch (e) {{ out.push(e.name); }}
	return out.join();
}});
await check("streaming", async () => {{
	const r = await WebAssembly.instantiateStreaming(Promise.resolve(new Response(bytes)), imports);
	return r.instance.exports.add(1, 1);
}});
const n = await native("Compat/wasm.native.ts", `"use strict";
exports.mul = async (a, b) => {{
	const {{ instance }} = await WebAssembly.instantiate({small});
	return instance.exports.mul(a, b);
}};
`);
await check("native", () => n("mul", 6, 7));
R.done = true;
"#,
        bytes = js_bytes(MODULE),
        small = js_bytes(SMALL),
    );
    let (r, luna, ..) = run("Compat", &body);
    assert_eq!(r["validate"], "true,false", "{r}");
    assert_eq!(
        r["instantiate"],
        "true,true,17,function/function/function/function/global,[object WebAssembly]",
        "{r}"
    );
    assert_eq!(r["add"], 105, "{r}");
    assert_eq!(r["indirect"], "42,102", "{r}");
    assert_eq!(r["greet"], "hello wasm:65536", "{r}");
    assert_eq!(r["sum"], 256, "{r}");
    assert_eq!(r["i64"], "-42,15,15,bigint", "{r}");
    assert_eq!(r["i64num"], "TypeError", "{r}");
    assert_eq!(r["floats"], 1.75, "{r}");
    assert_eq!(r["pair"], "7,-1", "{r}");
    assert_eq!(r["exception"], true, "{r}");
    assert_eq!(r["trap"], "RuntimeError", "{r}");
    assert_eq!(r["div0"], "RuntimeError", "{r}");
    assert_eq!(
        r["grow"], "0,3,hello wasm:196608,196608,3,262144,true",
        "{r}"
    );
    assert_eq!(r["reenter"], 142, "{r}");
    assert_eq!(r["memory"], "65536,1,0,131072,RangeError", "{r}");
    assert_eq!(r["limit"], "RangeError", "{r}");
    assert_eq!(r["global"], "9,1.5,TypeError", "{r}");
    assert_eq!(r["table"], "3,true,,TypeError", "{r}");
    assert_eq!(r["errors"], "CompileError,LinkError,CompileError", "{r}");
    assert_eq!(r["streaming"], 102, "{r}");
    assert_eq!(r["native"], 42, "{r}");
    luna.shutdown();
}

/// A module that never returns is stopped by the watchdog like a JS loop:
/// the plugin is disabled and the runtime keeps going.
#[test]
fn webassembly_watchdog() {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Spin.json".into(),
        json!({ "name": "Spin", "hash": "s" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/Spin.mjs".into(),
        format!(
            "export const unloads = new Set();\nconst {{ instance }} = await WebAssembly.instantiate({}, {{ env: {{ log() {{}}, twice: (x) => x, boom() {{}}, reenter: (x) => x, base: 0 }} }});\ninstance.exports.spin();\n",
            js_bytes(MODULE)
        )
        .into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("wasm-spin");
    let mut cfg = config(&dir);
    cfg.wasm_watchdog = Duration::from_secs(3);
    let luna = Luna::start(cfg, Arc::new(CompatHost::default())).unwrap();
    let t = Instant::now();
    luna.install(&format!("{base}/Spin.mjs"));
    let s = wait_until(&luna, 30, |s| {
        plugin(s, "Spin").is_some_and(|p| matches!(p.status, PluginStatus::Failed(_)))
    });
    let p = plugin(&s, "Spin").unwrap();
    assert!(
        matches!(&p.status, PluginStatus::Failed(e) if e.contains("without yielding")),
        "{:?}",
        p.status
    );
    assert!(!p.enabled);
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A wasm call that runs longer than the JavaScript watchdog but within the
/// wasm budget finishes, and the JS around it keeps running: Shazam's audio
/// fingerprint takes seconds on the interpreter and used to disable the
/// plugin ("ran for more than 5s without yielding"). The JS watchdog here is
/// 2 s, the call about 5 s, and the JS that wasm calls into runs past the
/// old deadline too (every iteration calls JS).
#[test]
fn a_slow_wasm_call_is_not_stopped_by_the_js_watchdog() {
    let wat = r#"(module
      (import "env" "tick" (func $tick (result i32)))
      (func (export "long") (loop $l (br_if $l (i32.eqz (call $tick))))))"#;
    let body = format!(
        r#"
const t0 = Date.now();
const {{ instance }} = await WebAssembly.instantiate({bytes}, {{ env: {{ tick: () => (Date.now() - t0 > 5000 ? 1 : 0) }} }});
instance.exports.long();
R.ms = Date.now() - t0;
let n = 0;
for (let i = 0; i < 1000; i++) n += i;
R.sum = n;
R.done = true;
"#,
        bytes = js_bytes(wat)
    );
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Compat.json".into(),
        json!({ "name": "Compat", "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Compat.mjs".into(), format!("{HEADER}{body}").into_bytes());
    let base = serve(files);
    let dir = temp_dir("wasm-slow");
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(2);
    cfg.wasm_watchdog = Duration::from_secs(60);
    let luna = Luna::start(cfg, Arc::new(CompatHost::default())).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    wait_until(&luna, 90, |_| results(&dir)["done"] == json!(true));
    let r = results(&dir);
    assert_eq!(r["sum"], 499500, "{r:?}");
    assert!(r["ms"].as_i64().unwrap() >= 5000, "{r:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fixed-width 128-bit SIMD (as Chromium supports): undici's llhttp ships a
/// SIMD build, and DiscordRPC logged "unexpected SIMD opcode: 0xfd" when the
/// runtime refused it. A module using v128 arithmetic must compile and run.
#[test]
fn webassembly_simd() {
    let wat = r#"(module
      (func (export "lane") (result i32)
        v128.const i32x4 1 2 3 4
        v128.const i32x4 10 20 30 40
        i32x4.add
        i32x4.extract_lane 2)
      (func (export "mix") (param i32) (result i32)
        local.get 0 i32x4.splat
        v128.const i32x4 1 1 1 1
        i32x4.mul
        i8x16.bitmask))"#;
    let body = format!(
        r#"
const bytes = {bytes};
R.valid = WebAssembly.validate(bytes);
const {{ instance }} = await WebAssembly.instantiate(bytes, {{}});
R.lane = instance.exports.lane();
R.mix = instance.exports.mix(-1);
R.done = true;
"#,
        bytes = js_bytes(wat)
    );
    let (r, luna, ..) = run("Compat", &body);
    assert_eq!(r["valid"], true, "{r:?}");
    assert_eq!(r["lane"], 33, "{r:?}");
    assert_eq!(r["mix"], 0xffff, "{r:?}");
    luna.shutdown();
}

/// undici's HTTP parser (llhttp, WebAssembly) as the real DiscordRPC plugin
/// bundles it: taken from the published plugin, instantiated with undici's
/// imports and fed a response. Needs the network.
#[test]
#[ignore]
fn real_undici_llhttp_parses_a_response() {
    use base64::Engine as _;
    let code = reqwest::blocking::get(
        "https://github.com/Inrixia/luna-plugins/releases/download/latest/DiscordRPC.mjs",
    )
    .unwrap()
    .text()
    .unwrap();
    let mut modules = Vec::new();
    let mut rest = code.as_str();
    while let Some(i) = rest.find("\"AGFzbQ") {
        let b64 = &rest[i + 1..];
        let end = b64.find('"').unwrap();
        modules.push(
            base64::engine::general_purpose::STANDARD
                .decode(&b64[..end])
                .unwrap(),
        );
        rest = &b64[end..];
    }
    assert!(!modules.is_empty(), "no WebAssembly in DiscordRPC.mjs");
    let list = modules
        .iter()
        .map(|b| {
            format!(
                "new Uint8Array([{}])",
                b.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"
const modules = [{list}];
const parse = async (bytes) => {{
	const seen = [];
	let mem;
	const text = (p, n) => new TextDecoder().decode(new Uint8Array(mem.buffer, p, n));
	const env = {{
		wasm_on_url: () => 0,
		wasm_on_status: (p, at, n) => (seen.push("status " + text(at, n)), 0),
		wasm_on_message_begin: () => (seen.push("begin"), 0),
		wasm_on_header_field: (p, at, n) => (seen.push("field " + text(at, n)), 0),
		wasm_on_header_value: (p, at, n) => (seen.push("value " + text(at, n)), 0),
		wasm_on_headers_complete: (p, status, upgrade, keepAlive) => (seen.push("headers " + status), 0),
		wasm_on_body: (p, at, n) => (seen.push("body " + text(at, n)), 0),
		wasm_on_message_complete: () => (seen.push("complete"), 0),
	}};
	let m;
	try {{ m = await WebAssembly.compile(bytes); }} catch (e) {{ return "compile: " + e.name; }}
	const {{ exports: x }} = await WebAssembly.instantiate(m, {{ env }});
	mem = x.memory;
	const parser = x.llhttp_alloc(2);
	const data = new TextEncoder().encode("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-Test: yes\r\n\r\nhello");
	const ptr = x.malloc(data.length);
	new Uint8Array(mem.buffer, ptr, data.length).set(data);
	const ret = x.llhttp_execute(parser, ptr, data.length);
	x.free(ptr);
	x.llhttp_free(parser);
	return ret + ":" + seen.join("|");
}};
const out = [];
for (const b of modules) out.push(await parse(b).catch((e) => "ERR " + e));
R.parsed = out;
R.done = true;
"#
    );
    let (r, luna, ..) = run("Compat", &body);
    let parsed = r["parsed"].as_array().unwrap();
    // Both builds (SIMD and plain) are fed; at least one must parse.
    assert!(
        parsed.iter().any(|p| p
            == "0:begin|status OK|field Content-Length|value 5|field X-Test|value yes|headers 200|body hello|complete"),
        "{parsed:?}"
    );
    luna.shutdown();
}
