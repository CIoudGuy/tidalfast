//! The displays plugins see: Electron's `screen` module in plugins' native
//! code, and `window.screen` and `devicePixelRatio` in the app's page and in
//! plugin windows' pages.
//!
//! The UI reads the monitors from winit ([`poll`], on the app window's
//! passes: at its first, then at most every two seconds once a plugin has
//! asked), and on Windows each monitor's work area from the system
//! (`GetMonitorInfoW`: the display without the taskbar). Elsewhere the work
//! area is the whole display. Lengths are what Electron and Chromium give:
//! DIPs, each monitor's size divided by its own scale factor, the primary
//! at its own place, and a monitor that touches one already placed next to
//! it, as Chromium lays out monitors of different scales (`dip_origins`).
//! `devicePixelRatio` is the window's own scale, the zoom included, as in
//! Chromium. Until the UI's first pass (and in tests) plugins see one
//! 1920 by 1080 display at scale 1.
//!
//! A plugin window's display is the one under its middle; the UI notes it
//! on the window ([`crate::luna::page::PageWindow::set_screen`]).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How often the UI looks at the monitors again once plugins read them.
const POLL: Duration = Duration::from_secs(2);

/// One monitor, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// Stable while the monitor stays connected (from its name).
    pub id: u32,
    pub label: String,
    /// `[x, y, width, height]`.
    pub bounds: [i32; 4],
    /// The part windows may use (no taskbar or dock).
    pub work_area: [i32; 4],
    pub scale: f32,
    /// Refresh rate in Hz.
    pub hz: f32,
}

impl Display {
    fn scale(&self) -> f64 {
        f64::from(if self.scale > 0.0 { self.scale } else { 1.0 })
    }

    /// Physical rectangle `r` on this display in DIPs, the display's own
    /// top-left corner at `origin`, as `{ x, y, width, height }`.
    fn dip(&self, origin: (f64, f64), r: [i32; 4]) -> Value {
        let s = self.scale();
        // Whole DIPs, as Electron's rectangles are.
        let round = |v: f64| v.round() as i64;
        let x = round(origin.0 + f64::from(r[0] - self.bounds[0]) / s);
        let y = round(origin.1 + f64::from(r[1] - self.bounds[1]) / s);
        let w = round(f64::from(r[2]) / s);
        let h = round(f64::from(r[3]) / s);
        json!({ "x": x, "y": y, "width": w, "height": h })
    }

    /// Whether physical point `(x, y)` is on this display.
    fn holds(&self, x: f64, y: f64) -> bool {
        let [bx, by, w, h] = self.bounds.map(f64::from);
        x >= bx && y >= by && x < bx + w && y < by + h
    }

    /// Electron's `Display` object, the display's corner at DIP `origin`.
    fn json(&self, origin: (f64, f64)) -> Value {
        let bounds = self.dip(origin, self.bounds);
        let work = self.dip(origin, self.work_area);
        json!({
            "id": self.id,
            "label": self.label,
            "bounds": bounds,
            "workArea": work,
            "size": { "width": bounds["width"], "height": bounds["height"] },
            "workAreaSize": { "width": work["width"], "height": work["height"] },
            "scaleFactor": self.scale,
            "rotation": 0,
            "internal": false,
            "detected": true,
            "monochrome": false,
            "touchSupport": "unknown",
            "accelerometerSupport": "unknown",
            "colorDepth": 24,
            "depthPerComponent": 8,
            "colorSpace": "{primaries:BT709, transfer:SRGB, matrix:RGB, range:FULL}",
            "displayFrequency": self.hz,
            "nativeOrigin": { "x": self.bounds[0], "y": self.bounds[1] },
            "maximumCursorSize": { "width": 0, "height": 0 },
        })
    }
}

