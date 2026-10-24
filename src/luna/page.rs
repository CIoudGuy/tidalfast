//! Plugin windows: Electron's `BrowserWindow` for plugins' `.native` code.
//!
//! In Electron each window's page runs in a renderer of its own, with its
//! own document, stylesheets and scripts. Here each window that loads a
//! page gets a QuickJS runtime on a thread of its own with the browser-ish
//! shims (`prelude.js`, `dom.js` and `css.js` in page mode, then `page.js`),
//! so a page never shares a realm, a document or a watchdog with the
//! plugins, and a busy page never holds up the plugins' thread. What the
//! page's document holds goes to the UI as a [`DomOverlay`] of its own,
//! which the UI draws in a native window with the same renderer as plugin
//! elements in the app (`src/ui/plugin_windows.rs`); input there comes back
//! as DOM events.
//!
//! * The page has no privileges: no `require`, no files, no network. Its
//!   preload script runs first with the renderer's `electron` module
//!   (`contextBridge`, `ipcRenderer`), as a sandboxed preload does, and
//!   messages to and from the plugin are copied (structured clone).
//! * A window exists while the plugin keeps it: its viewport, runtime and
//!   thread go when it closes, when its plugin stops, or when Luna stops.
//!   With no window open nothing here runs and the UI checks one counter.
//! * Nothing here knows any plugin: what a window shows is whatever page
//!   the plugin loads.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::function::Rest;
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Function, Persistent, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Msg;
use super::dom::{DomEvent, DomOverlay, HostTree};
use super::layout::TreeCell;

/// Windows one plugin may have at a time, and all plugins together.
pub const MAX_PER_PLUGIN: usize = 4;
pub const MAX_WINDOWS: usize = 8;
/// A page's heap.
const PAGE_MEMORY: usize = 128 << 20;
/// How long one task of a page's scripts may run before it is stopped (a
/// page stuck in a loop), as a browser offers to stop such a page.
const SCRIPT_BUDGET: Duration = Duration::from_secs(10);
/// The element ids of window `n`'s page start at `n * VID_STRIDE` (dom.js
/// `vidSeq`), apart from the app page's and every other window's: the UI
/// keys its per-element state by them.
pub const VID_STRIDE: u64 = 1 << 40;
/// The largest overlay a page may publish (as the plugins' page).
const MAX_OVERLAY_BYTES: usize = 16 << 20;
/// The largest message one side sends the other (serialised).
pub const MAX_MESSAGE: usize = 64 << 20;

/// A window as the plugin asked for it (Electron's constructor options and
/// what it changed since). Lengths are in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowSpec {
    pub title: String,
    pub width: f32,
    pub height: f32,
    /// Where the plugin placed it; `None` lets the system place it.
    pub x: Option<f32>,
    pub y: Option<f32>,
    /// Size limits; 0 is none.
    pub min_width: f32,
    pub min_height: f32,
    pub max_width: f32,
    pub max_height: f32,
    /// See-through where the page paints nothing.
    pub transparent: bool,
    /// With the system's title bar and borders.
    pub frame: bool,
    pub always_on_top: bool,
    pub resizable: bool,
    pub movable: bool,
    /// Shown (`show`, `show()`, `hide()`).
    pub visible: bool,
    /// Takes the keyboard focus when shown or clicked.
    pub focusable: bool,
    pub skip_taskbar: bool,
    /// The window's own colour behind the page (white when not
    /// transparent, as in Electron).
    pub background_color: Option<String>,
    /// Bumped whenever the plugin moves or resizes the window, so the UI
    /// applies the new bounds once and otherwise leaves where the user put
    /// it.
    pub placed: u64,
    /// Bumped by `focus()` and `moveTop()`.
    pub raised: u64,
}

impl Default for WindowSpec {
    fn default() -> Self {
        Self {
            title: String::new(),
            width: 800.0,
            height: 600.0,
            x: None,
            y: None,
            min_width: 0.0,
            min_height: 0.0,
            max_width: 0.0,
            max_height: 0.0,
            transparent: false,
            frame: true,
            always_on_top: false,
            resizable: true,
            movable: true,
            visible: true,
            focusable: true,
            skip_taskbar: false,
            background_color: None,
            placed: 0,
            raised: 0,
        }
    }
}

/// What a window's page is told.
pub enum ToPage {
    /// `webContents.send(channel, ...args)`: the arguments, serialised.
    Ipc { channel: String, args: String },
    /// User input on the window.
    Event(DomEvent),
    /// A new tree from the UI is in the window's [`TreeCell`].
    Sync,
    /// An asynchronous op the page asked for finished.
    OpDone { id: f64, ok: bool, json: String },
    /// The window closed (its stop flag is set): wakes the page's thread.
    Stop,
}

