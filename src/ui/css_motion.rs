//! CSS motion for what plugins and themes style: the timing functions
//! (`ease`, `cubic-bezier()`, `steps()`, `linear()`), transitions, and the
//! timeline of `@keyframes` animations, the way a browser runs them.
//!
//! A transition starts when the value the cascade gives a property changes
//! and the element's `transition` names the property (or `all`). It runs
//! from wherever the value is at that moment, so one interrupted halfway
//! (the pointer leaving before a hover fade finished) turns back from where
//! it got to, and going back to the value it came from takes only as long
//! as the way there took (CSS's reversing shortening factor).
//!
//! Nothing here asks for frames while nothing moves: a frame is asked for
//! only while a transition or an animation runs, once when a delay ends,
//! and, for `steps()`, once per step. With less motion asked for by the
//! desktop (see [`crate::motion`]) a transition is at its end at once, a
//! finite animation shows how it ends, and an endless one stays at its
//! start; the runtime also answers `prefers-reduced-motion: reduce` then, so
//! stylesheets that ask get what they wrote for it.

use egui::Color32;

use crate::luna::dom::Style;
use crate::motion::Bezier;

// ---------------------------------------------------------------- timing

/// Where a `steps()` function jumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    /// `jump-start`, `start`: the first step at the very beginning.
    Start,
    /// `jump-end`, `end` (the default): the last step at the very end.
    End,
    /// `jump-none`: neither; the first and last steps last a whole step.
    None,
    /// `jump-both`: both.
    Both,
}

/// A CSS `<easing-function>`.
#[derive(Clone, Debug, PartialEq)]
pub enum Timing {
    Bezier(Bezier),
    Steps(u32, Jump),
    /// `linear(...)`: (input, output) points in input order.
    Linear(Vec<(f32, f32)>),
}

impl Default for Timing {
    /// `ease`, what CSS runs when nothing is named.
    fn default() -> Self {
        Timing::Bezier(crate::motion::ease::EASE)
    }
}

impl Timing {
    /// Reads a timing function as the runtime writes it: lower case, no
    /// spaces around commas, other spaces as `_` (`cubic-bezier(.4,0,.2,1)`,
    /// `steps(4,jump-none)`, `linear(0,0.25_75%,1)`). Anything it does not
    /// know is `ease`, as an invalid value leaves CSS's initial one.
    pub fn parse(value: &str) -> Timing {
        use crate::motion::ease;
        let v = value.trim().to_ascii_lowercase();
        let args = |name: &str| -> Option<Vec<String>> {
            let inner = v.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?;
            Some(
                inner
                    .split(',')
                    .map(|a| a.replace('_', " ").trim().to_string())
                    .collect(),
            )
        };
        match v.as_str() {
            "linear" => return Timing::Bezier(ease::LINEAR),
            "ease" => return Timing::Bezier(ease::EASE),
            "ease-in" => return Timing::Bezier(ease::EASE_IN),
            "ease-out" => return Timing::Bezier(ease::EASE_OUT),
            "ease-in-out" => return Timing::Bezier(ease::EASE_IN_OUT),
            "step-start" => return Timing::Steps(1, Jump::Start),
            "step-end" => return Timing::Steps(1, Jump::End),
            _ => {}
        }
        if let Some(a) = args("cubic-bezier") {
            let n: Vec<f32> = a.iter().filter_map(|x| x.parse().ok()).collect();
            if let [x1, y1, x2, y2] = n.as_slice()
                && (0.0..=1.0).contains(x1)
                && (0.0..=1.0).contains(x2)
            {
                return Timing::Bezier(Bezier(*x1, *y1, *x2, *y2));
            }
            return Timing::default();
        }
        if let Some(a) = args("steps") {
            let count = a.first().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            let jump = match a.get(1).map(String::as_str) {
                Some("jump-start" | "start") => Jump::Start,
                Some("jump-none") => Jump::None,
                Some("jump-both") => Jump::Both,
                _ => Jump::End,
            };
            let least = if jump == Jump::None { 2 } else { 1 };
            return if count >= least {
                Timing::Steps(count, jump)
            } else {
                Timing::default()
            };
        }
        if let Some(a) = args("linear") {
            return linear_points(&a).map_or_else(Timing::default, Timing::Linear);
        }
        Timing::default()
    }

    /// The eased progress at `p` (0 to 1 of the interval).
    pub fn at(&self, p: f32) -> f32 {
        match self {
            Timing::Bezier(b) => b.at(p),
            Timing::Steps(n, jump) => {
                // CSS Easing 1, "step easing function".
                let n = *n as f32;
                let mut step = (p * n).floor();
                if matches!(jump, Jump::Start | Jump::Both) {
                    step += 1.0;
                }
                if p >= 0.0 && step < 0.0 {
                    step = 0.0;
                }
                let jumps = match jump {
                    Jump::Start | Jump::End => n,
                    Jump::None => n - 1.0,
                    Jump::Both => n + 1.0,
                };
                if p <= 1.0 && step > jumps {
                    step = jumps;
                }
                step / jumps
            }
            Timing::Linear(points) => {
                let first = points[0];
                let last = points[points.len() - 1];
                if p <= first.0 {
                    return first.1;
                }
                if p >= last.0 {
                    return last.1;
                }
                let i = points.iter().rposition(|(x, _)| *x <= p).unwrap_or(0);
                let (a, b) = (points[i], points[(i + 1).min(points.len() - 1)]);
                if b.0 <= a.0 {
                    return b.1;
                }
                a.1 + (b.1 - a.1) * (p - a.0) / (b.0 - a.0)
            }
        }
    }

