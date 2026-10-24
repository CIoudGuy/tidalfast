//! Module blobs, Workers and transformers.js's depth estimation, the way
//! @visiuun/3d-cover uses them: transformers.js loaded from a blob inside a
//! module worker, its model files fetched through `env`, an ImageBitmap
//! transferred in and the depth map posted back. The model is a tiny one
//! made here (grey level as depth), served with its configs over local
//! HTTP.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::luna::onnx_tests::{A, f32_tensor, i64_tensor, model, node};

const HEADER: &str = r#"export const unloads = new Set();
var core = luna?.core?.modules?.["@luna/core"];
const R = await core.ReactiveStore.getPluginStorage("Depth.results", {});
const fail = (k, e) => { R[k] = "error: " + (e?.stack ?? e); };
"#;

fn read(dir: &Path) -> Value {
    super::storage::read_json(&dir.join("settings").join(format!(
        "{}.json",
        super::storage::safe_name("Depth.results")
    )))
    .and_then(|v| v.get("Depth.results").cloned())
    .unwrap_or(Value::Null)
}

fn until(luna: &Luna, dir: &Path, secs: u64, mut cond: impl FnMut(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let out = read(dir);
        if cond(&out) {
            return out;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; results: {out}\nlogs: {:#?}",
                s.plugins
                    .iter()
                    .map(|p| (
                        &p.name,
                        p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Depth = the mean of the three channels: a 1x1 convolution, then the
/// channel axis dropped.
fn tiny_model() -> Vec<u8> {
    let third = 1.0 / 3.0;
    model(
        &[
            node(
                "Conv",
                &["pixel_values", "w"],
                &["c"],
                &[("kernel_shape", A::Is(&[1, 1]))],
            ),
            node("Squeeze", &["c", "axis"], &["predicted_depth"], &[]),
        ],
        &[
            f32_tensor("w", &[1, 3, 1, 1], &[third, third, third]),
            i64_tensor("axis", &[1], &[1]),
        ],
        "pixel_values",
        "predicted_depth",
    )
}

fn start(body: &str) -> (Luna, PathBuf, Files) {
    let files: Files = Arc::default();
    let base = serve(files.clone());
    {
        let mut f = files.lock().unwrap();
        f.insert(
            "/Depth.json".into(),
            json!({ "name": "Depth", "version": "1.0.0", "hash": "d1" })
                .to_string()
                .into_bytes(),
        );
        f.insert(
            "/Depth.mjs".into(),
            format!("{HEADER}{}", body.replace("__BASE__", &base)).into_bytes(),
        );
        let m = "/m/tiny/resolve/main";
        f.insert(
            format!("{m}/config.json"),
            br#"{"model_type":"depth_anything"}"#.to_vec(),
        );
        f.insert(
            format!("{m}/preprocessor_config.json"),
            json!({ "do_resize": true, "size": { "width": 518, "height": 518 }, "do_rescale": true, "rescale_factor": 1.0 / 255.0, "do_normalize": false })
                .to_string()
                .into_bytes(),
        );
        f.insert(format!("{m}/onnx/model_quantized.onnx"), tiny_model());
    }
    let dir = temp_dir("depth");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Depth.mjs"));
    (luna, dir, files)
}

#[test]
fn transformers_depth_estimation_runs_in_a_module_worker() {
    let (luna, dir, files) = start(
        r##"
try {
	// What jsDelivr serves for transformers.js, as far as Tidalfast looks.
	const fake = "// huggingface/transformers.js: transformers.js_config\nconst x = 1;\nexport { x as pipeline };\n" + " ".repeat(100001);
	const tfUrl = URL.createObjectURL(new Blob([fake], { type: "text/javascript" }));
	const script = `
let tf;
self.onmessage = async ({ data }) => {
	try {
		if (!tf) {
			tf = await import(${JSON.stringify(tfUrl)});
			tf.env.remoteHost = data.base + "/";
		}
		const pipe = await tf.pipeline("depth-estimation", "m/tiny", { device: "wasm", dtype: "q8" });
		pipe.processor.image_processor.size = { width: 4, height: 4 };
		const canvas = new OffscreenCanvas(data.bitmap.width, data.bitmap.height);
		const ctx = canvas.getContext("2d");
		ctx.drawImage(data.bitmap, 0, 0);
		data.bitmap.close();
		const image = new tf.RawImage(ctx.getImageData(0, 0, canvas.width, canvas.height).data, canvas.width, canvas.height, 4);
		const { depth, predicted_depth } = await pipe(image);
		const out = new Uint8Array(depth.data);
		const webgpu = await tf.pipeline("depth-estimation", "m/tiny", { device: "webgpu" }).then(() => "ok", (e) => e.message);
		postMessage({ width: depth.width, height: depth.height, channels: depth.channels, data: out, dims: predicted_depth.dims, scope: self instanceof WorkerGlobalScope, gpu: String(self.navigator?.gpu), webgpu }, [out.buffer]);
	} catch (e) {
		postMessage({ error: String(e?.message) + " " + String(e?.stack ?? e) });
	}
};`;
	const wUrl = URL.createObjectURL(new Blob([script], { type: "text/javascript" }));
	R.mainScope = typeof WorkerGlobalScope;
	const w = new Worker(wUrl, { type: "module" });
	w.onmessage = ({ data }) => {
		R.result = { ...data, data: Array.from(data.data ?? []) };
		w.terminate();
	};
	// 8x2, dark on the left to bright on the right.
	const c = new OffscreenCanvas(8, 2);
	const cx = c.getContext("2d");
	for (let x = 0; x < 8; x++) {
		cx.fillStyle = `rgb(${x * 32}, ${x * 32}, ${x * 32})`;
		cx.fillRect(x, 0, 1, 2);
	}
	const bm = await createImageBitmap(c);
	w.postMessage({ base: "__BASE__", bitmap: bm }, [bm]);
	R.textBlob = await import(URL.createObjectURL(new Blob(["export default 1"], { type: "text/plain" }))).then(() => "loaded", (e) => e.message);
	const own = await import(URL.createObjectURL(new Blob(["export default 7"], { type: "text/javascript" })));
	R.own = own.default;
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 60, |r| {
        !r["result"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    let res = &r["result"];
    assert_eq!(res["error"], Value::Null, "{r}");
    assert_eq!(
        r["mainScope"],
        json!("undefined"),
        "workers' globals stay in workers"
    );
    assert_eq!(res["scope"], json!(true));
    assert_eq!(res["gpu"], json!("undefined"));
    assert!(
        res["webgpu"]
            .as_str()
            .unwrap()
            .contains("Unsupported device"),
        "{r}"
    );
    assert_eq!(
        (
            res["width"].clone(),
            res["height"].clone(),
            res["channels"].clone()
        ),
        (json!(8), json!(2), json!(1))
    );
    assert_eq!(res["dims"], json!([2, 8]));
    let data: Vec<u64> = res["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(data.len(), 16);
    for row in data.chunks(8) {
        assert_eq!(row[0], 0, "{data:?}");
        assert_eq!(row[7], 255, "{data:?}");
        assert!(
            row.windows(2).all(|p| p[0] <= p[1]),
            "darker is farther: {data:?}"
        );
    }
    assert!(
        r["textBlob"]
            .as_str()
            .unwrap()
            .contains("not a JavaScript blob"),
        "{r}"
    );
    assert_eq!(r["own"], json!(7));
    drop(files);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's name is unique only within one runtime: removing another
/// runtime's plugin of the same name leaves this one's loaded model alone
/// (the models were kept by name for the whole process, so the removal
/// dropped it and the next estimate loaded the model file again).
#[test]
fn a_loaded_model_outlives_another_runtimes_plugin_of_the_same_name() {
    let (luna, dir, files) = start(
        r##"
try {
	const fake = "// huggingface/transformers.js: transformers.js_config\nconst x = 1;\nexport { x as pipeline };\n" + " ".repeat(100001);
	const tf = await import(URL.createObjectURL(new Blob([fake], { type: "text/javascript" })));
	tf.env.remoteHost = "__BASE__/";
	let loads = 0;
	tf.env.fetch = (url, o) => {
		if (String(url).endsWith(".onnx")) loads++;
		return fetch(url, o);
	};
	const pipe = await tf.pipeline("depth-estimation", "m/tiny", { device: "wasm", dtype: "q8" });
	pipe.processor.image_processor.size = { width: 4, height: 4 };
	const image = new tf.RawImage(new Uint8Array(4 * 4 * 4).fill(128), 4, 4, 4);
	await pipe(image);
	R.first = loads;
	R.ready = true;
	while ((await fetch("__BASE__/go")).status !== 200) await new Promise((res) => setTimeout(res, 20));
	await pipe(image);
	R.second = loads;
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 60, |r| {
        r["ready"] == json!(true) || !r["main"].is_null()
    });
    assert_eq!(r["first"], json!(1), "{r}");
    // Another runtime installs a plugin of the same name and removes it.
    let (other, other_dir, _other_files) = start("R.done = true;");
    until(&other, &other_dir, 30, |r| r["done"] == json!(true));
    other.uninstall("Depth");
    wait_until(&other, 30, |s| plugin(s, "Depth").is_none());
    other.shutdown();
    let _ = std::fs::remove_dir_all(&other_dir);
    files.lock().unwrap().insert("/go".into(), b"go".to_vec());
    let r = until(&luna, &dir, 60, |r| {
        !r["second"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["second"], json!(1), "the model was not loaded again: {r}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dom_matrix_reads_a_css_tilt() {
    let (luna, dir, _files) = start(
        r##"
try {
	const m = new DOMMatrix("perspective(1000px) rotateX(10deg) rotateY(-20deg)");
	R.m = [m.m31, m.m32, m.m33, m.m34, m.is2D];
	const flat = new DOMMatrix("translate(10px, 5px) scale(2)");
	R.flat = [flat.a, flat.b, flat.c, flat.d, flat.e, flat.f, flat.is2D, String(flat)];
	R.inv = Array.from(flat.inverse().multiply(flat).toFloat64Array()).map((v) => Math.round(v * 1e9) / 1e9);
	R.bad = (() => { try { new DOMMatrix("rotate(10)"); return "parsed"; } catch (e) { return e.name; } })();
	const el = document.createElement("div");
	el.style.transform = "rotateY(30deg)";
	document.body.appendChild(el);
	R.computed = getComputedStyle(el).transform;
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 30, |r| {
        !r["computed"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    let m: Vec<f64> = r["m"].as_array().unwrap()[..4]
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let (a, b) = (10f64.to_radians(), (-20f64).to_radians());
    // rotateX(a) rotateY(b) takes the z axis to (sin b, -sin a cos b, cos a cos b).
    for (got, want) in m
        .iter()
        .zip([b.sin(), -a.sin() * b.cos(), a.cos() * b.cos()])
    {
        assert!((got - want).abs() < 1e-9, "{r}");
    }
    assert!(m[3] < 0.0, "perspective is in m34: {r}");
    assert_eq!(r["m"][4], json!(false));
    assert_eq!(
        r["flat"],
        json!([2, 0, 0, 2, 10, 5, true, "matrix(2, 0, 0, 2, 10, 5)"])
    );
    assert_eq!(
        r["inv"],
        json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])
    );
    assert_eq!(r["bad"], json!("SyntaxError"));
    let computed = r["computed"].as_str().unwrap();
    assert!(computed.starts_with("matrix3d("), "{r}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `createImageBitmap` with resize options (3d-cover hands the model a
/// 640x640 bitmap made with `resizeQuality: "high"`): the size asked for,
/// one side alone keeping the aspect, and "high" averaging what it halves.
#[test]
fn image_bitmaps_resize_as_asked() {
    let (luna, dir, _files) = start(
        r##"
try {
	// 4x2: black and white columns.
	const c = new OffscreenCanvas(4, 2);
	const cx = c.getContext("2d");
	cx.fillStyle = "#000";
	cx.fillRect(0, 0, 4, 2);
	cx.fillStyle = "#fff";
	cx.fillRect(1, 0, 1, 2);
	cx.fillRect(3, 0, 1, 2);
	const read = (bm) => {
		const o = new OffscreenCanvas(bm.width, bm.height);
		const ox = o.getContext("2d");
		ox.drawImage(bm, 0, 0);
		return Array.from(ox.getImageData(0, 0, bm.width, bm.height).data.filter((_, i) => i % 4 === 0));
	};
	const high = await createImageBitmap(c, { resizeWidth: 2, resizeHeight: 1, resizeQuality: "high" });
	R.high = [high.width, high.height, ...read(high)];
	const tall = await createImageBitmap(c, { resizeHeight: 4 });
	R.tall = [tall.width, tall.height];
	const plain = await createImageBitmap(c);
	R.plain = [plain.width, plain.height];
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 30, |r| {
        !r["plain"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["tall"], json!([8, 4]));
    assert_eq!(r["plain"], json!([4, 2]));
    let high: Vec<i64> = r["high"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(&high[..2], &[2, 1]);
    for v in &high[2..] {
        assert!(
            (120..=136).contains(v),
            "each pixel is the mean of a black and a white one: {r}"
        );
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
