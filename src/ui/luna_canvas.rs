//! Draws Luna plugins' `<canvas>` elements (see `src/luna/canvas.rs`).
//!
//! A 2D canvas is a texture, uploaded again only when the plugin drew
//! since. A WebGL2 canvas replays the draw list its last frame recorded
//! with OpenGL inside an egui paint callback, straight into the element's
//! rectangle: `gl_FragCoord` is rewritten to the canvas's own pixel space,
//! so shaders that read it see what they would in a browser. Textures and
//! framebuffers are real GL objects kept per context; frames this context
//! has not run yet have their offscreen work replayed first, so what a
//! plugin rendered into a framebuffer once stays there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use egui::{Color32, Rect, Sense, TextureHandle, pos2, vec2};

use crate::luna::canvas::{self as lc, Frame, GlCmd, GlFrame};
use crate::luna::dom::VNode;

/// The canvas id a VNode carries (`markup` = `canvas:<id>`).
fn key(node: &VNode) -> Option<u64> {
    node.markup
        .as_deref()?
        .strip_prefix("canvas:")?
        .parse()
        .ok()
}

/// Whether `node` is a canvas with a context (others are not drawn).
pub fn has_frame(node: &VNode) -> bool {
    key(node).is_some()
}

struct Tex {
    version: u64,
    handle: TextureHandle,
    pass: u64,
}

thread_local! {
    static TEXTURES: RefCell<HashMap<u64, Tex>> = RefCell::default();
}

/// Draws the canvas at its CSS size (`width`/`height`), or its pixel size.
/// `true` when it took room.
pub fn show(ui: &mut egui::Ui, node: &VNode, width: Option<f32>, height: Option<f32>) -> bool {
    let Some(id) = key(node) else { return false };
    let frame = lc::frame(id);
    let attr = |n: &str| {
        node.attrs
            .iter()
            .find(|(k, _)| k == n)
            .and_then(|(_, v)| v.trim().parse::<f32>().ok())
    };
    let [fw, fh] = frame.as_ref().map_or([0, 0], Frame::size);
    let pw = attr("width").unwrap_or(fw as f32).max(0.0);
    let ph = attr("height").unwrap_or(fh as f32).max(0.0);
    let size = match (width, height) {
        (Some(w), Some(h)) => vec2(w, h),
        (Some(w), None) if pw > 0.0 => vec2(w, w * ph / pw),
        (None, Some(h)) if ph > 0.0 => vec2(h * pw / ph, h),
        (w, h) => vec2(w.unwrap_or(pw), h.unwrap_or(ph)),
    };
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) || size.x <= 0.0 || size.y <= 0.0 {
        return true;
    }
    match frame {
        Some(Frame::Pixels(p)) if p.w > 0 && p.h > 0 => {
            let pass = ui.ctx().cumulative_pass_nr();
            let tex = TEXTURES.with(|t| {
                let mut t = t.borrow_mut();
                let entry = t.get_mut(&id);
                let handle = match entry {
                    Some(e) if e.version == p.version => {
                        e.pass = pass;
                        e.handle.clone()
                    }
                    other => {
                        // A canvas may be wider than the GPU takes (its
                        // limit is on area): scaled to fit.
                        let img = crate::texture_fit::fit(
                            ui.ctx(),
                            egui::ColorImage::from_rgba_premultiplied(
                                [p.w as usize, p.h as usize],
                                &p.rgba,
                            ),
                        );
                        match other {
                            Some(e) => {
                                e.handle.set(img, egui::TextureOptions::LINEAR);
                                e.version = p.version;
                                e.pass = pass;
                                e.handle.clone()
                            }
                            None => {
                                let handle = ui.ctx().load_texture(
                                    format!("luna-canvas-{id}"),
                                    img,
                                    egui::TextureOptions::LINEAR,
                                );
                                t.insert(
                                    id,
                                    Tex {
                                        version: p.version,
                                        handle: handle.clone(),
                                        pass,
                                    },
                                );
                                handle
                            }
                        }
                    }
                };
                // Canvases no longer drawn give their textures back.
                t.retain(|_, e| pass.saturating_sub(e.pass) < 600);
                handle
            });
            ui.painter().image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        Some(Frame::Gl(g)) if g.w > 0 && g.h > 0 && !g.cmds.is_empty() => {
            // Inside the now-playing cover's tilt, or faded, the canvas is
            // drawn offscreen and laid on the screen as a (projected) quad:
            // a paint callback is not something the tilt can warp.
            let opacity = ui.opacity();
            let corners = super::cover_tilt::projected(rect).or_else(|| {
                (opacity < 1.0).then(|| {
                    [
                        rect.left_top(),
                        rect.right_top(),
                        rect.right_bottom(),
                        rect.left_bottom(),
                    ]
                    .map(|p| (p, 1.0))
                })
            });
            let (area, cb) = match corners {
                Some(corners) => {
                    let area = Rect::from_points(&corners.map(|(p, _)| p)).expand(1.0);
                    let cb = egui_glow::CallbackFn::new(move |info, painter| {
                        replay_quad(id, &g, &info, painter, corners, opacity);
                    });
                    (area, cb)
                }
                None => (
                    rect,
                    egui_glow::CallbackFn::new(move |info, painter| {
                        replay(id, &g, &info, painter);
                    }),
                ),
            };
            ui.painter().add(egui::PaintCallback {
                rect: area,
                callback: Arc::new(cb),
            });
        }
        _ => {}
    }
    true
}

