//! What the now-playing view shows behind the cover: TIDAL's "Player
//! background" setting (Settings → Display).
//!
//! - **Blur** (TIDAL's default): the cover, blurred and a little more
//!   saturated (`filter: blur(28px) saturate(1.3)`), as a square of
//!   160 vmax centred on the view, turning slowly while a song plays.
//!   TIDAL's rotator spins at 360° a minute; on pause it coasts on by 30°
//!   over 1.5 s (ease-out cubic) and stops; dragging the seek bar turns it
//!   by 180° per whole song and it coasts again on release. Playing again
//!   or opening the view starts the spin briefly faster, easing into the
//!   steady speed. The image grows from 1.18 to 1.26 while playing
//!   (`transform 1.2s cubic-bezier(.32,.72,0,1)`), and a new song's cover
//!   fades in over the old one (`.65s cubic-bezier(.45,0,.55,1)`).
//! - **Color**: a flat colour from the cover.
//!
//! Both sit under TIDAL's scrim: a radial darkening around a point a
//! little above the middle and a vertical one towards the player bar.
//!
//! The blurred cover is the 256-point texture `images::LyricsBackdrop`
//! makes off the interface thread, drawn as one rotated quad. The view
//! asks for 30 frames a second, and only while it is drawn, someone can
//! see it and something moves: a paused, settled view asks for none.

use std::time::Duration;

use egui::{Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::app::App;
use crate::motion::{Bezier, Transition, ease};
use crate::settings::PlayerBackground;
use crate::theme::Palette;

/// TIDAL's rotator: 360° a minute.
const DEGREES_PER_SECOND: f32 = 360.0 / 60.0;
/// The coast after a pause or a scrub: on by 30°, over 1.5 s.
const COAST_DEGREES: f32 = 30.0;
const COAST_SECONDS: f64 = 1.5;
/// Dragging the seek bar across the whole song turns the cover this far.
const SCRUB_DEGREES: f32 = 180.0;
/// Playing again or opening the view starts at five times the steady
/// speed and eases into it over 1.5 s (the owner's "little spin").
const BOOST_SECONDS: f32 = 1.5;
const BOOST_EXTRA: f32 = 4.0;
/// The square's side, in vmax: TIDAL's 40vmax scaled by `--blur-upscale: 4`.
const SIDE_VMAX: f32 = 1.6;
/// The image's scale while paused and while playing.
const SCALE_PAUSED: f32 = 1.18;
const SCALE_PLAYING: f32 = 1.26;
const SCALE: Transition = Transition::new(1.2, ease::PANEL);
/// A new song's cover (or colour) fades in over the old one.
const CROSSFADE: Transition = Transition::new(0.65, Bezier(0.45, 0.0, 0.55, 1.0));
/// `saturate(1.3)`.
pub(crate) const SATURATION: f32 = 1.3;
/// The time between the background's own frames while something moves:
/// thirty a second. The turn and the fades follow the clock, so this sets
/// how often they are drawn, not how fast they move.
const FRAME: f64 = 1.0 / 30.0;
/// Under everything, before the cover arrives.
const BASE: Color32 = Color32::from_rgb(0x12, 0x12, 0x14);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Standing at `angle`.
    Still,
    /// Turning from `angle`; `boost` starts it faster.
    Spin { boost: bool },
    /// Coasting from `angle` to `angle + 30°`.
    Coast,
    /// Following the seek bar: `angle` plus 180° per song dragged from
    /// `start`.
    Scrub { start: f32, fraction: f32 },
}

/// The rotator's angle, in degrees, as a function of time: each phase
/// starts at `since` from `angle`, so the angle at any moment can be asked
/// any number of times in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    phase: Phase,
    since: f64,
    angle: f32,
    playing: bool,
}

/// What the view is doing this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SpinInput {
    /// Drawn, and motion allowed.
    pub active: bool,
    pub playing: bool,
    /// Where the seek bar is being dragged, 0 to 1.
    pub scrub: Option<f32>,
    /// The view has just come into sight.
    pub opened: bool,
}