/// Where each display's top-left corner is in DIPs. The first (the
/// primary) and any display that touches none keep their physical corner
/// over their own scale; one that touches a display already placed goes
/// right next to it in DIPs, its offset along the shared edge in the placed
/// display's DIPs, so displays of different scales neither overlap nor
/// leave gaps (Chromium's layout of mixed-scale monitors).
fn dip_origins(displays: &[Display]) -> Vec<(f64, f64)> {
    let own = |d: &Display| {
        (
            f64::from(d.bounds[0]) / d.scale(),
            f64::from(d.bounds[1]) / d.scale(),
        )
    };
    let mut at: Vec<Option<(f64, f64)>> = vec![None; displays.len()];
    let mut queue = std::collections::VecDeque::new();
    for start in 0..displays.len() {
        if at[start].is_some() {
            continue;
        }
        at[start] = Some(own(&displays[start]));
        queue.push_back(start);
        while let Some(p) = queue.pop_front() {
            let (pd, po) = (&displays[p], at[p].unwrap_or_default());
            let [px, py, pw, ph] = pd.bounds;
            let ps = pd.scale();
            for (i, d) in displays.iter().enumerate() {
                if at[i].is_some() {
                    continue;
                }
                let [x, y, w, h] = d.bounds;
                let s = d.scale();
                let across_x = x < px + pw && px < x + w;
                let across_y = y < py + ph && py < y + h;
                let along_y = po.1 + f64::from(y - py) / ps;
                let along_x = po.0 + f64::from(x - px) / ps;
                let placed = if x == px + pw && across_y {
                    Some((po.0 + f64::from(pw) / ps, along_y))
                } else if x + w == px && across_y {
                    Some((po.0 - f64::from(w) / s, along_y))
                } else if y == py + ph && across_x {
                    Some((along_x, po.1 + f64::from(ph) / ps))
                } else if y + h == py && across_x {
                    Some((along_x, po.1 - f64::from(h) / s))
                } else {
                    None
                };
                if placed.is_some() {
                    at[i] = placed;
                    queue.push_back(i);
                }
            }
        }
    }
    at.into_iter().map(Option::unwrap_or_default).collect()
}

/// What plugins see before the UI looked (and in tests).
fn fallback() -> Display {
    Display {
        id: 1,
        label: "Display".into(),
        bounds: [0, 0, 1920, 1080],
        work_area: [0, 0, 1920, 1080],
        scale: 1.0,
        hz: 60.0,
    }
}

/// The monitors (the primary first) and which one shows the app's window.
#[derive(Clone, Debug, Default, PartialEq)]
struct Screens {
    displays: Vec<Display>,
    main: Option<u32>,
}

static SCREENS: Mutex<Option<Arc<Screens>>> = Mutex::new(None);
/// Bumped each time the monitors change after the first look.
static CHANGES: AtomicU64 = AtomicU64::new(0);
/// A plugin read the screen: keep looking.
static ASKED: AtomicBool = AtomicBool::new(false);
/// The app window's points per pixel (`f32` bits; 0 before the first pass).
static MAIN_SCALE: AtomicU32 = AtomicU32::new(0);
static LAST_LOOK: Mutex<Option<Instant>> = Mutex::new(None);

/// Sets the monitors; returns whether they changed. The primary comes first.
pub fn set(displays: Vec<Display>, main: Option<u32>) -> bool {
    let next = Screens { displays, main };
    let Ok(mut cur) = SCREENS.lock() else {
        return false;
    };
    let changed = cur
        .as_ref()
        .is_some_and(|c| c.displays != next.displays || c.main != next.main);
    if cur.as_deref() != Some(&next) {
        *cur = Some(Arc::new(next));
    }
    if changed {
        CHANGES.fetch_add(1, Ordering::Relaxed);
    }
    changed
}

/// How many times the monitors changed (for the app to tell plugins).
pub fn changes() -> u64 {
    CHANGES.load(Ordering::Relaxed)
}

/// The app window's scale (`devicePixelRatio` in the app's page).
pub fn set_main_scale(scale: f32) {
    let bits = scale.to_bits();
    if MAIN_SCALE.load(Ordering::Relaxed) != bits {
        MAIN_SCALE.store(bits, Ordering::Relaxed);
    }
}

/// The display that holds physical point `(x, y)`, else the nearest.
pub fn display_at(x: f64, y: f64) -> Option<u32> {
    let s = SCREENS.lock().ok()?.clone()?;
    s.displays
        .iter()
        .find(|d| d.holds(x, y))
        .or_else(|| {
            s.displays.iter().min_by(|a, b| {
                let dist = |d: &Display| {
                    let [bx, by, w, h] = d.bounds.map(f64::from);
                    let dx = (bx - x).max(0.0).max(x - (bx + w));
                    let dy = (by - y).max(0.0).max(y - (by + h));
                    dx * dx + dy * dy
                };
                dist(a).total_cmp(&dist(b))
            })
        })
        .map(|d| d.id)
}

