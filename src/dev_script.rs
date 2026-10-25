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
//! shot PATH [LOOKS] write a PNG of the window's frame buffer to PATH; with
//!                   LOOKS, the app's window also writes each anchor's look
//!                   there, of the pass the picture shows, as
//!                   `--demo-dump-looks` does. What follows waits until the
//!                   picture is written (to PATH whole: a reader that sees
//!                   the file sees all of it, and LOOKS before it)
//! window N          play what follows in plugin window N (a plugin's
//!                   `BrowserWindow`, by its id: 2 is the first), or in the
//!                   app's window with `window 0`
//! focus             from here on the window reports itself focused, as
//!                   one the user is working in (the app and plugins then
//!                   act as for a window in front, not in the background)
//! ```
//!
//! Demo controls change the app rather than play input, so one window can
//! take one demo shot after another (the theme parity checker's run, which
//! otherwise starts a new window for each): the app's window takes them
//! with [`DevScript::take_controls`] (see [`DemoControl`]).
//!
//! ```text
//! theme URL[,URL…]  the Luna themes become these, installed in that order
//!                   as `TIDALFAST_DEMO_LUNA` installs them at start, and
//!                   every theme installed before is removed; `none` for no
//!                   theme (needs Luna in the demo: `TIDALFAST_DEMO_LUNA`).
//!                   What follows waits until they are in and loaded
//! page PAGE [SHOW]  the demo's data and the window's state as a new demo
//!                   window has them, then `--demo-page PAGE` and
//!                   `--demo-show SHOW`
//! size W H          the window's inner size, in points, held there as
//!                   `--demo-size` holds it
//! quit              close the window
//! ```
//!
//! Once a control was played, the window draws frame after frame while
//! the script runs, as a `--demo-shot` window does until its shot.
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

/// A demo control (see the module's documentation), for the app's window
/// to apply to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum DemoControl {
    /// `theme`: the theme URLs to install, in order; empty for none.
    Themes(Vec<String>),
    /// `page`: the page and the `--demo-show` surfaces.
    Page { page: String, show: Option<String> },
    /// `size`: the inner size, in points.
    Size(egui::Vec2),
    /// `quit`.
    Quit,
}

enum Step {
    Events(Vec<egui::Event>),
    Wait(Duration),
    Shot(Shot),
    Drop(egui::DroppedFileHandle),
    Focus,
    /// What follows goes to this window (`None`: the app's).
    Window(Option<egui::ViewportId>),
    Control(DemoControl),
}

/// A picture `shot` asked for.
#[derive(Clone, Debug, PartialEq)]
struct Shot {
    png: PathBuf,
    /// Where the anchors' looks go (the app's window only).
    looks: Option<PathBuf>,
}

