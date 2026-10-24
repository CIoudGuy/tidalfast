//! `backdrop-filter`: what is painted behind a box, blurred and recoloured
//! as a browser does it, under the box's own fill.
//!
//! A theme's frosted glass (the cotton candy family's play queue,
//! `backdrop-filter: blur(.8em) brightness(.75) saturate(1.1)`; Blur's
//! player bar, `blur(12px)`) is drawn on the GPU in an egui paint callback
//! placed in the box's paint order before its fill: the callback copies the
//! window's pixels under the box, blurs them (a Gaussian of the filter's
//! radius, worked at a reduced size once the radius is a few pixels: each
//! halving is a 2x2 average, then a separable Gaussian of what remains,
//! then a bilinear stretch back), runs the colour functions over them in
//! order (`brightness`, `saturate`, `contrast`, ...), and lays the result
//! back inside the box's corners. Edges mirror what lies inside the box, as
//! Chromium's backdrop filter does. The box's own translucent fill then
//! goes over it as the theme gives it.
//!
//! Only a box whose theme asks for a backdrop filter adds a callback, so a
//! window with no theme draws exactly as before. Without OpenGL 3 (or GLES
//! 3) the parts keep their earlier stand-in: the filter's brightness folded
//! into the fill and a blurred box made opaque over the window's colour.
//! Nothing is kept between frames but the GPU textures, grown to the
//! largest box seen; no frame is drawn while the window is idle, so an idle
//! window costs nothing.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use egui::{CornerRadius, Rect};

/// The most colour functions one filter runs (a theme writes two or three).
const MAX_STEPS: usize = 8;
/// The most texels a Gaussian pass reads on each side of a pixel.
const MAX_TAPS: usize = 16;
/// The standard deviation, in texels, below which the blur is worked at
/// the size it is at: larger ones are halved first.
const SIGMA_AT_SIZE: f32 = 3.0;

static AVAILABLE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// A test's answer for [`available`] on its own thread.
    static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether backdrop filters are drawn: the window's OpenGL context can run
/// the passes (OpenGL 3 or GLES 3). Set once from the window's context.
pub fn available() -> bool {
    FORCED
        .with(Cell::get)
        .unwrap_or_else(|| AVAILABLE.load(Ordering::Relaxed))
}

/// Notes whether `gl` can draw backdrop filters (see [`available`]).
pub fn init(gl: &glow::Context) {
    AVAILABLE.store(shader_header(gl).is_some(), Ordering::Relaxed);
}

/// While the returned guard lives, [`available`] answers `on` on this
/// thread (tests: the parts as they look with or without the GPU passes).
#[cfg(test)]
pub fn force(on: bool) -> impl Drop {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FORCED.with(|f| f.set(self.0));
        }
    }
    Restore(FORCED.with(|f| f.replace(Some(on))))
}

/// One box's backdrop filter, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// The blur's standard deviation in points (0 for none).
    pub blur: f32,
    /// The colour functions in order, each three rows of `[r, g, b,
    /// offset]` (see [`crate::css_filter::ColorFilter::color_rows`]).
    pub rows: Vec<[f32; 12]>,
}

impl Backdrop {
    /// The backdrop filter in `value` (its variables resolved), or `None`
    /// when it changes nothing (`none`, `blur(0)`, `brightness(1)`).
    pub fn parse(value: &str) -> Option<Self> {
        let filter = crate::css_filter::ColorFilter::parse(value)?;
        let blur = filter.blur_sigma().unwrap_or(0.0);
        let mut rows = filter.color_rows();
        rows.truncate(MAX_STEPS);
        (blur >= 0.05 || !rows.is_empty()).then_some(Self { blur, rows })
    }
}

