//! Whether the desktop asks for less motion: Windows' "Animation effects",
//! macOS's "Reduce motion" and GNOME's "Animations". When it does, the
//! interface shows where each animation ends at once: egui's own easing and
//! scrolling to a place, the reveal of a new theme, the fades that follow
//! the song, the lyrics, and rows making room for a drag.

use std::time::{Duration, Instant};

/// How long an answer is trusted before the system is asked again.
const RECHECK: Duration = Duration::from_secs(1);

/// egui's easing time for its own state changes. It is also how long the
/// floating scroll bars take to fade in and out, which makes it TIDAL's
/// overlay scroll bar transition (opacity .15s).
pub const ANIMATION_TIME: f32 = 0.15;

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier(pub f32, pub f32, pub f32, pub f32);

impl Bezier {
    /// The eased progress at `t` (0..=1 of the duration), the way a browser
    /// evaluates it: solve the curve's x for `t`, then read its y.
    pub fn at(self, t: f32) -> f32 {
        let Bezier(x1, y1, x2, y2) = self;
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        if x1 == y1 && x2 == y2 {
            return t;
        }
        // Polynomial coefficients of x(s) and y(s), as browsers keep them.
        let cx = 3.0 * x1;
        let bx = 3.0 * (x2 - x1) - cx;
        let ax = 1.0 - cx - bx;
        let cy = 3.0 * y1;
        let by = 3.0 * (y2 - y1) - cy;
        let ay = 1.0 - cy - by;
        let x = |s: f32| ((ax * s + bx) * s + cx) * s;
        let dx = |s: f32| (3.0 * ax * s + 2.0 * bx) * s + cx;
        // Newton's method first, bisection when the slope is too flat.
        let mut s = t;
        let mut solved = false;
        for _ in 0..8 {
            let error = x(s) - t;
            if error.abs() < 1e-6 {
                solved = true;
                break;
            }
            let slope = dx(s);
            if slope.abs() < 1e-6 {
                break;
            }
            s -= error / slope;
        }
        if !solved || !(0.0..=1.0).contains(&s) {
            let (mut low, mut high) = (0.0_f32, 1.0_f32);
            s = t;
            for _ in 0..32 {
                let value = x(s);
                if (value - t).abs() < 1e-6 {
                    break;
                }
                if value < t {
                    low = s;
                } else {
                    high = s;
                }
                s = (low + high) / 2.0;
            }
        }
        ((ay * s + by) * s + cy) * s
    }
}

/// CSS's named timing functions and the curves TIDAL's stylesheets use.
pub mod ease {
    use super::Bezier;

    pub const LINEAR: Bezier = Bezier(0.0, 0.0, 1.0, 1.0);
    /// `ease`, what a transition runs when it names no curve.
    pub const EASE: Bezier = Bezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Bezier = Bezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Bezier = Bezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Bezier = Bezier(0.42, 0.0, 0.58, 1.0);
    /// `--market-core-animation-enter-transition-easing`.
    pub const ENTER: Bezier = Bezier(0.26, 0.1, 0.48, 1.0);
    /// `--market-core-animation-exit-transition-easing`.
    pub const EXIT: Bezier = Bezier(0.52, 0.0, 0.74, 0.0);
    /// `--player-panel-ease` and `--panel-transition-easing`: the
    /// now-playing view and its panels.
    pub const PANEL: Bezier = Bezier(0.32, 0.72, 0.0, 1.0);
    /// Dialogs and their backdrop.
    pub const DIALOG: Bezier = Bezier(0.65, 0.0, 0.35, 1.0);
}

/// One CSS transition: how long, which curve, and how long it waits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub seconds: f32,
    pub ease: Bezier,
    pub delay: f32,
}

impl Transition {
    pub const fn new(seconds: f32, ease: Bezier) -> Self {
        Self {
            seconds,
            ease,
            delay: 0.0,
        }
    }

    pub const fn delayed(self, delay: f32) -> Self {
        Self { delay, ..self }
    }
}

/// TIDAL's transitions, read from its stylesheets.
pub mod tidal {
    use super::{Bezier, Transition, ease};

