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

/// The scale the box's running animations give it now, when one of them
/// moves its `scale` (the cotton candy family's and Sugarcoat's `bounc`,
/// `0% { scale: 1 } 38% { scale: 1.044 } 100% { scale: .96 }` over a
/// fifth of a second as the pointer comes onto a button) or the scale of
/// its `transform`. As in Chromium a `scale` frame scales on top of the
/// box's own `transform` (`own`, the factor that has), a `transform` frame
/// takes its place; a frame without one stands at the box's own value. A
/// frame is asked for every sixtieth of the animation (16 to 250 ms).
pub fn scale(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
    own: f32,
) -> Option<f32> {
    let mut found = None;
    running(
        ctx,
        key,
        style,
        keyframes,
        |dur| ((dur * 1000.0 / 60.0) as u64).clamp(16, 250),
        |frames, a, p| {
            let property = frames
                .iter()
                .any(|(_, s)| s.custom.as_ref().is_some_and(|c| c.contains_key("scale")));
            let frames: Vec<_> = frames
                .iter()
                .filter_map(|(offset, s)| {
                    let k = if property {
                        s.custom
                            .as_ref()?
                            .get("scale")
                            .and_then(|v| scale_property(v))?
                    } else {
                        s.transform.as_deref().and_then(transform_scale)?
                    };
                    Some((*offset, k, css_motion::keyframe_timing(s)))
                })
                .collect();
            if frames.is_empty() {
                return false;
            }
            let base = if property { 1.0 } else { own };
            let k = css_motion::Track { frames }.at(p, &base, &a.timing);
            found = Some(if property { k * own } else { k });
            true
        },
    );
    found
}

/// The factor of a `scale` property's value (`1.044`, `110%`, `1.1 1.1`;
/// two different axes are taken as their mean, as for a box's
/// `transform`), none for `none`.
pub fn scale_property(value: &str) -> Option<f32> {
    let mut axes = value.split_whitespace().map(|a| match a.strip_suffix('%') {
        Some(p) => p.parse::<f32>().ok().map(|n| n / 100.0),
        None => a.parse::<f32>().ok(),
    });
    let x = axes.next()??;
    let y = axes.next().flatten().unwrap_or(x);
    let k = (x + y) / 2.0;
    k.is_finite().then_some(k)
}

/// The scale of a `transform` value: the product of its `scale()` and
/// `scale3d()` (two different axes taken as their mean); none when it has
/// no scale.
pub fn transform_scale(transform: &str) -> Option<f32> {
    let lower = transform.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut total: Option<f32> = None;
    while let Some(open) = rest.find('(') {
        let name = rest[..open]
            .trim()
            .rsplit([' ', ')'])
            .next()
            .unwrap_or("")
            .trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args = &rest[open + 1..open + close];
        if matches!(name, "scale" | "scale3d") {
            let axes: Vec<&str> = args
                .split([',', ' '])
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .take(2)
                .collect();
            if let Some(k) = scale_property(&axes.join(" ")) {
                total = Some(total.unwrap_or(1.0) * k);
            }
        }
        rest = &rest[open + close + 1..];
    }
    total
}

