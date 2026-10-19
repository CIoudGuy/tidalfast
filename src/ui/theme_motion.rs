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

use crate::luna::dom::Style;

/// Where the box's background sits, as fractions of the room around it
/// (`0%` is the start, `100%` the end), per axis.
pub type Position = (f32, f32);

/// A `background-position` value (`0% 50%`, `left top`, `center`) as
/// fractions; none for lengths, which move with the box's size.
pub fn parse_position(value: &str) -> Option<Position> {
    let lower = value.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let axis = |w: &str| match w {
        "left" | "top" => Some(0.0),
        "center" => Some(0.5),
        "right" | "bottom" => Some(1.0),
        w => Some(w.strip_suffix('%')?.trim().parse::<f32>().ok()? / 100.0),
    };
    match words.as_slice() {
        [x] => Some((axis(x)?, 0.5)),
        [x, y] => Some((axis(x)?, axis(y)?)),
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
    for entry in list.split(',') {
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
        |frames, p| {
            let moves: Vec<(f32, Position)> = frames
                .iter()
                .filter_map(|(offset, s)| {
                    Some((*offset, parse_position(s.background_position.as_deref()?)?))
                })
                .collect();
            if moves.is_empty() {
                return false;
            }
            found = Some(at(&moves, own, p));
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
        |frames, p| {
            let moves: Vec<(f32, f32)> = frames
                .iter()
                .filter_map(|(offset, s)| {
                    let value = s.custom.as_ref()?.get(name)?;
                    Some((*offset, super::theme_gradient::angle(value)?))
                })
                .collect();
            if moves.is_empty() {
                return false;
            }
            found = Some(at1(&moves, initial, p));
            true
        },
    );
    found
}

/// Runs `use_frames(keyframes, progress)` for each running animation of
/// `style` (progress eased, 0 to 1); when it returns true the animation is
/// the one that moves what the caller draws, and a frame is asked for
/// every `interval_ms(duration in seconds)` while it runs.
fn running(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
    interval_ms: impl Fn(f64) -> u64,
    mut use_frames: impl FnMut(&[(f32, Style)], f32) -> bool,
) {
    let Some(spec) = style.animation.as_deref() else {
        return;
    };
    let now = ctx.input(|i| i.time);
    let reduced = crate::motion::reduced(ctx);
    for a in spec.split(',') {
        let f: Vec<&str> = a.split(' ').collect();
        let [name, dur, delay, count, dir, timing, state] = f.as_slice() else {
            continue;
        };
        let Some(frames) = keyframes.get(*name) else {
            continue;
        };
        let dur = dur.parse::<f64>().unwrap_or(0.0).max(0.001);
        let delay = delay.parse::<f64>().unwrap_or(0.0);
        let paused = *state == "paused";
        let id = egui::Id::new(("theme-anim", key, *name));
        let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
        let elapsed = if paused || reduced {
            0.0
        } else {
            (now - start - delay).max(0.0)
        };
        let total = if *count == "infinite" {
            f64::INFINITY
        } else {
            count.parse::<f64>().unwrap_or(1.0)
        };
        let iterations = elapsed / dur;
        let finished = iterations >= total;
        let (iteration, p) = if finished {
            ((total.ceil() - 1.0).max(0.0), 1.0)
        } else {
            (iterations.floor(), iterations.fract())
        };
        let odd = iteration as u64 % 2 == 1;
        let p = match *dir {
            "reverse" => 1.0 - p,
            "alternate" if odd => 1.0 - p,
            "alternate-reverse" if !odd => 1.0 - p,
            _ => p,
        } as f32;
        let p = super::plugin_slots::ease(timing, p);
        if use_frames(frames, p) && !finished && !paused && !reduced {
            let interval = std::time::Duration::from_millis(interval_ms(dur));
            crate::theme::ambient_frame_every(ctx, interval);
        }
    }
}

/// The value at progress `p` of keyframes `moves` (offset, value); `own`
/// stands in for a missing `from` or `to`.
fn at1(moves: &[(f32, f32)], own: f32, p: f32) -> f32 {
    let first = moves[0];
    let last = moves[moves.len() - 1];
    let (a, b) = if p <= first.0 {
        (if first.0 > 0.0 { (0.0, own) } else { first }, first)
    } else if p >= last.0 {
        (last, if last.0 < 1.0 { (1.0, own) } else { last })
    } else {
        let i = moves.iter().rposition(|(o, _)| *o <= p).unwrap_or(0);
        (moves[i], moves[(i + 1).min(moves.len() - 1)])
    };
    let span = (b.0 - a.0).max(1e-4);
    let t = ((p - a.0) / span).clamp(0.0, 1.0);
    a.1 + (b.1 - a.1) * t
}

/// The position at progress `p` of keyframes `moves` (offset, position);
/// the box's own position stands in for a missing `from` or `to`.
fn at(moves: &[(f32, Position)], own: Position, p: f32) -> Position {
    let x: Vec<(f32, f32)> = moves.iter().map(|(o, v)| (*o, v.0)).collect();
    let y: Vec<(f32, f32)> = moves.iter().map(|(o, v)| (*o, v.1)).collect();
    (at1(&x, own.0, p), at1(&y, own.1, p))
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