/// What a window's page or its UI tells the Luna thread.
pub enum FromPage {
    /// `ipcRenderer.send` (`invoke` None) or `invoke` (its op id).
    Ipc {
        window: u32,
        channel: String,
        args: String,
        invoke: Option<f64>,
    },
    /// The page finished loading.
    Loaded { window: u32, title: String },
    /// A line the page logged, or an error it raised.
    Log {
        window: u32,
        level: String,
        msg: String,
    },
    /// The page called `window.close()`, or the user closed the window
    /// (`kind` "close"); the window moved or was resized ("bounds"), gained
    /// or lost the focus ("focus" / "blur").
    User {
        window: u32,
        kind: &'static str,
        data: Value,
    },
    /// The page's runtime stopped by itself (it failed to start).
    Ended { window: u32, generation: u64 },
}

/// A running page: its channel and its stop flag.
struct PageThread {
    tx: Sender<ToPage>,
    stop: Arc<AtomicBool>,
    generation: u64,
}

/// A plugin window, shared by the Luna thread, the window's page and the
/// UI.
pub struct PageWindow {
    pub id: u32,
    pub plugin: String,
    spec: Mutex<WindowSpec>,
    overlay: Mutex<Arc<DomOverlay>>,
    trees: TreeCell,
    page: Mutex<Option<PageThread>>,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    luna: Mutex<Sender<Msg>>,
    /// The display under the window's middle and the window's scale, as
    /// the UI last saw them (the page's `screen` and `devicePixelRatio`).
    screen: Mutex<(Option<u32>, f32)>,
}

impl PageWindow {
    fn new(id: u32, plugin: &str, spec: WindowSpec, luna: Sender<Msg>) -> Self {
        Self {
            id,
            plugin: plugin.to_string(),
            spec: Mutex::new(spec),
            overlay: Mutex::new(Arc::new(DomOverlay::default())),
            trees: TreeCell::default(),
            page: Mutex::new(None),
            waker: Mutex::new(None),
            luna: Mutex::new(luna),
            screen: Mutex::new((None, 0.0)),
        }
    }

    /// The window is on `display` (see [`super::screen`]) at `scale` points
    /// per pixel.
    pub fn set_screen(&self, display: Option<u32>, scale: f32) {
        if let Ok(mut s) = self.screen.lock()
            && *s != (display, scale)
        {
            *s = (display, scale);
        }
    }

    /// What [`PageWindow::set_screen`] last said.
    pub fn screen(&self) -> (Option<u32>, f32) {
        self.screen.lock().map(|s| *s).unwrap_or((None, 0.0))
    }

    /// The window as the plugin last set it.
    pub fn spec(&self) -> WindowSpec {
        self.spec.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// What the page's document holds, for the UI to draw. Cheap: an Arc
    /// clone.
    pub fn overlay(&self) -> Arc<DomOverlay> {
        self.overlay.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// User input on the window, for the page's listeners.
    pub fn dom_event(&self, event: DomEvent) {
        self.to_page(ToPage::Event(event));
    }

    /// The UI's layout of the page (its size and the boxes scripts asked
    /// for); send it only when it changed.
    pub fn sync_dom(&self, tree: impl Into<Arc<HostTree>>) {
        let (slot, arrived) = &*self.trees;
        if let Ok(mut slot) = slot.lock() {
            slot.tree = Some(tree.into());
            arrived.notify_all();
        }
        self.to_page(ToPage::Sync);
    }

    /// The UI starts drawing overlay `version`, which can move boxes (see
    /// [`crate::luna::Luna::drawing_boxes`]).
    pub fn drawing_boxes(&self, version: u64) {
        let (slot, arrived) = &*self.trees;
        if let Ok(mut slot) = slot.lock() {
            slot.drawing(version);
            arrived.notify_all();
        }
    }

    /// How to ask the UI to draw the window again (its viewport's repaint).
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(waker);
        }
    }

    /// Whether the UI gave this window a waker yet.
    pub fn has_waker(&self) -> bool {
        self.waker.lock().is_ok_and(|w| w.is_some())
    }

    /// The user asked to close the window (its close button, Alt+F4): the
    /// plugin hears `close` and, unless it keeps the window, it closes.
    pub fn user_close(&self) {
        self.user("close", Value::Null);
    }

    /// The window now sits at `[x, y, width, height]` (outer, points).
    pub fn user_bounds(&self, bounds: [f32; 4]) {
        self.user("bounds", json!(bounds));
    }

    /// The window gained or lost the keyboard focus.
    pub fn user_focus(&self, focused: bool) {
        self.user(if focused { "focus" } else { "blur" }, Value::Null);
    }

    fn user(&self, kind: &'static str, data: Value) {
        if let Ok(tx) = self.luna.lock() {
            let _ = tx.send(Msg::Page(FromPage::User {
                window: self.id,
                kind,
                data,
            }));
        }
    }