    /// Navigation rows, header buttons and menus' hover fill and text:
    /// `background-color .2s, color .2s`.
    pub const HOVER: Transition = Transition::new(0.2, ease::EASE);
    /// Icon buttons' colour (row "…" and heart, shuffle, repeat, the
    /// scrubber's times): `color .15s`.
    pub const ICON: Transition = Transition::new(0.15, ease::EASE);
    /// The fade of a page header's icon-over-label buttons to half
    /// opacity under the pointer.
    pub const HEADER_BUTTON: Transition = Transition::new(0.12, ease::EASE);
    /// How much of its colour a header button keeps while hovered.
    pub const HEADER_BUTTON_HOVER: f32 = 0.5;
    /// Player bar buttons grow to 1.1 on hover and shrink to .9 while
    /// pressed: `transform .15s`.
    pub const PRESS: Transition = Transition::new(0.15, ease::EASE);
    /// The scale a player bar button grows to under the pointer.
    pub const HOVER_SCALE: f32 = 1.1;
    /// The scale a button shrinks to while pressed.
    pub const PRESS_SCALE: f32 = 0.9;
    /// Large pill buttons (Play, Shuffle) shrink to .97 while pressed.
    pub const PILL_PRESS_SCALE: f32 = 0.97;
    /// The card's gradient, play and heart buttons: `opacity .3s ease-in-out`.
    pub const CARD_OVERLAY: Transition = Transition::new(0.3, ease::EASE_IN_OUT);
    /// The card's heart grows to 1.1 on hover: `transform .1s ease-in-out`.
    pub const CARD_BUTTON: Transition = Transition::new(0.1, ease::EASE_IN_OUT);
    /// Tabs (Home's, settings', now playing's): `color .2s`.
    pub const TAB: Transition = Transition::new(0.2, ease::EASE);
    /// Search pills' fill: `background-color .1s` (the market pill).
    pub const PILL: Transition = Transition::new(0.1, ease::EASE);
    /// The scrubber's knob appears on hover: `opacity .15s`.
    pub const KNOB: Transition = Transition::new(0.15, ease::EASE);
    /// The top bar's dark veil once the page scrolls: `opacity .2s`.
    pub const HEADER: Transition = Transition::new(0.2, ease::EASE);
    /// The now-playing view slides up from the bottom and back down:
    /// `transform .4s var(--player-panel-ease)`.
    pub const NOW_PLAYING: Transition = Transition::new(0.4, ease::PANEL);
    /// The sidebar widening to its full width and narrowing to the icon
    /// rail. TIDAL's stylesheet has no width rule for it (the rail is a
    /// re-render), so this borrows the panel curve at the overlay's 0.3 s.
    pub const SIDEBAR: Transition = Transition::new(0.3, ease::PANEL);
    /// A side panel (the now-playing tabs, the play queue) fades in from
    /// 40 px to the right: `opacity, transform .5s
    /// var(--panel-transition-easing) 50ms`.
    pub const PANEL: Transition = Transition::new(0.5, ease::PANEL).delayed(0.05);
    /// How far a panel moves while it fades in.
    pub const PANEL_SHIFT: f32 = 40.0;
    /// Dialogs fade in and grow from .95, and back:
    /// `.2s cubic-bezier(.65, 0, .35, 1)`.
    pub const DIALOG: Transition = Transition::new(0.2, ease::DIALOG);
    /// The scale a dialog grows from.
    pub const DIALOG_SCALE: f32 = 0.95;
    /// A dialog's backdrop darkens to black 40 % and back:
    /// `.35s cubic-bezier(.65, 0, .35, 1)`.
    pub const BACKDROP: Transition = Transition::new(0.35, ease::DIALOG);
    /// Notifications fade in and out: `fadeIn / fadeOut .5s ease`.
    pub const TOAST: Transition = Transition::new(0.5, ease::EASE);
    /// A skeleton's content fades in this fast, a fraction of TIDAL's
    /// `moduleIn .4s`, so navigation feels instant. Pages themselves open
    /// without any transition.
    pub const REVEAL: Transition = Transition::new(0.08, ease::EASE);
    /// Page modules fade in when they appear: `moduleIn .4s ease`.
    pub const CONTENT: Transition = Transition::new(0.4, ease::EASE);
    /// Lyrics: a line scales and fades between current and not current
    /// (`--lyrics-ease` over `--lyrics-duration`, after 90ms).
    pub const LYRICS_LINE: Transition =
        Transition::new(0.46, Bezier(0.4, 0.0, 0.2, 1.0)).delayed(0.09);
    /// The Sync Lyrics button coming in: `.2s cubic-bezier(.16,1,.3,1)`.
    pub const SYNC_ENTER: Transition = Transition::new(0.2, Bezier(0.16, 1.0, 0.3, 1.0));
}