/// The `screen` op: every display as Electron describes it, the primary's
/// id, the display of the asking window (`current`: a plugin window's, or
/// the app's when `window` is `None`) and its `devicePixelRatio`.
pub(crate) fn op(window: Option<(Option<u32>, f32)>) -> Value {
    if !ASKED.load(Ordering::Relaxed) {
        ASKED.store(true, Ordering::Relaxed);
    }
    let screens = SCREENS.lock().ok().and_then(|s| s.clone());
    let displays = screens
        .as_ref()
        .map(|s| s.displays.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| vec![fallback()]);
    let primary = displays[0].id;
    let (current, scale) = match window {
        Some((display, scale)) => (display, scale),
        None => (
            screens.as_ref().and_then(|s| s.main),
            f32::from_bits(MAIN_SCALE.load(Ordering::Relaxed)),
        ),
    };
    let current = current
        .filter(|id| displays.iter().any(|d| d.id == *id))
        .unwrap_or(primary);
    let origins = dip_origins(&displays);
    json!({
        "displays": displays
            .iter()
            .zip(origins)
            .map(|(d, o)| d.json(o))
            .collect::<Vec<_>>(),
        "primary": primary,
        "current": current,
        "dpr": if scale > 0.0 { scale } else { 1.0 },
    })
}

/// A stable id for a monitor: its name's hash (FNV-1a), else its place in
/// the list.
fn id_of(name: Option<&str>, index: usize) -> u32 {
    match name {
        Some(n) if !n.is_empty() => {
            let h = n.bytes().fold(0x811c_9dc5_u32, |h, b| {
                (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
            });
            // Never 0, and within a JS number's exact range.
            (h & 0x7fff_ffff).max(1)
        }
        _ => index as u32 + 1,
    }
}

/// The work area of the monitor with physical `bounds`, from the system.
#[cfg(windows)]
fn work_area(bounds: [i32; 4]) -> Option<[i32; 4]> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
    };
    let middle = POINT {
        x: bounds[0] + bounds[2] / 2,
        y: bounds[1] + bounds[3] / 2,
    };
    // SAFETY: MonitorFromPoint takes a point by value and returns a handle
    // or null; GetMonitorInfoW writes into a MONITORINFO whose size field is
    // set, and the handle came from the system just now.
    unsafe {
        let monitor = MonitorFromPoint(middle, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let r = info.rcWork;
        Some([r.left, r.top, r.right - r.left, r.bottom - r.top])
    }
}

/// Elsewhere the whole display (no taskbar or dock is taken off yet).
#[cfg(not(windows))]
fn work_area(bounds: [i32; 4]) -> Option<[i32; 4]> {
    Some(bounds)
}

/// Looks at the monitors from the app's window: call once per pass of it.
/// The first pass looks; later ones at most every `POLL`, and only once a
/// plugin read the screen. Otherwise this is two atomic loads.
pub fn poll(ctx: &egui::Context, frame: &eframe::Frame) {
    set_main_scale(ctx.pixels_per_point());
    let now = Instant::now();
    {
        let Ok(mut last) = LAST_LOOK.lock() else {
            return;
        };
        if last.is_some_and(|t| !ASKED.load(Ordering::Relaxed) || now - t < POLL) {
            return;
        }
        *last = Some(now);
    }
    let Some(window) = frame.winit_window() else {
        return;
    };
    let primary = window.primary_monitor();
    let mut monitors: Vec<_> = primary.clone().into_iter().collect();
    for m in window.available_monitors() {
        if !monitors.contains(&m) {
            monitors.push(m);
        }
    }
    let current = window.current_monitor();
    let mut main = None;
    let displays = monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (p, s) = (m.position(), m.size());
            let bounds = [
                p.x,
                p.y,
                i32::try_from(s.width).unwrap_or(i32::MAX),
                i32::try_from(s.height).unwrap_or(i32::MAX),
            ];
            let name = m.name();
            let id = id_of(name.as_deref(), i);
            if current.as_ref() == Some(m) {
                main = Some(id);
            }
            Display {
                id,
                label: name.unwrap_or_default(),
                bounds,
                work_area: work_area(bounds).unwrap_or(bounds),
                scale: m.scale_factor() as f32,
                hz: m
                    .refresh_rate_millihertz()
                    .map_or(60.0, |mhz| mhz as f32 / 1000.0),
            }
        })
        .collect();
    set(displays, main);
}