// ------------------------------------------------------------------ GL

struct Program {
    prog: Option<glow::Program>,
    xf: Option<glow::UniformLocation>,
    uniforms: HashMap<String, Option<glow::UniformLocation>>,
}

struct GlTex {
    tex: glow::Texture,
    w: i32,
    h: i32,
    /// Version of the pixels uploaded last.
    version: u64,
}

#[derive(Default)]
struct GlCache {
    programs: HashMap<u64, Program>,
    /// GL buffers with the version and bytes last uploaded to them (draws
    /// are checked against these bytes).
    buffers: HashMap<u64, (u64, glow::Buffer, Arc<Vec<u8>>)>,
    textures: HashMap<u64, GlTex>,
    /// Framebuffers by the texture they draw into.
    fbos: HashMap<u64, glow::Framebuffer>,
    /// The newest frame of each canvas this context ran.
    seen: HashMap<u64, u64>,
    vao: Option<glow::VertexArray>,
    uses: u32,
    /// Offscreen pictures of canvases drawn as quads ([`replay_quad`]).
    screens: HashMap<u64, Screen>,
    /// The program and buffer laying such a picture on the screen.
    blit: Option<Blit>,
}

/// A canvas's own picture: a texture and the framebuffer drawing into it.
struct Screen {
    tex: glow::Texture,
    fb: glow::Framebuffer,
    w: i32,
    h: i32,
}

struct Blit {
    prog: glow::Program,
    alpha: Option<glow::UniformLocation>,
    buf: glow::Buffer,
}

const BLIT_VS: &str = "in vec4 a_pos;\nin vec2 a_uv;\nout vec2 v_uv;\nvoid main() {\n  v_uv = a_uv;\n  gl_Position = a_pos;\n}\n";
const BLIT_FS: &str = "precision mediump float;\nuniform sampler2D u_tex;\nuniform float u_alpha;\nin vec2 v_uv;\nout vec4 o_color;\nvoid main() {\n  o_color = texture(u_tex, v_uv) * u_alpha;\n}\n";

thread_local! {
    /// Per GL context (the compact window has its own).
    static GL: RefCell<HashMap<usize, GlCache>> = RefCell::default();
}

