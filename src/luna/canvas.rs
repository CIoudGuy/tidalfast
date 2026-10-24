//! Native side of the plugins' `<canvas>`, `Image` and Web Audio analysers
//! (the JavaScript side is `js/media.js`).
//!
//! - 2D canvases and decoded images are tiny-skia pixmaps on the Luna
//!   thread, so `getImageData` is synchronous. Paths arrive already in
//!   device space (the context applies its transform as points are added);
//!   text is drawn from the interface font's outlines (skrifa).
//! - What a canvas shows is published here, keyed by its id, after each
//!   animation frame that drew on it: RGBA pixels for 2D, a recorded draw
//!   list for WebGL2. `src/ui/luna_canvas.rs` draws them.
//! - Analysers read the output tap (`crate::audio_tap`) and get their FFT
//!   here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use rquickjs::{ArrayBuffer, Ctx, Exception, Function, Object};
use serde::Deserialize;
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient,
    Mask, Paint, PathBuilder, Pattern, Pixmap, Point, RadialGradient, Rect, Shader, SpreadMode,
    Stroke, StrokeDash, Transform,
};

/// Largest canvas or image, in pixels.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

// ------------------------------------------------------------------ frames

/// What a canvas shows, for the UI.
#[derive(Clone)]
pub enum Frame {
    Pixels(Arc<Pixels>),
    Gl(Arc<GlFrame>),
}

impl Frame {
    pub fn version(&self) -> u64 {
        match self {
            Frame::Pixels(p) => p.version,
            Frame::Gl(g) => g.version,
        }
    }
    pub fn size(&self) -> [u32; 2] {
        match self {
            Frame::Pixels(p) => [p.w, p.h],
            Frame::Gl(g) => [g.w, g.h],
        }
    }
}

/// A 2D canvas: premultiplied RGBA, row by row.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub version: u64,
    pub rgba: Vec<u8>,
}

/// A WebGL2 canvas: what one frame drew.
pub struct GlFrame {
    pub w: u32,
    pub h: u32,
    pub version: u64,
    pub cmds: Vec<GlCmd>,
    /// Earlier frames whose offscreen work (textures uploaded, framebuffers
    /// drawn) no later frame redid, oldest first. A context that has not
    /// replayed them yet runs their offscreen commands before this frame,
    /// so framebuffers a plugin filled once (a blurred cover) stay filled.
    pub prior: Vec<Arc<GlFrame>>,
}

impl GlFrame {
    /// Offscreen targets (texture keys) this frame writes: uploads and
    /// framebuffer draws and clears.
    fn writes(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for c in &self.cmds {
            if let Some(t) = c.target()
                && !out.contains(&t)
            {
                out.push(t);
            }
        }
        out
    }