    /// For `steps()`, how much later (as a share of the interval) than `p`
    /// the next step comes; none for smooth functions.
    pub fn next_step(&self, p: f32) -> Option<f32> {
        let Timing::Steps(n, _) = self else {
            return None;
        };
        let n = *n as f32;
        let next = ((p * n).floor() + 1.0) / n;
        Some((next - p).max(1e-4))
    }
}

/// `linear()`'s arguments: outputs with up to two input percentages each;
/// inputs left out are spread evenly between their neighbours.
fn linear_points(args: &[String]) -> Option<Vec<(f32, f32)>> {
    let mut points: Vec<(Option<f32>, f32)> = Vec::new();
    for a in args {
        let mut words = a.split_whitespace();
        let out: f32 = words.next()?.parse().ok()?;
        let percents: Vec<f32> = words
            .filter_map(|w| w.strip_suffix('%')?.parse::<f32>().ok())
            .map(|p| p / 100.0)
            .collect();
        match percents.as_slice() {
            [] => points.push((None, out)),
            [x] => points.push((Some(*x), out)),
            [x, y, ..] => {
                points.push((Some(*x), out));
                points.push((Some(*y), out));
            }
        }
    }
    if points.len() < 2 {
        return None;
    }
    let last = points.len() - 1;
    if points[0].0.is_none() {
        points[0].0 = Some(0.0);
    }
    if points[last].0.is_none() {
        points[last].0 = Some(1.0);
    }
    // Inputs never go backwards.
    let mut most = f32::MIN;
    for p in &mut points {
        if let Some(x) = p.0 {
            most = most.max(x);
            p.0 = Some(most);
        }
    }
    let mut i = 0;
    while i < points.len() {
        if points[i].0.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let end = (i..points.len()).find(|&j| points[j].0.is_some())?;
        let (a, b) = (points[start].0?, points[end].0?);
        let gaps = (end - start) as f32;
        for (k, p) in points.iter_mut().enumerate().take(end).skip(i) {
            p.0 = Some(a + (b - a) * (k - start) as f32 / gaps);
        }
        i = end;
    }
    Some(
        points
            .into_iter()
            .map(|(x, y)| (x.unwrap_or(0.0), y))
            .collect(),
    )
}

// ---------------------------------------------------------------- transitions

/// One `transition` entry that applies to a property.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub seconds: f32,
    pub delay: f32,
    pub timing: Timing,
}

/// Longhands the shorthands in a `transition-property` list stand for (only
/// the ones the renderer moves).
fn covers(name: &str, property: &str) -> bool {
    name == property
        || name == "all"
        || match name {
            "background" => property == "background-color",
            "border" | "border-color" => property == "border-color",
            // `translate`, `rotate` and `scale` are folded into `transform`
            // by the runtime.
            "translate" | "rotate" | "scale" | "-webkit-transform" => property == "transform",
            "-webkit-filter" => property == "filter",
            "inset" => matches!(property, "top" | "right" | "bottom" | "left"),
            _ => false,
        }
}

/// The transition `style` gives `property`, from the runtime's `property
/// seconds delay timing` list: as in CSS the last entry that names it (or
/// `all`, or a shorthand holding it) wins.
pub fn transition(style: &Style, property: &str) -> Option<Transition> {
    let list = style.transition.as_deref()?;
    let mut found = None;
    for entry in split_list(list) {
        let words: Vec<&str> = entry.split(' ').collect();
        let [name, seconds, delay, timing] = words.as_slice() else {
            continue;
        };
        if !covers(name, property) {
            continue;
        }
        let seconds = seconds.parse::<f32>().unwrap_or(0.0).clamp(0.0, 60.0);
        let delay = delay.parse::<f32>().unwrap_or(0.0).clamp(-60.0, 60.0);
        found = Some(Transition {
            seconds,
            delay,
            timing: Timing::parse(timing),
        });
    }
    // A combined duration of zero or less starts no transition.
    found.filter(|t| t.seconds + t.delay.max(0.0) > 0.0 && t.seconds > 0.0)
}

/// The entries of one of the runtime's lists: commas outside parentheses
/// (`cubic-bezier(.4,0,.2,1)` is one word).
fn split_list(list: &str) -> Vec<&str> {
    super::plugin_slots::css::split_top(list, ',')
}

/// A value a transition can blend.
pub trait Lerp: Clone + PartialEq + Send + Sync + 'static {
    fn lerp(&self, to: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        self + (to - self) * t
    }
}

