//! Chromium's transition and animation events for plugins.
//!
//! When an overlay changes a property of an element (a TIDAL anchor or a
//! plugin's own) that its `transition` covers, Chromium fires
//! `transitionrun` at once, `transitionstart` when the delay is over and
//! `transitionend` when the transition finishes, with the longhand's
//! `propertyName` and `elapsedTime`; one interrupted by a new change fires
//! `transitioncancel`. An animation newly in an element's list fires
//! `animationstart` after its delay and `animationend` when a finite one
//! ends. radiant-lyrics waits for its tab's `min-width` transition to end
//! before it hangs its menu under the widened tab; StarryNight moves a
//! shooting star when its flight ends.
//!
//! The events are worked out here when a new overlay arrives, timed from
//! then (when the UI starts the same motion), and sent by
//! `plugin_dom::end_frame` once due, after the anchor tree with the boxes
//! as they are then, so a plugin measuring in its handler reads the end
//! state.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::css_motion;
use crate::luna::dom::{DomOverlay, DomTarget, Style, VNode};

/// The transition events, any of which makes an element's changes worth
/// timing.
const TRANSITION_KINDS: [&str; 4] = [
    "transitionrun",
    "transitionstart",
    "transitionend",
    "transitioncancel",
];

/// The animation events.
const ANIMATION_KINDS: [&str; 3] = ["animationstart", "animationiteration", "animationend"];

/// Style fields no transition moves (or that the runtime keeps for its own
/// use): a change in them fires nothing.
const NOT_ANIMATED: &[&str] = &[
    "display",
    "position",
    "cursor",
    "pointer-events",
    "font-family",
    "font-style",
    "white-space",
    "text-overflow",
    "text-transform",
    "text-decoration",
    "text-align",
    "overflow",
    "overflow-y",
    "flex-direction",
    "flex-wrap",
    "justify-content",
    "align-items",
    "object-fit",
    "image-rendering",
    "list-style-type",
    "grid-template-columns",
    "transition",
    "animation",
    "custom",
];

/// One event waiting for its time.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvent {
    pub due: Instant,
    pub target: DomTarget,
    pub kind: &'static str,
    pub data: Value,
    /// The property (or animation name) it is about.
    what: String,
}

/// The events waiting, in no order.
#[derive(Debug, Default)]
pub struct Pending {
    events: Vec<MotionEvent>,
}

