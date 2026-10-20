//! Canvas, image and Web Audio tests: 2D pixels read back through
//! getImageData, images decoded from data URLs, the WebGL2 context's
//! recorded draw list, and analysers fed by the output tap.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::canvas::{self, Frame, GlCmd};
use super::*;

const HEADER: &str = r#"export const unloads = new Set();
var core = luna?.core?.modules?.["@luna/core"];
const R = await core.ReactiveStore.getPluginStorage("Canvas.results", {});
const fail = (k, e) => { R[k] = "error: " + (e?.stack ?? e); };
"#;

/// The output tap is one for the process: tests that feed it take turns.
static TAP: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read(dir: &Path) -> Value {
    super::storage::read_json(&dir.join("settings").join(format!(
        "{}.json",
        super::storage::safe_name("Canvas.results")
    )))
    .and_then(|v| v.get("Canvas.results").cloned())
    .unwrap_or(Value::Null)
}

fn start(name: &str, body: &str) -> (Luna, PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        format!("/{name}.json"),
        json!({ "name": name, "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        format!("/{name}.mjs"),
        format!("{HEADER}{body}").into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("canvas");
    let host = Arc::new(MockHost::default());
    let luna = Luna::start(config(&dir), host).unwrap();
    luna.install(&format!("{base}/{name}.mjs"));
    (luna, dir)
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

fn finish(luna: Luna, dir: PathBuf) {
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn canvas_2d_draws_pixels_that_read_back() {
    let (luna, dir) = start(
        "Canvas2d",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = 40;
	c.height = 20;
	const x = c.getContext("2d");
	R.same = x === c.getContext("2d");
	R.gl_after_2d = c.getContext("webgl2") === null;
	x.fillStyle = "rgb(255, 0, 0)";
	x.fillRect(0, 0, 10, 10);
	x.fillStyle = "#00ff0080";
	x.fillRect(10, 0, 10, 10);
	x.save();
	x.translate(20, 0);
	x.beginPath();
	x.arc(5, 5, 4, 0, Math.PI * 2);
	x.fillStyle = "blue";
	x.fill();
	x.restore();
	x.strokeStyle = "white";
	x.lineWidth = 2;
	x.beginPath();
	x.moveTo(0, 15);
	x.lineTo(40, 15);
	x.stroke();
	const px = (X, Y) => Array.from(x.getImageData(X, Y, 1, 1).data);
	R.red = px(5, 5);
	R.green = px(15, 5);
	R.blue = px(25, 5);
	R.arc_corner = px(21, 1);
	R.line = px(20, 15);
	R.empty = px(35, 5);
	x.clearRect(0, 0, 5, 5);
	R.cleared = px(1, 1);
	x.fillStyle = "BLUE";
	R.style = x.fillStyle;
	const g = x.createLinearGradient(0, 0, 40, 0);
	g.addColorStop(0, "black");
	g.addColorStop(1, "white");
	x.fillStyle = g;
	x.fillRect(0, 18, 40, 2);
	R.grad_l = px(1, 19)[0];
	R.grad_r = px(38, 19)[0];
	const id = x.createImageData(2, 1);
	id.data.set([1, 2, 3, 255, 9, 8, 7, 255]);
	x.putImageData(id, 30, 10);
	R.put = Array.from(x.getImageData(30, 10, 2, 1).data);
	const t = document.createElement("canvas");
	t.width = 60;
	t.height = 20;
	const tx = t.getContext("2d");
	tx.font = "bold 16px sans-serif";
	tx.fillStyle = "#fff";
	tx.textBaseline = "top";
	tx.fillText("Hi", 2, 2);
	R.text_width = tx.measureText("Hi").width;
	R.text_ink = Array.from(tx.getImageData(0, 0, 60, 20).data).filter((v, i) => i % 4 === 3 && v > 0).length;
	const url = c.toDataURL();
	R.url = url.slice(0, 22);
	const img = new Image();
	img.onload = () => {
		try {
			R.loaded = [img.complete, img.naturalWidth, img.naturalHeight, img.width];
			const d = document.createElement("canvas");
			d.width = 40;
			d.height = 20;
			const dx = d.getContext("2d");
			dx.drawImage(img, 0, 0);
			R.drawn = Array.from(dx.getImageData(5, 5, 1, 1).data);
			dx.drawImage(img, 0, 0, 10, 10, 0, 0, 40, 20);
			R.scaled = Array.from(dx.getImageData(30, 15, 1, 1).data);
		} catch (e) { fail("loaded", e); }
	};
	img.onerror = () => { R.loaded = "error"; };
	R.complete_before = img.complete;
	img.src = url;
	const bad = new Image();
	bad.addEventListener("error", () => { R.bad = [bad.complete, bad.naturalWidth]; });
	bad.src = "data:image/png;base64,AAAA";
	const p = new Path2D("M0 0 L10 0 L10 10 Z");
	const e = document.createElement("canvas");
	e.width = e.height = 10;
	const ex = e.getContext("2d");
	ex.fill(p);
	R.path2d = [ex.getImageData(8, 2, 1, 1).data[3], ex.getImageData(2, 8, 1, 1).data[3]];
	c.width = 8;
	R.resized = [x.getImageData(0, 0, 1, 1).data[3], x.fillStyle];
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 30, |r| {
        !r["loaded"].is_null() && !r["bad"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["same"], json!(true));
    assert_eq!(r["gl_after_2d"], json!(true));
    assert_eq!(r["red"], json!([255, 0, 0, 255]));
    let green = r["green"].as_array().unwrap();
    assert_eq!(green[1], json!(255));
    assert!((126..=130).contains(&green[3].as_i64().unwrap()), "{r}");
    assert_eq!(r["blue"], json!([0, 0, 255, 255]));
    assert_eq!(r["arc_corner"][3], json!(0));
    assert_eq!(r["line"], json!([255, 255, 255, 255]));
    assert_eq!(r["empty"], json!([0, 0, 0, 0]));
    assert_eq!(r["cleared"], json!([0, 0, 0, 0]));
    assert_eq!(r["style"], json!("#0000ff"));
    assert!(
        r["grad_l"].as_i64().unwrap() < 30 && r["grad_r"].as_i64().unwrap() > 220,
        "{r}"
    );
    assert_eq!(r["put"], json!([1, 2, 3, 255, 9, 8, 7, 255]));
    assert!(r["text_width"].as_f64().unwrap() > 10.0, "{r}");
    assert!(r["text_ink"].as_i64().unwrap() > 20, "{r}");
    assert_eq!(r["url"], json!("data:image/png;base64,"));
    assert_eq!(r["complete_before"], json!(true));
    assert_eq!(r["loaded"], json!([true, 40, 20, 40]), "{r}");
    assert_eq!(r["drawn"], json!([255, 0, 0, 255]));
    assert_eq!(r["scaled"], json!([255, 0, 0, 255]));
    assert_eq!(r["bad"], json!([true, 0]));
    assert_eq!(r["path2d"], json!([255, 0]));
    assert_eq!(r["resized"], json!([0, "#000000"]));
    finish(luna, dir);
}

#[test]
fn canvas_in_the_document_publishes_its_pixels() {
    let (luna, dir) = start(
        "CanvasPub",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = 4;
	c.height = 2;
	c.style.cssText = "width: 8px; height: 4px";
	document.body.appendChild(c);
	const x = c.getContext("2d");
	let n = 0;
	const frame = () => {
		x.fillStyle = n % 2 ? "#00ff00" : "#ff0000";
		x.fillRect(0, 0, 4, 2);
		R.frames = ++n;
		if (n < 3) requestAnimationFrame(frame);
	};
	requestAnimationFrame(frame);
} catch (e) { fail("main", e); }
"##,
    );
    until(&luna, &dir, 20, |r| r["frames"] == json!(3));
    let o = {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            luna.sync(Duration::from_secs(5));
            let o = luna.dom_overlay();
            let found = o
                .injected
                .iter()
                .flat_map(|i| &i.nodes)
                .find(|n| n.tag == "canvas")
                .cloned();
            if let Some(n) = found.filter(|n| n.markup.is_some()) {
                break n;
            }
            assert!(Instant::now() < end, "no canvas in the overlay");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let id: u64 = o
        .markup
        .unwrap()
        .strip_prefix("canvas:")
        .unwrap()
        .parse()
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    luna.sync(Duration::from_secs(5));
    match canvas::frame(id) {
        Some(Frame::Pixels(p)) => {
            assert_eq!((p.w, p.h), (4, 2));
            // The third frame drew red.
            assert_eq!(&p.rgba[..4], &[255, 0, 0, 255]);
        }
        _ => panic!("no pixel frame"),
    }
    finish(luna, dir);
}

#[test]
fn webgl2_records_a_draw_list() {
    let (luna, dir) = start(
        "CanvasGl",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = 64;
	c.height = 16;
	document.body.appendChild(c);
	const gl = c.getContext("webgl2", { alpha: true, premultipliedAlpha: true });
	R.is_gl2 = gl instanceof WebGL2RenderingContext;
	R.no_2d = c.getContext("2d") === null;
	const sh = (t, s) => { const o = gl.createShader(t); gl.shaderSource(o, s); gl.compileShader(o); if (!gl.getShaderParameter(o, gl.COMPILE_STATUS)) throw new Error("compile"); return o; };
	const p = gl.createProgram();
	gl.attachShader(p, sh(gl.VERTEX_SHADER, "#version 300 es\nin vec2 a_position;\nvoid main() { gl_Position = vec4(a_position, 0.0, 1.0); }\n"));
	gl.attachShader(p, sh(gl.FRAGMENT_SHADER, "#version 300 es\nprecision highp float;\nuniform vec2 u_resolution;\nuniform float u_amplitudes[4];\nout vec4 fragColor;\nvoid main() { fragColor = vec4(gl_FragCoord.xy / u_resolution, u_amplitudes[0], 1.0); }\n"));
	gl.linkProgram(p);
	R.linked = gl.getProgramParameter(p, gl.LINK_STATUS);
	const vao = gl.createVertexArray();
	const buf = gl.createBuffer();
	gl.bindVertexArray(vao);
	gl.bindBuffer(gl.ARRAY_BUFFER, buf);
	gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
	const loc = gl.getAttribLocation(p, "a_position");
	R.loc = loc;
	gl.enableVertexAttribArray(loc);
	gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
	gl.bindVertexArray(null);
	gl.viewport(0, 0, 64, 16);
	gl.enable(gl.BLEND);
	gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
	requestAnimationFrame(() => {
		gl.clearColor(0, 0, 0, 0);
		gl.clear(gl.COLOR_BUFFER_BIT);
		gl.useProgram(p);
		gl.uniform2f(gl.getUniformLocation(p, "u_resolution"), 64, 16);
		gl.uniform1fv(gl.getUniformLocation(p, "u_amplitudes"), new Float32Array([0.5, 0.25, 0, 1]));
		gl.bindVertexArray(vao);
		gl.drawArrays(gl.TRIANGLES, 0, 3);
		gl.bindVertexArray(null);
		R.id = gl._id;
	});
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 20, |r| {
        !r["id"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["is_gl2"], json!(true));
    assert_eq!(r["no_2d"], json!(true));
    assert_eq!(r["linked"], json!(true));
    assert_eq!(r["loc"], json!(0));
    let id = r["id"].as_u64().unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let f = loop {
        luna.sync(Duration::from_secs(5));
        if let Some(Frame::Gl(f)) = canvas::frame(id) {
            break f;
        }
        assert!(Instant::now() < end, "no GL frame");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((f.w, f.h), (64, 16));
    assert_eq!(f.cmds.len(), 2, "{:?}", f.cmds);
    assert!(matches!(&f.cmds[0], GlCmd::Clear { color, fb: None } if color == &[0.0; 4]));
    let GlCmd::Draw(d) = &f.cmds[1] else { panic!() };
    assert_eq!((d.mode, d.first, d.count), (4, 0, 3));
    assert_eq!(d.blend, Some([0x302, 0x303, 0x302, 0x303]));
    assert!(
        d.uniforms
            .contains(&("u_resolution".into(), "2f".into(), vec![64.0, 16.0]))
    );
    assert!(d.uniforms.contains(&(
        "u_amplitudes".into(),
        "1fv".into(),
        vec![0.5, 0.25, 0.0, 1.0]
    )));
    assert_eq!(d.attribs.len(), 1);
    assert_eq!((d.attribs[0].loc, d.attribs[0].size), (0, 2));
    let prog = canvas::gl_program(d.prog).unwrap();
    assert_eq!(prog.attribs, vec!["a_position"]);
    assert!(prog.fs.contains("u_amplitudes[4]"));
    let (_, bytes) = canvas::gl_buffer(d.attribs[0].buffer).unwrap();
    assert_eq!(bytes.len(), 24);
    finish(luna, dir);
}

/// radiant-lyrics' kawarp background: an image uploaded to a texture,
/// blurred into a framebuffer once, then sampled every frame.
#[test]
fn webgl_textures_and_framebuffers_are_recorded_and_kept() {
    let (luna, dir) = start(
        "CanvasTex",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = 32;
	c.height = 32;
	document.body.appendChild(c);
	const gl = c.getContext("webgl", { preserveDrawingBuffer: true });
	const sh = (t, s) => { const o = gl.createShader(t); gl.shaderSource(o, s); gl.compileShader(o); return o; };
	const p = gl.createProgram();
	gl.attachShader(p, sh(gl.VERTEX_SHADER, "attribute vec2 a_position;\nvarying vec2 v_uv;\nvoid main() { v_uv = a_position * 0.5 + 0.5; gl_Position = vec4(a_position, 0.0, 1.0); }\n"));
	gl.attachShader(p, sh(gl.FRAGMENT_SHADER, "precision mediump float;\nuniform sampler2D u_tex;\nvarying vec2 v_uv;\nvoid main() { gl_FragColor = texture2D(u_tex, v_uv); }\n"));
	gl.linkProgram(p);
	const buf = gl.createBuffer();
	gl.bindBuffer(gl.ARRAY_BUFFER, buf);
	gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
	const loc = gl.getAttribLocation(p, "a_position");
	gl.enableVertexAttribArray(loc);
	gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
	const src = gl.createTexture();
	gl.bindTexture(gl.TEXTURE_2D, src);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
	gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, new ImageData(new Uint8ClampedArray([255, 0, 0, 255, 0, 0, 255, 255]), 2, 1));
	const target = gl.createTexture();
	gl.bindTexture(gl.TEXTURE_2D, target);
	gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 8, 8, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
	const fb = gl.createFramebuffer();
	gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
	gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
	R.status = gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
	gl.useProgram(p);
	gl.uniform1i(gl.getUniformLocation(p, "u_tex"), 0);
	gl.viewport(0, 0, 8, 8);
	gl.clearColor(0, 0, 0, 1);
	gl.clear(gl.COLOR_BUFFER_BIT);
	gl.bindTexture(gl.TEXTURE_2D, src);
	gl.drawArrays(gl.TRIANGLES, 0, 6);
	gl.bindFramebuffer(gl.FRAMEBUFFER, null);
	R.keys = [src._key, target._key];
	// Kept alive, as kawarp keeps its source texture.
	globalThis.__srcTexture = src;
	let frames = 0;
	const loop = () => {
		gl.viewport(0, 0, 32, 32);
		gl.bindTexture(gl.TEXTURE_2D, target);
		gl.drawArrays(gl.TRIANGLES, 0, 6);
		if (++frames < 3) requestAnimationFrame(loop);
		else R.id = gl._id;
	};
	requestAnimationFrame(loop);
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 20, |r| {
        !r["id"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["status"], json!(true), "{r}");
    let id = r["id"].as_u64().unwrap();
    let (src, target) = (
        r["keys"][0].as_u64().unwrap(),
        r["keys"][1].as_u64().unwrap(),
    );
    let end = Instant::now() + Duration::from_secs(5);
    let f = loop {
        luna.sync(Duration::from_secs(5));
        if let Some(Frame::Gl(f)) = canvas::frame(id)
            && f.version >= 3
        {
            break f;
        }
        assert!(Instant::now() < end, "no GL frame");
        std::thread::sleep(Duration::from_millis(20));
    };
    // The source pixels are kept straight, row by row.
    let px = canvas::gl_texture(src, |t| {
        let t = t.unwrap();
        (t.w, t.h, t.rgba.as_ref().map(|p| p.to_vec()))
    });
    assert_eq!(px, (2, 1, Some(vec![255, 0, 0, 255, 0, 0, 255, 255])));
    // The latest frame only samples the framebuffer's texture on the canvas.
    let GlCmd::Draw(d) = f.cmds.last().unwrap() else {
        panic!("{:?}", f.cmds)
    };
    assert_eq!(d.fb, None);
    assert_eq!(d.tex.len(), 1);
    assert_eq!((d.tex[0].unit, d.tex[0].tex), (0, target));
    // The frame that filled the framebuffer is still carried for contexts
    // that have not run it.
    let filled = f
        .prior
        .iter()
        .find(|p| p.cmds.iter().any(|c| c.target() == Some(target)))
        .expect("the framebuffer pass is kept");
    let kinds: Vec<(&str, Option<u64>)> = filled
        .cmds
        .iter()
        .map(|c| match c {
            GlCmd::Tex { tex, .. } => ("tex", Some(*tex)),
            GlCmd::Clear { fb, .. } => ("clear", *fb),
            GlCmd::Draw(d) => ("draw", d.fb),
            GlCmd::Mip { tex } => ("mip", Some(*tex)),
        })
        .collect();
    assert!(kinds.contains(&("tex", Some(src))), "{kinds:?}");
    assert!(kinds.contains(&("clear", Some(target))), "{kinds:?}");
    let pass = filled
        .cmds
        .iter()
        .find_map(|c| match c {
            GlCmd::Draw(d) if d.fb == Some(target) => Some(d),
            _ => None,
        })
        .unwrap();
    assert_eq!(pass.vp, Some([0, 0, 8, 8]));
    assert_eq!(pass.tex[0].tex, src);
    assert_eq!(pass.tex[0].params[0], 0x2601);
    finish(luna, dir);
}

/// oneko's sprite sheet is a GIF: it loads (size first, pixels when drawn).
#[test]
fn gif_images_load_and_draw() {
    let (luna, dir) = start(
        "CanvasGif",
        r##"
try {
	const img = new Image();
	img.onload = () => {
		R.size = [img.naturalWidth, img.naturalHeight];
		const c = document.createElement("canvas");
		c.width = c.height = 1;
		const x = c.getContext("2d");
		x.drawImage(img, 0, 0);
		R.px = Array.from(x.getImageData(0, 0, 1, 1).data);
	};
	img.onerror = () => (R.px = "error");
	img.src = "data:image/gif;base64,R0lGODlhAQABAIAAAAUEBAAAACwAAAAAAQABAAACAkQBADs=";
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 20, |r| {
        !r["px"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["size"], json!([1, 1]), "{r}");
    assert_eq!(r["px"], json!([5, 4, 4, 255]), "{r}");
    finish(luna, dir);
}

#[test]
fn analysers_read_the_output_tap() {
    let _tap = TAP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A 1 kHz sine, left louder. The ring is filled with it before the
    // plugin starts, so every window the plugin reads is whole however
    // soon it first looks.
    let feed = |n: u64| {
        let s = (std::f64::consts::TAU * 1000.0 * n as f64 / 48_000.0).sin() as f32;
        crate::audio_tap::push(&[s * 0.8, s * 0.2], 48_000);
    };
    let filled = 2 * crate::audio_tap::FRAMES as u64;
    (0..filled).for_each(feed);
    let (luna, dir) = start(
        "CanvasAudio",
        r##"
try {
	const ctx = new AudioContext();
	const a = ctx.createAnalyser();
	R.bins = a.frequencyBinCount;
	try { a.fftSize = 1000; R.bad_size = "accepted"; } catch (e) { R.bad_size = e.name; }
	a.fftSize = 2048;
	a.smoothingTimeConstant = 0;
	const idle = new Uint8Array(a.frequencyBinCount);
	a.getByteFrequencyData(idle);
	R.idle_max = Math.max(...idle);
	const el = document.querySelector("video, audio");
	R.player = [!!el, el instanceof HTMLMediaElement, typeof el.captureStream];
	const stream = el.captureStream();
	R.tracks = stream.getAudioTracks().length;
	const src = ctx.createMediaStreamSource(stream);
	const split = ctx.createChannelSplitter(2);
	const left = ctx.createAnalyser();
	src.connect(a);
	src.connect(split);
	split.connect(left, 0);
	const poll = setInterval(() => {
		const f = new Float32Array(a.frequencyBinCount);
		a.getFloatFrequencyData(f);
		let peak = 0;
		for (let k = 1; k < f.length; k++) if (f[k] > f[peak]) peak = k;
		const t = new Float32Array(left.fftSize);
		left.getFloatTimeDomainData(t);
		const amp = Math.max(...t.map(Math.abs));
		if (f[peak] > -40 && amp > 0.1) {
			const b = new Uint8Array(a.frequencyBinCount);
			a.getByteFrequencyData(b);
			const tb = new Uint8Array(32);
			left.getByteTimeDomainData(tb);
			R.result = { peak, db: f[peak], byte: b[peak], amp, byteRange: [Math.min(...tb), Math.max(...tb)], rate: ctx.sampleRate };
			clearInterval(poll);
		}
	}, 20);
	unloads.add(() => clearInterval(poll));
} catch (e) { fail("main", e); }
"##,
    );
    // Go on feeding the sine through the tap until the plugin sees it.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let feeder = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut n = filled;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                for _ in 0..480 {
                    feed(n);
                    n += 1;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let r = until(&luna, &dir, 30, |r| {
        !r["result"].is_null() || !r["main"].is_null()
    });
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = feeder.join();
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["bins"], json!(1024));
    assert_eq!(r["bad_size"], json!("IndexSizeError"));
    assert_eq!(r["idle_max"], json!(0));
    assert_eq!(r["player"], json!([true, true, "function"]));
    assert_eq!(r["tracks"], json!(1));
    let res = &r["result"];
    // 1000 Hz at 48 kHz in 2048 points: bin 42.7.
    let peak = res["peak"].as_i64().unwrap();
    assert!((42..=43).contains(&peak), "{r}");
    // A 0.5 mono sine through Blackman: 20*log10(0.5 * 0.21) = -19.6 dB.
    let db = res["db"].as_f64().unwrap();
    assert!((-24.0..-16.0).contains(&db), "{r}");
    assert!(res["byte"].as_i64().unwrap() > 200, "{r}");
    // The left channel alone peaks at 0.8.
    let amp = res["amp"].as_f64().unwrap();
    assert!((0.7..0.85).contains(&amp), "{r}");
    let br = res["byteRange"].as_array().unwrap();
    assert!(
        br[0].as_i64().unwrap() < 128 && br[1].as_i64().unwrap() > 128,
        "{r}"
    );
    finish(luna, dir);
}

#[test]
fn audiovisualiser_bands_come_from_the_output_tap() {
    let _tap = TAP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // jxnxsdev's audiovisualiser: a native module that runs ffmpeg on a
    // downloaded track, and MediaItem.download to fetch it. Until that
    // module is registered a download needs the permission to save TIDAL
    // audio (denied here); then its `analyse` answers with live bands from
    // the tap and the download saves nothing, without asking.
    let feed = |n: u64| {
        let s = (std::f64::consts::TAU * 1000.0 * n as f64 / 48_000.0).sin() as f32;
        // The same signal as `analysers_read_the_output_tap`, which may run
        // alongside: the tap is one for the process.
        crate::audio_tap::push(&[s * 0.8, s * 0.2], 48_000);
    };
    let filled = 2 * crate::audio_tap::FRAMES as u64;
    (0..filled).for_each(feed);
    let (luna, dir) = start(
        "Visualiser",
        r##"
try {
	const lib = luna.core.modules["@luna/lib"];
	const dl = (path) => lib.MediaItem.prototype.download.call({}, path).then(() => "saved nothing", (e) => "refused: " + e.message);
	R.before = await dl("/MusicVis/Cache/1.flac");
	const code = `"use strict"; const { execSync } = require("child_process");
exports.analyse = async (f) => { execSync(\`ffmpeg -y -i "\${f}" -map 0:a:0 -ac 1 -ar 44100 -f f32le out.pcm\`); return new Map(); };`;
	const ch = await __ipcRenderer.invoke("__Luna.registerNative", "@jxnxsdev/audiovisualiser/analyser.native.ts", code);
	R.after = await dl("/MusicVis/Cache/1.flac");
	const other = await __ipcRenderer.invoke("__Luna.registerNative", "Visualiser/other.native.ts", `exports.x = () => 1;`);
	R.other = await __ipcRenderer.invoke(other, "x");
	const map = await __ipcRenderer.invoke(ch, "analyse", "/MusicVis/Cache/1.flac");
	const entries = Array.from(map.entries()).sort((a, b) => a[0] - b[0]);
	R.timeline = entries.map((e) => e[0]);
	const live = entries[0][1];
	R.length = live.length;
	const poll = setInterval(() => {
		const bands = live.map((s) => (Number.isFinite(s) ? s : 0));
		const top = bands.indexOf(Math.max(...bands));
		if (bands[top] > 1) {
			R.result = { bands, top };
			clearInterval(poll);
		}
	}, 20);
	unloads.add(() => clearInterval(poll));
} catch (e) { fail("main", e); }
"##,
    );
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let feeder = {
        let stop = stop.clone();
        let (luna, asked) = (luna.clone(), asked.clone());
        std::thread::spawn(move || {
            let mut n = filled;
            let mut last = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                // Each question once (the snapshot shows it until the
                // runtime has the answer).
                if let Some(q) = luna.pending_permission()
                    && q.id != last
                {
                    last = q.id;
                    asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    luna.answer_permission(q.id, ProgramChoice::Deny);
                }
                for _ in 0..480 {
                    feed(n);
                    n += 1;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let r = until(&luna, &dir, 30, |r| {
        !r["result"].is_null() || !r["main"].is_null()
    });
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = feeder.join();
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(
        r["before"],
        json!("refused: EACCES: you did not allow this plugin to save TIDAL songs as files")
    );
    assert_eq!(r["after"], json!("saved nothing"));
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "only the download before the module asked"
    );
    assert_eq!(r["other"], json!(1), "other natives still load: {r}");
    assert_eq!(r["timeline"], json!([0]));
    assert_eq!(r["length"], json!(8));
    // 1 kHz falls in the 848-1821 Hz band; on the plugin's own scale a
    // 0.5 sine sums to tens, not fractions (its peak normaliser floors at 1).
    assert_eq!(r["result"]["top"], json!(4), "{r}");
    let peak = r["result"]["bands"][4].as_f64().unwrap();
    assert!((5.0..200.0).contains(&peak), "{r}");
    finish(luna, dir);
}

#[test]
fn video_elements_load_vp9_webm_and_keep_a_clock() {
    use base64::Engine as _;
    // Three 32x24 frames a tenth of a second apart (red, green, blue), the
    // left half transparent through the WebM alpha channel.
    let clip =
        base64::engine::general_purpose::STANDARD.encode(include_bytes!("fixtures/alpha-vp9.webm"));
    let (luna, dir) = start(
        "Video",
        &format!(
            r##"
try {{
	const v = document.createElement("video");
	R.can = [v.canPlayType('video/webm; codecs="vp9"'), v.canPlayType("video/webm; codecs=vp8"), v.canPlayType("video/mp4"), v.readyState, v.paused];
	const events = [];
	for (const t of ["loadstart", "loadedmetadata", "loadeddata", "canplay", "play", "playing", "pause", "ratechange"]) v.addEventListener(t, () => events.push(t));
	v.classList.add("Clip");
	v.loop = true;
	v.muted = true;
	v.onloadeddata = () => {{
		R.loaded = [v.videoWidth, v.videoHeight, v.duration, v.readyState, v.loop, v.muted];
		v.playbackRate = 2;
		v.play();
		const started = Date.now();
		setTimeout(() => {{
			R.running = v.currentTime;
			R.elapsed = (Date.now() - started) / 1000;
			v.pause();
			R.held = [v.paused, v.currentTime === v.currentTime];
			v.currentTime = 0.15;
			R.seek = v.currentTime;
			R.events = events;
		}}, 120);
	}};
	v.src = "data:video/webm;base64,{clip}";
	document.body.appendChild(v);
	const bad = document.createElement("video");
	bad.onerror = () => (R.bad = [bad.error?.code, bad.readyState]);
	bad.src = "data:video/webm;base64,R0lGODlh";
}} catch (e) {{ fail("main", e); }}
"##
        ),
    );
    let r = until(&luna, &dir, 20, |r| {
        !r["events"].is_null() && !r["bad"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["can"], json!(["probably", "", "", 0, true]));
    assert_eq!(r["loaded"], json!([32, 24, 0.3, 4, true, true]), "{r}");
    // Twice the rate for the time that really passed (0.12 s asked; a busy
    // machine runs the timer late), looping every 0.3 s.
    let running = r["running"].as_f64().unwrap();
    let expected = (r["elapsed"].as_f64().unwrap() * 2.0) % 0.3;
    let off = (running - expected)
        .abs()
        .min(0.3 - (running - expected).abs());
    assert!(off < 0.06, "{r}");
    assert_eq!(r["held"], json!([true, true]));
    assert_eq!(r["seek"], json!(0.15));
    assert_eq!(
        r["events"],
        json!([
            "loadstart",
            "loadedmetadata",
            "loadeddata",
            "ratechange",
            "canplay",
            "play",
            "playing",
            "pause"
        ]),
        "{r}"
    );
    assert_eq!(r["bad"], json!([4, 0]));
    // The overlay names the clip; its frames decode at the size asked.
    let node = {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            luna.sync(Duration::from_secs(5));
            let o = luna.dom_overlay();
            let found = o
                .injected
                .iter()
                .flat_map(|i| &i.nodes)
                .find(|n| n.tag == "video")
                .cloned();
            if let Some(n) = found.filter(|n| n.markup.is_some()) {
                break n;
            }
            assert!(Instant::now() < end, "no video in the overlay");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let id: u64 = node
        .markup
        .unwrap()
        .strip_prefix("video:")
        .unwrap()
        .parse()
        .unwrap();
    let clip = super::video::clip(id).expect("the clip is loaded");
    // Paused at 0.15 s: the second frame.
    let (t, runs) = clip.time(Instant::now());
    assert!((t - 0.15).abs() < 1e-6 && !runs);
    let frames = clip.frames([32, 24]);
    let end = Instant::now() + Duration::from_secs(10);
    while !frames.done() {
        assert!(Instant::now() < end, "decoding never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
    let px = |time: f64, x: usize| {
        let (_, img) = frames.frame_at(time).unwrap();
        let c = img.pixels[12 * 32 + x];
        [c.r(), c.g(), c.b(), c.a()]
    };
    let near = |got: [u8; 4], want: [u8; 4]| {
        got.iter()
            .zip(want)
            .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() <= 12)
    };
    assert_eq!(px(0.0, 4)[3], 0, "the left half is transparent");
    assert!(near(px(0.0, 28), [255, 0, 0, 255]), "{:?}", px(0.0, 28));
    assert!(near(px(t, 28), [0, 255, 0, 255]), "{:?}", px(t, 28));
    assert!(near(px(0.25, 28), [0, 0, 255, 255]), "{:?}", px(0.25, 28));
    // Smaller drawings reuse these frames; nothing decodes again.
    assert!(Arc::ptr_eq(&frames, &clip.frames([16, 12])));
    finish(luna, dir);
    assert!(
        super::video::clip(id).is_none(),
        "stopping Luna frees clips"
    );
}

#[test]
fn webgl_draws_past_their_buffers_are_dropped_with_an_error() {
    // A 3-vertex buffer: drawing a million vertices, or indices past it,
    // would have the driver read past the buffer. WebGL refuses both with
    // INVALID_OPERATION; so does the shim, and the draws never reach GL.
    let (luna, dir) = start(
        "CanvasGlBounds",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = c.height = 8;
	document.body.appendChild(c);
	const gl = c.getContext("webgl2");
	const sh = (t, s) => { const o = gl.createShader(t); gl.shaderSource(o, s); gl.compileShader(o); return o; };
	const p = gl.createProgram();
	gl.attachShader(p, sh(gl.VERTEX_SHADER, "#version 300 es\nin vec2 a;\nvoid main() { gl_Position = vec4(a, 0.0, 1.0); }\n"));
	gl.attachShader(p, sh(gl.FRAGMENT_SHADER, "#version 300 es\nprecision highp float;\nout vec4 o;\nvoid main() { o = vec4(1.0); }\n"));
	gl.linkProgram(p);
	const vao = gl.createVertexArray();
	gl.bindVertexArray(vao);
	gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
	gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
	gl.enableVertexAttribArray(0);
	gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
	gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, gl.createBuffer());
	gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array([0, 1, 2, 0, 1, 900]), gl.STATIC_DRAW);
	gl.useProgram(p);
	requestAnimationFrame(() => {
		gl.drawArrays(gl.TRIANGLES, 0, 3);
		gl.drawArrays(gl.TRIANGLES, 0, 1000000);
		gl.drawElements(gl.TRIANGLES, 3, gl.UNSIGNED_SHORT, 0);
		gl.drawElements(gl.TRIANGLES, 3, gl.UNSIGNED_SHORT, 6);
		gl.drawElements(gl.TRIANGLES, 30, gl.UNSIGNED_SHORT, 0);
		R.id = gl._id;
		requestAnimationFrame(() => {
			R.err = gl.getError();
			R.again = gl.getError();
		});
	});
} catch (e) { fail("main", e); }
"##,
    );
    let r = until(&luna, &dir, 20, |r| {
        !r["again"].is_null() || !r["main"].is_null()
    });
    assert_eq!(r["main"], Value::Null, "{r}");
    assert_eq!(r["err"], json!(canvas::INVALID_OPERATION));
    assert_eq!(r["again"], json!(0), "getError clears");
    let id = r["id"].as_u64().unwrap();
    let Some(Frame::Gl(f)) = canvas::frame(id) else {
        panic!("no GL frame")
    };
    let draws: Vec<(i32, bool)> = f
        .cmds
        .iter()
        .filter_map(|c| match c {
            GlCmd::Draw(d) => Some((d.count, d.elements.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(
        draws,
        vec![(3, false), (3, true)],
        "only the safe draws stay"
    );
    finish(luna, dir);
}

#[test]
fn oversized_image_data_and_gradient_counts_end_quickly() {
    // An ImageData claiming a billion rows: the Rust loop behind it used to
    // run past the watchdog's reach.
    let (luna, dir) = start(
        "CanvasBounds",
        r##"
try {
	const c = document.createElement("canvas");
	c.width = c.height = 4;
	const x = c.getContext("2d");
	const t0 = Date.now();
	x.putImageData({ width: 1e9, height: 1e9, data: new Uint8ClampedArray(4) }, 0, 0);
	x.putImageData({ width: 2, height: 1e9, data: new Uint8ClampedArray(8) }, -1e9, -1e9);
	R.ms = Date.now() - t0;
} catch (e) { fail("main", e); R.ms = -1; }
"##,
    );
    let r = until(&luna, &dir, 20, |r| !r["ms"].is_null());
    assert_eq!(r["main"], Value::Null, "{r}");
    assert!((0..2000).contains(&r["ms"].as_i64().unwrap()), "{r}");
    finish(luna, dir);
}
