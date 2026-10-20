//! TIDAL's tilt on the now-playing cover: under the pointer the cover
//! leans away from it in perspective (`perspective: 1000px`, up to 5° about
//! each axis), with a soft glare opposite the pointer, and springs back flat
//! when the pointer leaves (stiffness 200, damping 30).
//!
//! The amount is measured from TIDAL 2.43's own cover (the shots in
//! `.internal/luna-reference/visiuun-3d-cover`): the cover's corners there
//! fit `rotateX`/`rotateY` within a pixel at 4.5° with the pointer near the
//! right edge and -3.8°/3.7° near the bottom right, and the 3d-cover plugin,
//! which reads TIDAL's matrix, expects about 4°.
//!
//! The tilt works on whatever is painted while its guard lives: the still
//! cover, its stand-in, or an animated cover's current frame. Textured
//! rectangles become subdivided meshes whose corners are projected, so any
//! cover source tilts without knowing about it.

use egui::epaint::{RectShape, Vertex};
use egui::{Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use super::player_background::stops;

/// `perspective: 1000px` on the cover's parent.
const PERSPECTIVE: f32 = 1000.0;
/// The largest lean, in degrees, about either axis: reached where the edge
/// band starts, and never passed.
const MAX_TILT: f32 = 5.0;
/// Within this fraction of an edge the lean fades out.
const EDGE: f32 = 0.15;
const STIFFNESS: f32 = 200.0;
const DAMPING: f32 = 30.0;
/// The grid a tilted rectangle is cut into.
const GRID: usize = 8;
/// The glare's grid.
const GLARE_GRID: usize = 16;
/// `radial-gradient(circle at …, rgba(255,255,255,.15) 0%, .12 10%, .08
/// 20%, .04 35%, .02 50%, .005 65%, transparent 80%)`.
const GLARE: [(f32, f32); 7] = [
    (0.0, 0.15),
    (0.1, 0.12),
    (0.2, 0.08),
    (0.35, 0.04),
    (0.5, 0.02),
    (0.65, 0.005),
    (0.8, 0.0),
];

/// One spring: where it is and how fast it moves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    /// Moves towards `target` for `dt` seconds.
    fn step(&mut self, target: f32, dt: f32) {
        const STEP: f32 = 1.0 / 240.0;
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(STEP);
            let force = -STIFFNESS * (self.value - target) - DAMPING * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if (self.value - target).abs() < 1e-3 && self.velocity.abs() < 1e-2 {
            *self = Self {
                value: target,
                velocity: 0.0,
            };
        }
    }

    fn settled(&self, target: f32) -> bool {
        self.value == target && self.velocity == 0.0
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    /// Degrees about the horizontal and vertical axes.
    rotate_x: Spring,
    rotate_y: Spring,
    glare: Spring,
    /// Where the pointer was over the cover, 0 to 1.
    pointer: Option<Pos2>,
    time: Option<f64>,
}

/// The lean and glare for a pointer at `(x, y)` over the cover (0 to 1),
/// as TIDAL works them out: degrees about x and y, and the glare's
/// opacity. The lean grows from the centre to [`MAX_TILT`] where the edge
/// band begins, and fades out across the band.
pub(crate) fn targets(x: f32, y: f32) -> (f32, f32, f32) {
    let edge = x.min(1.0 - x).min(y.min(1.0 - y)).max(0.0);
    let f = (edge / EDGE).min(1.0);
    let reach = 0.5 - EDGE;
    let lean = |d: f32| (d / reach).clamp(-1.0, 1.0) * MAX_TILT * f;
    (lean(0.5 - y), lean(x - 0.5), f)
}

/// Projects points of `rect` turned by `rotate_x` then `rotate_y` degrees
/// (CSS `rotateX(a) rotateY(b)`) about its centre, seen from
/// [`PERSPECTIVE`] away.
#[derive(Clone, Copy)]
struct Projection {
    center: Pos2,
    sin_x: f32,
    cos_x: f32,
    sin_y: f32,
    cos_y: f32,
}

impl Projection {
    fn new(rect: Rect, rotate_x: f32, rotate_y: f32) -> Self {
        let (sin_x, cos_x) = rotate_x.to_radians().sin_cos();
        let (sin_y, cos_y) = rotate_y.to_radians().sin_cos();
        Self {
            center: rect.center(),
            sin_x,
            cos_x,
            sin_y,
            cos_y,
        }
    }

    fn apply(&self, point: Pos2) -> Pos2 {
        self.apply_w(point).0
    }

    /// The projected point and its perspective divisor (`1 / scale`).
    fn apply_w(&self, point: Pos2) -> (Pos2, f32) {
        let local = point - self.center;
        // rotateY first, then rotateX (CSS applies the rightmost first).
        let x = local.x * self.cos_y;
        let z = -local.x * self.sin_y;
        let y = local.y * self.cos_x - z * self.sin_x;
        let z = local.y * self.sin_x + z * self.cos_x;
        let scale = PERSPECTIVE / (PERSPECTIVE - z).max(1.0);
        (self.center + vec2(x * scale, y * scale), 1.0 / scale)
    }
}

thread_local! {
    /// The tilt being painted (its rect and lean), while a guard lives.
    static ACTIVE: std::cell::Cell<Option<(Rect, f32, f32)>> = const { std::cell::Cell::new(None) };
}

/// While the cover is being painted inside its tilt: where the corners of
/// `rect` (left-top, right-top, right-bottom, left-bottom) land, each with
/// its perspective divisor (for a correct texture mapping of something the
/// tilt cannot warp itself, a plugin's WebGL canvas).
pub fn projected(rect: Rect) -> Option<[(Pos2, f32); 4]> {
    let (cover, rotate_x, rotate_y) = ACTIVE.with(std::cell::Cell::get)?;
    let p = Projection::new(cover, rotate_x, rotate_y);
    Some(
        [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ]
        .map(|point| p.apply_w(point)),
    )
}

/// The lean the cover had when it was last drawn, degrees about x and y
/// (for Luna's `_tilt_` element).
pub fn current(ctx: &egui::Context) -> [f32; 2] {
    let id = egui::Id::new("now-playing-cover-tilt");
    ctx.data(|data| data.get_temp::<State>(id))
        .map_or([0.0, 0.0], |s| [s.rotate_x.value, s.rotate_y.value])
}

/// Tilts what is painted between [`begin`] and the guard's end.
pub struct Tilt {
    painter: egui::Painter,
    start: egui::layers::ShapeIdx,
    rect: Rect,
    state: State,
}

/// Starts tilting the cover at `rect`: everything painted on `ui`'s layer
/// until the returned guard is dropped leans with it.
pub fn begin(ui: &egui::Ui, rect: Rect) -> Tilt {
    let ctx = ui.ctx();
    let id = egui::Id::new("now-playing-cover-tilt");
    let response = ui.interact(rect, id, Sense::hover());
    let mut state = ctx.data(|data| data.get_temp::<State>(id).unwrap_or_default());
    let now = ctx.input(|input| input.time);
    let dt = state
        .time
        .map_or(0.0, |time| (now - time).clamp(0.0, 0.05) as f32);
    state.time = Some(now);
    let reduced = crate::motion::reduced(ctx);
    let pointer = response
        .hover_pos()
        .filter(|_| response.hovered() && !reduced)
        .map(|pos| {
            let at = (pos - rect.min) / rect.size().max(vec2(1.0, 1.0));
            pos2(at.x.clamp(0.0, 1.0), at.y.clamp(0.0, 1.0))
        });
    let (target_x, target_y, target_glare) =
        pointer.map_or((0.0, 0.0, 0.0), |at| targets(at.x, at.y));
    if pointer.is_some() {
        state.pointer = pointer;
    }
    if reduced {
        state.rotate_x = Spring::default();
        state.rotate_y = Spring::default();
        state.glare = Spring::default();
    } else {
        state.rotate_x.step(target_x, dt);
        state.rotate_y.step(target_y, dt);
        state.glare.step(target_glare, dt);
    }
    let moving = !(state.rotate_x.settled(target_x)
        && state.rotate_y.settled(target_y)
        && state.glare.settled(target_glare));
    if moving {
        ctx.request_repaint();
    }
    ctx.data_mut(|data| data.insert_temp(id, state));
    let painter = ui.painter().clone();
    let start = ctx.graphics_mut(|graphics| graphics.entry(painter.layer_id()).next_idx());
    ACTIVE.with(|a| a.set(Some((rect, state.rotate_x.value, state.rotate_y.value))));
    Tilt {
        painter,
        start,
        rect,
        state,
    }
}

impl Drop for Tilt {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(None));
        let (rotate_x, rotate_y) = (self.state.rotate_x.value, self.state.rotate_y.value);
        let flat = rotate_x == 0.0 && rotate_y == 0.0;
        let projection = Projection::new(self.rect, rotate_x, rotate_y);
        if !flat {
            let layer = self.painter.layer_id();
            let start = self.start.0;
            self.painter.ctx().graphics_mut(|graphics| {
                let list = graphics.entry(layer);
                let end = list.next_idx().0;
                for index in start..end {
                    list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                        warp(&mut clipped.shape, &projection);
                    });
                }
            });
        }
        let glare = self.state.glare.value;
        if glare > 0.0
            && let Some(pointer) = self.state.pointer
        {
            self.painter
                .add(glare_mesh(self.rect, pointer, glare, &projection));
        }
        // A thin line inside the edge keeps a dark cover apart from a dark
        // background.
        let inset = self.rect.shrink(0.5);
        let edge = [
            inset.left_top(),
            inset.right_top(),
            inset.right_bottom(),
            inset.left_bottom(),
        ]
        .map(|point| projection.apply(point));
        self.painter.add(Shape::closed_line(
            edge.to_vec(),
            Stroke::new(1.0, Color32::from_white_alpha(18)),
        ));
    }
}