impl Pending {
    /// Time the events of the changes from `old` to `new`, as of `now`.
    /// Nothing is timed from the first overlay: an element's first style
    /// starts no transition.
    pub fn schedule(&mut self, old: &DomOverlay, new: &DomOverlay, now: Instant) {
        if old.version == 0 {
            return;
        }
        for (key, host) in &new.hosts {
            if !listens_any(&host.listens, &TRANSITION_KINDS) {
                continue;
            }
            let before = old.hosts.get(key).map(|h| &h.style);
            let empty = Style::default();
            self.transitions(
                DomTarget::Host(key.clone()),
                before.unwrap_or(&empty),
                &host.style,
                &host.listens,
                now,
            );
        }
        let mut wanted: Vec<&VNode> = Vec::new();
        for injected in &new.injected {
            for node in &injected.nodes {
                collect(node, &mut wanted);
            }
        }
        if wanted.is_empty() {
            return;
        }
        let mut before: HashMap<u64, &VNode> = HashMap::new();
        for injected in &old.injected {
            for node in &injected.nodes {
                index(node, &mut before);
            }
        }
        for node in wanted {
            let was = before.get(&node.id).copied();
            if listens_any(&node.listens, &TRANSITION_KINDS) {
                // A new element moves from the style it had at its first
                // style pass, if it carries one.
                if let Some(from) = was.map(|n| &n.style).or(node.from.as_ref()) {
                    self.transitions(
                        DomTarget::Node(node.id),
                        from,
                        &node.style,
                        &node.listens,
                        now,
                    );
                }
            }
            if listens_any(&node.listens, &ANIMATION_KINDS) {
                let had: Vec<String> = was
                    .map(|n| css_motion::animations(&n.style))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| a.name)
                    .collect();
                for a in css_motion::animations(&node.style) {
                    if had.contains(&a.name) || a.paused || a.name == "none" {
                        continue;
                    }
                    self.animation(DomTarget::Node(node.id), &a, &node.listens, now);
                }
            }
        }
    }

    fn transitions(
        &mut self,
        target: DomTarget,
        from: &Style,
        to: &Style,
        listens: &[String],
        now: Instant,
    ) {
        if to.transition.is_none() {
            return;
        }
        for property in changed(from, to) {
            let Some(t) = css_motion::transition(to, &property) else {
                continue;
            };
            // A transition still running for it is cancelled.
            let running = self
                .events
                .iter()
                .any(|e| e.target == target && e.what == property && e.kind == "transitionend");
            self.events
                .retain(|e| !(e.target == target && e.what == property));
            let delay = f64::from(t.delay);
            let seconds = f64::from(t.seconds);
            let at = |s: f64| now + Duration::from_secs_f64(s.max(0.0));
            let data = |elapsed: f64| json!({ "propertyName": property, "elapsedTime": elapsed, "pseudoElement": "" });
            let mut push = |kind: &'static str, due: Instant, elapsed: f64| {
                if listens.iter().any(|l| l == kind) {
                    self.events.push(MotionEvent {
                        due,
                        target: target.clone(),
                        kind,
                        data: data(elapsed),
                        what: property.clone(),
                    });
                }
            };
            if running {
                push("transitioncancel", now, 0.0);
            }
            push("transitionrun", now, 0.0);
            push("transitionstart", at(delay), (-delay).max(0.0));
            push("transitionend", at(delay + seconds), seconds);
        }
    }

    fn animation(
        &mut self,
        target: DomTarget,
        a: &css_motion::Animation,
        listens: &[String],
        now: Instant,
    ) {
        self.events
            .retain(|e| !(e.target == target && e.what == a.name));
        let at = |s: f64| now + Duration::from_secs_f64(s.max(0.0));
        let mut push = |kind: &'static str, due: Instant, elapsed: f64| {
            if listens.iter().any(|l| l == kind) {
                self.events.push(MotionEvent {
                    due,
                    target: target.clone(),
                    kind,
                    data: json!({ "animationName": a.name, "elapsedTime": elapsed, "pseudoElement": "" }),
                    what: a.name.clone(),
                });
            }
        };
        push("animationstart", at(a.delay), (-a.delay).max(0.0));
        if a.count.is_finite() {
            let active = a.seconds * a.count;
            let whole = a.count.ceil() as u64;
            // Each iteration after the first, up to 100.
            for n in 1..whole.min(100) {
                let into = n as f64 * a.seconds;
                push("animationiteration", at(a.delay + into), into);
            }
            push("animationend", at(a.delay + active), active);
        }
    }

    /// The events due by `now`, in the order they fall due, taken out.
    pub fn take_due(&mut self, now: Instant) -> Vec<MotionEvent> {
        if self.events.iter().all(|e| e.due > now) {
            return Vec::new();
        }
        let (mut due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|e| e.due <= now);
        self.events = rest;
        due.sort_by_key(|e| e.due);
        due
    }

    /// When the next event falls due.
    pub fn next_due(&self) -> Option<Instant> {
        self.events.iter().map(|e| e.due).min()
    }
}

fn listens_any(listens: &[String], kinds: &[&str]) -> bool {
    listens.iter().any(|l| kinds.contains(&l.as_str()))
}

/// The nodes under `node` (itself included) that listen for a motion
/// event.
fn collect<'a>(node: &'a VNode, out: &mut Vec<&'a VNode>) {
    if listens_any(&node.listens, &TRANSITION_KINDS) || listens_any(&node.listens, &ANIMATION_KINDS)
    {
        out.push(node);
    }
    for child in &node.children {
        collect(child, out);
    }
}

fn index<'a>(node: &'a VNode, out: &mut HashMap<u64, &'a VNode>) {
    out.insert(node.id, node);
    for child in &node.children {
        index(child, out);
    }
}