    fn to_page(&self, msg: ToPage) {
        if let Ok(p) = self.page.lock()
            && let Some(p) = p.as_ref()
        {
            let _ = p.tx.send(msg);
        }
    }

    fn wake(&self) {
        let w = self.waker.lock().ok().and_then(|w| w.clone());
        if let Some(w) = w {
            w();
        }
    }

    fn stop_page(&self) {
        if let Ok(mut p) = self.page.lock()
            && let Some(p) = p.take()
        {
            p.stop.store(true, Ordering::Release);
            let _ = p.tx.send(ToPage::Stop);
        }
    }
}

/// The open plugin windows, shared by the Luna thread and the UI.
#[derive(Default)]
pub struct PageWindows {
    list: Mutex<Vec<Arc<PageWindow>>>,
    count: AtomicUsize,
}

impl PageWindows {
    /// The open windows; with none open, one atomic load.
    pub fn all(&self) -> Vec<Arc<PageWindow>> {
        if self.count.load(Ordering::Acquire) == 0 {
            return Vec::new();
        }
        self.list.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// How many windows are open.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    fn get(&self, id: u32) -> Option<Arc<PageWindow>> {
        self.list.lock().ok()?.iter().find(|w| w.id == id).cloned()
    }

    fn add(&self, w: Arc<PageWindow>) {
        if let Ok(mut l) = self.list.lock() {
            l.push(w);
            self.count.store(l.len(), Ordering::Release);
        }
    }

    fn remove(&self, id: u32) -> Option<Arc<PageWindow>> {
        let mut l = self.list.lock().ok()?;
        let i = l.iter().position(|w| w.id == id)?;
        let w = l.remove(i);
        self.count.store(l.len(), Ordering::Release);
        Some(w)
    }
}

/// The page a window loads.
pub struct Load {
    pub url: String,
    pub html: String,
    /// The preload script's source and file name (read when the page
    /// loads, as Electron reads it then).
    pub preload: Option<(String, String)>,
}

/// The plugin windows, on the Luna thread.
pub(crate) struct PageHub {
    windows: Arc<PageWindows>,
    generation: u64,
}

impl PageHub {
    pub fn new(windows: Arc<PageWindows>) -> Self {
        Self {
            windows,
            generation: 0,
        }
    }

    /// The plugin that owns window `id`.
    pub fn plugin_of(&self, id: u32) -> Option<String> {
        self.windows.get(id).map(|w| w.plugin.clone())
    }

    fn owned(&self, plugin: &str, id: u32) -> Result<Arc<PageWindow>, String> {
        self.windows
            .get(id)
            .filter(|w| w.plugin == plugin)
            .ok_or_else(|| format!("no window {id}"))
    }

    /// `new BrowserWindow(options)`: the window exists (and shows, unless
    /// `show: false`) with an empty page until the plugin loads one.
    pub fn create(
        &mut self,
        plugin: &str,
        id: u32,
        spec: WindowSpec,
        luna: Sender<Msg>,
    ) -> Result<(), String> {
        if self.windows.get(id).is_some() {
            return Err(format!("window {id} exists"));
        }
        let all = self.windows.all();
        if all.len() >= MAX_WINDOWS
            || all.iter().filter(|w| w.plugin == plugin).count() >= MAX_PER_PLUGIN
        {
            return Err("EAGAIN: too many plugin windows are open".into());
        }
        self.windows
            .add(Arc::new(PageWindow::new(id, plugin, spec, luna)));
        log::info!(target: "luna", "{plugin} opened window {id}");
        Ok(())
    }

    /// `loadURL` / `loadFile`: the window's page starts over with `load`.
    pub fn load(&mut self, plugin: &str, id: u32, load: Load) -> Result<(), String> {
        let w = self.owned(plugin, id)?;
        w.stop_page();
        if let Ok(mut o) = w.overlay.lock() {
            let version = o.version + 1;
            *o = Arc::new(DomOverlay {
                version,
                ..DomOverlay::default()
            });
        }
        self.generation += 1;
        let generation = self.generation;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let luna = w.luna.lock().map_err(|e| e.to_string())?.clone();
        let state = Spawn {
            window: w.clone(),
            luna,
            own: tx.clone(),
            stop: stop.clone(),
            generation,
        };
        std::thread::Builder::new()
            .name(format!("luna-window-{id}"))
            .stack_size(16 * 1024 * 1024)
            .spawn(move || page_main(state, load, rx))
            .map_err(|e| e.to_string())?;
        if let Ok(mut p) = w.page.lock() {
            *p = Some(PageThread {
                tx,
                stop,
                generation,
            });
        }
        w.wake();
        Ok(())
    }