impl Lerp for Color32 {
    /// Channel by channel on premultiplied sRGB, as browsers blend colours.
    fn lerp(&self, to: &Self, t: f32) -> Self {
        let l = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        let (a, b) = (self.to_array(), to.to_array());
        Color32::from_rgba_premultiplied(l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3]))
    }
}

#[derive(Clone, Debug)]
struct Run<V> {
    from: V,
    to: V,
    start: f64,
    seconds: f32,
    delay: f32,
    timing: Timing,
    /// CSS's reversing-adjusted start value: going back to it shortens.
    reversing_from: V,
    /// CSS's reversing shortening factor.
    factor: f32,
}

impl<V: Lerp> Run<V> {
    /// The value at `now`, how far the run is (0 to 1, before easing), and
    /// whether it is over.
    fn sample(&self, now: f64) -> (V, f32, bool) {
        let elapsed = (now - self.start) as f32 - self.delay;
        if elapsed < 0.0 {
            return (self.from.clone(), 0.0, false);
        }
        let t = elapsed / self.seconds.max(1e-6);
        if t >= 1.0 {
            return (self.to.clone(), 1.0, true);
        }
        (self.from.lerp(&self.to, self.timing.at(t)), t, false)
    }

    /// When to look again, in seconds from `now`: zero for the next frame.
    fn wake(&self, now: f64) -> f64 {
        let elapsed = (now - self.start) as f32 - self.delay;
        if elapsed < 0.0 {
            return f64::from(-elapsed);
        }
        let t = elapsed / self.seconds.max(1e-6);
        match self.timing.next_step(t) {
            Some(share) => f64::from(share * self.seconds),
            None => 0.0,
        }
    }
}

/// A property's value over time: where the cascade puts it, and the
/// transition taking it there.
#[derive(Clone, Debug)]
pub struct Eased<V> {
    target: V,
    run: Option<Run<V>>,
}

impl<V: Lerp> Eased<V> {
    /// A value first seen at `value`: as in CSS, nothing moves on first
    /// sight.
    pub fn new(value: V) -> Self {
        Self {
            target: value,
            run: None,
        }
    }

    /// Moves to `target` (the value the cascade gives now) with `spec`, the
    /// transition the after-change style gives the property, at `now`
    /// (seconds). Returns the value to draw and when to look again
    /// (seconds; zero for the next frame), or no wake while it stands still.
    pub fn step(&mut self, target: V, spec: Option<&Transition>, now: f64) -> (V, Option<f64>) {
        if target != self.target {
            let running = self.run.as_ref().filter(|r| !r.sample(now).2);
            let current = running.map_or_else(|| self.target.clone(), |r| r.sample(now).0);
            self.run = match spec {
                Some(spec) if current != target => {
                    // Back to where a running transition came from: it takes
                    // the share of its time the way there took.
                    let (factor, reversing_from) = match running {
                        Some(r) if r.reversing_from == target => {
                            let (_, t, _) = r.sample(now);
                            let f = (r.timing.at(t) * r.factor + 1.0 - r.factor)
                                .abs()
                                .clamp(0.0, 1.0);
                            (f, r.to.clone())
                        }
                        _ => (1.0, current.clone()),
                    };
                    let delay = if spec.delay < 0.0 {
                        spec.delay * factor
                    } else {
                        spec.delay
                    };
                    Some(Run {
                        from: current,
                        to: target.clone(),
                        start: now,
                        seconds: (spec.seconds * factor).max(1e-3),
                        delay,
                        timing: spec.timing.clone(),
                        reversing_from,
                        factor,
                    })
                }
                _ => None,
            };
            self.target = target;
        }
        let Some(run) = &self.run else {
            return (self.target.clone(), None);
        };
        let (value, _, done) = run.sample(now);
        if done {
            self.run = None;
            return (self.target.clone(), None);
        }
        let wake = run.wake(now);
        (value, Some(wake))
    }

    /// A transition is under way.
    pub fn moving(&self) -> bool {
        self.run.is_some()
    }
}

/// Asks `ctx` for a frame `wake` seconds from now (the next one for zero);
/// a plugin window's page takes it at the window's frame cap
/// ([`super::plugin_dom::request_frame_in`]).
pub fn wake(ctx: &egui::Context, wake: Option<f64>) {
    if let Some(s) = wake {
        super::plugin_dom::request_frame_in(ctx, s);
    }
}

/// [`Eased`] kept in egui's memory under `id`: the value of a property of a
/// drawn element, moving to `target` by `spec`. `from` is the value the
/// element had before it was first drawn (the style the runtime saw before
/// the change that brought it in), if any. An element not drawn for more
/// than a frame starts over at its value, as an element shown again does
/// in CSS.
pub fn eased<V: Lerp>(
    ctx: &egui::Context,
    id: egui::Id,
    target: V,
    spec: Option<&Transition>,
    from: Option<V>,
) -> V {
    #[derive(Clone)]
    struct Slot<V> {
        eased: Eased<V>,
        pass: u64,
    }
    let now = ctx.input(|i| i.time);
    let pass = ctx.cumulative_pass_nr();
    let spec = spec.filter(|_| !crate::motion::reduced(ctx));
    let (value, wake_in) = ctx.data_mut(|d| {
        let fresh = || Slot {
            eased: Eased::new(from.clone().unwrap_or_else(|| target.clone())),
            pass,
        };
        let slot = d.get_temp_mut_or_insert_with(id, fresh);
        if pass > slot.pass + 1 {
            *slot = fresh();
        }
        slot.pass = pass;
        slot.eased.step(target, spec, now)
    });
    wake(ctx, wake_in);
    value
}