/// `src` for this context: `#version 300 es` (or WebGL1's `100`) becomes
/// what the driver takes; fragment shaders get `gl_FragCoord` in the
/// canvas's pixel space through `_tf_xf` (origin xy, scale zw).
fn translate(src: &str, fragment: bool, gl: &glow::Context) -> String {
    let v = gl.version();
    let mut lines: Vec<String> = src.lines().map(str::to_string).collect();
    let at = lines
        .iter()
        .position(|l| l.trim_start().starts_with("#version"));
    let webgl1 = at.is_none_or(|i| !lines[i].contains("300"));
    let header = if v.is_embedded {
        if webgl1 {
            "#version 100"
        } else {
            "#version 300 es"
        }
    } else if (v.major, v.minor) >= (3, 3) {
        "#version 330"
    } else {
        "#version 140"
    };
    match at {
        Some(i) => lines[i] = header.to_string(),
        None => lines.insert(0, header.to_string()),
    }
    let at = lines
        .iter()
        .position(|l| l.trim_start().starts_with("#version"))
        .unwrap_or(0);
    let mut body = lines[at + 1..].join("\n");
    let mut extra = String::new();
    if webgl1 && !v.is_embedded {
        // GLSL ES 1.00 to desktop GLSL 1.40+.
        let word = |s: &str, from: &str, to: &str| {
            let mut out = String::new();
            let mut rest = s;
            while let Some(i) = rest.find(from) {
                let before_ok = rest[..i]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                let after_ok = rest[i + from.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                out.push_str(&rest[..i]);
                out.push_str(if before_ok && after_ok { to } else { from });
                rest = &rest[i + from.len()..];
            }
            out.push_str(rest);
            out
        };
        body = word(&body, "attribute", "in");
        body = word(&body, "varying", if fragment { "in" } else { "out" });
        body = word(&body, "texture2D", "texture");
        if fragment && body.contains("gl_FragColor") {
            body = word(&body, "gl_FragColor", "_tf_color");
            extra.push_str("out vec4 _tf_color;\n");
        }
    }
    if fragment {
        extra.push_str("uniform highp vec4 _tf_xf;\n");
        body = body.replace(
            "gl_FragCoord",
            "vec4((gl_FragCoord.xy - _tf_xf.xy) * _tf_xf.zw, gl_FragCoord.zw)",
        );
    }
    // Precision statements must come before declarations that need them
    // on GLSL ES; `_tf_xf` carries its own.
    format!("{}\n{extra}{body}\n", lines[..=at].join("\n"))
}

unsafe fn compile(gl: &glow::Context, p: &lc::GlProgram) -> Result<glow::Program, String> {
    unsafe {
        let prog = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src, frag) in [
            (glow::VERTEX_SHADER, &p.vs, false),
            (glow::FRAGMENT_SHADER, &p.fs, true),
        ] {
            let s = gl.create_shader(kind)?;
            gl.shader_source(s, &translate(src, frag, gl));
            gl.compile_shader(s);
            if !gl.get_shader_compile_status(s) {
                let log = gl.get_shader_info_log(s);
                gl.delete_shader(s);
                gl.delete_program(prog);
                return Err(log);
            }
            gl.attach_shader(prog, s);
            shaders.push(s);
        }
        for (i, name) in p.attribs.iter().enumerate() {
            if !name.is_empty() {
                gl.bind_attrib_location(prog, i as u32, name);
            }
        }
        gl.link_program(prog);
        for s in shaders {
            gl.detach_shader(prog, s);
            gl.delete_shader(s);
        }
        if !gl.get_program_link_status(prog) {
            let log = gl.get_program_info_log(prog);
            gl.delete_program(prog);
            return Err(log);
        }
        Ok(prog)
    }
}

unsafe fn set_uniform(gl: &glow::Context, loc: &glow::UniformLocation, kind: &str, v: &[f32]) {
    let l = Some(loc);
    let f = |i: usize| v.get(i).copied().unwrap_or(0.0);
    let ints: Vec<i32> = v.iter().map(|x| *x as i32).collect();
    let uints: Vec<u32> = v.iter().map(|x| *x as u32).collect();
    unsafe {
        match kind {
            "1f" => gl.uniform_1_f32(l, f(0)),
            "2f" => gl.uniform_2_f32(l, f(0), f(1)),
            "3f" => gl.uniform_3_f32(l, f(0), f(1), f(2)),
            "4f" => gl.uniform_4_f32(l, f(0), f(1), f(2), f(3)),
            "1i" => gl.uniform_1_i32(l, f(0) as i32),
            "2i" => gl.uniform_2_i32(l, f(0) as i32, f(1) as i32),
            "3i" => gl.uniform_3_i32(l, f(0) as i32, f(1) as i32, f(2) as i32),
            "4i" => gl.uniform_4_i32(l, f(0) as i32, f(1) as i32, f(2) as i32, f(3) as i32),
            "1ui" => gl.uniform_1_u32(l, f(0) as u32),
            "1fv" => gl.uniform_1_f32_slice(l, v),
            "2fv" => gl.uniform_2_f32_slice(l, v),
            "3fv" => gl.uniform_3_f32_slice(l, v),
            "4fv" => gl.uniform_4_f32_slice(l, v),
            "1iv" => gl.uniform_1_i32_slice(l, &ints),
            "2iv" => gl.uniform_2_i32_slice(l, &ints),
            "3iv" => gl.uniform_3_i32_slice(l, &ints),
            "4iv" => gl.uniform_4_i32_slice(l, &ints),
            "1uiv" => gl.uniform_1_u32_slice(l, &uints),
            "m2" => gl.uniform_matrix_2_f32_slice(l, false, v),
            "m3" => gl.uniform_matrix_3_f32_slice(l, false, v),
            "m4" => gl.uniform_matrix_4_f32_slice(l, false, v),
            _ => {}
        }
    }
}

