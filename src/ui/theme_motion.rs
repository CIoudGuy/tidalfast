//! CSS animations on the boxes a theme paints (not the elements plugins
//! draw, which [`super::plugin_slots`] animates): the cotton candy tiles'
//! colour drift, `animation: gradient 30s infinite` over `@keyframes` that
//! move `background-position`.
//!
//! It costs nothing unless a painted box has an animation: the sampling
//! is asked only for boxes that are on screen, a frame is requested only
//! while one is running, at the pace a slow drift needs (a hundredth of
//! its length, between 33 and 250 ms), at half that pace when the window is
//! out of focus or saving energy, and never when nobody can see the window.
//! With less motion asked for by the desktop the box stays at the start of
//! its animation.

use std::collections::BTreeMap;

use super::css_motion;
use crate::luna::dom::Style;

/// Where the box's background sits, as fractions of the room around it
/// (`0%` is the start, `100%` the end), per axis.
pub type Position = (f32, f32);

/// A CSS `<position>` (a `background-position`, a radial gradient's `at`)
/// as fractions, read as CSS reads it: one word (`top` is the middle of the
/// top edge), two keywords in either order (`center left`, `top right`),
/// a percentage and a keyword in the order CSS allows, and four words that
/// give offsets from edges (`right 10% bottom 20%`). None for lengths,
/// which move with the box's size, and for what CSS rejects.
pub fn parse_position(value: &str) -> Option<Position> {
    /// One word of a position: a keyword of one axis, `center`, or a
    /// percentage.
    #[derive(Clone, Copy)]
    enum Word {
        X(f32),
        Y(f32),
        Center,
        Share(f32),
    }
    use Word::{Center, Share, X, Y};
    let lower = value.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let word = |w: &str| match w {
        "left" => Some(X(0.0)),
        "right" => Some(X(1.0)),
        "top" => Some(Y(0.0)),
        "bottom" => Some(Y(1.0)),
        "center" => Some(Center),
        w => Some(Share(
            w.strip_suffix('%')?.trim().parse::<f32>().ok()? / 100.0,
        )),
    };
    match words.as_slice() {
        [one] => match word(one)? {
            X(x) | Share(x) => Some((x, 0.5)),
            Y(y) => Some((0.5, y)),
            Center => Some((0.5, 0.5)),
        },
        [first, second] => match (word(first)?, word(second)?) {
            (X(x) | Share(x), Y(y) | Share(y)) | (Y(y), X(x)) => Some((x, y)),
            (X(x) | Share(x), Center) | (Center, X(x)) => Some((x, 0.5)),
            (Center, Y(y) | Share(y)) | (Y(y), Center) => Some((0.5, y)),
            (Center, Center) => Some((0.5, 0.5)),
            _ => None,
        },
        // Offsets from edges: `right 10%` is 10 % in from the right.
        [k1, v1, k2, v2] => {
            let from = |edge: Word, by: &str| match (edge, word(by)?) {
                (X(e) | Y(e), Share(v)) => Some(if e == 0.0 { v } else { 1.0 - v }),
                _ => None,
            };
            match (word(k1)?, word(k2)?) {
                (a @ X(_), b @ Y(_)) => Some((from(a, v1)?, from(b, v2)?)),
                (a @ Y(_), b @ X(_)) => Some((from(b, v2)?, from(a, v1)?)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The transition a style asks for `property` (the entry naming it, else
/// `all`), from the runtime's `property seconds delay timing` list. Steps
/// run as a linear ease.
pub fn transition_for(style: &Style, property: &str) -> Option<crate::motion::Transition> {
    use crate::motion::{Bezier, Transition, ease};
    let list = style.transition.as_deref()?;
    let mut all = None;
    for entry in super::plugin_slots::css::split_top(list, ',') {
        let words: Vec<&str> = entry.split(' ').collect();
        let [name, seconds, delay, timing] = words.as_slice() else {
            continue;
        };
        let Ok(seconds) = seconds.parse::<f32>() else {
            continue;
        };
        let delay = delay.parse::<f32>().unwrap_or(0.0);
        let curve = match *timing {
            "ease" => ease::EASE,
            "ease-in" => ease::EASE_IN,
            "ease-out" => ease::EASE_OUT,
            "ease-in-out" => ease::EASE_IN_OUT,
            t if t.starts_with("cubic-bezier(") => {
                let n: Vec<f32> = t
                    .trim_start_matches("cubic-bezier(")
                    .trim_end_matches(')')
                    .split(',')
                    .filter_map(|v| v.parse().ok())
                    .collect();
                match n.as_slice() {
                    [a, b, c, d] => Bezier(*a, *b, *c, *d),
                    _ => ease::EASE,
                }
            }
            _ => ease::LINEAR,
        };
        let transition =
            Transition::new(seconds.clamp(0.0, 30.0), curve).delayed(delay.clamp(0.0, 30.0));
        if *name == property {
            return Some(transition);
        }
        if *name == "all" {
            all = Some(transition);
        }
    }
    all
}

/// Where the background of the box `key` (with `style`) is now, if one of
/// its running animations moves it. Asks for the next frame while it runs.
pub fn background_position(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
) -> Option<Position> {
    let own = style
        .background_position
        .as_deref()
        .and_then(parse_position)
        .unwrap_or((0.0, 0.0));
    let mut found = None;
    running(
        ctx,
        key,
        style,
        keyframes,
        |dur| ((dur * 10.0) as u64).clamp(33, 250),
        |frames, a, p| {
            let axis = |pick: fn(Position) -> f32| {
                let frames: Vec<_> = frames
                    .iter()
                    .filter_map(|(offset, s)| {
                        let at = parse_position(s.background_position.as_deref()?)?;
                        Some((*offset, pick(at), css_motion::keyframe_timing(s)))
                    })
                    .collect();
                (!frames.is_empty()).then_some(css_motion::Track { frames })
            };
            let (Some(x), Some(y)) = (axis(|p| p.0), axis(|p| p.1)) else {
                return false;
            };
            found = Some((x.at(p, &own.0, &a.timing), y.at(p, &own.1, &a.timing)));
            true
        },
    );
    found
}

/// The angle (degrees) the custom property `name` has now, when one of the
/// box's running animations moves it in its keyframes (`--rotate` of the
/// cotton candy family's gradients); `initial` stands in for a missing
/// `from` or `to`. A step of the turn is about six degrees at most, so a
/// frame is asked for as often as that takes (between 33 and 250 ms).
pub fn angle(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
    name: &str,
    initial: f32,
) -> Option<f32> {
    let mut found = None;
    running(
        ctx,
        key,
        style,
        keyframes,
        |dur| ((dur * 1000.0 / 60.0) as u64).clamp(33, 250),
        |frames, a, p| {
            let frames: Vec<_> = frames
                .iter()
                .filter_map(|(offset, s)| {
                    let value = s.custom.as_ref()?.get(name)?;
                    Some((
                        *offset,
                        super::theme_gradient::angle(value)?,
                        css_motion::keyframe_timing(s),
                    ))
                })
                .collect();
            if frames.is_empty() {
                return false;
            }
            found = Some(css_motion::Track { frames }.at(p, &initial, &a.timing));
            true
        },
    );
    found
}

/// Runs `use_frames(keyframes, animation, progress)` for each animation of
/// `style` that shows now (progress 0 to 1 with its direction, before the
/// keyframes' easing); when it returns true the animation is the one that
/// moves what the caller draws, and a frame is asked for every
/// `interval_ms(duration in seconds)` while it runs. Fill modes, delays,
/// pauses and less motion are as [`css_motion::Animation`] has them.
fn running(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
    interval_ms: impl Fn(f64) -> u64,
    mut use_frames: impl FnMut(&[(f32, Style)], &css_motion::Animation, f32) -> bool,
) {
    if style.animation.is_none() {
        return;
    }
    let list = css_motion::animations(style);
    let now = ctx.input(|i| i.time);
    let reduced = crate::motion::reduced(ctx);
    for a in &list {
        let Some(frames) = keyframes.get(&a.name) else {
            continue;
        };
        let id = egui::Id::new(("theme-anim", key, a.name.as_str()));
        let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
        let moment = if reduced {
            a.at_rest()
        } else if a.paused {
            a.at(a.delay.max(0.0))
        } else {
            a.at(now - start)
        };
        let Some(moment) = moment else {
            // Waiting out its delay: look again when it ends.
            let wait = a.delay - (now - start);
            if !reduced && !a.paused && wait > 0.0 {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.min(60.0)));
            }
            continue;
        };
        if use_frames(frames, a, moment.progress) && moment.running && !a.paused && !reduced {
            let interval = std::time::Duration::from_millis(interval_ms(a.seconds));
            crate::theme::ambient_frame_every(ctx, interval);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames() -> BTreeMap<String, Vec<(f32, Style)>> {
        let frame = |offset: f32, position: &str| {
            (
                offset,
                Style {
                    background_position: Some(position.into()),
                    ..Style::default()
                },
            )
        };
        BTreeMap::from([(
            "gradient".to_string(),
            vec![
                frame(0.0, "0% 50%"),
                frame(0.5, "100% 50%"),
                frame(1.0, "0% 50%"),
            ],
        )])
    }

    fn style() -> Style {
        Style {
            animation: Some("gradient 30 0 infinite normal linear running".into()),
            ..Style::default()
        }
    }

    fn ctx_at(time: f64) -> egui::Context {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |_| {},
        );
        out.textures_delta.clear();
        ctx
    }

    #[test]
    fn a_style_names_the_transition_of_a_property_or_of_all() {
        let style = Style {
            transition: Some("opacity 0.6 0 ease-in-out,all 2 0.5 linear".into()),
            ..Style::default()
        };
        let opacity = transition_for(&style, "opacity").expect("opacity has its own");
        assert_eq!((opacity.seconds, opacity.delay), (0.6, 0.0));
        let transform = transition_for(&style, "transform").expect("all covers the rest");
        assert_eq!((transform.seconds, transform.delay), (2.0, 0.5));
        assert_eq!(transition_for(&Style::default(), "opacity"), None);
        let only = Style {
            transition: Some("color 1 0 ease".into()),
            ..Style::default()
        };
        assert_eq!(transition_for(&only, "opacity"), None);
    }

    #[test]
    fn positions_read_percentages_and_keywords() {
        assert_eq!(parse_position("0% 50%"), Some((0.0, 0.5)));
        assert_eq!(parse_position("right top"), Some((1.0, 0.0)));
        assert_eq!(parse_position("center"), Some((0.5, 0.5)));
        assert_eq!(parse_position("12px 4px"), None);
    }

    /// CSS reads two keywords in either order and one keyword as an edge's
    /// middle; Chromium's computed `background-position` for each value is
    /// the expected pair (`center left` is `0% 50%`). These read the first
    /// word as x and the second as y: `top right` was the left edge's
    /// bottom.
    #[test]
    fn positions_read_keywords_in_either_order_as_css_does() {
        for (value, at) in [
            ("center left", (0.0, 0.5)),
            ("center right", (1.0, 0.5)),
            ("top right", (1.0, 0.0)),
            ("bottom left", (0.0, 1.0)),
            ("top center", (0.5, 0.0)),
            ("top", (0.5, 0.0)),
            ("bottom", (0.5, 1.0)),
            ("right", (1.0, 0.5)),
            ("25% top", (0.25, 0.0)),
            ("left 30%", (0.0, 0.3)),
            ("right 10% bottom 20%", (0.9, 0.8)),
            ("bottom 20% right 10%", (0.9, 0.8)),
        ] {
            assert_eq!(parse_position(value), Some(at), "{value}");
        }
        // What CSS rejects.
        for value in ["top 30%", "30% left", "left right", "top bottom"] {
            assert_eq!(parse_position(value), None, "{value}");
        }
    }

    #[test]
    fn the_drift_follows_the_keyframes_over_the_thirty_seconds() {
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        for time in [10.0, 25.0, 40.0, 55.0] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    seen.push(background_position(
                        ui.ctx(),
                        "tile-item:0",
                        &style(),
                        &frames(),
                    ));
                },
            );
            out.textures_delta.clear();
        }
        // Started at 10 s: 0, 15 s in (half way: the far end), a whole
        // turn, then 45 s in (half way into the next turn).
        let x: Vec<f32> = seen.iter().map(|p| p.expect("a position").0).collect();
        for (got, want) in x.iter().zip([0.0, 1.0, 0.0, 1.0]) {
            assert!((got - want).abs() < 1e-3, "{x:?}");
        }
    }

    /// `@property --rotate` with keyframes from 0 to 360 degrees (the cotton
    /// candy family's `rotate-gradient`) turns the angle with the time.
    #[test]
    fn a_custom_angle_turns_with_its_keyframes() {
        let frame = |offset: f32, value: &str| {
            (
                offset,
                Style {
                    custom: Some(BTreeMap::from([(
                        "--rotate".to_string(),
                        value.to_string(),
                    )])),
                    ..Style::default()
                },
            )
        };
        let keyframes = BTreeMap::from([(
            "spin".to_string(),
            vec![frame(0.0, "0deg"), frame(1.0, "1turn")],
        )]);
        let style = Style {
            animation: Some("spin 10 0 infinite normal linear running".into()),
            ..Style::default()
        };
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        for time in [10.0, 12.5, 15.0, 22.5] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    seen.push(angle(ui.ctx(), "pill", &style, &keyframes, "--rotate", 0.0));
                },
            );
            out.textures_delta.clear();
        }
        // Started at 10 s: 0 degrees, a quarter, half a turn, then a quarter
        // into the next turn.
        let got: Vec<f32> = seen.iter().map(|a| a.expect("an angle")).collect();
        for (got, want) in got.iter().zip([0.0, 90.0, 180.0, 90.0]) {
            assert!((got - want).abs() < 0.01, "{got:?}");
        }
        // Another property is not moved by it.
        let mut other = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            other = angle(ui.ctx(), "pill", &style, &keyframes, "--other", 0.0);
        });
        out.textures_delta.clear();
        assert_eq!(other, None);
    }

    #[test]
    fn a_box_without_a_moving_animation_asks_for_nothing() {
        let ctx = ctx_at(5.0);
        let mut none = Style::default();
        assert_eq!(background_position(&ctx, "a", &none, &frames()), None);
        none.animation = Some("missing 30 0 infinite normal linear running".into());
        assert_eq!(background_position(&ctx, "a", &none, &frames()), None);
    }

    #[test]
    fn less_motion_holds_the_start_and_asks_for_no_frames() {
        let ctx = egui::Context::default();
        crate::motion::apply(&ctx, true);
        for time in [1.0, 20.0] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    let p = background_position(ui.ctx(), "t", &style(), &frames()).unwrap();
                    assert_eq!(p, (0.0, 0.5), "at {time}");
                },
            );
            out.textures_delta.clear();
        }
    }
}