/// Draws `backdrop` behind a box in `rect` with `corners`, at the
/// painter's opacity: call it where the box's paint order starts, before
/// its fill. Nothing is added while backdrop filters are not drawn.
pub fn paint(painter: &egui::Painter, rect: Rect, corners: CornerRadius, backdrop: &Arc<Backdrop>) {
    if !available() || !rect.is_positive() {
        return;
    }
    let opacity = painter.opacity();
    if opacity <= 0.0 {
        return;
    }
    let backdrop = Arc::clone(backdrop);
    let callback = egui_glow::CallbackFn::new(move |info, painter| {
        // SAFETY: the callback runs inside egui_glow's paint, with its
        // context current; every state it changes egui_glow sets again
        // after it (`prepare_painting`), and the framebuffer is put back.
        unsafe { draw(&info, painter.gl(), &backdrop, corners, opacity) };
    });
    painter.add(egui::PaintCallback {
        rect,
        callback: Arc::new(callback),
    });
}

/// The GLSL header for `gl`, or `None` when it is older than OpenGL 3 or
/// GLES 3 (no `gl_VertexID`, no framebuffer blits).
fn shader_header(gl: &glow::Context) -> Option<&'static str> {
    let v = gl.version();
    if v.is_embedded {
        (v.major >= 3).then_some("#version 300 es\nprecision highp float;\nprecision highp int;\n")
    } else if v.major > 3 || (v.major == 3 && v.minor >= 3) {
        Some("#version 330 core\n")
    } else {
        (v.major == 3).then_some("#version 130\n")
    }
}

const QUAD_VS: &str = r"
uniform vec4 u_quad;
void main() {
    vec2 c = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    gl_Position = vec4(mix(u_quad.xy, u_quad.zw, c), 0.0, 1.0);
}
";

/// A pass from one texture region to another: a 2x2 average when it
/// halves, or one direction of a Gaussian. Reads mirror at the region's
/// edges.
const PASS_FS: &str = r"
uniform sampler2D u_tex;
uniform vec2 u_tex_size;
uniform vec2 u_src_size;
uniform vec2 u_scale;
uniform vec2 u_dir;
uniform int u_taps;
uniform float u_weights[17];
out vec4 o_color;
vec2 mirror(vec2 q) {
    q = abs(q);
    q = u_src_size - abs(u_src_size - q);
    return clamp(q, vec2(0.5), u_src_size - vec2(0.5));
}
void main() {
    vec2 p = gl_FragCoord.xy * u_scale;
    vec4 acc = texture(u_tex, mirror(p) / u_tex_size) * u_weights[0];
    for (int i = 1; i <= u_taps; i++) {
        vec2 d = u_dir * float(i);
        acc += (texture(u_tex, mirror(p + d) / u_tex_size)
            + texture(u_tex, mirror(p - d) / u_tex_size)) * u_weights[i];
    }
    o_color = acc;
}
";

/// The filtered backdrop laid into the box: the colour functions in
/// order, clamped after each, inside the box's corners.
const COMPOSITE_FS: &str = r"
uniform sampler2D u_tex;
uniform vec2 u_tex_size;
uniform vec2 u_src_size;
uniform vec4 u_region;
uniform vec4 u_box;
uniform vec4 u_radii;
uniform float u_opacity;
uniform int u_steps;
uniform vec4 u_rows[24];
out vec4 o_color;
void main() {
    vec2 q = (gl_FragCoord.xy - u_region.xy) * (u_src_size / u_region.zw);
    q = clamp(q, vec2(0.5), u_src_size - vec2(0.5));
    vec3 c = texture(u_tex, q / u_tex_size).rgb;
    for (int i = 0; i < u_steps; i++) {
        vec4 r = u_rows[i * 3];
        vec4 g = u_rows[i * 3 + 1];
        vec4 b = u_rows[i * 3 + 2];
        c = clamp(vec3(dot(r.xyz, c) + r.w, dot(g.xyz, c) + g.w, dot(b.xyz, c) + b.w), 0.0, 1.0);
    }
    vec2 half_size = u_box.zw * 0.5;
    vec2 d = gl_FragCoord.xy - u_box.xy - half_size;
    float rad = d.x < 0.0 ? (d.y < 0.0 ? u_radii.x : u_radii.w) : (d.y < 0.0 ? u_radii.y : u_radii.z);
    rad = min(rad, min(half_size.x, half_size.y));
    vec2 k = abs(d) - half_size + vec2(rad);
    float dist = length(max(k, vec2(0.0))) + min(max(k.x, k.y), 0.0) - rad;
    float a = clamp(0.5 - dist, 0.0, 1.0) * u_opacity;
    o_color = vec4(c * a, a);
}
";

