//! Depth estimation for Luna plugins, run natively.
//!
//! A plugin that imports transformers.js (`@visiuun/3d-cover` loads it from
//! a CDN into a worker) gets Tidalfast's stand-in for it (`js/ml.js`): its
//! `pipeline("depth-estimation", ...)` fetches the same model files through
//! the plugin's own `env.fetch`, and the model runs here instead of in
//! onnxruntime-web's WebAssembly, which an interpreter would take minutes
//! over. The image is prepared and the answer finished as transformers.js
//! does (resize, rescale, normalise; then the depth resized back to the
//! image and spread over 0..255).
//!
//! Bytes cross from JavaScript through `__depth.stage`/`take` (no base64);
//! the work goes to one low-priority thread that keeps the loaded models
//! while it is busy and ends, dropping them, after [`IDLE`] without work. A
//! model asked for after that answers [`UNLOADED`] and the stand-in loads it
//! again from the plugin's own cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rquickjs::{ArrayBuffer, Ctx, Exception, Function, Object};
use serde_json::{Value, json};

use super::onnx::{self, Data, Tensor};
use super::onnx_run::Plan;

/// Without work for this long the thread ends and its models go.
const IDLE: Duration = Duration::from_secs(120);
/// Staged bytes nobody took are dropped after this long.
const STAGE_TTL: Duration = Duration::from_secs(120);
/// The largest model or image a plugin may hand over.
const MAX_BYTES: usize = 512 * 1024 * 1024;
/// Models one plugin may keep loaded at once.
const MODELS_PER_PLUGIN: usize = 2;
/// The answer for a model the idle thread already dropped.
pub(super) const UNLOADED: &str = "depth model unloaded";

static NEXT: AtomicU64 = AtomicU64::new(1);
/// Bytes handed over and not taken yet, with when they came.
type Staged = HashMap<u64, (Instant, Vec<u8>)>;
static STAGED: LazyLock<Mutex<Staged>> = LazyLock::new(Mutex::default);
static WORKER: Mutex<Option<Sender<Job>>> = Mutex::new(None);

fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn stage(bytes: Vec<u8>) -> u64 {
    let id = next_id();
    let mut staged = STAGED.lock().unwrap_or_else(PoisonError::into_inner);
    staged.retain(|_, (at, _)| at.elapsed() < STAGE_TTL);
    staged.insert(id, (Instant::now(), bytes));
    id
}

fn take(id: u64) -> Option<Vec<u8>> {
    STAGED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id)
        .map(|(_, b)| b)
}

// ------------------------------------------------------------------ natives

fn js_stage<'js>(
    ctx: Ctx<'js>,
    buf: ArrayBuffer<'js>,
    off: f64,
    len: f64,
) -> rquickjs::Result<f64> {
    // SAFETY: copied out at once; no JavaScript runs while the slice lives.
    let all =
        unsafe { buf.as_bytes() }.ok_or_else(|| Exception::throw_type(&ctx, "detached buffer"))?;
    let off = (off.max(0.0) as usize).min(all.len());
    let len = (len.max(0.0) as usize).min(all.len() - off);
    if len > MAX_BYTES {
        return Err(Exception::throw_range(
            &ctx,
            "too many bytes for a depth model",
        ));
    }
    Ok(stage(all[off..off + len].to_vec()) as f64)
}

fn js_take<'js>(ctx: Ctx<'js>, id: f64) -> rquickjs::Result<Option<ArrayBuffer<'js>>> {
    take(id as u64)
        .map(|b| ArrayBuffer::new(ctx, b))
        .transpose()
}

/// Installs `__depth` (`stage`, `take`) on the global object; the shims
/// keep it and the lockdown removes it.
pub(super) fn install(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let o = Object::new(ctx.clone())?;
    o.set("stage", Function::new(ctx.clone(), js_stage)?)?;
    o.set("take", Function::new(ctx.clone(), js_take)?)?;
    ctx.globals().set("__depth", o)?;
    Ok(())
}

// ------------------------------------------------------------------ jobs

type Done = Box<dyn FnOnce(Result<Value, String>) + Send>;

struct Job {
    name: String,
    args: Value,
    plugin: String,
    done: Done,
}

struct Loaded {
    plan: Arc<Plan>,
    plugin: String,
    used: Instant,
}

/// Queues `depth.load`, `depth.run` or `depth.free` for `plugin`; `done`
/// gets the answer on the worker thread.
pub(super) fn submit(
    name: &str,
    args: Value,
    plugin: Option<String>,
    done: impl FnOnce(Result<Value, String>) + Send + 'static,
) {
    let Some(plugin) = plugin else {
        return done(Err("depth estimation is only for plugins".into()));
    };
    let job = Job {
        name: name.to_string(),
        args,
        plugin,
        done: Box::new(done),
    };
    let mut worker = WORKER.lock().unwrap_or_else(PoisonError::into_inner);
    let job = match worker.as_ref() {
        Some(tx) => match tx.send(job) {
            Ok(()) => return,
            Err(mpsc::SendError(job)) => job,
        },
        None => job,
    };
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("luna-depth".into())
        .spawn(move || work(&rx));
    match spawned {
        Ok(_) => {
            let _ = tx.send(job);
            *worker = Some(tx);
        }
        Err(e) => {
            drop(worker);
            (job.done)(Err(format!("could not start the depth thread: {e}")));
        }
    }
}