/// How far `on` has come, 0 to 1, eased by `transition` in both directions
/// the way CSS runs a transition and its reverse. egui keeps the linear
/// progress and repaints only while it moves; with less motion the answer
/// jumps to the end.
pub fn progress(ctx: &egui::Context, id: egui::Id, on: bool, transition: Transition) -> f32 {
    let total = transition.seconds + transition.delay;
    let raw = ctx.animate_bool_with_time(id, on, seconds(ctx, total));
    eased(raw, on, transition)
}

/// The eased value for egui's linear progress `raw` towards `on`.
fn eased(raw: f32, on: bool, transition: Transition) -> f32 {
    let total = transition.seconds + transition.delay;
    if total <= 0.0 || raw <= 0.0 || raw >= 1.0 {
        return raw.clamp(0.0, 1.0);
    }
    let elapsed = if on { raw } else { 1.0 - raw } * total;
    let local = ((elapsed - transition.delay) / transition.seconds.max(1e-6)).clamp(0.0, 1.0);
    let value = transition.ease.at(local);
    if on { value } else { 1.0 - value }
}

/// How far something that comes into view under `id` has faded in, 0 to 1:
/// like a CSS animation, it runs `transition` each time the thing appears
/// after a frame or more away, and stays at 1 while it is shown.
pub fn appear(ctx: &egui::Context, id: egui::Id, transition: Transition) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    let seen = id.with("appear-seen");
    let previous = ctx.data_mut(|data| {
        let previous = data.get_temp::<u64>(seen);
        data.insert_temp(seen, pass);
        previous
    });
    let away = previous.is_none_or(|previous| pass > previous + 1);
    if away {
        // Start from nothing: a new id would otherwise begin at its target.
        ctx.animate_bool_with_time(id, false, 0.0);
    }
    progress(ctx, id, true, transition)
}

/// `from` blended towards `to` by how far `on` has come.
pub fn color(
    ctx: &egui::Context,
    id: egui::Id,
    on: bool,
    from: egui::Color32,
    to: egui::Color32,
    transition: Transition,
) -> egui::Color32 {
    blend(from, to, progress(ctx, id, on, transition))
}

/// `from` blended towards `to` at `t`, channel by channel as a browser
/// blends colours.
pub fn blend(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    if t <= 0.0 {
        from
    } else if t >= 1.0 {
        to
    } else {
        from.lerp_to_gamma(to, t)
    }
}

/// A button's scale for TIDAL's hover-grow and press-shrink (`transform
/// .15s`): 1 at rest, `hover` under the pointer, `pressed` while held.
pub fn press_scale(
    ctx: &egui::Context,
    response: &egui::Response,
    hover: f32,
    pressed: f32,
) -> f32 {
    let id = response.id.with("press-scale");
    let target = if response.is_pointer_button_down_on() {
        pressed
    } else if response.hovered() {
        hover
    } else {
        1.0
    };
    value(ctx, id, target, tidal::PRESS)
}

/// Where a value moving to `target` is, eased by `transition`. Like a CSS
/// transition, a new target starts from wherever the value is now. Repaints
/// only while it moves; with less motion it is at `target` at once.
pub fn value(ctx: &egui::Context, id: egui::Id, target: f32, transition: Transition) -> f32 {
    #[derive(Clone, Copy)]
    struct Run {
        from: f32,
        to: f32,
        start: f64,
    }
    let now = ctx.input(|input| input.time);
    let total = seconds(ctx, transition.seconds + transition.delay);
    let run = ctx.data_mut(|data| {
        let run = data.get_temp_mut_or_insert_with(id, || Run {
            from: target,
            to: target,
            start: now,
        });
        if run.to != target {
            let current = sample(*run, now, total, transition);
            *run = Run {
                from: current,
                to: target,
                start: now,
            };
        }
        *run
    });
    fn sample(run: Run, now: f64, total: f32, transition: Transition) -> f32 {
        if total <= 0.0 {
            return run.to;
        }
        let elapsed = (now - run.start) as f32 - transition.delay;
        let t = (elapsed / transition.seconds.max(1e-6)).clamp(0.0, 1.0);
        run.from + (run.to - run.from) * transition.ease.at(t)
    }
    let current = sample(run, now, total, transition);
    if current != run.to && (now - run.start) < f64::from(total) {
        ctx.request_repaint();
        current
    } else {
        run.to
    }
}