/// Puts `shape` through `projection`: rectangles become cut meshes, meshes
/// have their corners moved, and anything else is left as it is.
fn warp(shape: &mut Shape, projection: &Projection) {
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                warp(shape, projection);
            }
        }
        Shape::Mesh(mesh) => {
            for vertex in &mut std::sync::Arc::make_mut(mesh).vertices {
                vertex.pos = projection.apply(vertex.pos);
            }
        }
        Shape::Rect(rect) if rect.angle == 0.0 && rect.blur_width > 0.0 => {
            *shape = shadow_mesh(rect, projection);
        }
        Shape::Rect(rect) if rect.angle == 0.0 && rect.blur_width == 0.0 => {
            *shape = rect_mesh(rect, projection);
        }
        _ => {}
    }
}

/// A rectangle (textured or filled) cut into a grid and projected.
fn rect_mesh(rect: &RectShape, projection: &Projection) -> Shape {
    let (mut mesh, uv) = match &rect.brush {
        Some(brush) => (Mesh::with_texture(brush.fill_texture_id), Some(brush.uv)),
        None => (Mesh::default(), None),
    };
    let area = rect.rect;
    for row in 0..=GRID {
        for column in 0..=GRID {
            let (u, v) = (column as f32 / GRID as f32, row as f32 / GRID as f32);
            let pos = projection.apply(area.lerp_inside(vec2(u, v)));
            match uv {
                Some(uv) => mesh.vertices.push(Vertex {
                    pos,
                    uv: uv.lerp_inside(vec2(u, v)),
                    color: rect.fill,
                }),
                None => mesh.colored_vertex(pos, rect.fill),
            }
        }
    }
    grid_triangles(&mut mesh, GRID);
    let mut shapes = vec![Shape::mesh(mesh)];
    if !rect.stroke.is_empty() {
        let corners = [
            area.left_top(),
            area.right_top(),
            area.right_bottom(),
            area.left_bottom(),
        ]
        .map(|point| projection.apply(point));
        shapes.push(Shape::closed_line(corners.to_vec(), rect.stroke));
    }
    Shape::Vec(shapes)
}