    /// The plugin changed the window (`setBounds`, `show`, ...): `patch`
    /// holds the [`WindowSpec`] fields it set.
    pub fn set(&mut self, plugin: &str, id: u32, patch: &Value) -> Result<(), String> {
        let w = self.owned(plugin, id)?;
        let mut spec = w.spec.lock().map_err(|e| e.to_string())?;
        let mut v = serde_json::to_value(&*spec).map_err(|e| e.to_string())?;
        if let (Some(v), Some(p)) = (v.as_object_mut(), patch.as_object()) {
            for (k, x) in p {
                v.insert(k.clone(), x.clone());
            }
        }
        let mut next: WindowSpec = serde_json::from_value(v).map_err(|e| e.to_string())?;
        let moved = ["x", "y", "width", "height"]
            .iter()
            .any(|k| patch.get(*k).is_some());
        if moved {
            next.placed = spec.placed + 1;
        }
        if patch.get("raise").and_then(Value::as_bool) == Some(true) {
            next.raised = spec.raised + 1;
        }
        *spec = next;
        drop(spec);
        w.wake();
        Ok(())
    }

    /// `webContents.send(channel, ...args)`.
    pub fn send(&self, plugin: &str, id: u32, channel: String, args: String) -> Result<(), String> {
        if args.len() > MAX_MESSAGE {
            return Err("the message is too large".into());
        }
        self.owned(plugin, id)?
            .to_page(ToPage::Ipc { channel, args });
        Ok(())
    }

    /// The plugin's answer to the page's `ipcRenderer.invoke`.
    pub fn reply(
        &self,
        plugin: &str,
        id: u32,
        op: f64,
        ok: bool,
        json: String,
    ) -> Result<(), String> {
        self.owned(plugin, id)?
            .to_page(ToPage::OpDone { id: op, ok, json });
        Ok(())
    }

    /// The window closes: its page stops and the UI lets go of it.
    pub fn close(&mut self, plugin: &str, id: u32) -> bool {
        if self.owned(plugin, id).is_err() {
            return false;
        }
        if let Some(w) = self.windows.remove(id) {
            w.stop_page();
            w.wake();
            log::info!(target: "luna", "{plugin} closed window {id}");
        }
        true
    }

    /// The page of window `id` stopped by itself.
    pub fn ended(&mut self, id: u32, generation: u64) -> bool {
        let Some(w) = self.windows.get(id) else {
            return false;
        };
        let Ok(mut page) = w.page.lock() else {
            return false;
        };
        if page.as_ref().is_some_and(|p| p.generation == generation) {
            *page = None;
            return true;
        }
        false
    }

    /// A plugin stopped: its windows close.
    pub fn close_plugin(&mut self, plugin: &str) -> Vec<u32> {
        let ids: Vec<u32> = self
            .windows
            .all()
            .iter()
            .filter(|w| w.plugin == plugin)
            .map(|w| w.id)
            .collect();
        for id in &ids {
            self.close(plugin, *id);
        }
        ids
    }

    /// Luna stops: every window closes.
    pub fn stop_all(&mut self) {
        for w in self.windows.all() {
            self.close(&w.plugin.clone(), w.id);
        }
    }
}

// ---------------------------------------------------------------- the page's thread

struct Spawn {
    window: Arc<PageWindow>,
    luna: Sender<Msg>,
    /// The page's own channel, for ops it settles itself.
    own: Sender<ToPage>,
    stop: Arc<AtomicBool>,
    generation: u64,
}

/// The page's thread state, for the natives.
struct State {
    window: Arc<PageWindow>,
    luna: Sender<Msg>,
    own: Sender<ToPage>,
    /// The newest overlay that can move boxes, and the newest one a tree
    /// said it was laid out with (as the plugins' runtime keeps them).
    boxes_version: Cell<u64>,
    laid_out: Cell<u64>,
    /// When the script running now must stop.
    deadline: Rc<Cell<Option<Instant>>>,
    /// Lines to log, sent after the call that logged them.
    logs: RefCell<VecDeque<(String, String)>>,
}

impl State {
    fn log(&self, level: &str, msg: String) {
        let _ = self.luna.send(Msg::Page(FromPage::Log {
            window: self.window.id,
            level: level.into(),
            msg,
        }));
    }
}

const ENTRIES: &[&str] = &[
    "__luna_tick",
    "__luna_resolve",
    "__luna_dom",
    "__page_load",
    "__page_ipc",
];

const LOCKDOWN: &str = r#"for (const k of ["__op", "__op_async", "__caller", "__eval", "__utf8_enc", "__utf8_dec", "__b64_enc", "__b64_dec", "__fetch_take", "__fs_bytes", "__lunaInternal", "__luna_tick", "__luna_resolve", "__luna_dom", "__page_load", "__page_ipc"]) delete globalThis[k];"#;