/// The system's preference, asked at most once a second.
#[derive(Default)]
pub struct Motion {
    reduced: bool,
    checked: Option<Instant>,
    /// What the Linux desktop portal said at the last check.
    desktop: Option<bool>,
    /// A preference that overrides the system's (tests that read finished
    /// frames).
    forced: Option<bool>,
}

impl Motion {
    /// Whether less motion is asked for, as of the last check.
    pub fn reduced(&self) -> bool {
        self.reduced
    }

    /// Asks the system again when the last answer is a second old, and
    /// answers whether the preference changed. `desktop_animations` is what
    /// the Linux desktop portal last said about animations, if anything.
    /// The portal wakes the app when that changes, and nothing asks again
    /// a second later, so a new answer from it applies at once.
    pub fn refresh(&mut self, now: Instant, desktop_animations: Option<bool>) -> bool {
        if let Some(forced) = self.forced {
            return self.set(forced);
        }
        if desktop_animations == self.desktop
            && self
                .checked
                .is_some_and(|checked| now.duration_since(checked) < RECHECK)
        {
            return false;
        }
        self.checked = Some(now);
        self.desktop = desktop_animations;
        self.set(system_reduced(desktop_animations))
    }

    /// Holds the preference at `reduced` whatever the system says.
    pub fn force(&mut self, reduced: bool) {
        self.forced = Some(reduced);
        self.set(reduced);
    }

    /// Takes `reduced` as the preference, and answers whether it changed.
    pub fn set(&mut self, reduced: bool) -> bool {
        let changed = self.reduced != reduced;
        self.reduced = reduced;
        changed
    }
}

/// Sets egui's easing and scrolling for `reduced`, none when less motion is
/// asked for, and keeps it for the views' own animations to read.
pub fn apply(ctx: &egui::Context, reduced: bool) {
    let time = if reduced { 0.0 } else { ANIMATION_TIME };
    let scroll = if reduced {
        egui::style::ScrollAnimation::none()
    } else {
        egui::style::ScrollAnimation::default()
    };
    ctx.all_styles_mut(|style| {
        style.animation_time = time;
        style.scroll_animation = scroll;
    });
    ctx.data_mut(|data| data.insert_temp(reduced_id(), reduced));
}

fn reduced_id() -> egui::Id {
    egui::Id::new("tidalfast-reduced-motion")
}

/// Whether less motion is asked for, as last applied to `ctx`.
pub fn reduced(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp(reduced_id()))
        .unwrap_or(false)
}

/// How long an animation of `seconds` takes: none when less motion is asked
/// for, so it shows where it ends at once.
pub fn seconds(ctx: &egui::Context, seconds: f32) -> f32 {
    if reduced(ctx) { 0.0 } else { seconds }
}

/// A scroll to a place over `seconds`, or at once when less motion is asked
/// for.
pub fn scroll(ctx: &egui::Context, seconds: f32) -> egui::style::ScrollAnimation {
    if reduced(ctx) {
        egui::style::ScrollAnimation::none()
    } else {
        egui::style::ScrollAnimation::duration(seconds)
    }
}

#[cfg(windows)]
fn system_reduced(_desktop_animations: Option<bool>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled: windows_sys::core::BOOL = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut windows_sys::core::BOOL).cast(),
            0,
        )
    };
    read != 0 && enabled == 0
}

