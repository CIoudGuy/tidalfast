//! How synced lyrics follow the song, as TIDAL does it: a line scales and
//! fades between current and not current, the view glides to the current
//! line on a critically damped spring, a scroll by hand pauses following,
//! and a Sync Lyrics button brings it back.
//!
//! Measured from TIDAL's stylesheet and script (2026-10-14): lines sit at
//! `scale(.9)` and `opacity .55` and move to `scale(1)` and full opacity over
//! `.46s cubic-bezier(.4,0,.2,1)` after 90 ms; the scroll spring is
//! `x'' = -2*11*x' - 11^2*(x - target)`, stepped at most 1/30 s at a time and
//! stopping within half a point at under 8 points a second; the current line
//! rests a quarter of the view from the top; the first placement is at once.

use egui::{Color32, Context, Id, Pos2, Rect, Sense, Shape, Vec2, epaint::TextShape};

use crate::motion::{self, tidal};

/// A line that is not being sung is drawn this fraction of its size.
pub const LINE_SCALE_INACTIVE: f32 = 0.9;
/// A line that is not being sung is drawn this opaque.
pub const LINE_OPACITY_INACTIVE: f32 = 0.55;
/// Where the current line rests, as a fraction of the view from its top.
pub const LINE_REST_AT: f32 = 0.25;
/// The gap under a line at TIDAL's 32 point text (48 at 1x).
pub const LINE_GAP: f32 = 30.0;

/// Spring stiffness of the scroll (`HYe`).
const OMEGA: f32 = 11.0;
/// The longest time step the spring takes (`UYe`).
const MAX_STEP: f32 = 1.0 / 30.0;
/// The spring is done within this distance (`WYe`)...
const STOP_DISTANCE: f32 = 0.5;
/// ...and under this speed (`GYe`).
const STOP_SPEED: f32 = 8.0;
/// A view this far from where it was put was moved by hand (`YYe`).
const MOVED_BY_HAND: f32 = 2.0;
/// Below this width of window the Sync Lyrics button is not shown.
pub const SYNC_MIN_WIDTH: f32 = 768.0;

/// How a line looks: its size and its opacity (1 is fully opaque).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineStyle {
    pub scale: f32,
    pub opacity: f32,
}

impl LineStyle {
    pub fn settled(current: bool) -> Self {
        Self::at(if current { 1.0 } else { 0.0 })
    }

    /// How far the line has come to being the current one, 0 to 1.
    pub fn lit(self) -> f32 {
        ((self.scale - LINE_SCALE_INACTIVE) / (1.0 - LINE_SCALE_INACTIVE)).clamp(0.0, 1.0)
    }

    fn at(lit: f32) -> Self {
        Self {
            scale: LINE_SCALE_INACTIVE + (1.0 - LINE_SCALE_INACTIVE) * lit,
            opacity: LINE_OPACITY_INACTIVE + (1.0 - LINE_OPACITY_INACTIVE) * lit,
        }
    }
}

/// The style of line `index` of the lyrics under `scope`, moving towards
/// whether it is the `current` one the way TIDAL's CSS transition does.
/// Asks for frames only while it moves.
pub fn line_style(ctx: &Context, scope: Id, index: usize, current: bool) -> LineStyle {
    let lit = motion::value(
        ctx,
        scope.with(("line-lit", index)),
        if current { 1.0 } else { 0.0 },
        tidal::LYRICS_LINE,
    );
    LineStyle::at(lit)
}

/// TIDAL blurs lines away from the current one while following (1.2, .7 and
/// .3 px at three or more, two and one lines away). A blur is dearer than
/// it is worth here, so the line is dimmed a touch instead; this is that
/// factor, moving with the same transition.
pub fn blur_dimming(
    ctx: &Context,
    scope: Id,
    index: usize,
    active: Option<usize>,
    following: bool,
) -> f32 {
    let distance = match active {
        Some(active) => index.abs_diff(active),
        None => index + 1,
    };
    let blur_px = match distance {
        0 => 0.0,
        1 => 0.3,
        2 => 0.7,
        _ => 1.2,
    };
    let target = if following { 1.0 - blur_px * 0.1 } else { 1.0 };
    motion::value(
        ctx,
        scope.with(("line-blur", index)),
        target,
        tidal::LYRICS_LINE,
    )
}