#[derive(Default)]
pub struct DevScript {
    path: Option<PathBuf>,
    started: bool,
    steps: VecDeque<Step>,
    resume_at: Option<Instant>,
    pos: egui::Pos2,
    shot: Option<Shot>,
    /// The app's window asked for the picture of [`DevScript::shot`]; its
    /// pass writes the looks.
    shot_asked: bool,
    looks_due: Option<PathBuf>,
    /// `focus` was played: the window reports itself focused.
    focus: bool,
    /// The plugin window the script plays in (`window N`); the app's
    /// window when `None`.
    target: Option<egui::ViewportId>,
    /// Controls played and not yet taken by the app's window.
    controls: Vec<DemoControl>,
    /// A control was played: the window draws while the script runs.
    controlled: bool,
    /// A `theme` control was played then: what follows waits until the
    /// themes are in ([`crate::demo::themes_ready`]), for half a minute at
    /// most.
    themes_since: Option<Instant>,
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
        // Each window's pass comes here; the script plays in its target's.
        // A plugin window that closed hands the script back to the app's.
        if raw.viewport_id == egui::ViewportId::ROOT
            && self.target.is_some_and(|t| !raw.viewports.contains_key(&t))
            && self.started
        {
            self.target = None;
        }
        let target = self.target.unwrap_or(egui::ViewportId::ROOT);
        if raw.viewport_id != target {
            if self.started && (!self.steps.is_empty() || self.shot.is_some() || path.exists()) {
                ctx.request_repaint_of(target);
            }
            return;
        }
        if self.target.is_some() {
            self.plugin_window_shot(raw);
        }
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
        // What follows a shot waits until its picture is written, and what
        // follows a `theme` control until the themes are in.
        if self.shot.is_some() {
            ctx.request_repaint_of(target);
            return;
        }
        if let Some(since) = self.themes_since {
            if !crate::demo::themes_ready() && since.elapsed() < Duration::from_secs(30) {
                ctx.request_repaint_of(target);
                return;
            }
            self.themes_since = None;
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
            Some(Step::Shot(shot)) => {
                // The app's window asks in its own pass ([`Self::frame`]),
                // as a `--demo-shot` window does, so the looks written at
                // that pass's end are the picture's.
                if self.target.is_some() {
                    ctx.send_viewport_cmd_to(
                        target,
                        egui::ViewportCommand::Screenshot(egui::UserData::default()),
                    );
                }
                self.shot = Some(shot);
                self.shot_asked = false;
            }
            Some(Step::Window(id)) => {
                self.target = id;
                ctx.request_repaint_of(id.unwrap_or(egui::ViewportId::ROOT));
            }
            Some(Step::Control(control)) => {
                // A new page starts with no pointer over the window, as a
                // new window does.
                if matches!(control, DemoControl::Page { .. }) {
                    raw.events.push(egui::Event::PointerGone);
                }
                if matches!(control, DemoControl::Themes(_)) {
                    self.themes_since = Some(Instant::now());
                }
                self.controlled = true;
                self.controls.push(control);
            }
            None => return,
        }
        ctx.request_repaint_of(target);
    }

    /// The demo controls played since the last call, oldest first, for the
    /// app's window to apply (from `logic`, before the pass draws).
    pub fn take_controls(&mut self) -> Vec<DemoControl> {
        std::mem::take(&mut self.controls)
    }

    /// Where the anchors' looks go at the end of this pass, the one the
    /// picture asked for shows (from the app's `ui`, after it drew).
    pub fn looks_due(&mut self) -> Option<PathBuf> {
        self.looks_due.take()
    }

    /// A plugin window's screenshot arrives with its input (the app's
    /// window takes its own in [`Self::frame`]).
    fn plugin_window_shot(&mut self, raw: &egui::RawInput) {
        if self.shot.is_none() {
            return;
        }
        let Some(image) = raw.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        }) else {
            return;
        };
        if let Some(shot) = self.shot.take() {
            save_png(&image, &shot.png);
        }
    }

    /// Ask for the app's window's picture asked for by `shot`, and write it
    /// once it arrives (from `logic`).
    pub fn frame(&mut self, ctx: &egui::Context) {
        // A `wait` asks for the frame that ends it from here: egui drops a
        // delayed repaint asked for in the input hook (the pass about to
        // run counts as serving it), and the script then stalled until
        // something else repainted the window.
        if let Some(at) = self.resume_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
        // A controlled window draws on, as a `--demo-shot` window does
        // until its shot: what the next picture shows comes about frame by
        // frame (pictures, the theme's arrival, its reveal).
        if self.controlled && (!self.steps.is_empty() || self.shot.is_some()) {
            ctx.request_repaint();
        }
        if self.target.is_some() {
            return;
        }
        let Some(shot) = &self.shot else {
            return;
        };
        if !std::mem::replace(&mut self.shot_asked, true) {
            self.looks_due = shot.looks.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            ctx.request_repaint();
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
        let Some(shot) = self.shot.take() else { return };
        save_png(&image, &shot.png);
        ctx.request_repaint();
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
                "window" if !nums.is_empty() => {
                    let n = nums[0] as u32;
                    self.steps.push_back(Step::Window(
                        (n > 0).then(|| crate::ui::plugin_windows::viewport_id(n)),
                    ));
                }
                "shot" => {
                    if let Some(p) = words.next() {
                        self.steps.push_back(Step::Shot(Shot {
                            png: PathBuf::from(p),
                            looks: words.next().map(PathBuf::from),
                        }));
                    }
                }
                "theme" => {
                    let urls = words.next().unwrap_or("none");
                    let urls = urls
                        .split(',')
                        .map(str::trim)
                        .filter(|u| !u.is_empty() && *u != "none")
                        .map(str::to_string)
                        .collect();
                    self.steps
                        .push_back(Step::Control(DemoControl::Themes(urls)));
                }
                "page" => {
                    if let Some(page) = words.next() {
                        self.steps.push_back(Step::Control(DemoControl::Page {
                            page: page.to_string(),
                            show: words.next().map(str::to_string),
                        }));
                    }
                }
                "size" if nums.len() >= 2 => {
                    self.steps
                        .push_back(Step::Control(DemoControl::Size(egui::vec2(
                            nums[0], nums[1],
                        ))))
                }
                "quit" => self.steps.push_back(Step::Control(DemoControl::Quit)),
                _ => log::warn!("dev script: cannot read {line:?}"),
            }
        }
    }
}