impl Default for Spin {
    fn default() -> Self {
        Self {
            phase: Phase::Still,
            since: 0.0,
            angle: 0.0,
            playing: false,
        }
    }
}

/// ease-out cubic: `1 - (1 - t)^3`.
fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Degrees turned `t` seconds into a spin. A boosted spin's speed starts at
/// `1 + BOOST_EXTRA` times the steady one and falls off as `(1 - t/T)^2`.
fn spun(t: f32, boost: bool) -> f32 {
    let t = t.max(0.0);
    let extra = if boost {
        let u = (t / BOOST_SECONDS).min(1.0);
        BOOST_EXTRA * BOOST_SECONDS / 3.0 * (1.0 - (1.0 - u).powi(3))
    } else {
        0.0
    };
    DEGREES_PER_SECOND * (t + extra)
}

impl Spin {
    /// The angle at `now`, in degrees.
    pub fn angle(&self, now: f64) -> f32 {
        let t = (now - self.since).max(0.0) as f32;
        match self.phase {
            Phase::Still => self.angle,
            Phase::Spin { boost } => self.angle + spun(t, boost),
            Phase::Coast => self.angle + COAST_DEGREES * ease_out_cubic(t / COAST_SECONDS as f32),
            Phase::Scrub { start, fraction } => self.angle + (fraction - start) * SCRUB_DEGREES,
        }
    }

    /// Whether the angle changes on its own at `now` (and frames are due).
    pub fn moving(&self, now: f64) -> bool {
        match self.phase {
            Phase::Spin { .. } => true,
            Phase::Coast => now - self.since < COAST_SECONDS,
            Phase::Still | Phase::Scrub { .. } => false,
        }
    }

    fn enter(&mut self, phase: Phase, now: f64) {
        self.angle = self.angle(now).rem_euclid(360.0);
        self.since = now;
        self.phase = phase;
    }

    /// Follows the view and the player to `now`.
    pub fn update(&mut self, now: f64, input: SpinInput) {
        let was_playing = std::mem::replace(&mut self.playing, input.playing);
        if !input.active {
            if self.phase != Phase::Still {
                self.enter(Phase::Still, now);
            }
            return;
        }
        // A coast that has run its course: the spin goes on if playing.
        if self.phase == Phase::Coast && now - self.since >= COAST_SECONDS {
            let end = self.since + COAST_SECONDS;
            self.angle = self.angle(end).rem_euclid(360.0);
            self.since = end;
            self.phase = if input.playing {
                Phase::Spin { boost: false }
            } else {
                Phase::Still
            };
        }
        match (input.scrub, self.phase) {
            (Some(fraction), Phase::Scrub { start, .. }) => {
                self.phase = Phase::Scrub { start, fraction };
                return;
            }
            (Some(fraction), _) => {
                self.enter(
                    Phase::Scrub {
                        start: fraction,
                        fraction,
                    },
                    now,
                );
                return;
            }
            (None, Phase::Scrub { .. }) => {
                self.enter(Phase::Coast, now);
                return;
            }
            (None, _) => {}
        }
        // Opening the view, playing again after a stop, or a pause: a
        // boosted spin or a coast from where it stands.
        let started = self.phase == Phase::Still && input.playing;
        if input.opened || started || input.playing != was_playing {
            let phase = if input.playing {
                Phase::Spin { boost: true }
            } else {
                Phase::Coast
            };
            self.enter(phase, now);
        }
    }
}

/// A texture or a colour fading in over the one before.
#[derive(Clone)]
struct Fade<T> {
    current: Option<T>,
    previous: Option<T>,
    since: f64,
}

impl<T> Default for Fade<T> {
    fn default() -> Self {
        Self {
            current: None,
            previous: None,
            since: 0.0,
        }
    }
}

impl<T: Clone + PartialEq> Fade<T> {
    /// Takes `next` as what is shown; a change starts a crossfade from what
    /// was shown before.
    fn show(&mut self, next: Option<T>, now: f64) {
        if next != self.current && next.is_some() {
            self.previous = self.current.take();
            self.current = next;
            self.since = now;
        }
    }