    /// Targets whose earlier contents this frame does not need: the first
    /// thing it does to them replaces every pixel (an upload, a clear, or
    /// an unblended draw).
    fn overwrites(&self) -> Vec<u64> {
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for c in &self.cmds {
            let Some(t) = c.target() else { continue };
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            let replaces = match c {
                GlCmd::Tex { .. } | GlCmd::Clear { .. } => true,
                GlCmd::Draw(d) => d.blend.is_none(),
                GlCmd::Mip { .. } => false,
            };
            if replaces {
                out.push(t);
            }
        }
        out
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum GlCmd {
    /// `clear` with the colour then current, on the bound framebuffer.
    Clear {
        color: [f32; 4],
        /// Texture key of the framebuffer's colour attachment; `None` is
        /// the canvas.
        #[serde(default)]
        fb: Option<u64>,
    },
    Draw(GlDraw),
    /// `texImage2D`: (re)allocate texture `tex` at `w`x`h`; its pixels, if
    /// any, are in [`gl_texture`] at `version` or later.
    Tex {
        tex: u64,
        w: i32,
        h: i32,
        version: u64,
    },
    /// `generateMipmap` on `tex`.
    Mip {
        tex: u64,
    },
}

impl GlCmd {
    /// The offscreen texture this command writes, if any.
    pub fn target(&self) -> Option<u64> {
        match self {
            GlCmd::Clear { fb, .. } => *fb,
            GlCmd::Draw(d) => d.fb,
            GlCmd::Tex { tex, .. } | GlCmd::Mip { tex } => Some(*tex),
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct GlDraw {
    /// Key from [`gl_program`].
    pub prog: u64,
    /// GL primitive mode (TRIANGLES = 4, ...).
    pub mode: u32,
    pub first: i32,
    pub count: i32,
    /// `(name, kind, values)`; kind is the WebGL suffix: `1f`, `2f`, `3f`,
    /// `4f`, `1i`.., `1fv`.., `m2`, `m3`, `m4`.
    #[serde(default)]
    pub uniforms: Vec<(String, String, Vec<f32>)>,
    #[serde(default)]
    pub attribs: Vec<GlAttrib>,
    /// `(src, dst)` blend factors when blending is on.
    #[serde(default)]
    pub blend: Option<[u32; 4]>,
    /// Texture key of the bound framebuffer's colour attachment; `None`
    /// draws on the canvas.
    #[serde(default)]
    pub fb: Option<u64>,
    /// The viewport, for framebuffer draws (`x, y, w, h`).
    #[serde(default)]
    pub vp: Option<[i32; 4]>,
    /// Textures bound to the units the draw can sample.
    #[serde(default)]
    pub tex: Vec<GlTexBind>,
    /// `drawElements`: the index buffer, its type and byte offset (`first`
    /// is unused then).
    #[serde(default)]
    pub elements: Option<[u64; 3]>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct GlTexBind {
    pub unit: u32,
    pub tex: u64,
    /// `TEXTURE_MIN_FILTER`, `MAG_FILTER`, `WRAP_S`, `WRAP_T`.
    pub params: [u32; 4],
}

/// A texture's pixels: straight-alpha RGBA, top row first.
pub struct TexData {
    pub version: u64,
    pub w: u32,
    pub h: u32,
    pub rgba: Option<Arc<Vec<u8>>>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct GlAttrib {
    pub loc: u32,
    pub buffer: u64,
    pub size: i32,
    pub stride: i32,
    pub offset: i32,
}

/// A linked program: sources and the vertex input bound to each location
/// (`attribs[i]` at location `i`, "" for none).
pub struct GlProgram {
    pub vs: String,
    pub fs: String,
    pub attribs: Vec<String>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static FRAMES: LazyLock<Mutex<HashMap<u64, Frame>>> = LazyLock::new(Default::default);
static PROGRAMS: LazyLock<Mutex<HashMap<u64, Arc<GlProgram>>>> = LazyLock::new(Default::default);
/// A WebGL buffer's contents: a version counter and the bytes.
type BufferData = (u64, Arc<Vec<u8>>);
static BUFFERS: LazyLock<Mutex<HashMap<u64, BufferData>>> = LazyLock::new(Default::default);
static TEXTURES: LazyLock<Mutex<HashMap<u64, TexData>>> = LazyLock::new(Default::default);

/// Reads a WebGL texture's latest pixels (`None` once it was deleted).
pub fn gl_texture<R>(key: u64, f: impl FnOnce(Option<&TexData>) -> R) -> R {
    f(TEXTURES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key))
}

/// The frame canvas `id` shows, if it drew anything.
pub fn frame(id: u64) -> Option<Frame> {
    FRAMES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .cloned()
}

pub fn gl_program(key: u64) -> Option<Arc<GlProgram>> {
    PROGRAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned()
}

/// `(version, bytes)` of a vertex buffer.
pub fn gl_buffer(key: u64) -> Option<(u64, Arc<Vec<u8>>)> {
    BUFFERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned()
}

/// The most vertices one draw may run (WebGL's own limit is the buffers;
/// a draw without attributes would otherwise run the vertex shader as
/// often as the plugin asks).
pub const MAX_DRAW_VERTICES: i32 = 1 << 24;
/// The largest texture side a draw list may allocate.
pub const MAX_TEXTURE_SIDE: i32 = 8192;
/// Texture units a draw may bind.
pub const MAX_TEXTURE_UNITS: u32 = 16;
/// Vertex attribute locations.
pub const MAX_ATTRIBS: u32 = 16;

/// WebGL's error codes, as the shim reports them from `getError`.
pub const INVALID_ENUM: u32 = 0x500;
pub const INVALID_VALUE: u32 = 0x501;
pub const INVALID_OPERATION: u32 = 0x502;

/// Why WebGL would refuse `d` (the error `getError` reports), checked
/// against `buffer`'s contents as they are uploaded. Desktop GL checks none
/// of this: a draw that reads past a buffer can show garbage, reset the GPU
/// or crash the driver, so draws that fail it are dropped. `None` for a
/// draw that is safe to run.
pub fn draw_error(d: &GlDraw, buffer: &dyn Fn(u64) -> Option<Arc<Vec<u8>>>) -> Option<u32> {
    if d.mode > 6 {
        return Some(INVALID_ENUM);
    }
    if d.first < 0 || d.count < 0 || d.count > MAX_DRAW_VERTICES {
        return Some(INVALID_VALUE);
    }
    if d.tex.iter().any(|t| t.unit >= MAX_TEXTURE_UNITS) {
        return Some(INVALID_VALUE);
    }
    // The vertices every enabled attribute can supply.
    let mut limit: Option<u64> = None;
    for a in &d.attribs {
        if a.loc >= MAX_ATTRIBS || !(1..=4).contains(&a.size) {
            return Some(INVALID_VALUE);
        }
        if !(0..=255).contains(&a.stride) || a.stride % 4 != 0 || a.offset < 0 || a.offset % 4 != 0
        {
            return Some(INVALID_OPERATION);
        }
        let Some(bytes) = buffer(a.buffer) else {
            return Some(INVALID_OPERATION);
        };
        let size = a.size as u64 * 4;
        let stride = if a.stride == 0 { size } else { a.stride as u64 };
        let (len, offset) = (bytes.len() as u64, a.offset as u64);
        let n = if len < offset + size {
            0
        } else {
            (len - offset - size) / stride + 1
        };
        limit = Some(limit.map_or(n, |l| l.min(n)));
    }
    let needed = match d.elements {
        None => i64::from(d.first) as u64 + d.count as u64,
        Some([key, ty, offset]) => {
            let width = match ty {
                0x1401 => 1,
                0x1403 => 2,
                0x1405 => 4,
                _ => return Some(INVALID_ENUM),
            };
            if offset % width != 0 {
                return Some(INVALID_OPERATION);
            }
            let Some(bytes) = buffer(key) else {
                return Some(INVALID_OPERATION);
            };
            let end = offset.saturating_add(d.count as u64 * width);
            if end > bytes.len() as u64 {
                return Some(INVALID_OPERATION);
            }
            if limit.is_none() || d.count == 0 {
                return None;
            }
            let indices = &bytes[offset as usize..end as usize];
            let top = match width {
                1 => indices.iter().copied().map(u64::from).max(),
                2 => indices
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u64::from(u16::from_le_bytes(*b)))
                    .max(),
                _ => indices
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u64::from(u32::from_le_bytes(*b)))
                    .max(),
            };
            top.map_or(0, |t| t + 1)
        }
    };
    match limit {
        Some(l) if needed > l => Some(INVALID_OPERATION),
        _ => None,
    }
}

/// Why WebGL would refuse a draw list command other than a draw.
fn cmd_error(c: &GlCmd) -> Option<u32> {
    match c {
        GlCmd::Tex { w, h, .. } => {
            let ok = (0..=MAX_TEXTURE_SIDE).contains(w)
                && (0..=MAX_TEXTURE_SIDE).contains(h)
                && (*w as u64) * (*h as u64) <= MAX_PIXELS;
            (!ok).then_some(INVALID_VALUE)
        }
        GlCmd::Draw(d) => draw_error(d, &|k| gl_buffer(k).map(|(_, b)| b)),
        _ => None,
    }
}

/// Sets the frame canvas `id` shows, as a draw would.
#[cfg(test)]
pub fn test_frame(id: u64, f: Option<Frame>) {
    set_frame(id, f);
}

fn set_frame(id: u64, f: Option<Frame>) {
    let mut m = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    match f {
        Some(f) => m.insert(id, f),
        None => m.remove(&id),
    };
}

/// The vertex inputs (`in` / `attribute`) a vertex shader declares, in
/// order.
pub fn vertex_inputs(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in strip_comments(src).split(';') {
        let words: Vec<&str> = stmt.split_whitespace().collect();
        let Some(i) = words.iter().position(|w| *w == "in" || *w == "attribute") else {
            continue;
        };
        // `in` must start the declaration (after layout/precision words).
        if words[..i]
            .iter()
            .any(|w| w.contains('(') && !w.starts_with("layout"))
        {
            continue;
        }
        if let Some(name) = words.last() {
            let name = name.split('[').next().unwrap_or(name);
            if words.len() >= i + 3 && !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("//") {
            rest = r.find('\n').map_or("", |i| &r[i..]);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.find("*/").map_or("", |i| &r[i + 2..]);
            out.push(' ');
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    // Function bodies are not declarations.
    let mut flat = String::new();
    let mut depth = 0;
    for c in out.chars() {
        match c {
            '{' => {
                depth += 1;
                flat.push(';');
            }
            '}' => {
                depth -= 1;
                flat.push(';');
            }
            _ if depth > 0 => {}
            '\n' => flat.push(' '),
            _ => flat.push(c),
        }
    }
    flat
}

// ------------------------------------------------------------------ surfaces

struct Surface {
    pix: Option<Pixmap>,
    w: u32,
    h: u32,
    clip: Option<Mask>,
    saved: Vec<Option<Mask>>,
    /// Bumped by every change; published when it differs.
    version: u64,
    published: u64,
    /// A loaded image's file, decoded into `pix` on first use: pages load
    /// many images they never draw (uwu-ifier preloads a hundred). Until
    /// then it holds no pixels and counts nothing against the budget.
    encoded: Option<Vec<u8>>,
}

impl Surface {
    fn new(w: u32, h: u32) -> Self {
        let ok = w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PIXELS;
        Self {
            pix: if ok { Pixmap::new(w, h) } else { None },
            w,
            h,
            clip: None,
            saved: Vec::new(),
            version: 1,
            published: 0,
            encoded: None,
        }
    }

    /// A loaded image of wxh whose pixels are decoded from `data` when it
    /// is first drawn or read.
    fn image(w: u32, h: u32, data: Vec<u8>) -> Self {
        Self {
            pix: None,
            w,
            h,
            clip: None,
            saved: Vec::new(),
            version: 1,
            published: 0,
            encoded: Some(data),
        }
    }

    /// Decodes a lazily loaded image (premultiplied, as tiny-skia wants),
    /// when its pixels fit (`room`: the budget has them). One that does not
    /// stays undrawn and keeps its file for a later try.
    fn realize(&mut self, room: bool) {
        let ok = self.w > 0 && self.h > 0 && u64::from(self.w) * u64::from(self.h) <= MAX_PIXELS;
        if self.encoded.is_none() || !room || !ok {
            return;
        }
        let Some(data) = self.encoded.take() else {
            return;
        };
        let Ok(img) = image::load_from_memory(&data) else {
            return;
        };
        let rgba = img.to_rgba8();
        if rgba.dimensions() != (self.w, self.h) {
            return;
        }
        let Some(mut pix) = Pixmap::new(self.w, self.h) else {
            return;
        };
        let (dst_px, _) = pix.data_mut().as_chunks_mut::<4>();
        let (src_px, _) = rgba.as_chunks::<4>();
        for (dst, src) in dst_px.iter_mut().zip(src_px) {
            let a = u16::from(src[3]);
            let pm = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
            dst.copy_from_slice(&[pm(src[0]), pm(src[1]), pm(src[2]), src[3]]);
        }
        self.pix = Some(pix);
    }
}

/// Surface `id`, decoded first if it is a lazily loaded image.
fn realized(s: &mut State, id: u64) -> Option<&mut Surface> {
    let (w, h, lazy) = s
        .surfaces
        .get(&id)
        .map(|x| (x.w, x.h, x.encoded.is_some()))?;
    let room = lazy && s.fits(w, h, Some(id));
    let sur = s.surfaces.get_mut(&id)?;
    sur.realize(room);
    Some(sur)
}

type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    surfaces: HashMap<u64, Surface>,
    notify: Option<Notify>,
    /// WebGL buffers and textures this runtime made (keys), and the bytes
    /// each texture holds: freed when Luna stops, counted in the budget.
    gl_buffers: std::collections::HashSet<u64>,
    gl_textures: HashMap<u64, u64>,
    /// WebGL canvases that published frames.
    gl_frames: std::collections::HashSet<u64>,
}

/// Pixels canvases, images and WebGL textures may hold together, per
/// runtime: they live outside the JavaScript heap's limit, so without this
/// a plugin could fill memory with 64 MB canvases.
const MEMORY_BUDGET: u64 = 512 * 1024 * 1024;

impl State {
    /// Bytes held by surfaces and textures, less except's own.
    fn held(&self, except: Option<u64>) -> u64 {
        let surfaces: u64 = self
            .surfaces
            .iter()
            .filter(|(k, _)| Some(**k) != except)
            .filter_map(|(_, s)| s.pix.as_ref())
            .map(|p| p.data().len() as u64)
            .sum();
        let textures: u64 = self
            .gl_textures
            .iter()
            .filter(|(k, _)| Some(**k) != except)
            .map(|(_, b)| *b)
            .sum();
        surfaces + textures
    }

    /// Whether wxh more pixels fit the budget (replacing except's).
    fn fits(&self, w: u32, h: u32, except: Option<u64>) -> bool {
        self.held(except) + u64::from(w) * u64::from(h) * 4 <= MEMORY_BUDGET
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn throw(ctx: &Ctx<'_>, msg: impl std::fmt::Display) -> rquickjs::Error {
    Exception::throw_message(ctx, &msg.to_string())
}

fn floats(buf: &ArrayBuffer<'_>) -> Vec<f32> {
    // SAFETY: copied out at once; no JavaScript runs while the slice lives.
    let bytes = unsafe { buf.as_bytes() }.unwrap_or_default();
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

fn bytes(buf: &ArrayBuffer<'_>) -> Vec<u8> {
    // SAFETY: as above.
    unsafe { buf.as_bytes() }.unwrap_or_default().to_vec()
}

fn f32_buffer<'js>(ctx: Ctx<'js>, v: &[f32]) -> rquickjs::Result<ArrayBuffer<'js>> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    ArrayBuffer::new(ctx, b)
}

fn transform(m: &[f32]) -> Transform {
    match m {
        [a, b, c, d, e, f, ..] => Transform::from_row(*a, *b, *c, *d, *e, *f),
        _ => Transform::identity(),
    }
}

fn blend(i: f32) -> BlendMode {
    use BlendMode::*;
    const MODES: [BlendMode; 27] = [
        SourceOver,
        SourceIn,
        SourceOut,
        SourceAtop,
        DestinationOver,
        DestinationIn,
        DestinationOut,
        DestinationAtop,
        Plus,
        Source,
        Xor,
        Multiply,
        Screen,
        Overlay,
        Darken,
        Lighten,
        ColorDodge,
        ColorBurn,
        HardLight,
        SoftLight,
        Difference,
        Exclusion,
        Hue,
        Saturation,
        Color,
        Luminosity,
        Clear,
    ];
    MODES.get(i as usize).copied().unwrap_or(SourceOver)
}

fn path_of(p: &[f32]) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    let mut i = 0;
    let finite = |s: &[f32]| s.iter().all(|v| v.is_finite());
    while i < p.len() {
        let op = p[i] as u32;
        let n = match op {
            0 | 1 => 2,
            2 => 4,
            3 => 6,
            _ => 0,
        };
        let a = p.get(i + 1..i + 1 + n)?;
        if finite(a) {
            match op {
                0 => b.move_to(a[0], a[1]),
                1 => b.line_to(a[0], a[1]),
                2 => b.quad_to(a[0], a[1], a[2], a[3]),
                3 => b.cubic_to(a[0], a[1], a[2], a[3], a[4], a[5]),
                _ => b.close(),
            }
        }
        i += 1 + n;
    }
    b.finish()
}

fn color(c: &[f32]) -> Color {
    let g = |i: usize| c.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    Color::from_rgba(g(0), g(1), g(2), g(3)).unwrap_or(Color::TRANSPARENT)
}

fn stops(p: &[f32], from: f32) -> Vec<(f32, Color)> {
    // No more stops than the buffer holds, whatever count it claims (a
    // count of 3e38 would otherwise loop past the watchdog's reach).
    let n =
        (p.first().copied().unwrap_or(0.0).max(0.0) as usize).min(p.len().saturating_sub(1) / 5);
    (0..n)
        .filter_map(|k| {
            let s = p.get(1 + k * 5..6 + k * 5)?;
            Some((from + s[0].clamp(0.0, 1.0) * (1.0 - from), color(&s[1..5])))
        })
        .collect()
}

fn gradient_stops(st: &[(f32, Color)]) -> Vec<GradientStop> {
    st.iter().map(|(t, c)| GradientStop::new(*t, *c)).collect()
}

/// Builds the paint for an encoded style (see `encodePaint` in media.js).
/// Patterns borrow their source pixmap from `src`.
fn shader<'a>(p: &[f32], src: Option<&'a Pixmap>) -> Shader<'a> {
    let kind = p.first().copied().unwrap_or(0.0) as u32;
    let none = Shader::SolidColor(Color::TRANSPARENT);
    match kind {
        0 => Shader::SolidColor(color(&p[1.min(p.len())..])),
        1 if p.len() >= 12 => {
            let st = stops(&p[11..], 0.0);
            if st.is_empty() {
                return none;
            }
            if st.len() == 1 {
                return Shader::SolidColor(st[0].1);
            }
            LinearGradient::new(
                Point::from_xy(p[1], p[2]),
                Point::from_xy(p[3], p[4]),
                gradient_stops(&st),
                SpreadMode::Pad,
                transform(&p[5..11]),
            )
            .unwrap_or(none)
        }
        2 if p.len() >= 14 => {
            let (r0, r1) = (p[3].max(0.0), p[6].max(0.0));
            let from = if r1 > 0.0 { (r0 / r1).min(1.0) } else { 0.0 };
            let st = stops(&p[13..], from);
            if st.is_empty() {
                return none;
            }
            if st.len() == 1 || r1 <= 0.0 {
                return Shader::SolidColor(st[st.len() - 1].1);
            }
            RadialGradient::new(
                Point::from_xy(p[1], p[2]),
                Point::from_xy(p[4], p[5]),
                r1,
                gradient_stops(&st),
                SpreadMode::Pad,
                transform(&p[7..13]),
            )
            .unwrap_or(none)
        }
        3 if p.len() >= 10 => {
            let Some(pix) = src else { return none };
            let spread = if p[2] as u32 == 3 {
                SpreadMode::Pad
            } else {
                SpreadMode::Repeat
            };
            Pattern::new(
                pix.as_ref(),
                spread,
                FilterQuality::Bilinear,
                p[9].clamp(0.0, 1.0),
                transform(&p[3..9]),
            )
        }
        _ => none,
    }
}

/// The pattern source a paint needs, taken out of the registry for the
/// draw (and put back by [`put_back`]).
fn take_source(s: &mut State, dst: u64, p: &[f32]) -> Option<(u64, Surface)> {
    if p.first().copied().unwrap_or(0.0) as u32 != 3 || p.len() < 2 {
        return None;
    }
    let sid = p[1] as u64;
    realized(s, sid);
    if sid == dst {
        let sur = s.surfaces.get(&sid)?;
        let mut copy = Surface::new(sur.w, sur.h);
        copy.pix = sur.pix.clone();
        return Some((u64::MAX, copy));
    }
    s.surfaces.remove(&sid).map(|x| (sid, x))
}

fn put_back(s: &mut State, src: Option<(u64, Surface)>) {
    if let Some((id, sur)) = src
        && id != u64::MAX
    {
        s.surfaces.insert(id, sur);
    }
}

fn paint_with<'a>(shader: Shader<'a>, mode: f32) -> Paint<'a> {
    Paint {
        shader,
        blend_mode: blend(mode),
        anti_alias: true,
        force_hq_pipeline: false,
    }
}

// ------------------------------------------------------------------ text

static FONT: LazyLock<Option<skrifa::FontRef<'static>>> =
    LazyLock::new(|| skrifa::FontRef::new(crate::theme::HANKEN).ok());

struct Pen<'a> {
    b: &'a mut PathBuilder,
    t: Transform,
    ox: f32,
    oy: f32,
    sx: f32,
    skew: f32,
}

impl Pen<'_> {
    fn pt(&self, x: f32, y: f32) -> (f32, f32) {
        let mut p = Point::from_xy(self.ox + (x + self.skew * y) * self.sx, self.oy - y);
        self.t.map_point(&mut p);
        (p.x, p.y)
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.pt(x, y);
        self.b.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.pt(x, y);
        self.b.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (cx, cy) = self.pt(cx, cy);
        let (x, y) = self.pt(x, y);
        self.b.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let (c0x, c0y) = self.pt(c0x, c0y);
        let (c1x, c1y) = self.pt(c1x, c1y);
        let (x, y) = self.pt(x, y);
        self.b.cubic_to(c0x, c0y, c1x, c1y, x, y);
    }
    fn close(&mut self) {
        self.b.close();
    }
}

fn location(font: &skrifa::FontRef<'_>, weight: f32) -> skrifa::instance::Location {
    font.axes().location([("wght", weight)])
}

/// The face for a CSS `font-family` list at a weight: the first installed
/// family it names (custom-fonts tells installed fonts apart by their
/// widths) or the system's face for a generic one it reaches first
/// (`system-ui` is Segoe UI on Windows: qr-share's caption), in the cut that
/// draws the weight, as a browser picks it; else the interface font.
fn face(family: &str, weight: f32) -> Option<&'static skrifa::FontRef<'static>> {
    use crate::ui::plugin_slots::css::{Family, family_list};
    family_list(family)
        .into_iter()
        .find_map(|f| match f {
            Family::Named(name) => crate::fonts::family_face_at(&name, weight),
            Family::Generic(g) => g
                .candidates()
                .iter()
                .find_map(|name| crate::fonts::family_face_at(name, weight)),
        })
        .or(FONT.as_ref())
}

/// `[width, ascent, descent]` (descent positive) of `text` at `px` in the
/// first installed family of `family` (a CSS list; empty for the
/// interface font).
pub fn measure(text: &str, px: f32, weight: f32, family: &str) -> [f32; 3] {
    let Some(font) = face(family, weight) else {
        let w = text.chars().count() as f32 * px * 0.55;
        return [w, px * 0.8, px * 0.2];
    };
    let loc = location(font, weight);
    let size = Size::new(px);
    let gm = font.glyph_metrics(size, LocationRef::from(&loc));
    let cmap = font.charmap();
    let w = text
        .chars()
        .map(|c| {
            let g = cmap.map(c).unwrap_or_default();
            gm.advance_width(g).unwrap_or(px * 0.5)
        })
        .sum();
    let m = font.metrics(size, LocationRef::from(&loc));
    [w, m.ascent, -m.descent]
}

/// The outline of `text` with its baseline origin at user `(x, y)`;
/// `p` = `[x, y, px, weight, italic, align, baseline, maxWidth, m0..m5]`.
fn text_path(text: &str, p: &[f32], family: &str) -> Option<tiny_skia::Path> {
    let font = face(family, p[3])?;
    let (x, y, px, weight) = (p[0], p[1], p[2].clamp(1.0, 1000.0), p[3]);
    let [w, asc, desc] = measure(text, px, weight, family);
    let max = p[7];
    let sx = if max > 0.0 && w > max { max / w } else { 1.0 };
    let width = w * sx;
    let ox = match p[5] as u32 {
        1 => x - width / 2.0,
        2 => x - width,
        _ => x,
    };
    let oy = match p[6] as u32 {
        1 => y + asc,
        2 => y + (asc - desc) / 2.0,
        3 | 5 => y - desc,
        4 => y + asc * 0.8,
        _ => y,
    };
    let loc = location(font, weight);
    let size = Size::new(px);
    let gm = font.glyph_metrics(size, LocationRef::from(&loc));
    let cmap = font.charmap();
    let outlines = font.outline_glyphs();
    let mut b = PathBuilder::new();
    let mut pen_x = 0.0;
    for c in text.chars() {
        let g = cmap.map(c).unwrap_or_default();
        if let Some(o) = outlines.get(g) {
            let mut pen = Pen {
                b: &mut b,
                t: transform(&p[8..14]),
                ox: ox + pen_x * sx,
                oy,
                sx,
                skew: if p[4] > 0.0 { 0.2 } else { 0.0 },
            };
            let _ = o.draw(
                DrawSettings::unhinted(size, LocationRef::from(&loc)),
                &mut pen,
            );
        }
        pen_x += gm.advance_width(g).unwrap_or(px * 0.5);
    }
    b.finish()
}

// ------------------------------------------------------------------ natives

/// A surface of wxh, or one without pixels when it would not fit the
/// budget (drawn as a canvas too large to allocate: nothing).
fn budgeted(s: &State, w: u32, h: u32, except: Option<u64>) -> Surface {
    if s.fits(w, h, except) {
        Surface::new(w, h)
    } else {
        Surface::new(0, 0)
    }
}

fn create(w: f64, h: f64) -> f64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with(|s| {
        let sur = budgeted(s, w.max(0.0) as u32, h.max(0.0) as u32, None);
        s.surfaces.insert(id, sur)
    });
    id as f64
}