/// The shims a page runs, in order: the browser-ish globals, the document
/// and the cascade (in page mode, set in between), then `page.js`.
const PAGE_SHIMS: &[(&str, &str)] = &[
    ("prelude.js", include_str!("js/prelude.js")),
    ("dom.js", include_str!("js/dom.js")),
    ("css.js", include_str!("js/css.js")),
    ("page.js", include_str!("js/page.js")),
];

fn page_main(spawn: Spawn, load: Load, rx: Receiver<ToPage>) {
    let (window, generation, luna) = (spawn.window.id, spawn.generation, spawn.luna.clone());
    if let Err(e) = run(spawn, load, rx) {
        let _ = luna.send(Msg::Page(FromPage::Log {
            window,
            level: "error".into(),
            msg: format!("the window's page stopped: {e}"),
        }));
        let _ = luna.send(Msg::Page(FromPage::Ended { window, generation }));
    }
}

fn run(spawn: Spawn, load: Load, rx: Receiver<ToPage>) -> Result<(), String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(PAGE_MEMORY);
    rt.set_max_stack_size(8 * 1024 * 1024);
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    {
        let d = deadline.clone();
        let stop = spawn.stop.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            stop.load(Ordering::Acquire) || d.get().is_some_and(|t| Instant::now() > t)
        })));
    }
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    let state = Rc::new(State {
        window: spawn.window.clone(),
        luna: spawn.luna.clone(),
        own: spawn.own.clone(),
        boxes_version: Cell::new(0),
        laid_out: Cell::new(0),
        deadline,
        logs: RefCell::new(VecDeque::new()),
    });
    let entries = install(&ctx, &state)?;
    let result = drive(&spawn, &state, &rx, &rt, &ctx, &entries, load);
    drop(entries);
    result
}

fn install(ctx: &Context, state: &Rc<State>) -> Result<Entries, String> {
    ctx.with(|ctx| -> Result<Entries, String> {
        let g = ctx.globals();
        let js = |e: rquickjs::Error| e.to_string();
        let s = state.clone();
        g.set(
            "__op",
            Function::new(
                ctx.clone(),
                move |name: String, arg: String, _claimed: Option<String>| -> String {
                    if name == "dom.overlay" && arg.len() > MAX_OVERLAY_BYTES {
                        return json!({ "err": "the page is too large to show" }).to_string();
                    }
                    let a: Value = serde_json::from_str(&arg).unwrap_or(Value::Null);
                    match op(&s, &name, &a) {
                        Ok(v) => json!({ "ok": v }),
                        Err(e) => json!({ "err": e }),
                    }
                    .to_string()
                },
            )
            .map_err(js)?,
        )
        .map_err(js)?;
        let s = state.clone();
        g.set(
            "__op_async",
            Function::new(
                ctx.clone(),
                move |name: String, arg: String, id: f64, _claimed: Option<String>| {
                    op_async(&s, &name, &arg, id);
                },
            )
            .map_err(js)?,
        )
        .map_err(js)?;
        // A page's code is nobody's plugin: nothing it does is granted as
        // a plugin's.
        g.set(
            "__caller",
            Function::new(ctx.clone(), || -> Option<String> { None }).map_err(js)?,
        )
        .map_err(js)?;
        g.set("__eval", Function::new(ctx.clone(), eval_as).map_err(js)?)
            .map_err(js)?;
        g.set(
            "__utf8_enc",
            Function::new(ctx.clone(), super::runtime::utf8_enc).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__utf8_dec",
            Function::new(ctx.clone(), super::runtime::utf8_dec).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__b64_enc",
            Function::new(ctx.clone(), super::runtime::b64_enc).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__b64_dec",
            Function::new(ctx.clone(), super::runtime::b64_dec).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__fetch_take",
            Function::new(
                ctx.clone(),
                |c: Ctx<'_>, _t: String| -> rquickjs::Result<()> {
                    Err(rquickjs::Exception::throw_message(
                        &c,
                        "not available in a plugin window",
                    ))
                },
            )
            .map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__fs_bytes",
            Function::new(ctx.clone(), |c: Ctx<'_>| -> rquickjs::Result<()> {
                Err(rquickjs::Exception::throw_message(
                    &c,
                    "not available in a plugin window",
                ))
            })
            .map_err(js)?,
        )
        .map_err(js)?;
        let eval = |src: &str, file: &str| -> Result<(), String> {
            let mut opts = EvalOptions::default();
            opts.strict = false;
            opts.filename = Some(format!("luna:{file}"));
            ctx.eval_with_options::<(), _>(src, opts)
                .catch(&ctx)
                .map_err(|e| format!("{file}: {}", failure(&ctx, e)))
        };
        eval(
            &format!(
                "globalThis.__lunaVersions = Object.freeze({}); globalThis.__lunaStatsOn = false;",
                super::versions::js_object()
            ),
            "versions.js",
        )?;
        for (file, src) in PAGE_SHIMS {
            if *file == "dom.js" {
                eval(
                    &format!(
                        "globalThis.__lunaInternal.page = {{ id: {id}, vidBase: {base} }};",
                        id = state.window.id,
                        base = u64::from(state.window.id) * VID_STRIDE
                    ),
                    "page-mode.js",
                )?;
            }
            eval(src, file)?;
        }
        let mut entries = std::collections::HashMap::new();
        for name in ENTRIES {
            let f: Function = g.get(*name).map_err(|e| format!("{name} missing: {e}"))?;
            entries.insert(*name, Persistent::save(&ctx, f));
        }
        eval(LOCKDOWN, "lockdown.js")?;
        Ok(Entries(entries))
    })
}