fn replay(id: u64, frame: &GlFrame, info: &egui::PaintCallbackInfo, painter: &egui_glow::Painter) {
    let gl = painter.gl();
    let ctx_key = Arc::as_ptr(gl) as usize;
    let vp = info.viewport_in_pixels();
    let clip = info.clip_rect_in_pixels();
    if vp.width_px <= 0 || vp.height_px <= 0 {
        return;
    }
    let xf = [
        vp.left_px as f32,
        vp.from_bottom_px as f32,
        frame.w as f32 / vp.width_px as f32,
        frame.h as f32 / vp.height_px as f32,
    ];
    GL.with(|all| {
        let mut all = all.borrow_mut();
        let cache = all.entry(ctx_key).or_default();
        // SAFETY: called by egui_glow while its context is current.
        unsafe {
            if !cache.prepare(gl) {
                return;
            }
            let seen = cache.seen.get(&id).copied().unwrap_or(0);
            let mut r = Replay {
                gl,
                xf,
                vp: &vp,
                clip: &clip,
                outer: gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING),
                screen: None,
            };
            // Offscreen work of frames this context has not run yet, then
            // this frame (only its canvas draws when it ran already: a
            // repaint, or the canvas drawn twice).
            for p in frame.prior.iter().filter(|p| p.version > seen) {
                r.run(cache, &p.cmds, false, true);
            }
            r.run(cache, &frame.cmds, true, frame.version > seen);
            cache.seen.insert(id, frame.version);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    });
}