fn resize(id: f64, w: f64, h: f64) {
    with(|s| {
        let id = id as u64;
        if s.surfaces.contains_key(&id) {
            let next = budgeted(s, w.max(0.0) as u32, h.max(0.0) as u32, Some(id));
            if let Some(sur) = s.surfaces.get_mut(&id) {
                let v = sur.version;
                *sur = next;
                sur.version = v + 1;
            }
        }
    });
}

fn free(id: f64) {
    with(|s| s.surfaces.remove(&(id as u64)));
    set_frame(id as u64, None);
}

/// Reads an image's size; its pixels are decoded when first used. As in
/// Chromium, loading an image never fails for its size or for how many
/// are loaded: only its pixels take memory, once it is drawn or read (and
/// one too large for the budget then draws nothing).
fn decode<'js>(ctx: Ctx<'js>, buf: ArrayBuffer<'js>) -> rquickjs::Result<Vec<f64>> {
    let data = bytes(&buf);
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&data))
        .with_guessed_format()
        .map_err(|e| throw(&ctx, e))?
        .into_dimensions()
        .map_err(|e| throw(&ctx, e))?;
    let sur = Surface::image(w, h, data);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with(|s| s.surfaces.insert(id, sur));
    Ok(vec![id as f64, f64::from(w), f64::from(h)])
}