    /// How far the current one has faded in, 0 to 1.
    fn progress(&self, ctx: &egui::Context, now: f64) -> f32 {
        let seconds = crate::motion::seconds(ctx, CROSSFADE.seconds);
        if self.previous.is_none() || seconds <= 0.0 {
            return 1.0;
        }
        let t = ((now - self.since) as f32 / seconds).clamp(0.0, 1.0);
        CROSSFADE.ease.at(t)
    }
}

/// The blurred cover's texture, compared by id.
#[derive(Clone)]
struct Texture(egui::TextureHandle);

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

#[derive(Clone, Default)]
struct State {
    spin: Spin,
    /// The pass the spin last followed, so a frame that draws the
    /// background twice (the view sliding) moves it once.
    pass: Option<u64>,
    /// When the background's next frame of its own falls due, on `input.time`.
    due: Option<f64>,
    covers: Fade<Texture>,
    colors: Fade<Color32>,
}

fn state_id() -> egui::Id {
    egui::Id::new("player-background")
}

/// Paints the background over `rect`, shown only inside `clip`.
pub fn paint(app: &mut App, ui: &egui::Ui, rect: Rect, clip: Rect) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().with_clip_rect(clip.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, BASE);
    let now = ctx.input(|input| input.time);
    let pass = ctx.cumulative_pass_nr();
    let mut state = ctx.data_mut(|data| data.get_temp::<State>(state_id()).unwrap_or_default());
    let playing = app.now_playing().is_some_and(|now| now.playing);
    let reduced = crate::motion::reduced(&ctx);
    if state.pass != Some(pass) {
        let opened = state.pass.is_none_or(|seen| pass > seen + 1);
        state.pass = Some(pass);
        state.spin.update(
            now,
            SpinInput {
                active: !reduced && app.settings.player_background == PlayerBackground::Blur,
                playing,
                scrub: app.seek_preview,
                opened,
            },
        );
    }
    let art = app
        .now_playing()
        .and_then(|now| now.art_small.or(now.art_url));
    let fading;
    match app.settings.player_background {
        PlayerBackground::Blur => {
            let texture = app
                .lyrics_backdrop
                .texture(&ctx, app.backend.art(), art.as_deref())
                .cloned()
                .map(Texture);
            state.covers.show(texture, now);
            let shown = state.covers.progress(&ctx, now);
            fading = shown < 1.0;
            if !fading {
                state.covers.previous = None;
            }
            let scale = crate::motion::value(
                &ctx,
                state_id().with("scale"),
                if playing { SCALE_PLAYING } else { SCALE_PAUSED },
                SCALE,
            );
            let vmax = rect.width().max(rect.height());
            let side = SIDE_VMAX * vmax * scale;
            let angle = state.spin.angle(now).to_radians();
            if let Some(Texture(previous)) = &state.covers.previous {
                painter.add(rotated_quad(previous.id(), rect.center(), side, angle, 1.0));
            }
            if let Some(Texture(current)) = &state.covers.current {
                painter.add(rotated_quad(
                    current.id(),
                    rect.center(),
                    side,
                    angle,
                    shown,
                ));
            }
        }
        PlayerBackground::Color => {
            let color = app.tint_for(art.as_deref()).map(vivid);
            state.colors.show(color, now);
            let shown = state.colors.progress(&ctx, now);
            fading = shown < 1.0;
            if !fading {
                state.colors.previous = None;
            }
            if let Some(previous) = state.colors.previous {
                painter.rect_filled(rect, 0.0, previous);
            }
            if let Some(current) = state.colors.current {
                painter.rect_filled(rect, 0.0, current.gamma_multiply(shown));
            }
        }
    }
    painter.add(scrim(rect));
    let minimised = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
    // Turning is by the clock, not by the pass, so an animated cover's own
    // frames (24 a second) show it as well as a timer of ours would, and
    // the two timers need not each wake the window.
    let turning = state.spin.moving(now) && !crate::video_cover::drives_passes_now(&ctx);
    if !minimised && !crate::theme::unseen(&ctx) && (fading || turning) {
        let due = next_frame(now, state.due);
        state.due = Some(due);
        // `repaint_after` adds back the frame egui takes off a timed
        // request, which made a 33 ms wait come back after 16 ms.
        crate::theme::repaint_after(&ctx, Duration::from_secs_f64((due - now).max(0.0)));
    } else {
        state.due = None;
    }
    ctx.data_mut(|data| data.insert_temp(state_id(), state));
}