/// A page's synchronous host operation.
fn op(s: &State, name: &str, a: &Value) -> Result<Value, String> {
    let str_of = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default();
    if let Some(r) = super::runtime::pure_op(name, a) {
        return r;
    }
    match name {
        "log" => {
            let level = match str_of("level") {
                l @ ("error" | "warn" | "info" | "debug") => l,
                _ => "info",
            };
            s.logs
                .borrow_mut()
                .push_back((level.to_string(), str_of("msg").to_string()));
            Ok(Value::Null)
        }
        "env" => Ok(json!({})),
        "heap.limit" => Ok(json!(PAGE_MEMORY)),
        // The displays, this window's among them.
        "screen" => Ok(super::screen::op(Some(s.window.screen()))),
        "plugin.uses_dom" => Ok(Value::Null),
        "dom.overlay" => {
            let mut ov: DomOverlay =
                serde_json::from_value(a.clone()).map_err(|e| format!("overlay: {e}"))?;
            let moved = {
                let mut cur = s.window.overlay.lock().map_err(|e| e.to_string())?;
                ov.version = cur.version;
                if **cur == ov {
                    return Ok(Value::Null);
                }
                let moved = cur.may_move_boxes(&ov);
                ov.version += 1;
                if moved && let Ok(mut slot) = s.window.trees.0.lock() {
                    slot.published(ov.version);
                }
                if moved {
                    s.boxes_version.set(ov.version);
                }
                *cur = Arc::new(ov);
                moved
            };
            s.window.wake();
            Ok(Value::Bool(moved))
        }
        "dom.wait_layout" => {
            let floor = Duration::from_millis(a["ms"].as_u64().unwrap_or(64).min(250));
            if s.laid_out.get() >= s.boxes_version.get() {
                return Ok(Value::Null);
            }
            let Some(tree) = super::layout::wait(&s.window.trees, s.boxes_version.get(), floor)
            else {
                return Ok(Value::Null);
            };
            s.laid_out.set(tree.laid_out);
            serde_json::to_value(&*tree).map_err(|e| e.to_string())
        }
        "page.ipc" => {
            let args = str_of("args");
            if args.len() > MAX_MESSAGE {
                return Err("the message is too large".into());
            }
            let _ = s.luna.send(Msg::Page(FromPage::Ipc {
                window: s.window.id,
                channel: str_of("channel").to_string(),
                args: args.to_string(),
                invoke: None,
            }));
            Ok(Value::Null)
        }
        "page.close" => {
            let _ = s.luna.send(Msg::Page(FromPage::User {
                window: s.window.id,
                kind: "close",
                data: json!({ "page": true }),
            }));
            Ok(Value::Null)
        }
        "page.loaded" => {
            let _ = s.luna.send(Msg::Page(FromPage::Loaded {
                window: s.window.id,
                title: str_of("title").to_string(),
            }));
            Ok(Value::Null)
        }
        // What the app page offers plugins (the clipboard, notifications,
        // the browser, the app's fullscreen) is not a page's.
        "clipboard" | "clipboard.read" | "notify" | "open_url" | "window.fullscreen" => {
            Ok(Value::Null)
        }
        _ => Err(format!("{name} is not available in a plugin window")),
    }
}