/// `fill(id, path, paint, evenodd, blend)`.
fn fill(id: f64, path: ArrayBuffer<'_>, paint: ArrayBuffer<'_>, evenodd: bool, mode: f64) {
    let (p, pa) = (floats(&path), floats(&paint));
    let Some(path) = path_of(&p) else { return };
    let id = id as u64;
    with(|s| {
        let src = take_source(s, id, &pa);
        if let Some(sur) = s.surfaces.get_mut(&id) {
            let rule = if evenodd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
            if let Some(pix) = &mut sur.pix {
                let paint = paint_with(
                    shader(&pa, src.as_ref().and_then(|x| x.1.pix.as_ref())),
                    mode as f32,
                );
                pix.fill_path(
                    &path,
                    &paint,
                    rule,
                    Transform::identity(),
                    sur.clip.as_ref(),
                );
            }
            sur.version += 1;
        }
        put_back(s, src);
    });
}

/// `stroke(id, path, paint, [width, cap, join, miter, blend, dashOffset,
/// dash...])`.
fn stroke(id: f64, path: ArrayBuffer<'_>, paint: ArrayBuffer<'_>, opts: Vec<f64>) {
    let (p, pa) = (floats(&path), floats(&paint));
    let Some(path) = path_of(&p) else { return };
    let o = |i: usize| opts.get(i).copied().unwrap_or(0.0) as f32;
    let width = o(0);
    // Also rejects NaN.
    if width.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return;
    }
    let dash: Vec<f32> = opts.iter().skip(6).map(|v| *v as f32).collect();
    let st = Stroke {
        width,
        miter_limit: o(3).max(1.0),
        line_cap: match o(1) as u32 {
            1 => LineCap::Round,
            2 => LineCap::Square,
            _ => LineCap::Butt,
        },
        line_join: match o(2) as u32 {
            1 => LineJoin::Round,
            2 => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        dash: if dash.is_empty() {
            None
        } else {
            StrokeDash::new(dash, o(5))
        },
    };
    let id = id as u64;
    with(|s| {
        let src = take_source(s, id, &pa);
        if let Some(sur) = s.surfaces.get_mut(&id) {
            if let Some(pix) = &mut sur.pix {
                let paint = paint_with(
                    shader(&pa, src.as_ref().and_then(|x| x.1.pix.as_ref())),
                    o(4),
                );
                pix.stroke_path(&path, &paint, &st, Transform::identity(), sur.clip.as_ref());
            }
            sur.version += 1;
        }
        put_back(s, src);
    });
}