#[cfg(target_os = "macos")]
fn system_reduced(_desktop_animations: Option<bool>) -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_reduced(desktop_animations: Option<bool>) -> bool {
    desktop_animations == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skeleton_reveal_is_too_quick_to_hold_content_back() {
        let quick = tidal::REVEAL;
        assert!(quick.seconds < tidal::CONTENT.seconds / 2.0);
        assert_eq!(eased(1.0, true, tidal::REVEAL), 1.0);
    }

    #[test]
    fn the_system_is_asked_at_most_once_a_second() {
        let mut motion = Motion::default();
        let start = Instant::now();
        motion.refresh(start, None);
        motion.set(true);
        assert!(!motion.refresh(start + Duration::from_millis(500), None));
        assert!(motion.reduced(), "a fresh answer is kept");
    }

    #[test]
    fn a_change_is_reported_once() {
        let mut motion = Motion::default();
        assert!(motion.set(true));
        assert!(!motion.set(true));
        assert!(motion.set(false));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn linux_follows_the_desktops_animations_setting() {
        assert!(system_reduced(Some(false)));
        assert!(!system_reduced(Some(true)));
        assert!(!system_reduced(None));
    }

    /// The portal wakes the app once when its switch changes; that answer
    /// applies then, even within a second of the last check.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_portal_change_applies_within_the_second() {
        let mut motion = Motion::default();
        let start = Instant::now();
        assert!(!motion.refresh(start, Some(true)));
        assert!(motion.refresh(start + Duration::from_millis(100), Some(false)));
        assert!(motion.reduced());
        assert!(
            !motion.refresh(start + Duration::from_millis(200), Some(false)),
            "an unchanged answer waits out the second"
        );
        assert!(motion.refresh(start + Duration::from_millis(300), Some(true)));
        assert!(!motion.reduced());
    }

    #[test]
    fn named_curves_match_the_browser() {
        // What a browser gives for these curves halfway through.
        assert!((ease::EASE.at(0.5) - 0.8024).abs() < 1e-3);
        assert!((ease::EASE_IN_OUT.at(0.5) - 0.5).abs() < 1e-3);
        assert!((ease::EASE_IN.at(0.5) - 0.3153).abs() < 1e-3);
        assert!((ease::EASE_OUT.at(0.5) - 0.6847).abs() < 1e-3);
        assert_eq!(ease::LINEAR.at(0.37), 0.37);
        for curve in [ease::ENTER, ease::EXIT, ease::PANEL, ease::DIALOG] {
            assert_eq!(curve.at(0.0), 0.0);
            assert_eq!(curve.at(1.0), 1.0);
            let mut last = 0.0;
            for step in 1..=20 {
                let value = curve.at(step as f32 / 20.0);
                assert!(value >= last - 1e-4, "{curve:?} runs backwards");
                last = value;
            }
        }
        // The panel curve rushes out: most of the way there by a third.
        assert!(ease::PANEL.at(0.33) > 0.8);
    }

    #[test]
    fn a_delay_holds_the_start_in_both_directions() {
        let transition = Transition::new(0.5, ease::LINEAR).delayed(0.05);
        let total = 0.55;
        assert_eq!(eased(0.04 / total, true, transition), 0.0);
        assert!((eased(0.30 / total, true, transition) - 0.5).abs() < 1e-3);
        assert_eq!(eased(1.0 - 0.04 / total, false, transition), 1.0);
        assert!((eased(1.0 - 0.30 / total, false, transition) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn with_less_motion_values_jump_to_the_end() {
        let ctx = egui::Context::default();
        apply(&ctx, true);
        let id = egui::Id::new("jump");
        assert_eq!(progress(&ctx, id, true, tidal::PANEL), 1.0);
        assert_eq!(progress(&ctx, id, false, tidal::PANEL), 0.0);
        let id = egui::Id::new("value");
        assert_eq!(value(&ctx, id, 1.0, tidal::PRESS), 1.0);
        assert_eq!(value(&ctx, id, 0.9, tidal::PRESS), 0.9);
    }

    #[test]
    fn a_value_starts_where_it_is() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("value");
        assert_eq!(
            value(&ctx, id, 1.0, tidal::PRESS),
            1.0,
            "first sight sits still"
        );
        let moving = value(&ctx, id, 2.0, tidal::PRESS);
        assert_eq!(moving, 1.0, "no time has passed yet");
    }

    #[test]
    fn reduced_motion_turns_egui_easing_off() {
        let ctx = egui::Context::default();
        apply(&ctx, true);
        assert_eq!(ctx.global_style().animation_time, 0.0);
        assert_eq!(
            ctx.global_style().scroll_animation,
            egui::style::ScrollAnimation::none()
        );
        apply(&ctx, false);
        assert_eq!(ctx.global_style().animation_time, ANIMATION_TIME);
        assert_eq!(
            ctx.global_style().scroll_animation,
            egui::style::ScrollAnimation::default()
        );
    }

    /// The views' own animations read the same preference: none with less
    /// motion, their own length otherwise.
    #[test]
    fn the_views_own_animations_follow_the_preference() {
        let ctx = egui::Context::default();
        assert_eq!(seconds(&ctx, 0.3), 0.3, "motion until told otherwise");
        apply(&ctx, true);
        assert_eq!(seconds(&ctx, 0.3), 0.0);
        assert_eq!(scroll(&ctx, 0.45), egui::style::ScrollAnimation::none());
        apply(&ctx, false);
        assert_eq!(seconds(&ctx, 0.3), 0.3);
        assert_eq!(
            scroll(&ctx, 0.45),
            egui::style::ScrollAnimation::duration(0.45)
        );
    }
}