/// Draws `galley` at `rect` scaled by `style` around the middle of the
/// start edge (left, or right for right-to-left text), faded by
/// `style.opacity * extra`.
pub fn paint_line(
    ui: &egui::Ui,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    style: LineStyle,
    rtl: bool,
    extra: f32,
) {
    let origin = Pos2::new(
        if rtl { rect.right() } else { rect.left() },
        rect.center().y,
    );
    let mut shape = Shape::Text(
        TextShape::new(
            crate::bidi::galley_pos(rect, &galley),
            galley,
            Color32::WHITE,
        )
        .with_opacity_factor((style.opacity * extra).clamp(0.0, 1.0)),
    );
    if style.scale != 1.0 {
        shape.transform(egui::emath::TSTransform::new(
            origin.to_vec2() * (1.0 - style.scale),
            style.scale,
        ));
    }
    ui.painter().add(shape);
}

/// The scroll glide towards the current line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pos: f32,
    vel: f32,
    animating: bool,
    /// The offset last put on the view; a view anywhere else was scrolled.
    written: Option<f32>,
}

impl Spring {
    pub fn animating(&self) -> bool {
        self.animating
    }

    /// Lets go of the view, as when following is paused.
    pub fn release(&mut self) {
        *self = Self::default();
    }

    /// Whether the view at `actual` was moved by hand since it was last set.
    pub fn moved_by_hand(&self, actual: f32) -> bool {
        self.written
            .is_some_and(|written| (actual - written).abs() >= MOVED_BY_HAND)
    }

    /// The offset to put on the view for this frame, if it is to change.
    /// `actual` is where the view is, `target` where the current line wants
    /// it, `dt` the seconds since the last frame; `immediate` jumps there.
    pub fn follow(&mut self, actual: f32, target: f32, dt: f32, immediate: bool) -> Option<f32> {
        if immediate {
            self.animating = false;
            self.vel = 0.0;
            self.pos = target;
            self.written = Some(target);
            return ((actual - target).abs() >= 0.01).then_some(target);
        }
        if !self.animating {
            if (actual - target).abs() < STOP_DISTANCE {
                return None;
            }
            self.pos = actual;
            self.vel = 0.0;
            self.animating = true;
        }
        let dt = dt.clamp(0.0, MAX_STEP);
        let accel = -2.0 * OMEGA * self.vel - OMEGA * OMEGA * (self.pos - target);
        self.vel += accel * dt;
        self.pos += self.vel * dt;
        if (self.pos - target).abs() < STOP_DISTANCE && self.vel.abs() < STOP_SPEED {
            self.pos = target;
            self.vel = 0.0;
            self.animating = false;
        }
        self.written = Some(self.pos);
        Some(self.pos)
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    spring: Spring,
    placed: bool,
    pending: Option<f32>,
    /// The `ScrollArea` the view was last drawn in.
    area: Option<Id>,
}

/// What a frame of following found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Verdict {
    /// The reader moved the view: following should pause.
    pub paused_by_hand: bool,
}

fn state_id(scope: Id) -> Id {
    scope.with("lyrics-follow")
}

/// The scroll offset to give the lyrics `ScrollArea` this frame, if any.
pub fn offset(ctx: &Context, scope: Id) -> Option<f32> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(state_id(scope))
            .pending
            .take()
    })
}