fn clip(id: f64, path: ArrayBuffer<'_>, evenodd: bool) {
    let p = floats(&path);
    let rule = if evenodd {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    };
    with(|s| {
        let Some(sur) = s.surfaces.get_mut(&(id as u64)) else {
            return;
        };
        let Some(path) = path_of(&p) else {
            // Clipping to nothing.
            sur.clip = Mask::new(sur.w.max(1), sur.h.max(1));
            return;
        };
        match &mut sur.clip {
            Some(m) => m.intersect_path(&path, rule, true, Transform::identity()),
            None => {
                if let Some(mut m) = Mask::new(sur.w.max(1), sur.h.max(1)) {
                    m.fill_path(&path, rule, true, Transform::identity());
                    sur.clip = Some(m);
                }
            }
        }
    });
}

fn save(id: f64) {
    with(|s| {
        if let Some(sur) = s.surfaces.get_mut(&(id as u64)) {
            let c = sur.clip.clone();
            sur.saved.push(c);
        }
    });
}

fn restore(id: f64) {
    with(|s| {
        if let Some(sur) = s.surfaces.get_mut(&(id as u64))
            && let Some(c) = sur.saved.pop()
        {
            sur.clip = c;
        }
    });
}

/// Back to the initial state (canvas `reset()`, or a size change).
fn clear_all(id: f64) {
    with(|s| {
        if let Some(sur) = s.surfaces.get_mut(&(id as u64)) {
            if let Some(p) = &mut sur.pix {
                p.fill(Color::TRANSPARENT);
            }
            sur.clip = None;
            sur.saved.clear();
            sur.version += 1;
        }
    });
}

/// `draw_image(id, [src, sx, sy, sw, sh, dx, dy, dw, dh, m0..m5, alpha,
/// blend, smooth])`.
fn draw_image(id: f64, a: Vec<f64>) {
    if a.len() < 18 {
        return;
    }
    let f = |i: usize| a[i] as f32;
    let (sid, did) = (a[0] as u64, id as u64);
    let (mut sx, mut sy, mut sw, mut sh) = (f(1), f(2), f(3), f(4));
    let (mut dx, mut dy, mut dw, mut dh) = (f(5), f(6), f(7), f(8));
    // Negative sizes flip the rectangle's origin.
    if sw < 0.0 {
        sx += sw;
        sw = -sw;
    }
    if sh < 0.0 {
        sy += sh;
        sh = -sh;
    }
    if dw < 0.0 {
        dx += dw;
        dw = -dw;
    }
    if dh < 0.0 {
        dy += dh;
        dh = -dh;
    }
    if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return;
    }
    let ctm = transform(&a[9..15].iter().map(|v| *v as f32).collect::<Vec<_>>());
    let (alpha, mode, smooth) = (f(15).clamp(0.0, 1.0), f(16), a[17] != 0.0);
    with(|s| {
        realized(s, sid);
        let src = if sid == did {
            s.surfaces
                .get(&sid)
                .and_then(|x| x.pix.clone())
                .map(|p| (u64::MAX, p))
        } else {
            s.surfaces
                .get_mut(&sid)
                .and_then(|x| x.pix.take())
                .map(|p| (sid, p))
        };
        let Some((back, pix)) = src else { return };
        // Clamp the source rectangle to the image, moving the destination
        // with it.
        let (iw, ih) = (pix.width() as f32, pix.height() as f32);
        let (kx, ky) = (dw / sw, dh / sh);
        let (cx0, cy0) = (sx.max(0.0), sy.max(0.0));
        let (cx1, cy1) = ((sx + sw).min(iw), (sy + sh).min(ih));
        if let Some(sur) = s.surfaces.get_mut(&did)
            && cx1 > cx0
            && cy1 > cy0
        {
            let rect = Rect::from_ltrb(
                dx + (cx0 - sx) * kx,
                dy + (cy0 - sy) * ky,
                dx + (cx1 - sx) * kx,
                dy + (cy1 - sy) * ky,
            );
            if let (Some(dst), Some(rect)) = (&mut sur.pix, rect) {
                let pt = Transform::from_translate(dx - sx * kx, dy - sy * ky).pre_scale(kx, ky);
                let quality = if smooth {
                    FilterQuality::Bilinear
                } else {
                    FilterQuality::Nearest
                };
                let paint = paint_with(
                    Pattern::new(pix.as_ref(), SpreadMode::Pad, quality, alpha, pt),
                    mode,
                );
                dst.fill_rect(rect, &paint, ctm, sur.clip.as_ref());
            }
            sur.version += 1;
        }
        if back != u64::MAX
            && let Some(x) = s.surfaces.get_mut(&back)
        {
            x.pix = Some(pix);
        }
    });
}

/// Unpremultiplied RGBA of a rectangle; outside the canvas is transparent.
fn get_data<'js>(
    ctx: Ctx<'js>,
    id: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> rquickjs::Result<ArrayBuffer<'js>> {
    let (w, h) = (w.max(0.0) as u32, h.max(0.0) as u32);
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(throw(&ctx, "IndexSizeError"));
    }
    let (x, y) = (x.floor() as i64, y.floor() as i64);
    let mut out = vec![0u8; (w * h * 4) as usize];
    with(|s| {
        let Some(pix) = realized(s, id as u64).and_then(|x| x.pix.as_ref()) else {
            return;
        };
        let (pw, ph) = (i64::from(pix.width()), i64::from(pix.height()));
        let data = pix.data();
        for row in 0..i64::from(h) {
            let sy = y + row;
            if sy < 0 || sy >= ph {
                continue;
            }
            for col in 0..i64::from(w) {
                let sx = x + col;
                if sx < 0 || sx >= pw {
                    continue;
                }
                let si = ((sy * pw + sx) * 4) as usize;
                let di = ((row * i64::from(w) + col) * 4) as usize;
                let a = data[si + 3];
                let un = |c: u8| {
                    if a == 0 {
                        0
                    } else {
                        ((u16::from(c) * 255 + u16::from(a) / 2) / u16::from(a)).min(255) as u8
                    }
                };
                out[di] = un(data[si]);
                out[di + 1] = un(data[si + 1]);
                out[di + 2] = un(data[si + 2]);
                out[di + 3] = a;
            }
        }
    });
    ArrayBuffer::new(ctx, out)
}

/// `put_data(id, rgba, [dx, dy, w, h, dirtyX, dirtyY, dirtyW, dirtyH])`.
fn put_data(id: f64, buf: ArrayBuffer<'_>, a: Vec<f64>) {
    if a.len() < 8 {
        return;
    }
    let data = bytes(&buf);
    let (dx, dy, w, h) = (
        a[0].floor() as i64,
        a[1].floor() as i64,
        a[2] as i64,
        a[3] as i64,
    );
    if w <= 0 || h <= 0 {
        return;
    }
    // The source rows the data holds: an ImageData claiming more than its
    // bytes ({width: 1e9, height: 1e9}) is cut to them.
    let h = h.min((data.len() as i64 / 4) / w);
    let (x0, y0) = (a[4].max(0.0) as i64, a[5].max(0.0) as i64);
    let (x1, y1) = (((a[4] + a[6]) as i64).min(w), ((a[5] + a[7]) as i64).min(h));
    with(|s| {
        let Some(sur) = s.surfaces.get_mut(&(id as u64)) else {
            return;
        };
        let Some(pix) = &mut sur.pix else { return };
        let (pw, ph) = (i64::from(pix.width()), i64::from(pix.height()));
        // Only what lands on the canvas is visited.
        let (x0, x1) = (x0.max(-dx), x1.min(pw - dx));
        let (y0, y1) = (y0.max(-dy), y1.min(ph - dy));
        let out = pix.data_mut();
        for row in y0..y1 {
            let ty = dy + row;
            if ty < 0 || ty >= ph {
                continue;
            }
            for col in x0..x1 {
                let tx = dx + col;
                if tx < 0 || tx >= pw {
                    continue;
                }
                let si = ((row * w + col) * 4) as usize;
                let Some(px) = data.get(si..si + 4) else {
                    continue;
                };
                let di = ((ty * pw + tx) * 4) as usize;
                let al = u16::from(px[3]);
                let pm = |c: u8| ((u16::from(c) * al + 127) / 255) as u8;
                out[di..di + 4].copy_from_slice(&[pm(px[0]), pm(px[1]), pm(px[2]), px[3]]);
            }
        }
        sur.version += 1;
    });
}