/// A page's asynchronous host operation: `ipcRenderer.invoke` and the
/// stylesheets its `<link>`s name; the rest fail as a page without the
/// app's privileges fails them.
fn op_async(s: &State, name: &str, arg: &str, id: f64) {
    if name == "page.stylesheet" {
        let a: Value = serde_json::from_str(arg).unwrap_or(Value::Null);
        let url = a["url"].as_str().unwrap_or_default().to_string();
        let own = s.own.clone();
        let window = s.window.id;
        let luna = s.luna.clone();
        let spawned = std::thread::Builder::new()
            .name("luna-page-sheet".into())
            .spawn(move || {
                let r = stylesheet(&url);
                if let Err(e) = &r {
                    let _ = luna.send(Msg::Page(FromPage::Log {
                        window,
                        level: "warn".into(),
                        msg: format!("the stylesheet {url} was not loaded: {e}"),
                    }));
                }
                let (ok, v) = match r {
                    Ok(text) => (true, json!(text)),
                    Err(e) => (false, json!(e)),
                };
                let _ = own.send(ToPage::OpDone {
                    id,
                    ok,
                    json: v.to_string(),
                });
            });
        if spawned.is_err() {
            let _ = s.own.send(ToPage::OpDone {
                id,
                ok: false,
                json: json!("no thread to load it on").to_string(),
            });
        }
        return;
    }
    if name == "page.invoke" {
        let a: Value = serde_json::from_str(arg).unwrap_or(Value::Null);
        let str_of = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default();
        let _ = s.luna.send(Msg::Page(FromPage::Ipc {
            window: s.window.id,
            channel: str_of("channel").to_string(),
            args: str_of("args").to_string(),
            invoke: Some(id),
        }));
        return;
    }
    let _ = s.own.send(ToPage::OpDone {
        id,
        ok: false,
        json: json!(format!("{name} is not available in a plugin window")).to_string(),
    });
}

/// The largest stylesheet a page may load.
const MAX_SHEET: usize = 8 << 20;

/// A stylesheet a page's `<link rel="stylesheet">` names, as Chromium loads
/// one for a page from a `data:` URL: a plain GET (no cookies, no
/// credentials) of an `https:` address (plain `http:` only from this
/// computer), every redirect held to that, answered as `text/css` (a
/// cross-origin sheet of another type is refused, as Chromium refuses it).
/// Each address is fetched once per run once it loaded.
pub(crate) fn stylesheet(url: &str) -> Result<String, String> {
    type Fetched = Mutex<std::collections::HashMap<String, Result<String, String>>>;
    static FETCHED: std::sync::LazyLock<Fetched> = std::sync::LazyLock::new(Default::default);
    if !super::runtime::is_secure_source(url) {
        return Err("only https: addresses load in a plugin window".into());
    }
    if let Some(hit) = FETCHED.lock().ok().and_then(|c| c.get(url).cloned()) {
        return hit;
    }
    let fetched = (|| {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Tidalfast-Luna/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many redirects")
                } else if super::runtime::is_secure_source(attempt.url().as_str()) {
                    attempt.follow()
                } else {
                    attempt.error("not https")
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        let css = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/css"));
        if !css {
            return Err("it is not served as text/css".to_string());
        }
        let bytes = super::runtime::read_capped(response, MAX_SHEET, "the stylesheet")?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    })();
    // A failure is tried again by the next page that names it.
    if fetched.is_ok()
        && let Ok(mut cache) = FETCHED.lock()
    {
        cache.insert(url.to_string(), fetched.clone());
    }
    fetched
}

fn eval_as<'js>(
    ctx: Ctx<'js>,
    code: String,
    filename: String,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let mut opts = EvalOptions::default();
    opts.strict = false;
    opts.filename = Some(filename);
    ctx.eval_with_options(code, opts)
}

/// A failure the page's JavaScript raised, as text.
fn failure<'js>(ctx: &Ctx<'js>, e: CaughtError<'js>) -> String {
    match e {
        CaughtError::Exception(e) => {
            let msg = e.message().unwrap_or_else(|| "Error".into());
            match e.stack() {
                Some(s) if !s.is_empty() => format!("{msg}\n{s}"),
                _ => msg,
            }
        }
        CaughtError::Value(v) => ctx
            .json_stringify(v)
            .ok()
            .flatten()
            .and_then(|s| s.to_string().ok())
            .unwrap_or_else(|| "a value".into()),
        CaughtError::Error(e) => e.to_string(),
    }
}

enum Arg {
    S(String),
    N(f64),
    B(bool),
}

struct Entries(std::collections::HashMap<&'static str, Persistent<Function<'static>>>);

impl Entries {
    /// Calls `name` with the script budget; its result as a number (`-1`
    /// for anything else), or the failure.
    fn call(&self, s: &State, ctx: &Context, name: &str, args: Vec<Arg>) -> Result<f64, String> {
        s.deadline.set(Some(Instant::now() + SCRIPT_BUDGET));
        let r = ctx.with(|ctx| {
            let f = self
                .0
                .get(name)
                .cloned()
                .ok_or_else(|| format!("{name} missing"))?
                .restore(&ctx)
                .map_err(|e| e.to_string())?;
            let vals: Vec<rquickjs::Value> = args
                .into_iter()
                .map(|a| match a {
                    Arg::S(s) => rquickjs::String::from_str(ctx.clone(), &s)
                        .map(|s| s.into_value())
                        .unwrap_or_else(|_| rquickjs::Value::new_undefined(ctx.clone())),
                    Arg::N(n) => rquickjs::Value::new_float(ctx.clone(), n),
                    Arg::B(b) => rquickjs::Value::new_bool(ctx.clone(), b),
                })
                .collect();
            match f.call::<_, rquickjs::Value>((Rest(vals),)).catch(&ctx) {
                Ok(v) => Ok(v.as_number().unwrap_or(-1.0)),
                Err(e) => Err(failure(&ctx, e)),
            }
        });
        s.deadline.set(None);
        r
    }
}