/// The values the box's running animations give the custom properties
/// `@property` registers with a type (Cotton Candy Nights' stars,
/// `@property --star1 { syntax: '<percentage>'; initial-value: 0% }` and
/// `@keyframes blink { 50% { --star1: 100% } }`), as CSS text, for
/// `var()` to read in place of the initial value: numbers with a unit
/// (the same in every keyframe) and colours, eased as a browser eases a
/// typed property. Angles are left to [`angle`] (the gradients they turn
/// keep their `var()`), unregistered properties to the cascade (CSS
/// flips them, it does not ease them). `vars` holds the registrations
/// (the runtime's `@property --name` entries). A frame is asked for every
/// fiftieth of the animation (between 33 and 250 ms): a star fading in
/// over five seconds changes by four percent a step, five times a second.
pub fn custom_values(
    ctx: &egui::Context,
    key: &str,
    style: &Style,
    keyframes: &BTreeMap<String, Vec<(f32, Style)>>,
    vars: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    running(
        ctx,
        key,
        style,
        keyframes,
        |dur| ((dur * 20.0) as u64).clamp(33, 250),
        |frames, a, p| {
            let mut names: Vec<&str> = frames
                .iter()
                .filter_map(|(_, s)| s.custom.as_ref())
                .flat_map(|c| c.keys())
                .map(String::as_str)
                .filter(|n| n.starts_with("--"))
                .collect();
            names.sort_unstable();
            names.dedup();
            let mut moved = false;
            for name in names {
                let Some(initial) = vars.get(&format!("@property {name}")) else {
                    continue;
                };
                let values = frames.iter().filter_map(|(offset, s)| {
                    let v = s.custom.as_ref()?.get(name)?;
                    Some((*offset, v.as_str(), css_motion::keyframe_timing(s)))
                });
                let value = if let Some((own, unit)) = number(initial) {
                    let frames: Option<Vec<_>> = values
                        .map(|(o, v, t)| match number(v) {
                            Some((n, u)) if u == unit => Some((o, n, t)),
                            _ => None,
                        })
                        .collect();
                    let Some(frames) = frames.filter(|f| !f.is_empty()) else {
                        continue;
                    };
                    let n = css_motion::Track { frames }.at(p, &own, &a.timing);
                    format!("{}{unit}", (n * 1000.0).round() / 1000.0)
                } else if let Some(own) = super::plugin_slots::css::color(initial) {
                    let frames: Option<Vec<_>> = values
                        .map(|(o, v, t)| Some((o, super::plugin_slots::css::color(v)?, t)))
                        .collect();
                    let Some(frames) = frames.filter(|f| !f.is_empty()) else {
                        continue;
                    };
                    let [r, g, b, alpha] = css_motion::Track { frames }
                        .at(p, &own, &a.timing)
                        .to_srgba_unmultiplied();
                    format!("rgba({r}, {g}, {b}, {:.3})", f32::from(alpha) / 255.0)
                } else {
                    continue;
                };
                out.retain(|(n, _)| n != name);
                out.push((name.to_string(), value));
                moved = true;
            }
            moved
        },
    );
    out
}

