//! Development only (demo builds): `TIDALFAST_DEV_SCRIPT=<file>` lets a test
//! drive an off-screen window without touching the desktop's pointer,
//! keyboard or focus. A line-based script written to that file is read
//! (and the file removed) by the running window, which plays it as egui
//! input:
//!
//! ```text
//! move X Y          pointer to (X, Y) in points
//! click X Y         press and release the primary button there
//! rclick X Y        the same with the secondary button
//! drag X1 Y1 X2 Y2 [MS]
//!                   press at (X1, Y1), move in steps to (X2, Y2) over MS
//!                   (default 300), release there
//! wheel X Y DY      scroll by DY points over (X, Y)
//! drop PATH [X Y]   drop the file at PATH on the window (the pointer moves
//!                   to (X, Y) first), as a file dragged in from the desktop
//! key NAME [MODS]   press and release an egui key (`Enter`, `F11`, `A`),
//!                   MODS any of `ctrl`, `shift`, `alt` joined by `+`
//! text STRING       type text
//! wait MS           pause before the next line
//! shot PATH         write a PNG of the window's frame buffer to PATH
//! focus             from here on the window reports itself focused, as
//!                   one the user is working in (the app and plugins then
//!                   act as for a window in front, not in the background)
//! ```
//!
//! Without the variable nothing here runs. With it, one thread checks for
//! the file every 300 ms and wakes the window when it appears.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DRAG_STEPS: u64 = 10;

/// A file dropped by a `drop` line: what winit hands egui for a real drop.
#[derive(Debug)]
struct ScriptedFile(PathBuf);

impl egui::DroppedFile for ScriptedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

enum Step {
    Events(Vec<egui::Event>),
    Wait(Duration),
    Shot(PathBuf),
    Drop(egui::DroppedFileHandle),
    Focus,
}

#[derive(Default)]
pub struct DevScript {
    path: Option<PathBuf>,
    started: bool,
    steps: VecDeque<Step>,
    resume_at: Option<Instant>,
    pos: egui::Pos2,
    shot: Option<PathBuf>,
    /// `focus` was played: the window reports itself focused.
    focus: bool,
}

impl DevScript {
    pub fn from_env() -> Self {
        Self {
            path: std::env::var_os("TIDALFAST_DEV_SCRIPT").map(PathBuf::from),
            ..Self::default()
        }
    }