/// Called after the lyrics `ScrollArea` is drawn. `target` is the offset that
/// puts the current line at its resting place (None while there is none to
/// follow), `max` the furthest the view can scroll, `actual` where it is.
/// `area` is the `ScrollArea`'s id: the view moves between layers as it
/// slides in (the now-playing view is drawn on its own layer until it covers
/// the window), which gives the area a new id and a new state at offset 0.
/// That is not the reader scrolling, so the view is placed again at once
/// instead of pausing following.
pub fn after(
    ctx: &Context,
    scope: Id,
    area: Id,
    following: bool,
    actual: f32,
    target: Option<f32>,
    max: f32,
) -> Verdict {
    let dt = ctx.input(|input| input.stable_dt);
    let reduced = motion::reduced(ctx);
    // Nobody sees a glide in a covered window: the line is put in place.
    let unseen = crate::theme::unseen(ctx);
    let mut verdict = Verdict::default();
    let mut repaint = false;
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id(scope));
        if state.area.replace(area).is_some_and(|was| was != area) {
            state.spring.release();
            state.placed = false;
        }
        if !following {
            state.spring.release();
            return;
        }
        if state.spring.moved_by_hand(actual) {
            state.spring.release();
            verdict.paused_by_hand = true;
            return;
        }
        let Some(target) = target else { return };
        let target = target.clamp(0.0, max.max(0.0));
        let immediate = reduced || !state.placed || unseen;
        state.pending = state.spring.follow(actual, target, dt, immediate);
        state.placed = true;
        // An offset to put on the view is put there next frame.
        repaint = state.pending.is_some();
    });
    if repaint {
        ctx.request_repaint();
    }
    verdict
}