/// A CSS number and its unit (`5%`, `.7px`, `2`); none for angles, which
/// [`angle`] turns, and for anything else.
fn number(value: &str) -> Option<(f32, &str)> {
    let v = value.trim();
    let at = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(v.len());
    // `e` of a unit (`em`) is not an exponent.
    let at = if v[..at].ends_with(['e', 'E']) && !v[at..].is_empty() {
        at - 1
    } else {
        at
    };
    let n = v[..at].parse::<f32>().ok()?;
    let unit = v[at..].trim();
    let angle = ["deg", "turn", "rad", "grad"].contains(&unit.to_ascii_lowercase().as_str());
    let plain = unit.is_empty() || unit == "%" || unit.bytes().all(|b| b.is_ascii_alphabetic());
    (!angle && plain).then_some((n, unit))
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
        if crate::motion::still() {
            if let Some(moment) = a.settled() {
                use_frames(frames, a, moment.progress);
            }
            continue;
        }
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

    /// The theme parity checker's demo shots (`--demo-show still`) show a
    /// box as its browser leaves TIDAL's page once it settles it: an
    /// endless animation cancelled (the box's own position), a finite one
    /// finished (its last frame, kept by `forwards`). Before, the 11 themes
    /// whose whole page animates could not be compared: each side showed a
    /// frame of its own.
    #[test]
    fn a_still_shot_shows_animations_as_a_settled_page_does() {
        let own = Style {
            background_position: Some("20% 50%".into()),
            ..style()
        };
        let finite = Style {
            animation: Some("gradient 30 0 1 normal linear running forwards".into()),
            ..own.clone()
        };
        let at = |style: &Style| {
            let ctx = ctx_at(12.0);
            let mut out = None;
            let mut o = ctx.run_ui(
                egui::RawInput {
                    time: Some(12.5),
                    ..Default::default()
                },
                |ui| out = background_position(ui.ctx(), "tile-item:0", style, &frames()),
            );
            o.textures_delta.clear();
            out
        };
        let moving = at(&own);
        crate::motion::set_still(true);
        let still = [at(&own), at(&finite)];
        crate::motion::set_still(false);
        assert!(moving.is_some(), "an endless drift moves the box");
        assert_eq!(still, [None, Some((0.0, 0.5))]);
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

    /// The `bounc` keyframes (`0% { scale: 1 } 38% { scale: 1.044 } 100% {
    /// scale: .96 }`, once over a fifth of a second) scale a box on top of
    /// its own `transform: scale(.7)`, as in Chromium; a `transform` frame
    /// takes the box's own scale's place; once over, nothing is moved.
    #[test]
    fn a_scale_keyframe_scales_on_top_of_the_boxs_own_transform() {
        let frame = |offset: f32, scale: Option<&str>, transform: Option<&str>| {
            (
                offset,
                Style {
                    transform: transform.map(str::to_string),
                    custom: scale.map(|v| BTreeMap::from([("scale".to_string(), v.to_string())])),
                    ..Style::default()
                },
            )
        };
        let keyframes = BTreeMap::from([
            (
                "bounc".to_string(),
                vec![
                    frame(0.0, Some("1.0"), Some("scale(1.0)")),
                    frame(0.38, Some("1.044"), Some("scale(1.044)")),
                    frame(1.0, Some(".96"), Some("scale(.96)")),
                ],
            ),
            (
                "grow".to_string(),
                vec![
                    frame(0.0, None, Some("scale(1)")),
                    frame(1.0, None, Some("scale(2)")),
                ],
            ),
        ]);
        let style = |name: &str| Style {
            transform: Some("scale(.7)".into()),
            animation: Some(format!("{name} 0.2 0 1 normal linear running none")),
            ..Style::default()
        };
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        for time in [10.0, 10.076, 10.19, 10.5] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    seen.push((
                        scale(ui.ctx(), "button", &style("bounc"), &keyframes, 0.7),
                        scale(ui.ctx(), "pill", &style("grow"), &keyframes, 0.7),
                    ));
                },
            );
            out.textures_delta.clear();
        }
        let near = |got: Option<f32>, want: f32| got.is_some_and(|g| (g - want).abs() < 0.005);
        assert!(near(seen[0].0, 0.7), "{seen:?}");
        assert!(near(seen[1].0, 0.7 * 1.044), "{seen:?}");
        assert!(near(seen[2].0, 0.7 * 0.967), "{seen:?}");
        assert!(near(seen[1].1, 1.38), "transform frames replace: {seen:?}");
        assert_eq!(seen[3], (None, None), "over");
        assert_eq!(transform_scale("scale(.98) translateX(-.1em)"), Some(0.98));
        assert_eq!(transform_scale("scale(2) scale(1.5, 1.5)"), Some(3.0));
        assert_eq!(transform_scale("translateY(2px)"), None);
        assert_eq!(scale_property("110%"), Some(1.1));
    }

    /// A registered colour or length eases between its keyframes; an angle,
    /// an unregistered property and keyframes in another unit are left out.
    #[test]
    fn registered_colours_and_lengths_ease_and_the_rest_is_left_out() {
        let frame = |offset: f32, pairs: &[(&str, &str)]| {
            (
                offset,
                Style {
                    custom: Some(
                        pairs
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                    ),
                    ..Style::default()
                },
            )
        };
        let keyframes = BTreeMap::from([(
            "glow".to_string(),
            vec![
                frame(
                    0.0,
                    &[
                        ("--tint", "rgb(0, 0, 0)"),
                        ("--gap", "0px"),
                        ("--turn", "0deg"),
                        ("--free", "1px"),
                        ("--mixed", "0px"),
                    ],
                ),
                frame(
                    1.0,
                    &[
                        ("--tint", "rgb(200, 100, 50)"),
                        ("--gap", "10px"),
                        ("--turn", "90deg"),
                        ("--free", "3px"),
                        ("--mixed", "1em"),
                    ],
                ),
            ],
        )]);
        let vars = BTreeMap::from(
            [
                ("@property --tint", "black"),
                ("@property --gap", "0px"),
                ("@property --turn", "0deg"),
                ("@property --mixed", "0px"),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string())),
        );
        let style = Style {
            animation: Some("glow 10 0 infinite normal linear running".into()),
            ..Style::default()
        };
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        for time in [0.0, 5.0] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| seen.push(custom_values(ui.ctx(), "box", &style, &keyframes, &vars)),
            );
            out.textures_delta.clear();
        }
        let half: BTreeMap<String, String> = seen[1].iter().cloned().collect();
        assert_eq!(
            half.get("--gap").map(String::as_str),
            Some("5px"),
            "{half:?}"
        );
        let tint = half
            .get("--tint")
            .and_then(|v| super::super::plugin_slots::css::color(v));
        assert_eq!(tint, Some(egui::Color32::from_rgb(100, 50, 25)), "{half:?}");
        assert_eq!(half.len(), 2, "{half:?}");
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