// ---------------------------------------------------------------- animations

/// `animation-fill-mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    Forwards,
    Backwards,
    Both,
}

/// `animation-direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// One animation of an element, from the runtime's `name seconds delay
/// count direction timing state fill` list.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub seconds: f64,
    pub delay: f64,
    /// `infinite` is [`f64::INFINITY`].
    pub count: f64,
    pub direction: Direction,
    pub timing: Timing,
    pub paused: bool,
    pub fill: Fill,
}

/// The animations a style runs, in order (later ones win a property).
pub fn animations(style: &Style) -> Vec<Animation> {
    let Some(list) = style.animation.as_deref() else {
        return Vec::new();
    };
    split_list(list)
        .into_iter()
        .filter_map(|a| {
            let f: Vec<&str> = a.split(' ').collect();
            let (name, seconds, delay, count, dir, timing, state) = (
                f.first()?,
                f.get(1)?,
                f.get(2)?,
                f.get(3)?,
                f.get(4)?,
                f.get(5)?,
                f.get(6)?,
            );
            Some(Animation {
                name: name.to_string(),
                seconds: seconds.parse::<f64>().ok()?.max(0.0),
                delay: delay.parse::<f64>().unwrap_or(0.0),
                count: if *count == "infinite" {
                    f64::INFINITY
                } else {
                    count.parse::<f64>().unwrap_or(1.0).max(0.0)
                },
                direction: match *dir {
                    "reverse" => Direction::Reverse,
                    "alternate" => Direction::Alternate,
                    "alternate-reverse" => Direction::AlternateReverse,
                    _ => Direction::Normal,
                },
                timing: Timing::parse(timing),
                paused: *state == "paused",
                fill: match f.get(7).copied() {
                    Some("forwards") => Fill::Forwards,
                    Some("backwards") => Fill::Backwards,
                    Some("both") => Fill::Both,
                    _ => Fill::None,
                },
            })
        })
        .collect()
}

/// Where an animation is at a moment: the keyframe progress (0 to 1, the
/// direction applied, before the keyframes' easing) and whether it still
/// runs; none when it has no effect then (before its delay or after its end
/// without a fill mode that keeps a frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub progress: f32,
    pub running: bool,
}

impl Animation {
    /// The moment `elapsed` seconds after the animation started (its delay
    /// counts from there), CSS Animations' timeline.
    pub fn at(&self, elapsed: f64) -> Option<Moment> {
        let local = elapsed - self.delay;
        let active = self.seconds * self.count;
        let (iteration, p, running) = if local < 0.0 {
            if !matches!(self.fill, Fill::Backwards | Fill::Both) {
                return None;
            }
            (0.0, 0.0, true)
        } else if local >= active {
            if !matches!(self.fill, Fill::Forwards | Fill::Both) {
                return None;
            }
            // Where the last iteration ends (a fractional count ends inside
            // one).
            if self.seconds <= 0.0 || self.count <= 0.0 {
                (0.0, 0.0, false)
            } else {
                let frac = self.count.fract();
                if frac == 0.0 {
                    (self.count - 1.0, 1.0, false)
                } else {
                    (self.count.floor(), frac, false)
                }
            }
        } else {
            let n = local / self.seconds.max(1e-9);
            (n.floor(), n.fract(), true)
        };
        let odd = (iteration as u64) % 2 == 1;
        let reverse = match self.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => odd,
            Direction::AlternateReverse => !odd,
        };
        let p = if reverse { 1.0 - p } else { p } as f32;
        Some(Moment {
            progress: p,
            running,
        })
    }

    /// The moment a browser leaves once it settles the page as the theme
    /// parity checker does (`finish()`, `cancel()` when it cannot finish):
    /// a finite animation as it ends (nothing, without a fill mode that
    /// keeps its last frame), an endless one not applied
    /// ([`crate::motion::still`]).
    pub fn settled(&self) -> Option<Moment> {
        if self.count.is_infinite() {
            return None;
        }
        self.at(self.delay + self.seconds * self.count + 1.0)
    }

    /// The moment under less motion: a finite animation as it ends, an
    /// endless one at its start.
    pub fn at_rest(&self) -> Option<Moment> {
        if self.count.is_infinite() {
            let mut start = self.clone();
            start.fill = Fill::Both;
            start.delay = 0.0;
            return start.at(0.0).map(|m| Moment {
                running: false,
                ..m
            });
        }
        self.at(self.delay + self.seconds * self.count + 1.0)
    }
}

/// When each animation of an element started, kept between frames: an
/// animation newly in an element's list starts now, one that stays keeps
/// its start, and pausing holds the time it had reached.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    started: Vec<(String, f64, Option<f64>)>,
    pass: u64,
}