fn work(rx: &Receiver<Job>) {
    super::video::step_back();
    let mut models: HashMap<u64, Loaded> = HashMap::new();
    loop {
        let job = match rx.recv_timeout(IDLE) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                // End unless a job came in meanwhile: under the lock, so a
                // submit either reached this channel or starts a new thread.
                let mut worker = WORKER.lock().unwrap_or_else(PoisonError::into_inner);
                match rx.try_recv() {
                    Ok(job) => {
                        drop(worker);
                        job
                    }
                    Err(_) => {
                        *worker = None;
                        return;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_job(&mut models, &job.name, &job.args, &job.plugin)
        }))
        .unwrap_or_else(|_| Err(format!("{} failed", job.name)));
        (job.done)(result);
    }
}

fn num(a: &Value, key: &str) -> Result<f64, String> {
    a.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{key} is missing"))
}

fn run_job(
    models: &mut HashMap<u64, Loaded>,
    name: &str,
    a: &Value,
    plugin: &str,
) -> Result<Value, String> {
    match name {
        "depth.load" => {
            let bytes = take(num(a, "bytes")? as u64).ok_or("the model bytes are gone")?;
            let started = Instant::now();
            let model = onnx::parse(&bytes).map_err(|e| e.to_string())?;
            drop(bytes);
            let plan = Plan::new(model).map_err(|e| e.to_string())?;
            // One plugin keeps at most a few: the least recently used goes.
            let mut own: Vec<(u64, Instant)> = models
                .iter()
                .filter(|(_, m)| m.plugin == plugin)
                .map(|(k, m)| (*k, m.used))
                .collect();
            own.sort_by_key(|(_, used)| *used);
            while own.len() >= MODELS_PER_PLUGIN {
                models.remove(&own.remove(0).0);
            }
            let id = next_id();
            models.insert(
                id,
                Loaded {
                    plan: Arc::new(plan),
                    plugin: plugin.to_string(),
                    used: Instant::now(),
                },
            );
            Ok(json!({ "model": id, "ms": started.elapsed().as_millis() as u64 }))
        }
        "depth.free" => {
            let id = num(a, "model")? as u64;
            if models.get(&id).is_some_and(|m| m.plugin == plugin) {
                models.remove(&id);
            }
            Ok(Value::Null)
        }
        "depth.run" => {
            let id = num(a, "model")? as u64;
            let image = take(num(a, "image")? as u64).ok_or("the image bytes are gone")?;
            let plan = match models.get_mut(&id) {
                Some(m) if m.plugin == plugin => {
                    m.used = Instant::now();
                    m.plan.clone()
                }
                _ => return Err(UNLOADED.into()),
            };
            let request = Request::from_json(a)?;
            let started = Instant::now();
            let out = estimate(&plan, &image, &request)?;
            let ms = started.elapsed().as_millis() as u64;
            let predicted: Vec<u8> = out.predicted.iter().flat_map(|v| v.to_le_bytes()).collect();
            Ok(json!({
                "width": request.width,
                "height": request.height,
                "depth": stage(out.depth),
                "predicted": stage(predicted),
                "ms": ms,
            }))
        }
        other => Err(format!("unknown depth operation {other}")),
    }
}

// ------------------------------------------------------------------ the image

/// What `depth.run` is asked: the image and how transformers.js would
/// prepare it for this model.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Request {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    /// The model's input size (after the processor's resize).
    pub size: [usize; 2],
    pub rescale: Option<f32>,
    pub mean: Option<[f32; 3]>,
    pub std: Option<[f32; 3]>,
}

impl Request {
    fn from_json(a: &Value) -> Result<Self, String> {
        let dim = |key: &str| -> Result<usize, String> {
            let v = num(a, key)?;
            if !(1.0..=8192.0).contains(&v) {
                return Err(format!("{key} {v} is out of range"));
            }
            Ok(v as usize)
        };
        let three = |key: &str| -> Option<[f32; 3]> {
            let v = a.get(key)?.as_array()?;
            let f = |i: usize| v.get(i).and_then(Value::as_f64).map(|x| x as f32);
            Some([f(0)?, f(1)?, f(2)?])
        };
        let size = a
            .get("size")
            .and_then(Value::as_array)
            .ok_or("size is missing")?;
        let side = |i: usize| -> Result<usize, String> {
            let v = size
                .get(i)
                .and_then(Value::as_f64)
                .ok_or("size is malformed")?;
            if !(1.0..=4096.0).contains(&v) {
                return Err(format!("model size {v} is out of range"));
            }
            Ok(v as usize)
        };
        Ok(Self {
            width: dim("width")?,
            height: dim("height")?,
            channels: dim("channels")?,
            size: [side(0)?, side(1)?],
            rescale: a.get("rescale").and_then(Value::as_f64).map(|x| x as f32),
            mean: three("mean"),
            std: three("std"),
        })
    }
}