/// A texture that only grows, the size it has, and a framebuffer drawing
/// into it (attached once: re-attaching for every pass costs the driver a
/// check each time).
#[derive(Clone, Copy)]
struct Target {
    texture: glow::Texture,
    fbo: glow::Framebuffer,
    size: [i32; 2],
}

struct Pass {
    program: glow::Program,
    quad: Option<glow::UniformLocation>,
    tex: Option<glow::UniformLocation>,
    tex_size: Option<glow::UniformLocation>,
    src_size: Option<glow::UniformLocation>,
    scale: Option<glow::UniformLocation>,
    dir: Option<glow::UniformLocation>,
    taps: Option<glow::UniformLocation>,
    weights: Option<glow::UniformLocation>,
}

struct Composite {
    program: glow::Program,
    quad: Option<glow::UniformLocation>,
    tex: Option<glow::UniformLocation>,
    tex_size: Option<glow::UniformLocation>,
    src_size: Option<glow::UniformLocation>,
    region: Option<glow::UniformLocation>,
    bounds: Option<glow::UniformLocation>,
    radii: Option<glow::UniformLocation>,
    opacity: Option<glow::UniformLocation>,
    steps: Option<glow::UniformLocation>,
    rows: Option<glow::UniformLocation>,
}

/// The GPU objects for one GL context.
struct Gpu {
    pass: Pass,
    composite: Composite,
    vao: glow::VertexArray,
    /// The copy of the window under the box, then each halving.
    levels: Vec<Option<Target>>,
    /// The Gaussian's first direction.
    scratch: Option<Target>,
}

/// The GPU objects per GL context (the main window's and any other
/// window's), made on first use; `None` where they could not be made.
static GPU: Mutex<Vec<(usize, Option<Gpu>)>> = Mutex::new(Vec::new());