/// Writes the picture whole: to a file beside `path`, then moved there, so
/// a script waiting for `path` never reads half of it.
fn save_png(image: &egui::ColorImage, path: &std::path::Path) {
    let [w, h] = [image.size[0] as u32, image.size[1] as u32];
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    let Some(buffer) = image::RgbaImage::from_raw(w, h, pixels) else {
        return;
    };
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = buffer
        .save_with_format(&part, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
        .and_then(|()| std::fs::rename(&part, path).map_err(|e| e.to_string()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        log::error!("dev script: could not write {}: {e}", path.display());
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
                Step::Shot(_)
                | Step::Drop(_)
                | Step::Focus
                | Step::Window(_)
                | Step::Control(_) => {}
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

    /// The parity checker's one-window run: the controls reach the app's
    /// window in order, a `shot` asks for the picture and the looks in one
    /// pass, and nothing after it plays until its picture is written whole.
    #[test]
    fn controls_reach_the_app_and_a_shot_holds_what_follows() {
        let dir = std::env::temp_dir().join(format!(
            "dev-script-controls-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (png, looks) = (dir.join("album.png"), dir.join("album.json"));
        let ctx = egui::Context::default();
        let mut script = DevScript {
            path: Some(dir.join("no-such-dev-script")),
            started: true,
            ..DevScript::default()
        };
        script.parse(&format!(
            "theme http://h/a/base.css,http://h/a/theme.css
page album:alb0 queue,quality-pill
shot {} {}
theme none
size 800 600
quit",
            png.display(),
            looks.display()
        ));
        // One pass: what the app's window takes, whether it asked for a
        // picture, and where its looks go.
        let pass = |script: &mut DevScript, picture: bool| {
            let mut raw = egui::RawInput::default();
            if picture {
                raw.events.push(egui::Event::Screenshot {
                    viewport_id: egui::ViewportId::ROOT,
                    user_data: egui::UserData::default(),
                    image: std::sync::Arc::new(egui::ColorImage::filled(
                        [2, 2],
                        egui::Color32::RED,
                    )),
                });
            }
            script.input(&ctx, &mut raw);
            let gone = raw.events.contains(&egui::Event::PointerGone);
            let mut out = ctx.run_ui(raw, |ui| script.frame(ui.ctx()));
            out.textures_delta.clear();
            let asked = out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Screenshot(_)));
            (script.take_controls(), gone, asked, script.looks_due())
        };
        let theme = ["http://h/a/base.css", "http://h/a/theme.css"].map(String::from);
        assert_eq!(
            pass(&mut script, false),
            (
                vec![DemoControl::Themes(theme.to_vec())],
                false,
                false,
                None
            )
        );
        // What follows waits until the app has the themes in.
        crate::demo::set_themes_ready(false);
        assert_eq!(pass(&mut script, false), (vec![], false, false, None));
        crate::demo::set_themes_ready(true);
        // A new page starts with no pointer over the window.
        let page = DemoControl::Page {
            page: "album:alb0".into(),
            show: Some("queue,quality-pill".into()),
        };
        assert_eq!(pass(&mut script, false), (vec![page], true, false, None));
        assert_eq!(
            pass(&mut script, false),
            (vec![], false, true, Some(looks.clone()))
        );
        // Until the picture arrives, nothing else plays.
        for _ in 0..3 {
            assert_eq!(pass(&mut script, false), (vec![], false, false, None));
        }
        assert!(!png.exists());
        assert_eq!(pass(&mut script, true), (vec![], false, false, None));
        assert!(png.exists(), "the picture is written");
        let mut part = png.clone().into_os_string();
        part.push(".part");
        assert!(!std::path::Path::new(&part).exists());
        let rest: Vec<DemoControl> = (0..3).flat_map(|_| pass(&mut script, false).0).collect();
        assert_eq!(
            rest,
            [
                DemoControl::Themes(Vec::new()),
                DemoControl::Size(egui::vec2(800.0, 600.0)),
                DemoControl::Quit
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
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