/// Tests: two made-up monitors, the same in every test that sets them (the
/// state is the process's): a 2560 by 1440 primary at scale 1.25 with a
/// 48-pixel taskbar at its bottom, and a 1920 by 1080 one at scale 1 to its
/// right with a 40-pixel taskbar at its top. The app's window is on the
/// second.
#[cfg(test)]
pub(crate) fn set_test_displays() -> (u32, u32) {
    let d = |id, label: &str, bounds, work_area, scale| Display {
        id,
        label: label.into(),
        bounds,
        work_area,
        scale,
        hz: 144.0,
    };
    set(
        vec![
            d(11, "Primary", [0, 0, 2560, 1440], [0, 0, 2560, 1392], 1.25),
            d(
                12,
                "Side",
                [2560, 0, 1920, 1080],
                [2560, 40, 1920, 1040],
                1.0,
            ),
        ],
        Some(12),
    );
    set_main_scale(1.5);
    (11, 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u32, bounds: [i32; 4], work_area: [i32; 4], scale: f32) -> Display {
        Display {
            id,
            label: format!("D{id}"),
            bounds,
            work_area,
            scale,
            hz: 144.0,
        }
    }

    /// Lengths are DIPs (physical over the monitor's own scale), the work
    /// area is the system's, and the asking window's display and scale
    /// come back with the list.
    #[test]
    fn displays_are_described_as_electron_describes_them() {
        let d = display(7, [0, 0, 2560, 1440], [0, 0, 2560, 1392], 1.25);
        let j = d.json((0.0, 0.0));
        assert_eq!(
            j["bounds"],
            json!({ "x": 0, "y": 0, "width": 2048, "height": 1152 })
        );
        assert_eq!(j["workArea"]["height"], json!(1114));
        assert_eq!(j["size"], json!({ "width": 2048, "height": 1152 }));
        assert_eq!(j["scaleFactor"], json!(1.25));
        assert_eq!(j["displayFrequency"], json!(144.0));
        assert_eq!(j["nativeOrigin"], json!({ "x": 0, "y": 0 }));
        let second = display(9, [2560, 0, 1920, 1080], [2560, 0, 1920, 1040], 1.0);
        assert!(second.holds(3000.0, 10.0) && !second.holds(10.0, 10.0));
        // A window's display and scale win over the app's.
        let s = Screens {
            displays: vec![d, second],
            main: Some(9),
        };
        let at = |x, y| s.displays.iter().find(|d| d.holds(x, y)).map(|d| d.id);
        assert_eq!(at(3000.0, 500.0), Some(9));
        assert_eq!(at(-5.0, 500.0), None);
        assert_eq!(
            id_of(Some(r"\\.\DISPLAY1"), 0),
            id_of(Some(r"\\.\DISPLAY1"), 3)
        );
        assert_ne!(
            id_of(Some(r"\\.\DISPLAY1"), 0),
            id_of(Some(r"\\.\DISPLAY2"), 0)
        );
        assert_eq!(id_of(None, 2), 3);
    }

    /// Monitors of different scales sit side by side in DIPs, as Chromium
    /// lays them out: no gap, no overlap, the offset along the shared edge
    /// in the placed monitor's DIPs; a monitor touching none keeps its own.
    #[test]
    fn mixed_scale_monitors_touch_in_dips() {
        let primary = display(1, [0, 0, 3840, 2160], [0, 0, 3840, 2100], 1.5);
        let right = display(2, [3840, 300, 1920, 1080], [3840, 300, 1920, 1040], 1.0);
        let above = display(3, [0, -1440, 2560, 1440], [0, -1440, 2560, 1440], 2.0);
        let apart = display(4, [10000, 0, 1000, 1000], [10000, 0, 1000, 1000], 2.0);
        let all = [primary.clone(), right.clone(), above, apart];
        let o = dip_origins(&all);
        assert_eq!(o[0], (0.0, 0.0));
        assert_eq!(o[1], (2560.0, 200.0), "right of the primary's 2560 DIPs");
        assert_eq!(o[2], (0.0, -720.0), "above, its own 720 DIPs high");
        assert_eq!(o[3], (5000.0, 0.0));
        let j = right.json(o[1]);
        assert_eq!(
            j["workArea"],
            json!({ "x": 2560, "y": 200, "width": 1920, "height": 1040 })
        );
        assert_eq!(primary.json(o[0])["workArea"]["height"], json!(1400));
    }
}