/// The Sync Lyrics button, in the bottom right corner of `area` while
/// `shown`. True when clicked. TIDAL: white pill, at least 44 high, black
/// label and 16 point icon, shadow `0 2px 8px rgba(0,0,0,.2)`, 32 up and 16
/// in from the corner, arriving over .2s from 8 points down at .92 size,
/// shrinking to .96 while pressed.
pub fn sync_button(ui: &egui::Ui, area: Rect, label: &str, shown: bool) -> bool {
    let id = Id::new("lyrics-sync-button");
    if !shown {
        // Forget it so it arrives again next time.
        ui.ctx()
            .data_mut(|data| data.remove::<u64>(id.with("appear-seen")));
        return false;
    }
    let arrive = motion::appear(ui.ctx(), id, tidal::SYNC_ENTER);
    let font = crate::theme::semibold(14.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::BLACK);
    let icon = 16.0;
    let size = Vec2::new(galley.size().x + icon + 8.0 + 40.0, 44.0);
    let rest = Rect::from_min_size(
        Pos2::new(area.right() - 16.0 - size.x, area.bottom() - 32.0 - size.y),
        size,
    );
    let response = ui.interact(rest, id.with("hit"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let press = motion::press_scale(ui.ctx(), &response, 1.0, 0.96);
    let enter_scale = if motion::reduced(ui.ctx()) {
        1.0
    } else {
        0.92 + 0.08 * arrive
    };
    let lift = if motion::reduced(ui.ctx()) {
        0.0
    } else {
        8.0 * (1.0 - arrive)
    };
    let rect = Rect::from_center_size(
        rest.center() + Vec2::new(0.0, lift),
        size * (press * enter_scale),
    );
    let fade = arrive.clamp(0.0, 1.0);
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha((51.0 * fade) as u8),
        }
        .as_shape(rect, radius),
    );
    painter.rect_filled(rect, radius, Color32::WHITE.gamma_multiply(fade));
    let content = galley.size().x + icon + 8.0;
    let left = rect.center().x - content / 2.0;
    crate::theme::paint_icon(
        ui,
        crate::theme::Icon::LyricsSync,
        Rect::from_center_size(
            Pos2::new(left + icon / 2.0, rect.center().y),
            Vec2::splat(icon),
        ),
        icon,
        Color32::BLACK.gamma_multiply(fade),
    );
    painter.galley(
        Pos2::new(left + icon + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        Color32::BLACK.gamma_multiply(fade),
    );
    if arrive < 1.0 {
        ui.ctx().request_repaint();
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spring: &mut Spring, mut at: f32, target: f32) -> (f32, usize) {
        let mut frames = 0;
        while let Some(next) = spring.follow(at, target, 1.0 / 60.0, false) {
            at = next;
            frames += 1;
            assert!(frames < 600, "the glide never settles");
            if !spring.animating() {
                break;
            }
        }
        (at, frames)
    }

    #[test]
    fn the_glide_settles_on_the_line_without_overshooting() {
        let mut spring = Spring::default();
        let (end, frames) = run(&mut spring, 0.0, 300.0);
        assert_eq!(end, 300.0);
        assert!(frames > 20, "a glide, not a jump: {frames} frames");
        assert!(!spring.animating());
    }

    #[test]
    fn a_settled_view_asks_for_nothing() {
        let mut spring = Spring::default();
        assert_eq!(spring.follow(100.2, 100.0, 1.0 / 60.0, false), None);
        assert!(!spring.animating());
        let mut spring = Spring::default();
        run(&mut spring, 0.0, 120.0);
        assert_eq!(spring.follow(120.0, 120.0, 1.0 / 60.0, false), None);
    }

    #[test]
    fn the_first_placement_is_at_once() {
        let mut spring = Spring::default();
        assert_eq!(spring.follow(0.0, 500.0, 1.0 / 60.0, true), Some(500.0));
        assert!(!spring.animating());
    }

    #[test]
    fn a_view_moved_by_hand_pauses_but_the_springs_own_writes_do_not() {
        let mut spring = Spring::default();
        let mut at = 0.0;
        for _ in 0..5 {
            at = spring.follow(at, 400.0, 1.0 / 60.0, false).unwrap();
            assert!(!spring.moved_by_hand(at));
        }
        assert!(spring.moved_by_hand(at + 3.0));
        assert!(!spring.moved_by_hand(at + 1.0));
        spring.release();
        assert!(!spring.moved_by_hand(at + 50.0));
    }

    #[test]
    fn a_view_drawn_in_a_new_scroll_area_is_placed_again_not_paused() {
        // The now-playing view moves from its sliding layer to the page as it
        // finishes opening: a new `ScrollArea` id, at offset 0, with the
        // song's line wanted at 92.
        let ctx = Context::default();
        let scope = Id::new("test-lyrics");
        let sliding = Id::new("sliding-area");
        let settled = Id::new("settled-area");
        let frame = |area: Id, actual: f32| {
            let mut verdict = Verdict::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let ctx = ui.ctx();
                let _ = offset(ctx, scope);
                verdict = after(ctx, scope, area, true, actual, Some(92.0), 5000.0);
            });
            output.textures_delta.clear();
            verdict
        };
        assert!(!frame(sliding, 0.0).paused_by_hand);
        // The view was put at 92 and shows there.
        assert!(!frame(sliding, 92.0).paused_by_hand);
        // The area changed and starts over at 0.
        assert!(
            !frame(settled, 0.0).paused_by_hand,
            "a new scroll area is not the reader scrolling"
        );
        // It is put at the line again, at once.
        let put = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<State>(state_id(scope))
                .pending
        });
        assert_eq!(put, Some(92.0));
        // A reader who then scrolls it still pauses following.
        let _ = frame(settled, 92.0);
        assert!(frame(settled, 300.0).paused_by_hand);
    }

    #[test]
    fn long_frames_take_short_steps() {
        let mut a = Spring::default();
        let mut b = Spring::default();
        let fast = a.follow(0.0, 100.0, 1.0 / 30.0, false);
        let slow = b.follow(0.0, 100.0, 2.0, false);
        assert_eq!(fast, slow);
    }

    #[test]
    fn lines_settle_on_their_styles() {
        let ctx = Context::default();
        let scope = Id::new("test-lyrics");
        let frame = |time: f64, current: bool| {
            let mut out = None;
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                out = Some(line_style(ui.ctx(), scope, 3, current));
            });
            output.textures_delta.clear();
            out.unwrap()
        };
        assert_eq!(frame(0.0, false), LineStyle::settled(false));
        let middle = frame(0.1, true);
        let mid = frame(0.4, true);
        assert!(
            mid.scale > LINE_SCALE_INACTIVE && mid.scale < 1.0,
            "{middle:?} {mid:?}"
        );
        assert_eq!(frame(5.0, true), LineStyle::settled(true));
        assert_eq!(LineStyle::settled(true).scale, 1.0);
        assert_eq!(LineStyle::settled(false).opacity, LINE_OPACITY_INACTIVE);
    }
}