/// When the background's next frame falls due, for a pass at `now` with
/// the frame due at `due`. A pass within half a frame of it is that frame
/// (it draws the background as it stands at `now`) and the next is a whole
/// frame later; an earlier pass (the player bar's, a cover's) leaves the
/// frame where it was, so passes that come anyway add no frames of ours.
fn next_frame(now: f64, due: Option<f64>) -> f64 {
    match due {
        Some(due) if now < due - FRAME / 2.0 => due,
        _ => now + FRAME,
    }
}

/// The rotator's angle at `now`, as the last pass left it.
#[cfg(test)]
pub(crate) fn angle_at(ctx: &egui::Context, now: f64) -> Option<f32> {
    ctx.data(|data| data.get_temp::<State>(state_id()))
        .map(|state| state.spin.angle(now))
}

/// The cover as a square of `side` centred on `center`, turned by `angle`
/// radians, at `opacity`.
/// The cover colour as TIDAL's Color background shows it: the app's
/// accent from the art is kept dark for text, so it is brightened back to
/// a rich wash (hue and saturation kept, brightest channel at 190) before
/// the scrim darkens it again.
pub(crate) fn vivid(color: Color32) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let max = r.max(g).max(b);
    if max == 0 {
        return Color32::from_rgb(40, 40, 44);
    }
    let scale = 190.0 / f32::from(max);
    let channel = |value: u8| (f32::from(value) * scale).round().min(255.0) as u8;
    Color32::from_rgb(channel(r), channel(g), channel(b))
}