/// Replays the canvas into a picture of its own, then lays that on the
/// screen at `corners` (points, each with its perspective divisor) at
/// `opacity`: the now-playing cover's tilt leans it like the cover.
fn replay_quad(
    id: u64,
    frame: &GlFrame,
    info: &egui::PaintCallbackInfo,
    painter: &egui_glow::Painter,
    corners: [(egui::Pos2, f32); 4],
    opacity: f32,
) {
    let gl = painter.gl();
    let ctx_key = Arc::as_ptr(gl) as usize;
    let vp = info.viewport_in_pixels();
    let clip = info.clip_rect_in_pixels();
    if vp.width_px <= 0 || vp.height_px <= 0 {
        return;
    }
    let (w, h) = (frame.w as i32, frame.h as i32);
    GL.with(|all| {
        let mut all = all.borrow_mut();
        let cache = all.entry(ctx_key).or_default();
        // SAFETY: called by egui_glow while its context is current.
        unsafe {
            if !cache.prepare(gl) {
                return;
            }
            let outer = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let Some((fb, tex)) = cache.screen(gl, id, w, h) else {
                gl.bind_framebuffer(glow::FRAMEBUFFER, outer);
                return;
            };
            let seen = cache.seen.get(&id).copied().unwrap_or(0);
            let mut r = Replay {
                gl,
                xf: [0.0, 0.0, 1.0, 1.0],
                vp: &vp,
                clip: &clip,
                outer,
                screen: Some((fb, w, h)),
            };
            r.target(cache, None, None);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            for p in frame.prior.iter().filter(|p| p.version > seen) {
                r.run(cache, &p.cmds, false, true);
            }
            r.run(cache, &frame.cmds, true, frame.version > seen);
            cache.seen.insert(id, frame.version);
            // Back to egui's framebuffer, viewport and clip.
            r.screen = None;
            r.target(cache, None, None);
            cache.blit(gl, tex, info.viewport, corners, opacity);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    });
}

impl GlCache {
    /// Canvas `id`'s own picture at `w`x`h`: its framebuffer and texture.
    unsafe fn screen(
        &mut self,
        gl: &glow::Context,
        id: u64,
        w: i32,
        h: i32,
    ) -> Option<(glow::Framebuffer, glow::Texture)> {
        if let Some(s) = self.screens.get(&id)
            && s.w == w
            && s.h == h
        {
            return Some((s.fb, s.tex));
        }
        unsafe {
            if let Some(old) = self.screens.remove(&id) {
                gl.delete_framebuffer(old.fb);
                gl.delete_texture(old.tex);
            }
            let tex = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            for (param, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, param, value as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            let Ok(fb) = gl.create_framebuffer() else {
                gl.delete_texture(tex);
                return None;
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            self.screens.insert(id, Screen { tex, fb, w, h });
            Some((fb, tex))
        }
    }

    /// Draws texture `tex` as the quad `corners` (premultiplied, faded to
    /// `opacity`) into the callback's `viewport` (points).
    unsafe fn blit(
        &mut self,
        gl: &glow::Context,
        tex: glow::Texture,
        viewport: Rect,
        corners: [(egui::Pos2, f32); 4],
        opacity: f32,
    ) {
        unsafe {
            if self.blit.is_none() {
                let src = lc::GlProgram {
                    vs: format!("#version 300 es\n{BLIT_VS}"),
                    fs: format!("#version 300 es\n{BLIT_FS}"),
                    attribs: vec!["a_pos".into(), "a_uv".into()],
                };
                let (Ok(prog), Ok(buf)) = (compile(gl, &src), gl.create_buffer()) else {
                    log::warn!("Luna canvas quad program failed");
                    return;
                };
                let alpha = gl.get_uniform_location(prog, "u_alpha");
                self.blit = Some(Blit { prog, alpha, buf });
            }
            let Some(b) = &self.blit else { return };
            let mut v: Vec<f32> = Vec::with_capacity(24);
            // A strip: left-top, right-top, left-bottom, right-bottom; the
            // picture's first row is the canvas's bottom.
            for (i, [u, t]) in [
                (0, [0.0, 1.0]),
                (1, [1.0, 1.0]),
                (3, [0.0, 0.0]),
                (2, [1.0, 0.0]),
            ] {
                let (p, w) = corners[i];
                let x = (p.x - viewport.min.x) / viewport.width().max(1e-3) * 2.0 - 1.0;
                let y = 1.0 - (p.y - viewport.min.y) / viewport.height().max(1e-3) * 2.0;
                v.extend([x * w, y * w, 0.0, w, u, t]);
            }
            let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_ne_bytes()).collect();
            gl.use_program(Some(b.prog));
            gl.uniform_1_f32(b.alpha.as_ref(), opacity.clamp(0.0, 1.0));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.buf));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STREAM_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 4, glow::FLOAT, false, 24, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 24, 16);
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            gl.disable_vertex_attrib_array(0);
            gl.disable_vertex_attrib_array(1);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    /// Creates the vertex array and, now and then, drops GL objects whose
    /// canvases, buffers or textures went away. `false` when GL failed.
    unsafe fn prepare(&mut self, gl: &glow::Context) -> bool {
        unsafe {
            if self.vao.is_none() {
                match gl.create_vertex_array() {
                    Ok(v) => self.vao = Some(v),
                    Err(_) => return false,
                }
            }
            self.uses = self.uses.wrapping_add(1);
            if self.uses.is_multiple_of(600) {
                self.buffers.retain(|k, (_, b, _)| {
                    let keep = lc::gl_buffer(*k).is_some();
                    if !keep {
                        gl.delete_buffer(*b);
                    }
                    keep
                });
                let fbos = &mut self.fbos;
                self.textures.retain(|k, t| {
                    let keep = lc::gl_texture(*k, |d| d.is_some());
                    if !keep {
                        gl.delete_texture(t.tex);
                        if let Some(f) = fbos.remove(k) {
                            gl.delete_framebuffer(f);
                        }
                    }
                    keep
                });
                self.seen.retain(|id, _| lc::frame(*id).is_some());
                self.screens.retain(|id, s| {
                    let keep = lc::frame(*id).is_some();
                    if !keep {
                        gl.delete_framebuffer(s.fb);
                        gl.delete_texture(s.tex);
                    }
                    keep
                });
            }
            gl.bind_vertex_array(self.vao);
            true
        }
    }

    /// GL buffer for `key` holding its latest contents, bound to `target`.
    unsafe fn buffer(&mut self, gl: &glow::Context, key: u64, target: u32) -> Option<glow::Buffer> {
        let (version, bytes) = lc::gl_buffer(key)?;
        unsafe {
            let buf = match self.buffers.get_mut(&key) {
                Some((v, b, held)) => {
                    gl.bind_buffer(target, Some(*b));
                    if *v != version {
                        gl.buffer_data_u8_slice(target, &bytes, glow::DYNAMIC_DRAW);
                        *v = version;
                        *held = bytes;
                    }
                    *b
                }
                None => {
                    let b = gl.create_buffer().ok()?;
                    gl.bind_buffer(target, Some(b));
                    gl.buffer_data_u8_slice(target, &bytes, glow::STATIC_DRAW);
                    self.buffers.insert(key, (version, b, bytes));
                    b
                }
            };
            Some(buf)
        }
    }

    /// Texture `key` at `w`x`h`, its pixels uploaded up to `version`.
    unsafe fn texture(&mut self, gl: &glow::Context, key: u64, w: i32, h: i32, version: u64) {
        if w <= 0 || h <= 0 {
            return;
        }
        unsafe {
            let fresh = match self.textures.get(&key) {
                Some(t) if t.w == w && t.h == h => {
                    if t.version >= version {
                        return;
                    }
                    false
                }
                _ => true,
            };
            let tex = match self.textures.get(&key) {
                Some(t) => t.tex,
                None => match gl.create_texture() {
                    Ok(t) => t,
                    Err(_) => return,
                },
            };
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            let uploaded = lc::gl_texture(key, |d| {
                let d = d?;
                let px = d
                    .rgba
                    .as_ref()
                    .filter(|_| d.w as i32 == w && d.h as i32 == h)?;
                if fresh {
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        w,
                        h,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(px)),
                    );
                } else {
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        w,
                        h,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(px)),
                    );
                }
                Some(d.version)
            });
            if uploaded.is_none() && fresh {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    w,
                    h,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            if fresh && let Some(f) = self.fbos.remove(&key) {
                gl.delete_framebuffer(f);
            }
            self.textures.insert(
                key,
                GlTex {
                    tex,
                    w,
                    h,
                    version: uploaded.unwrap_or(version),
                },
            );
        }
    }

    /// The framebuffer drawing into texture `key`, bound.
    unsafe fn framebuffer(&mut self, gl: &glow::Context, key: u64) -> bool {
        let Some(t) = self.textures.get(&key) else {
            return false;
        };
        let tex = t.tex;
        unsafe {
            let fb = match self.fbos.get(&key) {
                Some(f) => *f,
                None => {
                    let Ok(f) = gl.create_framebuffer() else {
                        return false;
                    };
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
                    gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        glow::COLOR_ATTACHMENT0,
                        glow::TEXTURE_2D,
                        Some(tex),
                        0,
                    );
                    self.fbos.insert(key, f);
                    f
                }
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            true
        }
    }
}