impl Clock {
    /// The seconds each of `list` has run at `now`, in order; `away` starts
    /// them all again (an element not drawn for a while, as a hidden
    /// element's animations restart when it shows).
    pub fn elapsed(&mut self, list: &[Animation], now: f64, away: bool) -> Vec<f64> {
        if away {
            self.started.clear();
        }
        let mut next = Vec::with_capacity(list.len());
        let mut out = Vec::with_capacity(list.len());
        for a in list {
            let (start, mut paused_at) = self
                .started
                .iter()
                .find(|(name, ..)| *name == a.name)
                .map_or((now, None), |(_, s, p)| (*s, *p));
            let mut start = start;
            if a.paused {
                paused_at.get_or_insert(now - start);
            } else if let Some(held) = paused_at.take() {
                start = now - held;
            }
            out.push(paused_at.unwrap_or(now - start));
            next.push((a.name.clone(), start, paused_at));
        }
        self.started = next;
        out
    }
}

/// The animations of the element `id` in egui's memory: each with the
/// moment it is at now, and asks for the frames they need (every frame
/// while a smooth one runs; at each step for `steps()`; when a delay ends).
pub fn moments(ctx: &egui::Context, id: egui::Id, list: &[Animation]) -> Vec<Option<Moment>> {
    let mut clock = ctx.data_mut(|d| d.get_temp::<Clock>(id).unwrap_or_default());
    let out = moments_with(ctx, &mut clock, list);
    ctx.data_mut(|d| d.insert_temp(id, clock));
    out
}

/// [`moments`] with the element's [`Clock`] kept by the caller.
pub fn moments_with(
    ctx: &egui::Context,
    clock: &mut Clock,
    list: &[Animation],
) -> Vec<Option<Moment>> {
    if crate::motion::still() {
        return list.iter().map(Animation::settled).collect();
    }
    let now = ctx.input(|i| i.time);
    let pass = ctx.cumulative_pass_nr();
    let reduced = crate::motion::reduced(ctx);
    let away = clock.pass > 0 && pass > clock.pass + 1;
    clock.pass = pass.max(1);
    let elapsed = clock.elapsed(list, now, away);
    let mut wake_in: Option<f64> = None;
    let mut soonest = |s: f64| wake_in = Some(wake_in.map_or(s, |w: f64| w.min(s)));
    // An endless animation (spinny-cover's turning cover, a twinkling sky)
    // is an endless decoration: drawn as the app's own are, thirty times a
    // second, fifteen out of focus and not at all while nobody sees the
    // window, rather than at the display's every refresh.
    let endless_frame = match crate::theme::budget(ctx) {
        crate::power::Budget::Normal => Some(crate::theme::AMBIENT_FRAME.as_secs_f64()),
        crate::power::Budget::Saver => Some(2.0 * crate::theme::AMBIENT_FRAME.as_secs_f64()),
        crate::power::Budget::Background => None,
    };
    let out = list
        .iter()
        .zip(elapsed)
        .map(|(a, e)| {
            if reduced {
                return a.at_rest();
            }
            let m = a.at(e);
            if a.paused {
                return m;
            }
            let local = e - a.delay;
            if local < 0.0 {
                soonest(-local);
            } else if m.is_some_and(|m| m.running) {
                let into = (local / a.seconds.max(1e-9)).fract() as f32;
                match (a.timing.next_step(into), a.count.is_infinite()) {
                    (Some(share), true) => {
                        if let Some(frame) = endless_frame {
                            soonest((f64::from(share) * a.seconds).max(frame));
                        }
                    }
                    (Some(share), false) => soonest(f64::from(share) * a.seconds),
                    (None, true) => {
                        if let Some(frame) = endless_frame {
                            soonest(frame);
                        }
                    }
                    (None, false) => soonest(0.0),
                }
            } else if local < a.seconds * a.count {
                soonest(0.0);
            }
            m
        })
        .collect();
    wake(ctx, wake_in);
    out
}

/// The keyframes of one property: `(offset, value)` in offset order, with
/// the easing each keyframe gives the interval after it.
pub struct Track<V> {
    pub frames: Vec<(f32, V, Option<Timing>)>,
}

impl<V: Lerp> Track<V> {
    /// The value at keyframe progress `p`. `own` stands in for a missing
    /// `0%` or `100%` keyframe (the element's value without the animation);
    /// `timing` eases each interval unless its keyframe names its own.
    pub fn at(&self, p: f32, own: &V, timing: &Timing) -> V {
        let mut frames: Vec<(f32, &V, Option<&Timing>)> = self
            .frames
            .iter()
            .map(|(o, v, t)| (*o, v, t.as_ref()))
            .collect();
        if frames.first().is_none_or(|f| f.0 > 0.0) {
            frames.insert(0, (0.0, own, None));
        }
        if frames.last().is_none_or(|f| f.0 < 1.0) {
            frames.push((1.0, own, None));
        }
        let p = p.clamp(0.0, 1.0);
        // The last keyframe at or before `p` (for equal offsets, the last).
        let i = frames.iter().rposition(|f| f.0 <= p).unwrap_or(0);
        if i + 1 >= frames.len() {
            return frames[i].1.clone();
        }
        let (a, b) = (&frames[i], &frames[i + 1]);
        let span = b.0 - a.0;
        if span <= 0.0 {
            return b.1.clone();
        }
        let t = (p - a.0) / span;
        let t = a.2.unwrap_or(timing).at(t);
        a.1.lerp(b.1, t)
    }
}