unsafe fn program(gl: &glow::Context, header: &str, fs: &str) -> Option<glow::Program> {
    unsafe {
        let program = gl.create_program().ok()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, QUAD_VS), (glow::FRAGMENT_SHADER, fs)] {
            let shader = gl.create_shader(kind).ok()?;
            gl.shader_source(shader, &format!("{header}{src}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                log::warn!("backdrop shader: {}", gl.get_shader_info_log(shader));
                gl.delete_shader(shader);
                return None;
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if !gl.get_program_link_status(program) {
            log::warn!("backdrop program: {}", gl.get_program_info_log(program));
            gl.delete_program(program);
            return None;
        }
        Some(program)
    }
}

impl Gpu {
    unsafe fn new(gl: &glow::Context) -> Option<Self> {
        let header = shader_header(gl)?;
        unsafe {
            let p = program(gl, header, PASS_FS)?;
            let c = program(gl, header, COMPOSITE_FS)?;
            let u = |prog, name: &str| gl.get_uniform_location(prog, name);
            let pass = Pass {
                program: p,
                quad: u(p, "u_quad"),
                tex: u(p, "u_tex"),
                tex_size: u(p, "u_tex_size"),
                src_size: u(p, "u_src_size"),
                scale: u(p, "u_scale"),
                dir: u(p, "u_dir"),
                taps: u(p, "u_taps"),
                weights: u(p, "u_weights"),
            };
            let composite = Composite {
                program: c,
                quad: u(c, "u_quad"),
                tex: u(c, "u_tex"),
                tex_size: u(c, "u_tex_size"),
                src_size: u(c, "u_src_size"),
                region: u(c, "u_region"),
                bounds: u(c, "u_box"),
                radii: u(c, "u_radii"),
                opacity: u(c, "u_opacity"),
                steps: u(c, "u_steps"),
                rows: u(c, "u_rows"),
            };
            Some(Self {
                pass,
                composite,
                vao: gl.create_vertex_array().ok()?,
                levels: Vec::new(),
                scratch: None,
            })
        }
    }

    /// A target at least `size`, grown (never shrunk) as needed.
    unsafe fn grow(
        gl: &glow::Context,
        slot: &mut Option<Target>,
        size: [i32; 2],
    ) -> Option<Target> {
        if let Some(t) = slot
            && t.size[0] >= size[0]
            && t.size[1] >= size[1]
        {
            return Some(*t);
        }
        unsafe {
            let grown = slot
                .as_ref()
                .map_or(size, |t| [t.size[0].max(size[0]), t.size[1].max(size[1])]);
            let (texture, fbo) = match slot.take() {
                Some(t) => (t.texture, t.fbo),
                None => (gl.create_texture().ok()?, gl.create_framebuffer().ok()?),
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (k, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, k, v as i32);
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                grown[0],
                grown[1],
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let target = Target {
                texture,
                fbo,
                size: grown,
            };
            *slot = Some(target);
            Some(target)
        }
    }

    /// Level `i` (0 the copy, then each halving), at least `size`.
    unsafe fn level(&mut self, gl: &glow::Context, i: usize, size: [i32; 2]) -> Option<Target> {
        if self.levels.len() <= i {
            self.levels.resize_with(i + 1, || None);
        }
        unsafe { Self::grow(gl, &mut self.levels[i], size) }
    }

    /// One pass from the region `src_size` of `src` into the region
    /// `dst_size` of `dst`.
    unsafe fn run(
        &self,
        gl: &glow::Context,
        (src, src_size): (Target, [i32; 2]),
        (dst, dst_size): (Target, [i32; 2]),
        dir: [f32; 2],
        weights: &[f32],
    ) {
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(dst.fbo));
            gl.viewport(0, 0, dst_size[0], dst_size[1]);
            gl.bind_texture(glow::TEXTURE_2D, Some(src.texture));
            let p = &self.pass;
            gl.uniform_4_f32(p.quad.as_ref(), -1.0, -1.0, 1.0, 1.0);
            gl.uniform_1_i32(p.tex.as_ref(), 0);
            gl.uniform_2_f32(p.tex_size.as_ref(), src.size[0] as f32, src.size[1] as f32);
            gl.uniform_2_f32(p.src_size.as_ref(), src_size[0] as f32, src_size[1] as f32);
            gl.uniform_2_f32(
                p.scale.as_ref(),
                src_size[0] as f32 / dst_size[0] as f32,
                src_size[1] as f32 / dst_size[1] as f32,
            );
            gl.uniform_2_f32(p.dir.as_ref(), dir[0], dir[1]);
            let mut w = [0.0f32; MAX_TAPS + 1];
            for (out, v) in w.iter_mut().zip(weights) {
                *out = *v;
            }
            gl.uniform_1_i32(p.taps.as_ref(), weights.len().saturating_sub(1) as i32);
            gl.uniform_1_f32_slice(p.weights.as_ref(), &w);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

/// A Gaussian's weights for offsets 0 to its reach, which add to one over
/// both sides.
fn gaussian(sigma: f32) -> Vec<f32> {
    let reach = ((sigma * 3.0).ceil() as usize).clamp(1, MAX_TAPS);
    let mut w: Vec<f32> = (0..=reach)
        .map(|i| (-(i as f32).powi(2) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total = w[0] + 2.0 * w[1..].iter().sum::<f32>();
    for v in &mut w {
        *v /= total;
    }
    w
}

/// How many halvings a blur of `sigma` pixels is worked at, and the
/// standard deviation left for the Gaussian there, in that size's texels.
/// Each halving is a 2x2 average, which blurs too: the variance it adds
/// (a quarter of the texel spacing squared at each step) is taken off.
fn plan(sigma: f32, size: [i32; 2]) -> (u32, f32) {
    let mut halvings = 0;
    let mut scale = 1.0f32;
    while sigma / scale > SIGMA_AT_SIZE
        && size[0] as f32 / (scale * 2.0) >= 4.0
        && size[1] as f32 / (scale * 2.0) >= 4.0
        && halvings < 6
    {
        halvings += 1;
        scale *= 2.0;
    }
    let added = (scale * scale - 1.0) / 12.0;
    let left = (sigma * sigma - added).max(0.0).sqrt() / scale;
    (halvings, left)
}

/// The callback's work: copy, halve, blur, recolour, lay back.
unsafe fn draw(
    info: &egui::PaintCallbackInfo,
    gl: &Arc<glow::Context>,
    backdrop: &Backdrop,
    corners: CornerRadius,
    opacity: f32,
) {
    let context = Arc::as_ptr(gl) as usize;
    let Ok(mut all) = GPU.lock() else {
        return;
    };
    if !all.iter().any(|(c, _)| *c == context) {
        let made = unsafe { Gpu::new(gl) };
        all.push((context, made));
    }
    let Some(gpu) = all
        .iter_mut()
        .find(|(c, _)| *c == context)
        .and_then(|(_, g)| g.as_mut())
    else {
        return;
    };
    let ppp = info.pixels_per_point;
    let [screen_w, screen_h] = info.screen_size_px.map(|v| v as i32);
    // The box as the window has it (its corners), and the part of it on
    // screen, which is what is copied.
    let r = info.viewport;
    let bounds = [
        r.min.x * ppp,
        screen_h as f32 - r.max.y * ppp,
        r.width() * ppp,
        r.height() * ppp,
    ];
    let region = info.viewport_in_pixels();
    let clip = info.clip_rect_in_pixels();
    let size = [region.width_px, region.height_px];
    if size[0] <= 0 || size[1] <= 0 || clip.width_px <= 0 || clip.height_px <= 0 {
        return;
    }
    let (halvings, sigma) = if backdrop.blur * ppp >= 0.5 {
        plan(backdrop.blur * ppp, size)
    } else {
        (0, 0.0)
    };
    unsafe {
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::BLEND);
        gl.bind_vertex_array(Some(gpu.vao));
        gl.active_texture(glow::TEXTURE0);
        // The window's pixels under the box.
        let Some(src) = gpu.level(gl, 0, size) else {
            return restore(gl);
        };
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(src.fbo));
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        gl.blit_framebuffer(
            region.left_px,
            region.from_bottom_px,
            region.left_px + size[0],
            region.from_bottom_px + size[1],
            0,
            0,
            size[0],
            size[1],
            glow::COLOR_BUFFER_BIT,
            glow::NEAREST,
        );
        gl.use_program(Some(gpu.pass.program));
        // Halve it while the blur is wide.
        let (mut tex, mut at) = (src, size);
        for i in 1..=halvings as usize {
            let next = [(at[0] + 1) / 2, (at[1] + 1) / 2];
            let Some(dst) = gpu.level(gl, i, next) else {
                return restore(gl);
            };
            gpu.run(gl, (tex, at), (dst, next), [0.0, 0.0], &[1.0]);
            (tex, at) = (dst, next);
        }
        if sigma >= 0.3 {
            let weights = gaussian(sigma);
            let Some(mid) = Gpu::grow(gl, &mut gpu.scratch, at) else {
                return restore(gl);
            };
            gpu.run(gl, (tex, at), (mid, at), [1.0, 0.0], &weights);
            gpu.run(gl, (mid, at), (tex, at), [0.0, 1.0], &weights);
        }
        // Into the window, inside the box's corners.
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.viewport(0, 0, screen_w, screen_h);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(
            clip.left_px,
            clip.from_bottom_px,
            clip.width_px,
            clip.height_px,
        );
        gl.enable(glow::BLEND);
        gl.blend_equation(glow::FUNC_ADD);
        gl.blend_func_separate(
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE_MINUS_DST_ALPHA,
            glow::ONE,
        );
        let c = &gpu.composite;
        gl.use_program(Some(c.program));
        gl.bind_texture(glow::TEXTURE_2D, Some(tex.texture));
        let ndc = |x: f32, y: f32| {
            [
                x / screen_w as f32 * 2.0 - 1.0,
                y / screen_h as f32 * 2.0 - 1.0,
            ]
        };
        let lo = ndc(region.left_px as f32, region.from_bottom_px as f32);
        let hi = ndc(
            (region.left_px + size[0]) as f32,
            (region.from_bottom_px + size[1]) as f32,
        );
        gl.uniform_4_f32(c.quad.as_ref(), lo[0], lo[1], hi[0], hi[1]);
        gl.uniform_1_i32(c.tex.as_ref(), 0);
        gl.uniform_2_f32(c.tex_size.as_ref(), tex.size[0] as f32, tex.size[1] as f32);
        gl.uniform_2_f32(c.src_size.as_ref(), at[0] as f32, at[1] as f32);
        gl.uniform_4_f32(
            c.region.as_ref(),
            region.left_px as f32,
            region.from_bottom_px as f32,
            size[0] as f32,
            size[1] as f32,
        );
        gl.uniform_4_f32(
            c.bounds.as_ref(),
            bounds[0],
            bounds[1],
            bounds[2],
            bounds[3],
        );
        // GL's y runs up: bottom left, bottom right, top right, top left.
        gl.uniform_4_f32(
            c.radii.as_ref(),
            f32::from(corners.sw) * ppp,
            f32::from(corners.se) * ppp,
            f32::from(corners.ne) * ppp,
            f32::from(corners.nw) * ppp,
        );
        gl.uniform_1_f32(c.opacity.as_ref(), opacity.clamp(0.0, 1.0));
        let mut rows = [0.0f32; MAX_STEPS * 12];
        for (i, m) in backdrop.rows.iter().take(MAX_STEPS).enumerate() {
            rows[i * 12..i * 12 + 12].copy_from_slice(m);
        }
        gl.uniform_1_i32(c.steps.as_ref(), backdrop.rows.len().min(MAX_STEPS) as i32);
        gl.uniform_4_f32_slice(c.rows.as_ref(), &rows);
        gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        restore(gl);
    }
}

/// What egui_glow does not set again after a callback.
unsafe fn restore(gl: &glow::Context) {
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.bind_vertex_array(None);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_parses_into_its_blur_and_colour_rows() {
        let b = Backdrop::parse("blur(.8em) brightness(.75) saturate(1.1)").expect("a filter");
        assert!((b.blur - 11.2).abs() < 1e-3, "{}", b.blur);
        assert_eq!(b.rows.len(), 2);
        assert!((b.rows[0][0] - 0.75).abs() < 1e-6);
        assert_eq!(Backdrop::parse("none"), None);
        assert_eq!(Backdrop::parse("blur(0px) brightness(1)"), None);
        assert_eq!(Backdrop::parse("blur(3px)").map(|b| b.rows.len()), Some(0));
    }

    #[test]
    fn a_wide_blur_is_worked_smaller() {
        // 3 px stays at its size; 11.2 px at 2x is halved twice.
        assert_eq!(plan(3.0, [400, 400]).0, 0);
        let (halvings, left) = plan(22.4, [400, 1000]);
        assert_eq!(halvings, 3);
        assert!(left > 2.0 && left <= 3.0, "{left}");
        // A thin box is not halved below a few texels.
        assert_eq!(plan(40.0, [400, 10]).0, 1);
        let w = gaussian(2.0);
        let total = w[0] + 2.0 * w[1..].iter().sum::<f32>();
        assert!((total - 1.0).abs() < 1e-5);
    }
}