fn png<'js>(ctx: Ctx<'js>, id: f64) -> rquickjs::Result<ArrayBuffer<'js>> {
    let out = with(|s| {
        let sur = realized(s, id as u64)?;
        match &sur.pix {
            Some(p) => p.encode_png().ok(),
            // A 0x0 canvas gives "data:," in browsers; an empty buffer here.
            None => Some(Vec::new()),
        }
    });
    ArrayBuffer::new(ctx, out.unwrap_or_default())
}

/// `text(id, string, [x, y, px, weight, italic, align, baseline, maxWidth,
/// m0..m5], paint, [stroke?, width, blend], font-family)`.
fn text(
    id: f64,
    s: String,
    p: Vec<f64>,
    paint: ArrayBuffer<'_>,
    o: Vec<f64>,
    family: rquickjs::function::Opt<String>,
) {
    let family = family.0.unwrap_or_default();
    if p.len() < 14 || o.len() < 3 {
        return;
    }
    let pf: Vec<f32> = p.iter().map(|v| *v as f32).collect();
    let Some(path) = text_path(&s, &pf, &family) else {
        return;
    };
    let pa = floats(&paint);
    let id = id as u64;
    with(|st| {
        let src = take_source(st, id, &pa);
        if let Some(sur) = st.surfaces.get_mut(&id) {
            if let Some(pix) = &mut sur.pix {
                let paint = paint_with(
                    shader(&pa, src.as_ref().and_then(|x| x.1.pix.as_ref())),
                    o[2] as f32,
                );
                if o[0] != 0.0 {
                    let stroke = Stroke {
                        width: o[1] as f32,
                        ..Stroke::default()
                    };
                    pix.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        sur.clip.as_ref(),
                    );
                } else {
                    pix.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        sur.clip.as_ref(),
                    );
                }
            }
            sur.version += 1;
        }
        put_back(st, src);
    });
}

fn measure_js(
    s: String,
    px: f64,
    weight: f64,
    family: rquickjs::function::Opt<String>,
) -> Vec<f64> {
    let family = family.0.unwrap_or_default();
    measure(&s, px as f32, weight as f32, &family)
        .iter()
        .map(|v| f64::from(*v))
        .collect()
}

/// Publishes a 2D canvas's pixels when they changed since the last time.
fn publish(id: f64) {
    let id = id as u64;
    let (frame, notify) = with(|s| {
        let notify = s.notify.clone();
        let Some(sur) = s.surfaces.get_mut(&id) else {
            return (None, None);
        };
        if sur.published == sur.version {
            return (None, None);
        }
        sur.published = sur.version;
        let px = Pixels {
            w: sur.w,
            h: sur.h,
            version: sur.version,
            rgba: sur
                .pix
                .as_ref()
                .map(|p| p.data().to_vec())
                .unwrap_or_default(),
        };
        (Some(Frame::Pixels(Arc::new(px))), notify)
    });
    if let Some(f) = frame {
        set_frame(id, Some(f));
        if let Some(n) = notify {
            n();
        }
    }
}

/// Registers a WebGL program; returns its key.
/// `attribs[i]` is the vertex input bound to location `i` ("" for none).
fn program(vs: String, fs: String, attribs: Vec<String>) -> f64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    vs.hash(&mut h);
    fs.hash(&mut h);
    attribs.hash(&mut h);
    // Exactly representable in a JS number.
    let key = h.finish() & ((1 << 52) - 1);
    PROGRAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(key)
        .or_insert_with(|| Arc::new(GlProgram { vs, fs, attribs }));
    key as f64
}

fn program_inputs(vs: String) -> Vec<String> {
    vertex_inputs(&vs)
}

fn buffer(key: f64, version: f64, data: ArrayBuffer<'_>) {
    with(|s| s.gl_buffers.insert(key as u64));
    BUFFERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key as u64, (version as u64, Arc::new(bytes(&data))));
}

fn buffer_free(key: f64) {
    with(|s| s.gl_buffers.remove(&(key as u64)));
    BUFFERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&(key as u64));
}

/// Straight-alpha RGBA of surface `surf` (a 2D canvas or decoded image),
/// cut or padded to `w`x`h`.
fn surface_rgba(surf: u64, w: u32, h: u32) -> Option<Vec<u8>> {
    with(|s| {
        let pix = realized(s, surf)?.pix.as_ref()?;
        let mut v = vec![0u8; (w * h * 4) as usize];
        let data = pix.data();
        for y in 0..pix.height().min(h) {
            for x in 0..pix.width().min(w) {
                let si = ((y * pix.width() + x) * 4) as usize;
                let di = ((y * w + x) * 4) as usize;
                let a = data[si + 3];
                for c in 0..3 {
                    v[di + c] = if a == 0 {
                        0
                    } else {
                        (u32::from(data[si + c]) * 255 / u32::from(a)).min(255) as u8
                    };
                }
                v[di + 3] = a;
            }
        }
        Some(v)
    })
}

/// `texImage2D` / `texSubImage2D` (`at` = `[x, y]`): stores texture
/// `key`'s pixels and returns their version. The pixels are `src` (straight
/// RGBA) or surface `surf` (>= 0); with neither the texture is allocated
/// empty. `flip` is `UNPACK_FLIP_Y_WEBGL`.
fn tex_image(
    key: f64,
    size: Vec<f64>,
    src: Option<ArrayBuffer<'_>>,
    surf: f64,
    flip: bool,
    at: Vec<f64>,
) -> f64 {
    let (w, h) = match size.as_slice() {
        [w, h, ..] => (w.max(0.0) as u32, h.max(0.0) as u32),
        _ => return 0.0,
    };
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return 0.0;
    }
    // A sub-image keeps the texture's size; a new image replaces it.
    if at.is_empty() && !with(|s| s.fits(w, h, Some(key as u64))) {
        return 0.0;
    }
    let len = (w * h * 4) as usize;
    let mut px = match src {
        Some(b) => {
            let mut v = bytes(&b);
            v.resize(len, 0);
            Some(v)
        }
        None if surf >= 0.0 => surface_rgba(surf as u64, w, h),
        None => None,
    };
    if flip && let Some(v) = px.as_mut() {
        let row = (w * 4) as usize;
        let rows = h as usize;
        for y in 0..rows / 2 {
            let (a, b) = v.split_at_mut((rows - 1 - y) * row);
            a[y * row..(y + 1) * row].swap_with_slice(&mut b[..row]);
        }
    }
    let key = key as u64;
    let mut m = TEXTURES.lock().unwrap_or_else(PoisonError::into_inner);
    let version = m.get(&key).map_or(1, |t| t.version + 1);
    let data = match (at.as_slice(), px) {
        ([x, y], Some(part)) => {
            let Some(old) = m.get(&key) else { return 0.0 };
            let (ow, oh) = (old.w, old.h);
            let mut base = old
                .rgba
                .as_ref()
                .map_or_else(|| vec![0u8; (ow * oh * 4) as usize], |r| r.to_vec());
            let (x0, y0) = (*x as i64, *y as i64);
            for row in 0..i64::from(h) {
                let ty = y0 + row;
                if ty < 0 || ty >= i64::from(oh) {
                    continue;
                }
                for col in 0..i64::from(w) {
                    let tx = x0 + col;
                    if tx < 0 || tx >= i64::from(ow) {
                        continue;
                    }
                    let si = ((row * i64::from(w) + col) * 4) as usize;
                    let di = ((ty * i64::from(ow) + tx) * 4) as usize;
                    base[di..di + 4].copy_from_slice(&part[si..si + 4]);
                }
            }
            TexData {
                version,
                w: ow,
                h: oh,
                rgba: Some(Arc::new(base)),
            }
        }
        ([_, _], None) => return 0.0,
        (_, px) => TexData {
            version,
            w,
            h,
            rgba: px.map(Arc::new),
        },
    };
    // Counted whole even without pixels: the GPU allocates it all.
    let held = u64::from(data.w) * u64::from(data.h) * 4;
    m.insert(key, data);
    drop(m);
    with(|s| s.gl_textures.insert(key, held));
    version as f64
}

fn tex_free(key: f64) {
    with(|s| s.gl_textures.remove(&(key as u64)));
    TEXTURES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&(key as u64));
}

/// Most earlier frames kept for contexts that have not replayed them.
const MAX_PRIOR: usize = 32;