/// The properties whose values differ between `from` and `to`, by their
/// CSS names, leaving out those no transition moves.
fn changed(from: &Style, to: &Style) -> Vec<String> {
    let (Ok(Value::Object(a)), Ok(Value::Object(b))) =
        (serde_json::to_value(from), serde_json::to_value(to))
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| !NOT_ANIMATED.contains(&k.as_str()))
        .filter(|k| a.get(*k).filter(|v| !v.is_null()) != b.get(*k).filter(|v| !v.is_null()))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luna::dom::{HostOverride, Injected, Position};

    fn host_overlay(version: u64, min_width: Option<&str>, listens: &[&str]) -> DomOverlay {
        let mut o = DomOverlay {
            version,
            ..DomOverlay::default()
        };
        o.hosts.insert(
            "toggle-lyrics".into(),
            HostOverride {
                style: Style {
                    min_width: min_width.map(Into::into),
                    transition: Some("min-width 0.12 0 ease-out".into()),
                    ..Style::default()
                },
                listens: listens.iter().map(|s| s.to_string()).collect(),
                ..HostOverride::default()
            },
        );
        o
    }

    /// radiant-lyrics' tab widens by its `min-width` transition (0.12 s):
    /// `transitionrun` at once, `transitionend` with `propertyName:
    /// "min-width"` when it ends, as Chromium fires them.
    #[test]
    fn a_host_transition_fires_run_and_end_in_time() {
        let mut p = Pending::default();
        let now = Instant::now();
        let listens = ["transitionend", "transitionrun"];
        p.schedule(
            &host_overlay(1, None, &listens),
            &host_overlay(2, Some("150px"), &listens),
            now,
        );
        let run = p.take_due(now);
        assert_eq!(run.len(), 1, "{run:?}");
        assert_eq!(run[0].kind, "transitionrun");
        assert!(p.take_due(now + Duration::from_millis(100)).is_empty());
        let end = p.take_due(now + Duration::from_millis(121));
        assert_eq!(end.len(), 1, "{end:?}");
        assert_eq!(end[0].kind, "transitionend");
        assert_eq!(end[0].target, DomTarget::Host("toggle-lyrics".into()));
        assert_eq!(end[0].data["propertyName"], "min-width");
        assert!((end[0].data["elapsedTime"].as_f64().unwrap() - 0.12).abs() < 1e-6);
        assert_eq!(p.next_due(), None);
    }

    /// Nothing is timed for an element nobody listens to, for a property
    /// no transition covers, or from the first overlay.
    #[test]
    fn no_events_without_a_listener_a_transition_or_an_earlier_style() {
        let now = Instant::now();
        let mut p = Pending::default();
        p.schedule(
            &host_overlay(1, None, &[]),
            &host_overlay(2, Some("150px"), &[]),
            now,
        );
        assert_eq!(p.next_due(), None);
        let mut q = Pending::default();
        q.schedule(
            &DomOverlay::default(),
            &host_overlay(1, Some("150px"), &["transitionend"]),
            now,
        );
        assert_eq!(q.next_due(), None);
        let mut r = Pending::default();
        let mut old = host_overlay(1, None, &["transitionend"]);
        let mut new = host_overlay(2, None, &["transitionend"]);
        old.hosts.get_mut("toggle-lyrics").unwrap().style.opacity = Some("1".into());
        new.hosts.get_mut("toggle-lyrics").unwrap().style.opacity = Some("0".into());
        r.schedule(&old, &new, now);
        assert_eq!(r.next_due(), None, "opacity has no transition here");
    }

    /// A new change before the end cancels the running transition.
    #[test]
    fn a_new_change_cancels_the_running_transition() {
        let now = Instant::now();
        let listens = ["transitioncancel", "transitionend"];
        let mut p = Pending::default();
        p.schedule(
            &host_overlay(1, None, &listens),
            &host_overlay(2, Some("150px"), &listens),
            now,
        );
        p.schedule(
            &host_overlay(2, Some("150px"), &listens),
            &host_overlay(3, None, &listens),
            now + Duration::from_millis(50),
        );
        let due = p.take_due(now + Duration::from_millis(60));
        assert_eq!(
            due.iter().map(|e| e.kind).collect::<Vec<_>>(),
            ["transitioncancel"]
        );
        let end = p.take_due(now + Duration::from_millis(171));
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].kind, "transitionend");
    }

    /// StarryNight's shooting star: a plugin element inserted with a
    /// finite animation fires `animationend` after its delay and duration.
    #[test]
    fn an_inserted_elements_finite_animation_ends() {
        let star = |version| DomOverlay {
            version,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![VNode {
                    id: 7,
                    tag: "span".into(),
                    style: Style {
                        animation: Some("shoot 3 2 1 normal linear running none".into()),
                        ..Style::default()
                    },
                    listens: vec!["animationend".into()],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        };
        let now = Instant::now();
        let mut p = Pending::default();
        p.schedule(
            &DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
            &star(2),
            now,
        );
        assert!(p.take_due(now + Duration::from_millis(4900)).is_empty());
        let end = p.take_due(now + Duration::from_millis(5001));
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].target, DomTarget::Node(7));
        assert_eq!(end[0].data["animationName"], "shoot");
        // The same animation in the next overlay is not started again.
        p.schedule(&star(2), &star(3), now);
        assert_eq!(p.next_due(), None);
    }
}