struct Replay<'a> {
    gl: &'a glow::Context,
    xf: [f32; 4],
    vp: &'a egui::epaint::ViewportInPixels,
    clip: &'a egui::epaint::ViewportInPixels,
    /// egui's framebuffer, bound again after offscreen draws.
    outer: Option<glow::Framebuffer>,
    /// The canvas's own picture standing in for egui's framebuffer
    /// ([`replay_quad`]): framebuffer, width, height.
    screen: Option<(glow::Framebuffer, i32, i32)>,
}

impl Replay<'_> {
    /// Binds texture `fb`'s framebuffer (with `vp` as the viewport) or, for
    /// `None`, egui's with the canvas's rectangle. `false` when the
    /// framebuffer cannot be made.
    unsafe fn target(&self, cache: &mut GlCache, fb: Option<u64>, vp: Option<[i32; 4]>) -> bool {
        let gl = self.gl;
        unsafe {
            match (fb, self.screen) {
                (Some(key), _) => {
                    if !cache.framebuffer(gl, key) {
                        gl.bind_framebuffer(glow::FRAMEBUFFER, self.outer);
                        return false;
                    }
                    gl.disable(glow::SCISSOR_TEST);
                    let [x, y, w, h] = vp.unwrap_or_else(|| {
                        cache
                            .textures
                            .get(&key)
                            .map_or([0, 0, 1, 1], |t| [0, 0, t.w, t.h])
                    });
                    gl.viewport(x, y, w, h);
                    true
                }
                (None, Some((screen, w, h))) => {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(screen));
                    gl.disable(glow::SCISSOR_TEST);
                    gl.viewport(0, 0, w, h);
                    true
                }
                (None, None) => {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, self.outer);
                    gl.viewport(
                        self.vp.left_px,
                        self.vp.from_bottom_px,
                        self.vp.width_px,
                        self.vp.height_px,
                    );
                    gl.enable(glow::SCISSOR_TEST);
                    gl.scissor(
                        self.clip.left_px,
                        self.clip.from_bottom_px,
                        self.clip.width_px,
                        self.clip.height_px,
                    );
                    true
                }
            }
        }
    }

    /// Runs `cmds`: what draws on the canvas when `screen`, uploads and
    /// framebuffer work when `offscreen`.
    unsafe fn run(&mut self, cache: &mut GlCache, cmds: &[GlCmd], screen: bool, offscreen: bool) {
        let gl = self.gl;
        let (vp, clip) = (self.vp, self.clip);
        unsafe {
            for cmd in cmds {
                if cmd.target().is_some() && !offscreen {
                    continue;
                }
                match cmd {
                    GlCmd::Tex { tex, w, h, version } => {
                        cache.texture(gl, *tex, *w, *h, *version);
                    }
                    GlCmd::Mip { tex } => {
                        if let Some(t) = cache.textures.get(tex) {
                            gl.active_texture(glow::TEXTURE0);
                            gl.bind_texture(glow::TEXTURE_2D, Some(t.tex));
                            gl.generate_mipmap(glow::TEXTURE_2D);
                            gl.bind_texture(glow::TEXTURE_2D, None);
                        }
                    }
                    GlCmd::Clear {
                        color,
                        fb: Some(key),
                    } => {
                        if self.target(cache, Some(*key), None) {
                            gl.clear_color(color[0], color[1], color[2], color[3]);
                            gl.clear(glow::COLOR_BUFFER_BIT);
                        }
                        self.target(cache, None, None);
                    }
                    GlCmd::Clear { color, fb: None } => {
                        // A transparent clear leaves what is behind the canvas.
                        if !screen || color[3] <= 0.0 {
                            continue;
                        }
                        if self.screen.is_some() {
                            let a = color[3].clamp(0.0, 1.0);
                            gl.clear_color(color[0] * a, color[1] * a, color[2] * a, a);
                            gl.clear(glow::COLOR_BUFFER_BIT);
                            continue;
                        }
                        let x0 = vp.left_px.max(clip.left_px);
                        let y0 = vp.from_bottom_px.max(clip.from_bottom_px);
                        let x1 = (vp.left_px + vp.width_px).min(clip.left_px + clip.width_px);
                        let y1 = (vp.from_bottom_px + vp.height_px)
                            .min(clip.from_bottom_px + clip.height_px);
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }
                        gl.scissor(x0, y0, x1 - x0, y1 - y0);
                        let a = color[3].clamp(0.0, 1.0);
                        gl.clear_color(color[0] * a, color[1] * a, color[2] * a, a);
                        gl.clear(glow::COLOR_BUFFER_BIT);
                        gl.scissor(
                            clip.left_px,
                            clip.from_bottom_px,
                            clip.width_px,
                            clip.height_px,
                        );
                    }
                    GlCmd::Draw(d) => {
                        if !screen && d.fb.is_none() {
                            continue;
                        }
                        self.draw(cache, d);
                    }
                }
            }
        }
    }

    unsafe fn draw(&mut self, cache: &mut GlCache, d: &lc::GlDraw) {
        let gl = self.gl;
        unsafe {
            if let std::collections::hash_map::Entry::Vacant(slot) = cache.programs.entry(d.prog) {
                let Some(src) = lc::gl_program(d.prog) else {
                    return;
                };
                let prog = match compile(gl, &src) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        log::warn!("Luna WebGL program failed: {e}");
                        None
                    }
                };
                let xf_loc = prog.and_then(|p| gl.get_uniform_location(p, "_tf_xf"));
                slot.insert(Program {
                    prog,
                    xf: xf_loc,
                    uniforms: HashMap::new(),
                });
            }
            let Some(prog) = cache.programs.get(&d.prog).and_then(|p| p.prog) else {
                return;
            };
            if d.fb.is_some() && !self.target(cache, d.fb, d.vp) {
                return;
            }
            let Some(p) = cache.programs.get_mut(&d.prog) else {
                return;
            };
            gl.use_program(Some(prog));
            if let Some(l) = &p.xf {
                // A framebuffer's gl_FragCoord is already in its own pixels.
                let xf = if d.fb.is_some() || self.screen.is_some() {
                    [0.0, 0.0, 1.0, 1.0]
                } else {
                    self.xf
                };
                gl.uniform_4_f32(Some(l), xf[0], xf[1], xf[2], xf[3]);
            }
            for (name, kind, v) in &d.uniforms {
                let loc = p
                    .uniforms
                    .entry(name.clone())
                    .or_insert_with(|| gl.get_uniform_location(prog, name));
                if let Some(loc) = loc {
                    set_uniform(gl, loc, kind, v);
                }
            }
            // Upload what the draw reads, then check it against those very
            // bytes, as WebGL does: desktop GL would read past a buffer.
            for a in &d.attribs {
                cache.buffer(gl, a.buffer, glow::ARRAY_BUFFER);
            }
            if let Some([buffer, ..]) = d.elements {
                cache.buffer(gl, buffer, glow::ELEMENT_ARRAY_BUFFER);
            }
            let held = |k: u64| cache.buffers.get(&k).map(|(_, _, b)| b.clone());
            if lc::draw_error(d, &held).is_some() {
                if d.fb.is_some() {
                    self.target(cache, None, None);
                }
                return;
            }
            // The buffers as checked (not uploaded again: the plugin may have
            // replaced one since).
            let checked = |k: u64| cache.buffers.get(&k).map(|(_, b, _)| *b);
            let mut enabled = Vec::new();
            for a in &d.attribs {
                let Some(b) = checked(a.buffer) else { continue };
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.enable_vertex_attrib_array(a.loc);
                gl.vertex_attrib_pointer_f32(a.loc, a.size, glow::FLOAT, false, a.stride, a.offset);
                enabled.push(a.loc);
            }
            let mut units = Vec::new();
            for b in &d.tex {
                let Some(t) = cache.textures.get(&b.tex) else {
                    continue;
                };
                gl.active_texture(glow::TEXTURE0 + b.unit);
                gl.bind_texture(glow::TEXTURE_2D, Some(t.tex));
                for (param, value) in [
                    glow::TEXTURE_MIN_FILTER,
                    glow::TEXTURE_MAG_FILTER,
                    glow::TEXTURE_WRAP_S,
                    glow::TEXTURE_WRAP_T,
                ]
                .into_iter()
                .zip(b.params)
                {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, param, value as i32);
                }
                units.push(b.unit);
            }
            match (d.blend, d.fb) {
                (Some([s, dst, sa, da]), _) => {
                    gl.enable(glow::BLEND);
                    gl.blend_equation(glow::FUNC_ADD);
                    gl.blend_func_separate(s, dst, sa, da);
                }
                (None, Some(_)) => gl.disable(glow::BLEND),
                (None, None) => {
                    // An unblended canvas pixel replaces what is under it in
                    // the canvas, which starts transparent: over the page
                    // that is blending premultiplied colour.
                    gl.enable(glow::BLEND);
                    gl.blend_func_separate(
                        glow::ONE,
                        glow::ONE_MINUS_SRC_ALPHA,
                        glow::ONE,
                        glow::ONE_MINUS_SRC_ALPHA,
                    );
                }
            }
            if d.count > 0 {
                match d.elements {
                    Some([buffer, ty, offset]) => {
                        if let Some(b) = checked(buffer) {
                            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(b));
                            gl.draw_elements(d.mode, d.count, ty as u32, offset as i32);
                        }
                    }
                    None => gl.draw_arrays(d.mode, d.first, d.count),
                }
            }
            for l in enabled {
                gl.disable_vertex_attrib_array(l);
            }
            for u in units {
                gl.active_texture(glow::TEXTURE0 + u);
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
            gl.active_texture(glow::TEXTURE0);
            if d.fb.is_some() {
                self.target(cache, None, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_nodes_carry_their_key() {
        let node = VNode {
            markup: Some("canvas:42".into()),
            ..Default::default()
        };
        assert_eq!(key(&node), Some(42));
        assert!(has_frame(&node));
        let plain = VNode::default();
        assert!(!has_frame(&plain));
    }

    /// A canvas the runtime allows (its limit is on area, 16 million
    /// pixels) can be wider than the GPU's largest texture: it is drawn
    /// from a texture scaled to fit, as the glow painter asserts on a
    /// larger one (and release builds abort on a panic).
    #[test]
    fn a_canvas_wider_than_the_gpu_takes_is_scaled_to_fit() {
        let id = 9_000_001;
        let (w, h) = (3000u32, 1u32);
        lc::test_frame(
            id,
            Some(Frame::Pixels(Arc::new(lc::Pixels {
                w,
                h,
                version: 1,
                rgba: vec![255; (w * h * 4) as usize],
            }))),
        );
        let node = VNode {
            markup: Some(format!("canvas:{id}")),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let most = ctx.input(|i| i.max_texture_side);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &node, Some(300.0), Some(10.0));
        });
        lc::test_frame(id, None);
        let sizes: Vec<[usize; 2]> = output
            .textures_delta
            .set
            .values()
            .flat_map(|d| d.iter().map(|d| d.image.size()))
            .collect();
        output.textures_delta.clear();
        assert!(
            sizes.iter().any(|s| s[1] == 1),
            "no canvas texture: {sizes:?}"
        );
        for s in &sizes {
            assert!(s[0] <= most && s[1] <= most, "{s:?} is over {most}");
        }
    }
}