/// `next`'s earlier frames: `last` and its own earlier frames, less those
/// that wrote nothing offscreen or whose every offscreen write `next`
/// replaces.
pub fn carry(last: Option<&Arc<GlFrame>>, next: &GlFrame) -> Vec<Arc<GlFrame>> {
    let Some(last) = last else {
        return Vec::new();
    };
    let over = next.overwrites();
    let mut out: Vec<Arc<GlFrame>> = last
        .prior
        .iter()
        .cloned()
        .chain(std::iter::once(Arc::new(GlFrame {
            w: last.w,
            h: last.h,
            version: last.version,
            cmds: last.cmds.clone(),
            prior: Vec::new(),
        })))
        .filter(|f| {
            let w = f.writes();
            !w.is_empty() && !w.iter().all(|t| over.contains(t))
        })
        .collect();
    if out.len() > MAX_PRIOR {
        out.drain(..out.len() - MAX_PRIOR);
    }
    out
}

/// Publishes one WebGL2 frame (`cmds` is the JSON draw list). Commands
/// WebGL would refuse are left out; the first one's error code is returned
/// for the context's `getError` (0 when all were fine).
fn gl_publish<'js>(ctx: Ctx<'js>, id: f64, w: f64, h: f64, cmds: String) -> rquickjs::Result<u32> {
    let mut cmds: Vec<GlCmd> = serde_json::from_str(&cmds).map_err(|e| throw(&ctx, e))?;
    let mut error = 0;
    cmds.retain(|c| match cmd_error(c) {
        Some(e) => {
            if error == 0 {
                error = e;
            }
            false
        }
        None => true,
    });
    let id = id as u64;
    with(|s| s.gl_frames.insert(id));
    let last = frame(id);
    let version = last.as_ref().map_or(1, |f| f.version() + 1);
    let mut next = GlFrame {
        w: w.max(0.0) as u32,
        h: h.max(0.0) as u32,
        version,
        cmds,
        prior: Vec::new(),
    };
    if let Some(Frame::Gl(last)) = &last {
        next.prior = carry(Some(last), &next);
        // A frame that drew nothing on the canvas leaves it showing what
        // the last one drew there.
        if next.cmds.iter().all(|c| c.target().is_some()) {
            next.cmds
                .extend(last.cmds.iter().filter(|c| c.target().is_none()).cloned());
        }
    }
    set_frame(id, Some(Frame::Gl(Arc::new(next))));
    if let Some(n) = with(|s| s.notify.clone()) {
        n();
    }
    Ok(error)
}

// ------------------------------------------------------------------ audio

/// Blackman window, as the Web Audio spec has it (alpha 0.16).
pub fn blackman(n: usize) -> Vec<f32> {
    let a = 0.16f64;
    let (a0, a1, a2) = ((1.0 - a) / 2.0, 0.5, a / 2.0);
    (0..n)
        .map(|i| {
            let x = std::f64::consts::TAU * i as f64 / n as f64;
            (a0 - a1 * x.cos() + a2 * (2.0 * x).cos()) as f32
        })
        .collect()
}

/// In-place radix-2 FFT (`re.len()` a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n < 2 || !n.is_power_of_two() {
        return;
    }
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f64::consts::TAU / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (br, bi) = (re[b] as f64, im[b] as f64);
                let (tr, ti) = (br * cr - bi * ci, br * ci + bi * cr);
                re[b] = (re[a] as f64 - tr) as f32;
                im[b] = (im[a] as f64 - ti) as f32;
                re[a] = (re[a] as f64 + tr) as f32;
                im[a] = (im[a] as f64 + ti) as f32;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// `|X[k]| / N` for `k < N/2` of the Blackman-windowed block (the
/// spec's magnitudes before smoothing).
pub fn magnitudes(block: &[f32]) -> Vec<f32> {
    let n = block.len();
    let w = blackman(n);
    let mut re: Vec<f32> = block.iter().zip(&w).map(|(x, w)| x * w).collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..n / 2)
        .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() / n as f32)
        .collect()
}

fn demo_audio() -> bool {
    cfg!(feature = "demo") && std::env::var_os("TIDALFAST_DEMO_LUNA").is_some()
}

/// A moving chord for demo builds, which play no audio.
fn synth(n: usize, channel: i32) -> Vec<f32> {
    static T0: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    let rate = 48_000.0f64;
    let now = T0.elapsed().as_secs_f64();
    let end = (now * rate) as i64;
    let pan = match channel {
        0 => 1.0,
        1 => 0.6,
        _ => 0.8,
    };
    (0..n)
        .map(|k| {
            let t = (end - n as i64 + k as i64) as f64 / rate;
            let beat = (1.0 - (t * 2.0).fract()).powi(3);
            let mut v = 0.0;
            // Log-spaced partials from 55 Hz to ~14 kHz, each swelling on
            // its own, falling off towards the top like music does.
            for i in 0..24 {
                let f = 55.0 * 2f64.powf(i as f64 / 3.0);
                let lfo = 0.5 + 0.5 * (t * (0.5 + i as f64 * 0.29) + i as f64).sin();
                let ph = if channel == 1 { 0.9 * i as f64 } else { 0.0 };
                v += (std::f64::consts::TAU * f * t + ph).sin() * lfo.powi(2)
                    / (1.0 + i as f64 * 0.35);
            }
            (v * (0.12 + 0.2 * beat) * pan) as f32
        })
        .collect()
}

/// The rate plugins' audio runs at. Chromium runs an `AudioContext` at the
/// output device's mixing rate, 48 kHz on Windows' shared-mode default,
/// and resamples what it is given (TIDAL's playing song, through
/// `captureStream()`) to it, whatever the song's own rate. Spectra keep
/// their 0 to 24 kHz span: at an 88.2 kHz song's own rate half the bins
/// would always be empty, and a plugin that averages them (audio-visualizer
/// hides its panel under a level) takes a quiet song for silence.
const CONTEXT_RATE: f64 = 48_000.0;

fn samples(n: usize, channel: i32) -> Vec<f32> {
    crate::audio_tap::arm();
    if demo_audio() && !crate::audio_tap::live() {
        return synth(n, channel);
    }
    let src = f64::from(crate::audio_tap::rate());
    if src <= 0.0 || (src - CONTEXT_RATE).abs() < 1.0 {
        return crate::audio_tap::read(n, channel);
    }
    let need = ((n as f64 * src / CONTEXT_RATE).ceil() as usize + 2).min(crate::audio_tap::FRAMES);
    to_context_rate(&crate::audio_tap::read(need, channel), src, n)
}

/// The last `n` samples at [`CONTEXT_RATE`] of `block`, the latest samples
/// at `src` Hz, by linear interpolation back from its last sample; what
/// the block does not reach is silence.
fn to_context_rate(block: &[f32], src: f64, n: usize) -> Vec<f32> {
    let step = src / CONTEXT_RATE;
    let last = block.len() as f64 - 1.0;
    (0..n)
        .map(|k| {
            let at = last - (n - 1 - k) as f64 * step;
            if at < 0.0 {
                return 0.0;
            }
            let i = at.floor() as usize;
            let f = (at - i as f64) as f32;
            let a = block[i];
            let b = block.get(i + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}

/// The last `n` samples of a channel (0, 1, or -1 for mono).
fn audio_read<'js>(ctx: Ctx<'js>, n: f64, channel: f64) -> rquickjs::Result<ArrayBuffer<'js>> {
    let v = samples(
        (n.max(0.0) as usize).min(crate::audio_tap::FRAMES),
        channel as i32,
    );
    f32_buffer(ctx, &v)
}

/// Spectrum magnitudes of the last `n` samples of a channel.
fn audio_fft<'js>(ctx: Ctx<'js>, n: f64, channel: f64) -> rquickjs::Result<ArrayBuffer<'js>> {
    let n = (n.max(32.0) as usize)
        .min(crate::audio_tap::FRAMES)
        .next_power_of_two();
    let v = magnitudes(&samples(n, channel as i32));
    f32_buffer(ctx, &v)
}

/// `[sampleRate, position, live]`; position changes as audio arrives.
fn audio_info() -> Vec<f64> {
    if demo_audio() && !crate::audio_tap::live() {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        return vec![48_000.0, (t / 16.0).floor(), 1.0];
    }
    let src = f64::from(crate::audio_tap::rate());
    let position = crate::audio_tap::position() as f64;
    vec![
        CONTEXT_RATE,
        if src > 0.0 {
            (position * CONTEXT_RATE / src).floor()
        } else {
            position
        },
        if crate::audio_tap::live() { 1.0 } else { 0.0 },
    ]
}