/// The per-keyframe easing the runtime keeps in a keyframe's
/// [`Style::custom`] (`animation-timing-function` written in a keyframe).
pub fn keyframe_timing(style: &Style) -> Option<Timing> {
    style
        .custom
        .as_ref()?
        .get("animation-timing-function")
        .map(|t| Timing::parse(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn named_functions_and_bezier_curves_match_the_browser() {
        assert!(close(Timing::parse("ease").at(0.5), 0.8024));
        assert!(close(Timing::parse("linear").at(0.37), 0.37));
        assert!(close(Timing::parse("ease-in-out").at(0.5), 0.5));
        assert!(close(
            Timing::parse("cubic-bezier(0.42,0,1,1)").at(0.5),
            0.3153
        ));
        // An x outside 0..1 is invalid: the initial `ease`.
        assert_eq!(Timing::parse("cubic-bezier(2,0,1,1)"), Timing::default());
        assert_eq!(Timing::parse("bogus"), Timing::default());
    }

    #[test]
    fn steps_jump_where_css_says() {
        let end = Timing::parse("steps(4)");
        assert_eq!(end.at(0.0), 0.0);
        assert_eq!(end.at(0.24), 0.0);
        assert_eq!(end.at(0.25), 0.25);
        assert_eq!(end.at(0.99), 0.75);
        assert_eq!(end.at(1.0), 1.0);
        let start = Timing::parse("steps(4,jump-start)");
        assert_eq!(start.at(0.0), 0.25);
        assert_eq!(start.at(0.5), 0.75);
        assert_eq!(Timing::parse("step-start").at(0.1), 1.0);
        assert_eq!(Timing::parse("step-end").at(0.9), 0.0);
        let none = Timing::parse("steps(5,jump-none)");
        assert_eq!(none.at(0.0), 0.0);
        assert_eq!(none.at(0.5), 0.5);
        assert_eq!(none.at(0.99), 1.0);
        let both = Timing::parse("steps(3,jump-both)");
        assert_eq!(both.at(0.0), 0.25);
        assert_eq!(both.at(0.99), 0.75);
        assert_eq!(both.at(1.0), 1.0);
        // The next step of steps(4) at 0.3 is at 0.5.
        assert!(close(end.next_step(0.3).unwrap(), 0.2));
        assert_eq!(Timing::parse("ease").next_step(0.3), None);
    }

    #[test]
    fn linear_points_spread_and_interpolate() {
        let t = Timing::parse("linear(0,0.25_75%,1)");
        assert!(close(t.at(0.75), 0.25));
        assert!(close(t.at(0.375), 0.125));
        assert!(close(t.at(0.875), 0.625));
        let even = Timing::parse("linear(0,1,0)");
        assert!(close(even.at(0.5), 1.0));
        assert!(close(even.at(0.25), 0.5));
    }

    fn style(transition: &str) -> Style {
        Style {
            transition: Some(transition.into()),
            ..Style::default()
        }
    }

    #[test]
    fn the_last_entry_naming_a_property_wins() {
        let s = style("all 0.3 0 ease,opacity 1 0.2 linear");
        assert_eq!(transition(&s, "opacity").unwrap().seconds, 1.0);
        assert_eq!(transition(&s, "transform").unwrap().seconds, 0.3);
        let s = style("opacity 1 0 linear,all 0.3 0 ease");
        assert_eq!(transition(&s, "opacity").unwrap().seconds, 0.3);
        let s = style("background 0.2 0 ease");
        assert!(transition(&s, "background-color").is_some());
        assert!(transition(&s, "opacity").is_none());
        let s = style("scale 0.2 0 ease");
        assert!(transition(&s, "transform").is_some());
        assert_eq!(
            transition(&style("opacity 0.5 0 steps(2,jump-start)"), "opacity")
                .unwrap()
                .timing,
            Timing::Steps(2, Jump::Start)
        );
    }

    #[test]
    fn a_transition_runs_from_the_old_value_over_its_time() {
        let spec = transition(&style("opacity 1 0 linear"), "opacity").unwrap();
        let mut e = Eased::new(0.0_f32);
        assert_eq!(e.step(0.0, Some(&spec), 0.0), (0.0, None));
        let (v, wake) = e.step(1.0, Some(&spec), 10.0);
        assert_eq!(v, 0.0);
        assert_eq!(wake, Some(0.0));
        assert!(close(e.step(1.0, Some(&spec), 10.25).0, 0.25));
        assert!(close(e.step(1.0, Some(&spec), 10.5).0, 0.5));
        assert_eq!(e.step(1.0, Some(&spec), 11.0), (1.0, None));
        assert!(!e.moving());
    }

    #[test]
    fn without_a_transition_the_value_jumps() {
        let mut e = Eased::new(0.0_f32);
        assert_eq!(e.step(1.0, None, 1.0), (1.0, None));
    }

    #[test]
    fn a_delay_holds_the_start_and_wakes_when_it_ends() {
        let spec = transition(&style("opacity 1 0.5 linear"), "opacity").unwrap();
        let mut e = Eased::new(0.0_f32);
        let (v, wake) = e.step(1.0, Some(&spec), 0.0);
        assert_eq!(v, 0.0);
        assert!(close(wake.unwrap() as f32, 0.5));
        assert!(close(e.step(1.0, Some(&spec), 1.0).0, 0.5));
    }

    /// The pointer leaves a quarter of the way into a hover fade: the fade
    /// turns back from a quarter, and takes a quarter of the time.
    #[test]
    fn an_interrupted_transition_turns_back_from_where_it_got_to() {
        let spec = transition(&style("opacity 1 0 linear"), "opacity").unwrap();
        let mut e = Eased::new(0.0_f32);
        e.step(1.0, Some(&spec), 0.0);
        assert!(close(e.step(1.0, Some(&spec), 0.25).0, 0.25));
        let (back, _) = e.step(0.0, Some(&spec), 0.25);
        assert!(close(back, 0.25), "starts from the current value");
        assert!(close(e.step(0.0, Some(&spec), 0.375).0, 0.125));
        assert_eq!(
            e.step(0.0, Some(&spec), 0.5).0,
            0.0,
            "a quarter of a second"
        );
        // A new target that is not the way back runs the whole duration,
        // from where the value is.
        let mut e = Eased::new(0.0_f32);
        e.step(1.0, Some(&spec), 0.0);
        assert!(close(e.step(0.75, Some(&spec), 0.5).0, 0.5));
        assert!(close(e.step(0.75, Some(&spec), 1.0).0, 0.625));
        assert!(e.moving());
        assert_eq!(e.step(0.75, Some(&spec), 1.5).0, 0.75);
        // A new target equal to where the value is: nothing to run.
        let mut e = Eased::new(0.0_f32);
        e.step(1.0, Some(&spec), 0.0);
        e.step(0.5, Some(&spec), 0.5);
        assert!(!e.moving());
    }

    #[test]
    fn steps_wake_once_per_step() {
        let spec = transition(&style("opacity 1 0 steps(4)"), "opacity").unwrap();
        let mut e = Eased::new(0.0_f32);
        let (_, wake) = e.step(1.0, Some(&spec), 0.0);
        assert!(close(wake.unwrap() as f32, 0.25));
        let (v, wake) = e.step(1.0, Some(&spec), 0.3);
        assert_eq!(v, 0.25);
        assert!(close(wake.unwrap() as f32, 0.2));
    }

    #[test]
    fn colours_blend_premultiplied() {
        let a = Color32::from_rgb(255, 0, 0);
        let b = Color32::TRANSPARENT;
        let half = a.lerp(&b, 0.5);
        assert_eq!(half.to_array(), [128, 0, 0, 128]);
    }

    fn anim(spec: &str) -> Animation {
        animations(&Style {
            animation: Some(spec.into()),
            ..Style::default()
        })
        .remove(0)
    }

    #[test]
    fn fill_modes_decide_what_shows_outside_the_active_time() {
        let none = anim("fade 1 0.5 1 normal linear running none");
        assert_eq!(none.at(0.2), None, "no fill: the element's own style");
        assert_eq!(none.at(2.0), None);
        let both = anim("fade 1 0.5 1 normal linear running both");
        assert_eq!(both.at(0.2).unwrap().progress, 0.0);
        let end = both.at(2.0).unwrap();
        assert_eq!((end.progress, end.running), (1.0, false));
        let back = anim("fade 1 0.5 1 normal linear running backwards");
        assert!(back.at(0.2).is_some() && back.at(2.0).is_none());
        let fwd = anim("fade 1 0.5 1 normal linear running forwards");
        assert!(fwd.at(0.2).is_none() && fwd.at(2.0).is_some());
        // Old runtimes wrote no fill mode: none.
        assert_eq!(anim("fade 1 0 1 normal linear running").fill, Fill::None);
    }

    #[test]
    fn iterations_directions_and_negative_delays() {
        let alt = anim("pulse 2 0 infinite alternate linear running none");
        assert!(close(alt.at(0.5).unwrap().progress, 0.25));
        assert!(
            close(alt.at(2.5).unwrap().progress, 0.75),
            "the second runs back"
        );
        let rev = anim("spin 1 0 infinite reverse linear running none");
        assert!(close(rev.at(0.25).unwrap().progress, 0.75));
        // A negative delay starts part way through.
        let late = anim("spin 1 -0.25 infinite normal linear running none");
        assert!(close(late.at(0.0).unwrap().progress, 0.25));
        // A count of 1.5 with forwards ends half way through the second.
        let half = anim("x 1 0 1.5 normal linear running forwards");
        assert!(close(half.at(5.0).unwrap().progress, 0.5));
        // alternate with an even count ends at the start.
        let even = anim("x 1 0 2 alternate linear running forwards");
        assert!(close(even.at(5.0).unwrap().progress, 0.0));
    }

    #[test]
    fn less_motion_shows_the_end_or_holds_the_start() {
        let finite = anim("fade 0.5 0 1 normal ease-out running forwards");
        assert_eq!(finite.at_rest().unwrap().progress, 1.0);
        let finite_none = anim("fade 0.5 0 1 normal ease-out running none");
        assert_eq!(
            finite_none.at_rest(),
            None,
            "its own style, as after it ends"
        );
        let spin = anim("spin 0.8 0 infinite normal linear running none");
        let rest = spin.at_rest().unwrap();
        assert_eq!((rest.progress, rest.running), (0.0, false));
    }

    /// An endless animation (spinny-cover turns its cover with `animation:
    /// spin 24s linear infinite`) asked for every frame, so the window
    /// redrew at the display's every refresh, out of focus as well (a
    /// window that no display paces drew some 800 times a second). It is
    /// drawn as the app's own endless decorations are: thirty times a
    /// second, fifteen out of focus, not while nobody sees the window. A
    /// finite one still gets every frame.
    #[test]
    fn endless_animations_ask_for_frames_as_the_apps_decorations_do() {
        use crate::power::Budget;
        let delay = |budget: Budget, list: &[Animation]| {
            let ctx = egui::Context::default();
            let mut out = None;
            for _ in 0..3 {
                let mut o = ctx.run_ui(egui::RawInput::default(), |ui| {
                    crate::theme::set_budget(ui.ctx(), budget);
                    let _ = moments(ui.ctx(), egui::Id::new("spinning"), list);
                });
                o.textures_delta.clear();
                out = Some(o);
            }
            out.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        let spin = [anim("spin 24 0 infinite normal linear running none")];
        let frame = crate::theme::AMBIENT_FRAME;
        let normal = delay(Budget::Normal, &spin);
        // (egui counts the delay from the frame's start, a frame's time on.)
        assert!(
            normal > std::time::Duration::ZERO && normal <= frame,
            "{normal:?}"
        );
        let saver = delay(Budget::Saver, &spin);
        assert!(saver > frame && saver <= frame * 2, "{saver:?}");
        assert!(
            delay(Budget::Background, &spin) > std::time::Duration::from_secs(60),
            "no frames while nobody sees the window"
        );
        let fade = [anim("fade 24 0 1 normal linear running none")];
        assert_eq!(delay(Budget::Normal, &fade), std::time::Duration::ZERO);
    }

    #[test]
    fn a_clock_keeps_starts_restarts_new_names_and_holds_pauses() {
        let mut clock = Clock::default();
        let spin = anim("spin 1 0 infinite normal linear running none");
        let mut paused = spin.clone();
        paused.paused = true;
        let fade = anim("fade 1 0 1 normal linear running none");
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&spin), 10.0, false),
            vec![0.0]
        );
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&spin), 12.0, false),
            vec![2.0]
        );
        // Paused at 2 s in, held there, then on from there.
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&paused), 12.0, false),
            vec![2.0]
        );
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&paused), 20.0, false),
            vec![2.0]
        );
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&spin), 21.0, false),
            vec![2.0]
        );
        assert_eq!(
            clock.elapsed(std::slice::from_ref(&spin), 22.0, false),
            vec![3.0]
        );
        // A name new to the list starts now; the one that stays keeps on.
        let both = [spin.clone(), fade.clone()];
        assert_eq!(clock.elapsed(&both, 23.0, false), vec![4.0, 0.0]);
        // Gone from the list and back: it starts over.
        clock.elapsed(std::slice::from_ref(&fade), 24.0, false);
        assert_eq!(clock.elapsed(&both, 25.0, false), vec![0.0, 2.0]);
        // Away (not drawn): everything starts over.
        assert_eq!(clock.elapsed(&both, 30.0, true), vec![0.0, 0.0]);
    }

    #[test]
    fn a_track_eases_each_interval_and_fills_missing_ends() {
        let track = Track {
            frames: vec![(0.5, 0.0_f32, None)],
        };
        let own = 1.0;
        let linear = Timing::parse("linear");
        // `50% { opacity: 0 }` alone: from the element's 1 down and back.
        assert!(close(track.at(0.25, &own, &linear), 0.5));
        assert!(close(track.at(0.5, &own, &linear), 0.0));
        assert!(close(track.at(0.75, &own, &linear), 0.5));
        // `ease` applies to each interval, not the whole iteration.
        let ease = Timing::parse("ease");
        assert!(close(track.at(0.25, &own, &ease), 1.0 - 0.8024));
        // A keyframe's own easing wins for the interval after it.
        let stepped = Track {
            frames: vec![
                (0.0, 0.0_f32, Some(Timing::parse("step-end"))),
                (1.0, 1.0, None),
            ],
        };
        assert_eq!(stepped.at(0.9, &own, &linear), 0.0);
    }
}