fn rotated_quad(
    texture: egui::TextureId,
    center: Pos2,
    side: f32,
    angle: f32,
    opacity: f32,
) -> egui::Shape {
    let mut mesh = Mesh::with_texture(texture);
    let tint = Color32::WHITE.gamma_multiply(opacity);
    let (sin, cos) = angle.sin_cos();
    let half = side / 2.0;
    for (x, y, u, v) in [
        (-half, -half, 0.0, 0.0),
        (half, -half, 1.0, 0.0),
        (half, half, 1.0, 1.0),
        (-half, half, 0.0, 1.0),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center + vec2(x * cos - y * sin, x * sin + y * cos),
            uv: pos2(u, v),
            color: tint,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

/// The scrim's opacity at `(x, y)`, both 0 to 1 across the view: TIDAL's
/// `radial-gradient(120% 100% at 50% 38%, transparent, .12 62%, .28)`
/// under `linear-gradient(180deg, .34, .42 24%, .5 45%, .62 72%, .76)`,
/// both black.
pub(crate) fn scrim_alpha(x: f32, y: f32) -> f32 {
    // The ellipse's radii are 120 % of the width and 100 % of the height.
    let dx = (x - 0.5) / 1.2;
    let dy = y - 0.38;
    let d = (dx * dx + dy * dy).sqrt();
    let radial = stops(d, &[(0.0, 0.0), (0.62, 0.12), (1.0, 0.28)]);
    let linear = stops(
        y,
        &[
            (0.0, 0.34),
            (0.24, 0.42),
            (0.45, 0.5),
            (0.72, 0.62),
            (1.0, 0.76),
        ],
    );
    1.0 - (1.0 - radial) * (1.0 - linear)
}

/// A gradient's value at `t` between its colour stops.
pub(crate) fn stops(t: f32, stops: &[(f32, f32)]) -> f32 {
    let mut previous = stops[0];
    if t <= previous.0 {
        return previous.1;
    }
    for &(at, value) in &stops[1..] {
        if t <= at {
            let span = (at - previous.0).max(1e-6);
            return previous.1 + (value - previous.1) * (t - previous.0) / span;
        }
        previous = (at, value);
    }
    previous.1
}

/// The scrim as one vertex-coloured grid, with rows on the linear
/// gradient's stops so its bands come out exact.
fn scrim(rect: Rect) -> egui::Shape {
    const COLUMNS: usize = 12;
    let mut rows: Vec<f32> = (0..=12).map(|row| row as f32 / 12.0).collect();
    rows.extend([0.24, 0.45, 0.72]);
    rows.sort_by(f32::total_cmp);
    let mut mesh = Mesh::default();
    for &y in &rows {
        for column in 0..=COLUMNS {
            let x = column as f32 / COLUMNS as f32;
            let alpha = scrim_alpha(x, y);
            mesh.colored_vertex(
                pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                ),
                Color32::from_black_alpha((alpha * 255.0).round() as u8),
            );
        }
    }
    let width = (COLUMNS + 1) as u32;
    for row in 0..rows.len() as u32 - 1 {
        for column in 0..COLUMNS as u32 {
            let a = row * width + column;
            let b = a + width;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }
    egui::Shape::mesh(mesh)
}

/// `saturate(amount)` as CSS defines it, on the blurred cover before it
/// becomes a texture.
pub(crate) fn saturate(image: &mut egui::ColorImage, amount: f32) {
    let s = amount;
    let m = [
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ];
    for pixel in &mut image.pixels {
        let [r, g, b, a] = pixel.to_array();
        let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
        let channel = |row: [f32; 3]| {
            (row[0] * r + row[1] * g + row[2] * b)
                .round()
                .clamp(0.0, f32::from(a)) as u8
        };
        *pixel = Color32::from_rgba_premultiplied(channel(m[0]), channel(m[1]), channel(m[2]), a);
    }
}

/// The picker's previews: 72 by 48 with 10-point corners, the caption
/// under each.
const SWATCH: egui::Vec2 = vec2(72.0, 48.0);
const SWATCH_GAP: f32 = 14.0;
const CAPTION_GAP: f32 = 12.0;
/// The width the picker needs in a settings row.
pub const PICKER_WIDTH: f32 = SWATCH.x * 2.0 + SWATCH_GAP + 8.0;

/// TIDAL's two previews, side by side: "Color" a warm red wash, "Blur" a
/// blurred slate-to-red. The chosen one has a 2-point ring with a gap and
/// a white caption; the other's caption is grey. Answers whether the
/// choice changed.
pub fn picker(
    ui: &mut egui::Ui,
    palette: &Palette,
    labels: [&str; 2],
    value: &mut PlayerBackground,
) -> bool {
    let caption = crate::theme::semibold(14.0);
    let height = SWATCH.y + 4.0 + CAPTION_GAP + 18.0;
    let (rect, _) = ui.allocate_exact_size(vec2(PICKER_WIDTH, height), Sense::hover());
    let mut changed = false;
    let first = Rect::from_min_size(rect.min + vec2(4.0, 4.0), SWATCH);
    for (index, (choice, label)) in [PlayerBackground::Color, PlayerBackground::Blur]
        .into_iter()
        .zip(labels)
        .enumerate()
    {
        let swatch = first.translate(vec2(index as f32 * (SWATCH.x + SWATCH_GAP), 0.0));
        let area = Rect::from_min_max(
            swatch.min - vec2(4.0, 4.0),
            pos2(swatch.right() + 4.0, rect.bottom()),
        );
        let response = ui.interact(
            area,
            ui.id().with(("player-background", index)),
            Sense::click(),
        );
        let selected = *value == choice;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
        });
        if response.clicked() && !selected {
            *value = choice;
            changed = true;
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let texture = preview(ui.ctx(), choice);
        ui.painter().add(
            egui::epaint::RectShape::filled(swatch, CornerRadius::same(10), Color32::WHITE)
                .with_texture(texture.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))),
        );
        let (ring, text) = if palette.dark {
            (Color32::WHITE, Color32::from_rgb(0x8a, 0x8a, 0x90))
        } else {
            (palette.text, palette.secondary)
        };
        if selected {
            ui.painter().rect_stroke(
                swatch.expand(3.0),
                CornerRadius::same(13),
                Stroke::new(2.0, ring),
                egui::StrokeKind::Middle,
            );
        }
        ui.painter().text(
            pos2(swatch.center().x, swatch.bottom() + 4.0 + CAPTION_GAP),
            egui::Align2::CENTER_TOP,
            label,
            caption.clone(),
            if selected { ring } else { text },
        );
        crate::theme::focus_ring(ui, &response);
    }
    changed
}