pub(super) struct Estimate {
    /// 0..255 per pixel of the image (transformers.js's `depth`).
    pub depth: Vec<u8>,
    /// The raw prediction resized to the image (`predicted_depth`).
    pub predicted: Vec<f32>,
}

/// Source pixel pairs and weights of a bilinear resize along one axis:
/// pixel centres, clamped at the edges (a canvas's `drawImage` with
/// smoothing, and ONNX `Resize` linear / half_pixel).
pub(super) fn bilinear_taps(n_in: usize, n_out: usize) -> Vec<(usize, usize, f32)> {
    let scale = n_out as f32 / n_in as f32;
    let last = n_in.saturating_sub(1);
    (0..n_out)
        .map(|x| {
            let o = ((x as f32 + 0.5) / scale - 0.5).clamp(0.0, last as f32);
            let i0 = o.floor() as usize;
            (i0, (i0 + 1).min(last), o - i0 as f32)
        })
        .collect()
}

/// The model's `pixel_values` for an RGBA/RGB/grey image.
pub(super) fn pixel_values(image: &[u8], r: &Request) -> Result<Tensor, String> {
    let (w, h, c) = (r.width, r.height, r.channels);
    if !(1..=4).contains(&c) || image.len() != w * h * c {
        return Err(format!("the image is not {w}x{h} with {c} channels"));
    }
    let [tw, th] = r.size;
    let tx = bilinear_taps(w, tw);
    let ty = bilinear_taps(h, th);
    let at = |x: usize, y: usize, ch: usize| -> f32 {
        let ch = if c >= 3 { ch } else { 0 };
        f32::from(image[(y * w + x) * c + ch])
    };
    let mut out = vec![0.0f32; 3 * th * tw];
    for ch in 0..3 {
        let scale = r.rescale.unwrap_or(1.0);
        let (mean, std) = match (r.mean, r.std) {
            (Some(m), Some(s)) => (m[ch], s[ch]),
            _ => (0.0, 1.0),
        };
        for (yo, &(y0, y1, fy)) in ty.iter().enumerate() {
            for (xo, &(x0, x1, fx)) in tx.iter().enumerate() {
                let top = at(x0, y0, ch) * (1.0 - fx) + at(x1, y0, ch) * fx;
                let bottom = at(x0, y1, ch) * (1.0 - fx) + at(x1, y1, ch) * fx;
                // A canvas stores whole 8-bit values.
                let v = (top * (1.0 - fy) + bottom * fy).round();
                out[(ch * th + yo) * tw + xo] = (v * scale - mean) / std;
            }
        }
    }
    Tensor::f32(vec![1, 3, th, tw], out).map_err(|e| e.to_string())
}

/// The prediction (`[.., h, w]`) resized to the image and spread over
/// 0..255, as transformers.js's depth-estimation pipeline finishes it.
pub(super) fn finish(pred: &Tensor, width: usize, height: usize) -> Result<Estimate, String> {
    let Data::F32(v) = &pred.data else {
        return Err("the model's output is not float".into());
    };
    let rank = pred.shape.len();
    if rank < 2 {
        return Err("the model's output is not an image".into());
    }
    let (ph, pw) = (pred.shape[rank - 2], pred.shape[rank - 1]);
    if ph == 0 || pw == 0 || v.len() < ph * pw {
        return Err("the model's output is empty".into());
    }
    let tx = bilinear_taps(pw, width);
    let ty = bilinear_taps(ph, height);
    let mut predicted = vec![0.0f32; width * height];
    for (yo, &(y0, y1, fy)) in ty.iter().enumerate() {
        for (xo, &(x0, x1, fx)) in tx.iter().enumerate() {
            let p = |x: usize, y: usize| v[y * pw + x];
            let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
            let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
            predicted[yo * width + xo] = top * (1.0 - fy) + bottom * fy;
        }
    }
    let (lo, hi) = predicted
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    let range = hi - lo;
    // `sub(min).div_(max - min).mul_(255).to("uint8")`: a typed array
    // conversion, which truncates (NaN, from a flat prediction, is 0).
    let depth = predicted
        .iter()
        .map(|&x| {
            let q = (x - lo) / range * 255.0;
            if q.is_finite() {
                q.clamp(0.0, 255.0) as u8
            } else {
                0
            }
        })
        .collect();
    Ok(Estimate { depth, predicted })
}

pub(super) fn estimate(plan: &Plan, image: &[u8], r: &Request) -> Result<Estimate, String> {
    let input = pixel_values(image, r)?;
    let out = plan.run(input).map_err(|e| e.to_string())?;
    finish(&out, r.width, r.height)
}