/// Runs queued promise jobs (each with the script budget).
fn pump(rt: &Runtime, s: &State, stop: &AtomicBool) {
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        s.deadline.set(Some(Instant::now() + SCRIPT_BUDGET));
        let r = rt.execute_pending_job();
        s.deadline.set(None);
        match r {
            Ok(true) => {}
            Ok(false) => return,
            Err(ex) => {
                let msg = ex.0.with(|ctx| {
                    let v = ctx.catch();
                    v.as_object()
                        .and_then(|o| o.get::<_, String>("message").ok())
                        .unwrap_or_else(|| "a job failed".into())
                });
                s.log("error", format!("Uncaught {msg}"));
            }
        }
    }
}

/// Sends what the page logged since the last call.
fn flush_logs(s: &State) {
    let logs: Vec<(String, String)> = s.logs.borrow_mut().drain(..).collect();
    for (level, msg) in logs {
        s.log(&level, msg);
    }
}

/// The page's event loop: it loads, then timers, promise jobs and what
/// arrives from the plugin and the UI, until the window closes.
fn drive(
    spawn: &Spawn,
    s: &Rc<State>,
    rx: &Receiver<ToPage>,
    rt: &Runtime,
    ctx: &Context,
    entries: &Entries,
    load: Load,
) -> Result<(), String> {
    let stop = &*spawn.stop;
    let Load { url, html, preload } = load;
    let (preload, preload_file) = preload.unzip();
    let start = json!({
        "url": url,
        "html": html,
        "preload": preload,
        "preloadFile": preload_file,
    })
    .to_string();
    drop(html);
    if let Err(e) = entries.call(s, ctx, "__page_load", vec![Arg::S(start)]) {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        s.log("error", format!("Uncaught {e}"));
    }
    loop {
        pump(rt, s, stop);
        flush_logs(s);
        if stop.load(Ordering::Acquire) {
            break;
        }
        let next = match entries.call(s, ctx, "__luna_tick", vec![]) {
            Ok(n) => n,
            Err(e) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                s.log("error", format!("Uncaught {e}"));
                -1.0
            }
        };
        pump(rt, s, stop);
        flush_logs(s);
        if stop.load(Ordering::Acquire) {
            break;
        }
        let next = if next >= 0.0 {
            next
        } else {
            entries
                .call(s, ctx, "__luna_tick", vec![Arg::S("peek".into())])
                .unwrap_or(-1.0)
        };
        let wait = if next >= 0.0 {
            Duration::from_millis(next.min(3_600_000.0) as u64)
        } else {
            Duration::from_secs(3600)
        };
        let first = match rx.recv_timeout(wait) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            // The window closed.
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut synced = false;
        for msg in first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
        {
            if stop.load(Ordering::Acquire) {
                break;
            }
            let r = match msg {
                ToPage::Ipc { channel, args } => {
                    entries.call(s, ctx, "__page_ipc", vec![Arg::S(channel), Arg::S(args)])
                }
                ToPage::Event(ev) => match serde_json::to_string(&ev) {
                    Ok(json) => entries.call(
                        s,
                        ctx,
                        "__luna_dom",
                        vec![Arg::S("event".into()), Arg::S(json)],
                    ),
                    Err(_) => Ok(0.0),
                },
                // Trees coalesce: the newest is reconciled once.
                ToPage::Sync if synced => Ok(0.0),
                ToPage::Sync => {
                    synced = true;
                    let tree = s.window.trees.0.lock().ok().and_then(|mut t| t.tree.take());
                    match tree {
                        Some(tree) => {
                            s.laid_out.set(tree.laid_out);
                            match serde_json::to_string(&*tree) {
                                Ok(json) => entries.call(
                                    s,
                                    ctx,
                                    "__luna_dom",
                                    vec![Arg::S("sync".into()), Arg::S(json)],
                                ),
                                Err(_) => Ok(0.0),
                            }
                        }
                        None => Ok(0.0),
                    }
                }
                ToPage::OpDone { id, ok, json } => entries.call(
                    s,
                    ctx,
                    "__luna_resolve",
                    vec![Arg::N(id), Arg::B(ok), Arg::S(json)],
                ),
                ToPage::Stop => break,
            };
            if let Err(e) = r {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                s.log("error", format!("Uncaught {e}"));
            }
            pump(rt, s, stop);
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