fn demo_flag() -> bool {
    demo_audio()
}

// ------------------------------------------------------------------ install

/// Installs `__canvas` on the global object. `notify` asks the UI to
/// repaint (a canvas published a frame).
pub(super) fn install(ctx: &Ctx<'_>, notify: Notify) -> rquickjs::Result<()> {
    with(|s| s.notify = Some(notify));
    let o = Object::new(ctx.clone())?;
    macro_rules! native {
        ($($name:literal => $f:expr),* $(,)?) => {
            $( o.set($name, Function::new(ctx.clone(), $f)?)?; )*
        };
    }
    native! {
        "create" => create,
        "resize" => resize,
        "free" => free,
        "decode" => decode,
        "fill" => fill,
        "stroke" => stroke,
        "clip" => clip,
        "save" => save,
        "restore" => restore,
        "reset" => clear_all,
        "drawImage" => draw_image,
        "getData" => get_data,
        "putData" => put_data,
        "png" => png,
        "text" => text,
        "measure" => measure_js,
        "publish" => publish,
        "glProgram" => program,
        "glInputs" => program_inputs,
        "glBuffer" => buffer,
        "glBufferFree" => buffer_free,
        "glTexImage" => tex_image,
        "glTexFree" => tex_free,
        "glPublish" => gl_publish,
        "audioRead" => audio_read,
        "audioFft" => audio_fft,
        "audioInfo" => audio_info,
        "demo" => demo_flag,
    }
    ctx.globals().set("__canvas", o)?;
    Ok(())
}

/// Drops this thread's canvases, their frames, and the WebGL buffers and
/// textures its plugins made (Luna stopped: finalizers do not run at
/// teardown, so nothing else would).
pub(super) fn shutdown() {
    let (ids, buffers, textures) = with(|s| {
        s.notify = None;
        let mut ids: Vec<u64> = s.surfaces.drain().map(|(k, _)| k).collect();
        ids.extend(s.gl_frames.drain());
        let buffers: Vec<u64> = s.gl_buffers.drain().collect();
        let textures: Vec<u64> = s.gl_textures.drain().map(|(k, _)| k).collect();
        (ids, buffers, textures)
    });
    let mut m = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    for id in ids {
        m.remove(&id);
    }
    drop(m);
    let mut b = BUFFERS.lock().unwrap_or_else(PoisonError::into_inner);
    for k in buffers {
        b.remove(&k);
    }
    drop(b);
    let mut t = TEXTURES.lock().unwrap_or_else(PoisonError::into_inner);
    for k in textures {
        t.remove(&k);
    }
    drop(t);
    crate::audio_tap::disarm();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gradient_has_no_more_stops_than_its_buffer() {
        let p = [3e38, 0.5, 1.0, 0.0, 0.0, 1.0, 0.7];
        assert_eq!(stops(&p, 0.0).len(), 1);
        assert!(stops(&[-5.0], 0.0).is_empty());
    }

    fn draw(first: i32, count: i32) -> GlDraw {
        GlDraw {
            prog: 1,
            mode: 4,
            first,
            count,
            uniforms: Vec::new(),
            attribs: vec![GlAttrib {
                loc: 0,
                buffer: 7,
                size: 2,
                stride: 0,
                offset: 0,
            }],
            blend: None,
            fb: None,
            vp: None,
            tex: Vec::new(),
            elements: None,
        }
    }

    #[test]
    fn draws_are_checked_against_their_buffers_like_webgl() {
        // Three 2D vertices (24 bytes) and six u16 indices, the last 900.
        let verts = Arc::new(vec![0u8; 24]);
        let idx: Vec<u8> = [0u16, 1, 2, 0, 1, 900]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        let idx = Arc::new(idx);
        let buffers = |k: u64| match k {
            7 => Some(verts.clone()),
            8 => Some(idx.clone()),
            _ => None,
        };
        assert_eq!(draw_error(&draw(0, 3), &buffers), None);
        assert_eq!(draw_error(&draw(1, 3), &buffers), Some(INVALID_OPERATION));
        assert_eq!(
            draw_error(&draw(0, 1_000_000), &buffers),
            Some(INVALID_OPERATION)
        );
        assert_eq!(draw_error(&draw(-1, 3), &buffers), Some(INVALID_VALUE));
        let mut d = draw(0, 3);
        d.attribs[0].offset = 8;
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "offset eats a vertex"
        );
        d.attribs[0].offset = 2;
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "unaligned"
        );
        let mut d = draw(0, 3);
        d.attribs[0].buffer = 99;
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "no buffer"
        );
        let mut d = draw(0, 3);
        d.elements = Some([8, 0x1403, 0]);
        assert_eq!(draw_error(&d, &buffers), None);
        d.elements = Some([8, 0x1403, 6]);
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "index 900"
        );
        d.elements = Some([8, 0x1403, 1]);
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "unaligned"
        );
        d.count = 30;
        d.elements = Some([8, 0x1403, 0]);
        assert_eq!(
            draw_error(&d, &buffers),
            Some(INVALID_OPERATION),
            "past the indices"
        );
        d.elements = Some([8, 0x1406, 0]);
        assert_eq!(draw_error(&d, &buffers), Some(INVALID_ENUM));
        // Without attributes, only the vertex cap applies.
        let mut d = draw(0, MAX_DRAW_VERTICES + 1);
        d.attribs.clear();
        assert_eq!(draw_error(&d, &buffers), Some(INVALID_VALUE));
        assert_eq!(
            cmd_error(&GlCmd::Tex {
                tex: 1,
                w: 100_000,
                h: 100_000,
                version: 1
            }),
            Some(INVALID_VALUE)
        );
    }

    #[test]
    fn canvas_memory_has_a_budget_and_luna_stopping_frees_gl_objects() {
        std::thread::spawn(|| {
            // Textures already hold all but 8 bytes of the budget: a 2x2
            // canvas (16 bytes) gets no pixels, a 1x2 one does.
            with(|s| s.gl_textures.insert(1, MEMORY_BUDGET - 8));
            let big = create(2.0, 2.0) as u64;
            let small = create(1.0, 2.0) as u64;
            with(|s| {
                assert!(s.surfaces[&big].pix.is_none());
                assert!(s.surfaces[&small].pix.is_some());
            });
            assert_eq!(
                tex_image(5.0, vec![4.0, 4.0], None, -1.0, false, Vec::new()),
                0.0
            );
            with(|s| s.gl_textures.remove(&1));
            resize(big as f64, 2.0, 2.0);
            with(|s| assert!(s.surfaces[&big].pix.is_some()));
            let key = 9_000_000_123.0;
            assert!(tex_image(key, vec![2.0, 2.0], None, -1.0, false, Vec::new()) > 0.0);
            BUFFERS
                .lock()
                .unwrap()
                .insert(9_000_000_124, (1, Arc::new(vec![1])));
            with(|s| s.gl_buffers.insert(9_000_000_124));
            shutdown();
            assert!(gl_texture(9_000_000_123, |t| t.is_none()));
            assert!(gl_buffer(9_000_000_124).is_none());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn fft_finds_a_sine_in_its_bin() {
        let n = 1024;
        let block: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 64.0 * i as f32 / n as f32).sin())
            .collect();
        let m = magnitudes(&block);
        let peak = m
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, 64);
        // A full-scale sine through the Blackman window: 0.42 / 2.
        assert!((m[64] - 0.21).abs() < 0.01, "{}", m[64]);
        assert!(m[200] < 1e-4);
    }

    #[test]
    fn blackman_matches_the_spec() {
        let w = blackman(8);
        assert!(w[0].abs() < 1e-6);
        assert!((w[4] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn vertex_inputs_are_found_in_order() {
        let vs = "#version 300 es\n// in vec2 nope;\nin vec2 a_position;\nlayout(location = 1) in vec4 a_color;\nuniform float u;\nvoid main() { vec2 x; gl_Position = vec4(a_position, 0.0, 1.0); }";
        assert_eq!(vertex_inputs(vs), vec!["a_position", "a_color"]);
    }

    #[test]
    fn text_has_ink_and_width() {
        let [w, asc, desc] = measure("Hello", 20.0, 400.0, "");
        assert!(w > 30.0 && w < 70.0, "{w}");
        assert!(asc > 10.0 && desc > 2.0, "{asc} {desc}");
        let p: Vec<f32> = vec![
            0.0, 20.0, 20.0, 400.0, 0.0, 0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
        ];
        assert!(text_path("Hi", &p, "").is_some());
    }
}