    /// Feed this frame's scripted input (from `raw_input_hook`).
    pub fn input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(path) = self.path.clone() else {
            return;
        };
        // The scripted window has the keyboard, as a window being typed
        // into does: egui gives text fields no input while the window is
        // unfocused, and an off-screen test window is never focused.
        raw.focused = true;
        if self.focus {
            raw.viewports.entry(raw.viewport_id).or_default().focused = Some(true);
        }
        if !std::mem::replace(&mut self.started, true) {
            let ctx = ctx.clone();
            let watched = path.clone();
            let _ = std::thread::Builder::new()
                .name("dev-script".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(300));
                        if watched.exists() {
                            ctx.request_repaint();
                        }
                    }
                });
        }
        if self.steps.is_empty()
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            let _ = std::fs::remove_file(&path);
            self.parse(&text);
        }
        if let Some(at) = self.resume_at {
            if Instant::now() < at {
                ctx.request_repaint_after(at - Instant::now());
                return;
            }
            self.resume_at = None;
        }
        match self.steps.pop_front() {
            Some(Step::Events(events)) => raw.events.extend(events),
            Some(Step::Drop(file)) => raw.dropped_files.push(file),
            Some(Step::Focus) => {
                self.focus = true;
                raw.viewports.entry(raw.viewport_id).or_default().focused = Some(true);
            }
            Some(Step::Wait(d)) => self.resume_at = Some(Instant::now() + d),
            Some(Step::Shot(p)) => {
                self.shot = Some(p);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            None => return,
        }
        ctx.request_repaint();
    }

    /// Write a screenshot asked for by `shot` once it arrives (from `logic`).
    pub fn frame(&mut self, ctx: &egui::Context) {
        // A `wait` asks for the frame that ends it from here: egui drops a
        // delayed repaint asked for in the input hook (the pass about to
        // run counts as serving it), and the script then stalled until
        // something else repainted the window.
        if let Some(at) = self.resume_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
        if self.shot.is_none() {
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            ctx.request_repaint();
            return;
        };
        let Some(path) = self.shot.take() else { return };
        let [w, h] = [image.size[0] as u32, image.size[1] as u32];
        let pixels: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect();
        if let Some(buffer) = image::RgbaImage::from_raw(w, h, pixels)
            && let Err(e) = buffer.save(&path)
        {
            log::error!("dev script: could not write {}: {e}", path.display());
        }
    }

    fn parse(&mut self, text: &str) {
        let button = |pos, button, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let Some(cmd) = words.next() else { continue };
            let nums: Vec<f32> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|w| w.parse().ok())
                .collect();
            let at = |nums: &[f32]| egui::pos2(nums[0], nums[1]);
            match cmd {
                "move" if nums.len() >= 2 => {
                    self.pos = at(&nums);
                    self.steps
                        .push_back(Step::Events(vec![egui::Event::PointerMoved(self.pos)]));
                }
                "click" | "rclick" if nums.len() >= 2 => {
                    let b = if cmd == "click" {
                        egui::PointerButton::Primary
                    } else {
                        egui::PointerButton::Secondary
                    };
                    self.pos = at(&nums);
                    self.steps
                        .push_back(Step::Events(vec![egui::Event::PointerMoved(self.pos)]));
                    self.steps
                        .push_back(Step::Events(vec![button(self.pos, b, true)]));
                    self.steps
                        .push_back(Step::Events(vec![button(self.pos, b, false)]));
                }
                "drag" if nums.len() >= 4 => {
                    let (from, to) = (egui::pos2(nums[0], nums[1]), egui::pos2(nums[2], nums[3]));
                    let ms = nums.get(4).copied().unwrap_or(300.0).max(0.0);
                    let b = egui::PointerButton::Primary;
                    self.steps
                        .push_back(Step::Events(vec![egui::Event::PointerMoved(from)]));
                    self.steps
                        .push_back(Step::Events(vec![button(from, b, true)]));
                    for i in 1..=DRAG_STEPS {
                        let p = from + (to - from) * (i as f32 / DRAG_STEPS as f32);
                        self.steps
                            .push_back(Step::Wait(Duration::from_millis(ms as u64 / DRAG_STEPS)));
                        self.steps
                            .push_back(Step::Events(vec![egui::Event::PointerMoved(p)]));
                    }
                    self.pos = to;
                    self.steps
                        .push_back(Step::Events(vec![button(to, b, false)]));
                }
                "wheel" if nums.len() >= 3 => {
                    self.pos = at(&nums);
                    self.steps.push_back(Step::Events(vec![
                        egui::Event::PointerMoved(self.pos),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, nums[2]),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]));
                }
                "drop" => {
                    // The path is everything after `drop` except trailing
                    // numbers (the pointer position), so it may hold spaces.
                    let rest = line.trim_start().strip_prefix("drop").unwrap_or("").trim();
                    let mut parts: Vec<&str> = rest.split_whitespace().collect();
                    let mut at_xy = Vec::new();
                    while at_xy.len() < 2
                        && parts.len() > 1
                        && let Ok(n) = parts[parts.len() - 1].parse::<f32>()
                    {
                        at_xy.insert(0, n);
                        parts.pop();
                    }
                    if parts.is_empty() {
                        continue;
                    }
                    let path = PathBuf::from(parts.join(" "));
                    if at_xy.len() == 2 {
                        self.pos = at(&at_xy);
                    }
                    let file: egui::DroppedFileHandle = std::sync::Arc::new(ScriptedFile(path));
                    self.steps
                        .push_back(Step::Events(vec![egui::Event::PointerMoved(self.pos)]));
                    self.steps.push_back(Step::Drop(file));
                }
                "key" => {
                    let Some(key) = words.next().and_then(egui::Key::from_name) else {
                        continue;
                    };
                    let mods = words.next().unwrap_or("");
                    let modifiers = egui::Modifiers {
                        ctrl: mods.contains("ctrl"),
                        command: mods.contains("ctrl"),
                        shift: mods.contains("shift"),
                        alt: mods.contains("alt"),
                        mac_cmd: false,
                    };
                    for pressed in [true, false] {
                        self.steps.push_back(Step::Events(vec![egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        }]));
                    }
                }
                "text" => {
                    let rest = line.trim_start().strip_prefix("text").unwrap_or("").trim();
                    self.steps
                        .push_back(Step::Events(vec![egui::Event::Text(rest.to_string())]));
                }
                "wait" if !nums.is_empty() => self
                    .steps
                    .push_back(Step::Wait(Duration::from_millis(nums[0] as u64))),
                "focus" => self.steps.push_back(Step::Focus),
                "shot" => {
                    if let Some(p) = words.next() {
                        self.steps.push_back(Step::Shot(PathBuf::from(p)));
                    }
                }
                _ => log::warn!("dev script: cannot read {line:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_presses_moves_in_steps_and_releases_at_the_end() {
        let mut script = DevScript::default();
        script.parse(
            "drag 10 20 110 20 200
click 1 1",
        );
        let mut events = Vec::new();
        let mut waited = Duration::ZERO;
        for step in script.steps.drain(..) {
            match step {
                Step::Events(e) => events.extend(e),
                Step::Wait(d) => waited += d,
                Step::Shot(_) | Step::Drop(_) | Step::Focus => {}
            }
        }
        assert_eq!(waited, Duration::from_millis(200));
        // Press comes first at the start, release after the last move at the end.
        assert!(matches!(
            events[1],
            egui::Event::PointerButton { pressed: true, pos, .. } if pos == egui::pos2(10.0, 20.0)
        ));
        let moves: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::PointerMoved(p) => Some(p.x),
                _ => None,
            })
            .collect();
        assert!(moves.windows(2).take(10).all(|w| w[0] <= w[1]));
        assert!(moves.contains(&60.0), "moves in between: {moves:?}");
        let release = events
            .iter()
            .position(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. }))
            .unwrap();
        assert!(matches!(
            events[release - 1],
            egui::Event::PointerMoved(p) if p == egui::pos2(110.0, 20.0)
        ));
        assert!(matches!(
            events[release],
            egui::Event::PointerButton { pos, .. } if pos == egui::pos2(110.0, 20.0)
        ));
    }

    #[test]
    fn drop_hands_the_window_a_file_at_a_pointer_position() {
        let mut script = DevScript::default();
        script.parse(
            "drop C:/some dir/a b.mp3 40 50
drop x.wav",
        );
        let mut drops = Vec::new();
        let mut moves = Vec::new();
        for step in script.steps.drain(..) {
            match step {
                Step::Drop(f) => drops.push(f.path().to_path_buf()),
                Step::Events(e) => moves.extend(e.into_iter().filter_map(|e| match e {
                    egui::Event::PointerMoved(p) => Some(p),
                    _ => None,
                })),
                _ => {}
            }
        }
        assert_eq!(
            drops,
            [PathBuf::from("C:/some dir/a b.mp3"), PathBuf::from("x.wav")]
        );
        // The pointer is at the given position, and stays there for a drop without one.
        assert_eq!(moves, [egui::pos2(40.0, 50.0), egui::pos2(40.0, 50.0)]);
    }

    /// A `wait` keeps the window drawing until it ends. egui drops a
    /// delayed repaint asked for from the input hook, so a script's `wait`
    /// stalled (for seconds, live) until something else repainted.
    #[test]
    fn a_wait_asks_for_the_frame_that_ends_it() {
        let ctx = egui::Context::default();
        // A new context repaints its first passes for itself.
        for _ in 0..5 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
            out.textures_delta.clear();
        }
        let mut script = DevScript {
            path: Some(PathBuf::from("no-such-dev-script")),
            started: true,
            ..DevScript::default()
        };
        script.parse(
            "wait 200
move 1 1",
        );
        let mut delay = Duration::MAX;
        for _ in 0..2 {
            let mut raw = egui::RawInput::default();
            script.input(&ctx, &mut raw);
            let mut out = ctx.run_ui(raw, |ui| script.frame(ui.ctx()));
            out.textures_delta.clear();
            delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        }
        assert!(delay <= Duration::from_millis(200), "{delay:?}");
    }
}