/// The preview's picture, made once: a small gradient, drawn with rounded
/// corners as a texture.
fn preview(ctx: &egui::Context, choice: PlayerBackground) -> egui::TextureHandle {
    let id = egui::Id::new(("player-background-preview", choice));
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let (w, h) = (36usize, 24usize);
    let mut pixels = Vec::with_capacity(w * h);
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
        Color32::from_rgb(
            (a[0] + (b[0] - a[0]) * t) as u8,
            (a[1] + (b[1] - a[1]) * t) as u8,
            (a[2] + (b[2] - a[2]) * t) as u8,
        )
    };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / (w - 1) as f32, y as f32 / (h - 1) as f32);
            pixels.push(match choice {
                PlayerBackground::Color => mix(
                    [0xbe as f32, 0x4a as f32, 0x3e as f32],
                    [0x93 as f32, 0x37 as f32, 0x32 as f32],
                    fy,
                ),
                PlayerBackground::Blur => mix(
                    [0x5a as f32, 0x5f as f32, 0x8c as f32],
                    [0xb4 as f32, 0x48 as f32, 0x42 as f32],
                    (fx * 0.35 + fy * 0.65).clamp(0.0, 1.0),
                ),
            });
        }
    }
    let image = egui::ColorImage::new([w, h], pixels);
    let texture = ctx.load_texture(format!("{id:?}"), image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_color_background_brightens_the_dark_accent_keeping_its_hue() {
        assert_eq!(
            vivid(Color32::from_rgb(76, 30, 24)),
            Color32::from_rgb(190, 75, 60)
        );
        assert_eq!(vivid(Color32::BLACK), Color32::from_rgb(40, 40, 44));
    }

    fn frame(spin: &mut Spin, now: f64, playing: bool) {
        spin.update(
            now,
            SpinInput {
                active: true,
                playing,
                ..Default::default()
            },
        );
    }

    /// A pass that comes anyway early in a frame leaves the next frame
    /// where it was due; one within half a frame of it is that frame.
    #[test]
    fn passes_between_frames_add_no_frames() {
        let first = next_frame(0.0, None);
        assert!((first - FRAME).abs() < 1e-9);
        assert_eq!(next_frame(0.010, Some(first)), first, "early pass");
        let late = next_frame(0.020, Some(first));
        assert!((late - (0.020 + FRAME)).abs() < 1e-9, "near the frame");
        // After a stop the old frame is long past: the next is a frame on.
        assert!((next_frame(5.0, Some(first)) - (5.0 + FRAME)).abs() < 1e-9);
    }

    #[test]
    fn opening_while_playing_spins_faster_then_steadily() {
        let mut spin = Spin::default();
        spin.update(
            0.0,
            SpinInput {
                active: true,
                playing: true,
                opened: true,
                scrub: None,
            },
        );
        assert!(spin.moving(0.0));
        // The first half second turns well past the steady 3°.
        assert!(spin.angle(0.5) > 3.0 * 2.0);
        // Once the boost has run out the speed is TIDAL's 6° a second.
        let a = spin.angle(10.0);
        let b = spin.angle(11.0);
        assert!((b - a - DEGREES_PER_SECOND).abs() < 1e-3);
        // The boost adds BOOST_EXTRA * T / 3 seconds' worth of turning.
        let extra = a - 10.0 * DEGREES_PER_SECOND;
        assert!((extra - DEGREES_PER_SECOND * BOOST_EXTRA * BOOST_SECONDS / 3.0).abs() < 1e-3);
    }

    #[test]
    fn a_pause_coasts_thirty_degrees_then_stops() {
        let mut spin = Spin::default();
        frame(&mut spin, 0.0, true);
        frame(&mut spin, 20.0, true);
        let at_pause = spin.angle(20.0);
        frame(&mut spin, 20.0, false);
        assert!(spin.moving(20.5));
        let coasted = spin.angle(20.5) - at_pause.rem_euclid(360.0);
        assert!(coasted > 0.0 && coasted < COAST_DEGREES);
        // ease-out cubic: most of the way there by half time.
        assert!(spin.angle(20.75) - at_pause.rem_euclid(360.0) > COAST_DEGREES * 0.8);
        frame(&mut spin, 22.0, false);
        assert!(!spin.moving(22.0), "settled after 1.5 s");
        let rest = spin.angle(22.0);
        assert!((rest - (at_pause + COAST_DEGREES).rem_euclid(360.0)).abs() < 1e-3);
        assert_eq!(spin.angle(60.0), rest, "no turning while paused");
    }

    #[test]
    fn playing_again_starts_from_where_it_stopped() {
        let mut spin = Spin::default();
        frame(&mut spin, 0.0, false);
        frame(&mut spin, 5.0, false);
        let rest = spin.angle(5.0);
        frame(&mut spin, 5.0, true);
        assert_eq!(spin.angle(5.0), rest);
        assert!(spin.moving(5.1));
        assert!(spin.angle(6.0) > rest + DEGREES_PER_SECOND);
    }

    #[test]
    fn hidden_or_reduced_motion_stops_at_the_current_angle() {
        let mut spin = Spin::default();
        frame(&mut spin, 0.0, true);
        let angle = spin.angle(3.0);
        spin.update(
            3.0,
            SpinInput {
                active: false,
                playing: true,
                ..Default::default()
            },
        );
        assert!(!spin.moving(3.0));
        assert!((spin.angle(9.0) - angle.rem_euclid(360.0)).abs() < 1e-3);
    }

    #[test]
    fn scrubbing_turns_with_the_seek_bar_and_coasts_on_release() {
        let mut spin = Spin::default();
        frame(&mut spin, 0.0, false);
        frame(&mut spin, 2.0, false);
        let base = spin.angle(2.0);
        let drag = |spin: &mut Spin, now: f64, fraction: f32| {
            spin.update(
                now,
                SpinInput {
                    active: true,
                    playing: false,
                    scrub: Some(fraction),
                    opened: false,
                },
            );
        };
        drag(&mut spin, 2.0, 0.2);
        drag(&mut spin, 2.2, 0.7);
        assert!((spin.angle(2.2) - (base + 90.0)).abs() < 1e-3);
        frame(&mut spin, 2.3, false);
        assert!(spin.moving(2.4), "coasts after release");
        frame(&mut spin, 4.0, false);
        assert!((spin.angle(4.0) - (base + 90.0 + COAST_DEGREES).rem_euclid(360.0)).abs() < 1e-3);
    }

    #[test]
    fn the_scrim_matches_tidals_gradients() {
        // Top centre: the linear .34 with the radial's bit over it.
        let top = scrim_alpha(0.5, 0.0);
        assert!(top > 0.34 && top < 0.42);
        // Bottom corners: the linear .76 under the radial's .12 to .28.
        let corner = scrim_alpha(0.0, 1.0);
        assert!(corner > 1.0 - 0.24 * 0.88 && corner < 1.0 - 0.24 * 0.72);
        // The radial centre adds nothing.
        let centre = stops(0.38, &[(0.24, 0.42), (0.45, 0.5)]);
        assert!((scrim_alpha(0.5, 0.38) - centre).abs() < 1e-3);
    }

    #[test]
    fn saturation_keeps_greys_and_pushes_colours() {
        let mut image = egui::ColorImage::new(
            [2, 1],
            vec![Color32::from_gray(128), Color32::from_rgb(160, 100, 100)],
        );
        saturate(&mut image, SATURATION);
        let grey = image.pixels[0];
        assert!((i32::from(grey.r()) - 128).abs() <= 1 && grey.r() == grey.b());
        let red = image.pixels[1];
        assert!(red.r() > 160 && red.g() < 100);
    }
}