/// A soft shadow (a blurred rectangle) following the tilted cover: the
/// rectangle's solid core and its faded rim are projected corner by corner,
/// so the shadow leans with the cover instead of staying flat behind it.
fn shadow_mesh(rect: &RectShape, projection: &Projection) -> Shape {
    let half = rect.blur_width / 2.0;
    let rings = [rect.rect.shrink(half), rect.rect.expand(half)];
    let mut mesh = Mesh::default();
    for (ring, area) in rings.iter().enumerate() {
        let color = if ring == 0 {
            rect.fill
        } else {
            Color32::TRANSPARENT
        };
        for corner in [
            area.left_top(),
            area.right_top(),
            area.right_bottom(),
            area.left_bottom(),
        ] {
            mesh.colored_vertex(projection.apply(corner), color);
        }
    }
    // The core, then the rim as four quads between the rings.
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    for side in 0..4u32 {
        let next = (side + 1) % 4;
        mesh.add_triangle(side, next, 4 + next);
        mesh.add_triangle(side, 4 + next, 4 + side);
    }
    Shape::mesh(mesh)
}

fn grid_triangles(mesh: &mut Mesh, grid: usize) {
    let width = (grid + 1) as u32;
    for row in 0..grid as u32 {
        for column in 0..grid as u32 {
            let a = row * width + column;
            let b = a + width;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }
}

/// The glare: a white radial gradient centred opposite the pointer,
/// reaching the farthest corner, at `opacity`.
fn glare_mesh(rect: Rect, pointer: Pos2, opacity: f32, projection: &Projection) -> Shape {
    let center = pos2(
        rect.left() + (1.0 - pointer.x) * rect.width(),
        rect.top() + (1.0 - pointer.y) * rect.height(),
    );
    let reach = [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ]
    .iter()
    .map(|corner| corner.distance(center))
    .fold(1.0_f32, f32::max);
    let mut mesh = Mesh::default();
    for row in 0..=GLARE_GRID {
        for column in 0..=GLARE_GRID {
            let point = rect.lerp_inside(vec2(
                column as f32 / GLARE_GRID as f32,
                row as f32 / GLARE_GRID as f32,
            ));
            let alpha = stops(point.distance(center) / reach, &GLARE) * opacity;
            mesh.colored_vertex(
                projection.apply(point),
                Color32::from_white_alpha((alpha * 255.0).round() as u8),
            );
        }
    }
    grid_triangles(&mut mesh, GLARE_GRID);
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin's WebGL canvas in the cover lands where the tilt puts the
    /// cover's own picture, with a divisor that maps it in perspective;
    /// outside a tilt there is no projection.
    #[test]
    fn a_canvas_in_the_tilt_is_projected_like_the_cover() {
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(400.0, 400.0));
        assert_eq!(projected(rect), None);
        ACTIVE.with(|a| a.set(Some((rect, 6.0, -9.0))));
        let corners = projected(rect).unwrap();
        ACTIVE.with(|a| a.set(None));
        let p = Projection::new(rect, 6.0, -9.0);
        assert_eq!(corners[0].0, p.apply(rect.left_top()));
        assert_eq!(corners[2].0, p.apply(rect.right_bottom()));
        // rotateY(-9deg) turns the left edge away (smaller: divisor > 1)
        // and the right edge nearer.
        assert!(corners[0].1 > 1.0 && corners[1].1 < 1.0, "{corners:?}");
        // Flat: the rect itself.
        ACTIVE.with(|a| a.set(Some((rect, 0.0, 0.0))));
        let flat = projected(rect).unwrap();
        ACTIVE.with(|a| a.set(None));
        assert_eq!(flat[0], (rect.left_top(), 1.0));
        assert_eq!(flat[2], (rect.right_bottom(), 1.0));
    }

    #[test]
    fn the_lean_fades_out_towards_the_edges() {
        assert_eq!(targets(0.5, 0.5), (0.0, 0.0, 1.0));
        // Near the top-right, short of the edge band: most of the lean.
        let (x, y, glare) = targets(0.8, 0.2);
        let share = 0.3 / 0.35 * MAX_TILT;
        assert!((x - share).abs() < 1e-4 && (y - share).abs() < 1e-4);
        assert_eq!(glare, 1.0);
        // On the very edge the lean is gone.
        assert_eq!(targets(1.0, 0.5).2, 0.0);
        let (_, y, glare) = targets(0.95, 0.5);
        assert!((glare - 1.0 / 3.0).abs() < 1e-4);
        assert!((y - MAX_TILT / 3.0).abs() < 1e-4);
    }

    /// TIDAL leans its cover about 5° at most (its own cover, measured in
    /// the 3d-cover reference shots); Tidalfast leaned it up to 12°.
    #[test]
    fn the_lean_never_passes_tidals_five_degrees() {
        let mut most = 0.0_f32;
        for i in 0..=100 {
            for j in 0..=100 {
                let (x, y, _) = targets(i as f32 / 100.0, j as f32 / 100.0);
                most = most.max(x.abs()).max(y.abs());
            }
        }
        assert!((most - 5.0).abs() < 1e-3, "{most}");
    }

    #[test]
    fn the_spring_settles_on_its_target() {
        let mut spring = Spring::default();
        spring.step(10.0, 0.05);
        assert!(spring.value > 0.0 && spring.value < 10.0);
        for _ in 0..40 {
            spring.step(10.0, 0.05);
        }
        assert!(spring.settled(10.0));
    }

    #[test]
    fn the_shadow_follows_the_tilted_cover() {
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(600.0, 600.0));
        let shadow = egui::epaint::Shadow {
            offset: [0, 16],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(77),
        }
        .as_shape(rect, 0.0);
        let projection = Projection::new(rect, 9.0, -12.0);
        let mut shape = Shape::Rect(shadow.clone());
        warp(&mut shape, &projection);
        let Shape::Mesh(mesh) = shape else {
            panic!("a tilted shadow is a mesh");
        };
        // The solid core's corners are the projected corners of the shadow.
        let core = shadow.rect.shrink(shadow.blur_width / 2.0);
        assert_eq!(mesh.vertices[0].pos, projection.apply(core.left_top()));
        assert_eq!(mesh.vertices[2].pos, projection.apply(core.right_bottom()));
        assert_ne!(mesh.vertices[2].pos, core.right_bottom());
        assert_eq!(mesh.vertices[4].color, Color32::TRANSPARENT);
        // Flat, the same projection leaves the corners where they were.
        let flat = Projection::new(rect, 0.0, 0.0);
        assert_eq!(flat.apply(core.right_bottom()), core.right_bottom());
    }

    #[test]
    fn a_lean_brings_the_far_side_in() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 600.0));
        // Leaning right (pointer on the right): the right edge goes back
        // and shrinks, the left comes forward and grows.
        let projection = Projection::new(rect, 0.0, 12.0);
        let right = projection.apply(rect.right_top()).y - projection.apply(rect.right_bottom()).y;
        let left = projection.apply(rect.left_top()).y - projection.apply(rect.left_bottom()).y;
        assert!(right.abs() < 600.0 && left.abs() > 600.0);
        // Flat projects onto itself.
        let flat = Projection::new(rect, 0.0, 0.0);
        assert_eq!(flat.apply(pos2(10.0, 20.0)), pos2(10.0, 20.0));
    }
}
