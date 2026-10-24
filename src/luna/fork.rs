//! `child_process.fork` for plugins' `.native` code: the module runs as Node
//! runs a forked script, in a process of its own, here a QuickJS runtime on
//! a thread of its own with the same Node-like shims (`prelude.js`,
//! `node.js`, then `fork.js`), talking to its parent over the IPC channel
//! (`process.send` / `child.send`, `'message'`, `disconnect`, `exit`, `kill`).
//!
//! * It is the plugin's own code and acts as the plugin: every host
//!   operation it makes goes to the Luna thread and is checked there as that
//!   plugin's (its files, its permissions, the programs the user allowed it),
//!   except file access, which runs here with the same sandbox
//!   ([`super::fsops`]), and a synchronous program run, which the Luna thread
//!   allows or asks about and this thread then waits for.
//! * Like a process, it has no watchdog and does not hold up the plugins'
//!   thread: a script crunching numbers for a minute only keeps its own
//!   thread busy. Its heap has the plugins' limit; `kill` interrupts it at
//!   once, and it stops with its plugin.
//! * Nothing here knows any plugin: what a fork runs is whatever file the
//!   plugin names, read through its sandboxed `fs`.

use std::cell::Cell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rquickjs::context::EvalOptions;
use rquickjs::function::Rest;
use rquickjs::{
    ArrayBuffer, CatchResultExt, CaughtError, Context, Ctx, Function, Persistent, Runtime,
};
use serde_json::{Value, json};

use super::Msg;
use super::child::{self, ChildEvent};

/// Forked processes one plugin may run at a time, and all plugins together.
pub const MAX_PER_PLUGIN: usize = 4;
pub const MAX_FORKS: usize = 8;
/// The largest message one side sends the other (serialised).
pub const MAX_MESSAGE: usize = 64 << 20;
/// Async op ids handed to the Luna thread for forks count down from here
/// (the plugins' own count up from 1; `permits::NO_OP` is -1).
const FIRST_OP: f64 = -1_000_000.0;

/// What the Luna thread tells a forked process.
pub enum ToChild {
    /// A message from the parent (`child.send`), serialised by the shim.
    Message(String),
    /// The parent closed the channel (`child.disconnect()`).
    Disconnect,
    /// `kill()`: wakes the thread, whose interrupt flag is already set.
    Wake,
    /// An asynchronous op the fork asked for finished.
    OpDone { id: f64, ok: bool, json: String },
    /// A program the fork started did something.
    Child(ChildEvent),
    /// A socket the fork opened did something.
    Net {
        handle: u32,
        kind: &'static str,
        data: String,
    },
    /// A process the fork forked did something.
    Fork {
        handle: u32,
        kind: &'static str,
        data: String,
    },
}

/// What a forked process asks of the Luna thread.
pub enum FromChild {
    /// For the parent's `ChildProcess`: "spawn" (pid), "message" (serialised),
    /// "stdout" / "stderr" (base64), "disconnect", "exit" and "close"
    /// (`{"code", "signal"}`); "log" (`{"level", "msg"}`) goes to the
    /// plugin's log.
    Event {
        handle: u32,
        kind: &'static str,
        data: String,
    },
    /// A synchronous op, answered on `reply`.
    Op {
        handle: u32,
        name: String,
        arg: String,
        claimed: Option<String>,
        reply: SyncSender<Reply>,
    },
    /// An asynchronous op; its result comes back as [`ToChild::OpDone`].
    OpAsync {
        handle: u32,
        name: String,
        arg: String,
        id: f64,
        claimed: Option<String>,
    },
}

/// The Luna thread's answer to a [`FromChild::Op`].
pub enum Reply {
    /// The op's `{"ok": ...}` / `{"err": ...}`.
    Json(String),
    /// `child.spawnSync`: the program to run here (allowed), or why not.
    Program(Result<child::Spec, String>),
}

/// How a fork is stopped from outside: the flag its interrupt handler reads,
/// and the signal `kill` named.
#[derive(Default)]
pub struct Killer {
    flag: AtomicBool,
    signal: Mutex<Option<String>>,
}

impl Killer {
    fn kill(&self, signal: &str) {
        let mut s = self.signal.lock().unwrap_or_else(|e| e.into_inner());
        if s.is_none() {
            *s = Some(signal.to_string());
        }
        self.flag.store(true, Ordering::Release);
    }
    fn killed(&self) -> Option<String> {
        self.flag
            .load(Ordering::Acquire)
            .then(|| self.signal.lock().ok().and_then(|s| s.clone()))
            .map(|s| s.unwrap_or_else(|| "SIGTERM".into()))
    }
}

/// What a fork runs, read by the Luna thread through the plugin's `fs`.
pub struct Spawn {
    pub plugin: String,
    /// Its plugin's runtime: the fork acts as that runtime's plugin (its
    /// large downloads, the files the user picked for it).
    pub runtime: super::RuntimeId,
    /// The plugin's data folder (its `fs` sandbox).
    pub home: PathBuf,
    /// The module's path, as Node gives it to the script (`__filename`).
    pub file: String,
    pub code: String,
    pub args: Vec<String>,
    pub exec_argv: Vec<String>,
    /// Variables the plugin gave (`options.env`), or none to inherit.
    pub env: Option<Value>,
    pub cwd: String,
    /// Whether the parent reads the fork's stdout and stderr (`silent`).
    pub pipes: [bool; 2],
    /// "json" or "advanced" (`options.serialization`).
    pub serialization: String,
    /// Whether there is an IPC channel (always for `fork`).
    pub ipc: bool,
    pub memory_limit: usize,
}

/// Which handle space a forwarded event belongs to.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Route {
    Child,
    Net,
    Fork,
}

struct Fork {
    plugin: String,
    tx: Sender<ToChild>,
    killer: Arc<Killer>,
}

/// The forked processes of every plugin. Lives on the Luna thread.
#[derive(Default)]
pub struct ForkHub {
    next: u32,
    forks: HashMap<u32, Fork>,
    next_op: f64,
    /// Async op id given to the Luna thread -> (fork, the fork's own id).
    ops: HashMap<u64, (u32, f64)>,
    /// Programs, sockets and forks a fork opened: their events go to it.
    routes: HashMap<(Route, u32), u32>,
}

fn op_key(id: f64) -> u64 {
    id.to_bits()
}

impl ForkHub {
    /// Starts `spawn` as a forked process; its events come to `luna`.
    pub fn start(&mut self, spawn: Spawn, luna: Sender<Msg>) -> Result<u32, String> {
        if self.forks.len() >= MAX_FORKS {
            return Err("EAGAIN: too many forked processes are running".into());
        }
        if self
            .forks
            .values()
            .filter(|f| f.plugin == spawn.plugin)
            .count()
            >= MAX_PER_PLUGIN
        {
            return Err("EAGAIN: too many forked processes are running for this plugin".into());
        }
        self.next += 1;
        let handle = self.next;
        let (tx, rx) = mpsc::channel();
        let killer = Arc::new(Killer::default());
        let plugin = spawn.plugin.clone();
        let k = killer.clone();
        std::thread::Builder::new()
            .name(format!("luna-fork-{handle}"))
            .stack_size(16 * 1024 * 1024)
            .spawn(move || child_main(handle, spawn, rx, luna, k))
            .map_err(|e| format!("EAGAIN: {e}"))?;
        self.forks.insert(handle, Fork { plugin, tx, killer });
        Ok(handle)
    }

    pub fn plugin_of(&self, handle: u32) -> Option<&str> {
        self.forks.get(&handle).map(|f| f.plugin.as_str())
    }

    fn owned(&self, plugin: &str, handle: u32) -> Option<&Fork> {
        self.forks.get(&handle).filter(|f| f.plugin == plugin)
    }

    pub fn send(&self, plugin: &str, handle: u32, data: String) -> Result<(), String> {
        if data.len() > MAX_MESSAGE {
            return Err("ERR_IPC_MESSAGE_TOO_LARGE: the message is too large".into());
        }
        let f = self
            .owned(plugin, handle)
            .ok_or("ERR_IPC_CHANNEL_CLOSED: Channel closed")?;
        f.tx.send(ToChild::Message(data))
            .map_err(|_| "ERR_IPC_CHANNEL_CLOSED: Channel closed".to_string())
    }

    pub fn disconnect(&self, plugin: &str, handle: u32) {
        if let Some(f) = self.owned(plugin, handle) {
            let _ = f.tx.send(ToChild::Disconnect);
        }
    }

    pub fn kill(&self, plugin: &str, handle: u32, signal: &str) -> bool {
        match self.owned(plugin, handle) {
            Some(f) => {
                f.killer.kill(signal);
                let _ = f.tx.send(ToChild::Wake);
                true
            }
            None => false,
        }
    }

    /// The fork ended (its "close" event arrived).
    pub fn ended(&mut self, handle: u32) {
        self.forks.remove(&handle);
        self.routes.retain(|_, f| *f != handle);
        self.ops.retain(|_, (f, _)| *f != handle);
    }

    /// A plugin stopped: its forks are killed.
    pub fn close_plugin(&mut self, plugin: &str) {
        for f in self.forks.values().filter(|f| f.plugin == plugin) {
            f.killer.kill("SIGTERM");
            let _ = f.tx.send(ToChild::Wake);
        }
    }

    /// The app is closing.
    pub fn stop_all(&mut self) {
        for f in self.forks.values() {
            f.killer.kill("SIGTERM");
            let _ = f.tx.send(ToChild::Wake);
        }
    }

    /// The id the Luna thread uses for fork `handle`'s async op `id`.
    pub fn op_id(&mut self, handle: u32, id: f64) -> f64 {
        if self.next_op == 0.0 {
            self.next_op = FIRST_OP;
        }
        self.next_op -= 1.0;
        self.ops.insert(op_key(self.next_op), (handle, id));
        self.next_op
    }

    /// Hands a finished async op to the fork that asked; gives the result
    /// back when `id` is not a fork's.
    pub fn op_done(&mut self, id: f64, ok: bool, json: String) -> Option<String> {
        if id > FIRST_OP {
            return Some(json);
        }
        if let Some((handle, own)) = self.ops.remove(&op_key(id))
            && let Some(f) = self.forks.get(&handle)
        {
            let _ = f.tx.send(ToChild::OpDone { id: own, ok, json });
        }
        None
    }

    pub fn route(&mut self, route: Route, handle: u32, fork: u32) {
        if self.forks.contains_key(&fork) {
            self.routes.insert((route, handle), fork);
        }
    }

    /// Hands an event of a routed handle to its fork; gives it back when the
    /// handle is not a fork's.
    pub fn forward(
        &mut self,
        route: Route,
        handle: u32,
        msg: ToChild,
        last: bool,
    ) -> Option<ToChild> {
        let Some(&fork) = self.routes.get(&(route, handle)) else {
            return Some(msg);
        };
        if last {
            self.routes.remove(&(route, handle));
        }
        if let Some(f) = self.forks.get(&fork) {
            let _ = f.tx.send(msg);
        }
        None
    }

    pub fn routed(&self, route: Route, handle: u32) -> Option<u32> {
        self.routes.get(&(route, handle)).copied()
    }
}

// ---------------------------------------------------------------- the fork's thread

/// The fork's own state, shared by its natives and its loop.
struct State {
    handle: u32,
    plugin: String,
    runtime: super::RuntimeId,
    home: PathBuf,
    luna: Sender<Msg>,
    killer: Arc<Killer>,
    /// `process.exit(code)` was called.
    exit: Rc<Cell<Option<i32>>>,
    exiting: Arc<AtomicBool>,
    connected: Cell<bool>,
}

impl State {
    fn event(&self, kind: &'static str, data: String) {
        let _ = self.luna.send(Msg::Fork(FromChild::Event {
            handle: self.handle,
            kind,
            data,
        }));
    }

    fn log(&self, level: &str, msg: &str) {
        self.event("log", json!({ "level": level, "msg": msg }).to_string());
    }

    /// A synchronous op on the Luna thread, as this fork's plugin.
    fn forward(&self, name: &str, arg: String, claimed: Option<String>) -> Reply {
        let (tx, rx) = mpsc::sync_channel(1);
        let sent = self.luna.send(Msg::Fork(FromChild::Op {
            handle: self.handle,
            name: name.to_string(),
            arg,
            claimed,
            reply: tx,
        }));
        match sent.ok().and_then(|()| rx.recv().ok()) {
            Some(r) => r,
            None => {
                Reply::Json(json!({ "err": "EPIPE: the plugins' runtime has stopped" }).to_string())
            }
        }
    }

    fn claimed_ok(&self, claimed: &Option<String>) -> Result<(), String> {
        match claimed {
            Some(c) if *c != self.plugin => {
                Err(format!("EACCES: plugin {} cannot act as {c}", self.plugin))
            }
            _ => Ok(()),
        }
    }

    /// `__op`: file access and synchronous programs here, the rest on the
    /// Luna thread.
    fn op(&self, name: &str, arg: String, claimed: Option<String>) -> String {
        let wrap = |r: Result<Value, String>| match r {
            Ok(v) => json!({ "ok": v }).to_string(),
            Err(e) => json!({ "err": e }).to_string(),
        };
        match name {
            "fs" => wrap(self.claimed_ok(&claimed).and_then(|()| {
                let a: Value = serde_json::from_str(&arg).unwrap_or(Value::Null);
                super::fsops::fs_op_for(self.runtime, &self.plugin, &self.home, &a)
            })),
            "child.spawnSync" => {
                let a: Value = serde_json::from_str(&arg).unwrap_or(Value::Null);
                match self.forward(name, arg, claimed) {
                    Reply::Json(j) => j,
                    Reply::Program(Err(e)) => wrap(Err(e)),
                    Reply::Program(Ok(spec)) => wrap(Ok(self.run_sync(&spec, &a))),
                }
            }
            _ => match self.forward(name, arg, claimed) {
                Reply::Json(j) => j,
                Reply::Program(_) => wrap(Err("EINVAL: unexpected answer".into())),
            },
        }
    }

    /// `spawnSync` / `execSync` in a fork: the program runs to its end on
    /// this thread (a process waits for it too), within the plugin's
    /// `timeout` or an hour, and stops when the fork is killed.
    fn run_sync(&self, spec: &child::Spec, a: &Value) -> Value {
        let timeout = a["timeout"]
            .as_u64()
            .filter(|t| *t > 0)
            .map_or(Duration::from_secs(3600), Duration::from_millis);
        let max = a["maxBuffer"].as_u64().unwrap_or(1 << 20).min(64 << 20) as usize;
        let input = a["input"].as_str().and_then(|s| B64.decode(s).ok());
        let r = child::run_sync_until(spec, input, timeout, max, Some(&self.killer.flag));
        json!({
            "pid": r.pid,
            "status": r.status,
            "signal": r.signal,
            "stdout": B64.encode(&r.stdout),
            "stderr": B64.encode(&r.stderr),
            "error": r.error,
        })
    }

    fn fs_bytes(
        &self,
        arg: &str,
        claimed: &Option<String>,
        data: Option<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>, String> {
        self.claimed_ok(claimed)?;
        let a: Value = serde_json::from_str(arg).unwrap_or(Value::Null);
        let path = a["path"].as_str().unwrap_or_default();
        match a["op"].as_str().unwrap_or_default() {
            "read" => {
                super::fsops::read_bytes(self.runtime, &self.plugin, &self.home, path).map(Some)
            }
            "write" => super::fsops::write_bytes(
                self.runtime,
                &self.plugin,
                &self.home,
                path,
                &data.unwrap_or_default(),
                a["append"].as_bool() == Some(true),
            )
            .map(|()| None),
            op => Err(format!("EINVAL: fs {op} has no bytes")),
        }
    }
}

/// The shims' functions the loop calls.
const ENTRIES: &[&str] = &[
    "__luna_tick",
    "__luna_resolve",
    "__luna_child",
    "__luna_net",
    "__luna_fork",
    "__fork_start",
    "__fork_event",
    "__fork_idle",
];

const LOCKDOWN: &str = r#"for (const k of ["__op", "__op_async", "__caller", "__eval", "__utf8_enc", "__utf8_dec", "__b64_enc", "__b64_dec", "__fetch_take", "__fetch_drop", "__fs_bytes", "__fork", "__lunaInternal", "__luna_tick", "__luna_resolve", "__luna_child", "__luna_net", "__luna_fork", "__fork_start", "__fork_event", "__fork_idle"]) delete globalThis[k];"#;

fn child_main(
    handle: u32,
    spawn: Spawn,
    rx: Receiver<ToChild>,
    luna: Sender<Msg>,
    killer: Arc<Killer>,
) {
    let state = Rc::new(State {
        handle,
        plugin: spawn.plugin.clone(),
        runtime: spawn.runtime,
        home: spawn.home.clone(),
        luna,
        killer,
        exit: Rc::new(Cell::new(None)),
        exiting: Arc::new(AtomicBool::new(false)),
        connected: Cell::new(spawn.ipc),
    });
    state.event("spawn", (100_000 + handle).to_string());
    #[cfg(test)]
    RUNNING.lock().unwrap().push(spawn.home.clone());
    let ran = std::panic::catch_unwind(AssertUnwindSafe(|| run(&spawn, &state, &rx)));
    #[cfg(test)]
    {
        let mut r = RUNNING.lock().unwrap();
        if let Some(at) = r.iter().position(|h| *h == spawn.home) {
            r.remove(at);
        }
    }
    let code = match ran {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => {
            state.log("error", &format!("Forked {}: {e}", spawn.file));
            1
        }
        Err(_) => {
            state.log("error", &format!("Forked {} crashed", spawn.file));
            1
        }
    };
    let exit = match state.killer.killed() {
        Some(signal) => json!({ "code": null, "signal": signal }),
        None => json!({ "code": code, "signal": null }),
    }
    .to_string();
    if state.connected.replace(false) {
        state.event("disconnect", String::new());
    }
    state.event("exit", exit.clone());
    state.event("close", exit);
}

/// The data folders of the forks running in this test process.
#[cfg(test)]
static RUNNING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// How many forks of plugins whose data folders are under `dir` run.
#[cfg(test)]
pub fn running_under(dir: &std::path::Path) -> usize {
    RUNNING
        .lock()
        .unwrap()
        .iter()
        .filter(|h| h.starts_with(dir))
        .count()
}

/// A failure the fork's JavaScript raised, as text.
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

/// One JS argument for [`Entries::call`].
enum Arg {
    S(String),
    N(f64),
    B(bool),
}

struct Entries(HashMap<&'static str, Persistent<Function<'static>>>);

impl Entries {
    /// Calls `name`; its result as a number (`-1` for anything else), or the
    /// failure.
    fn call(&self, ctx: &Context, name: &str, args: Vec<Arg>) -> Result<f64, String> {
        ctx.with(|ctx| {
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
        })
    }
}

/// Whether the fork must stop now: `process.exit` or `kill`.
fn stopping(state: &State) -> bool {
    state.exit.get().is_some() || state.killer.flag.load(Ordering::Acquire)
}

fn run(spawn: &Spawn, state: &Rc<State>, rx: &Receiver<ToChild>) -> Result<i32, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(spawn.memory_limit);
    rt.set_max_stack_size(8 * 1024 * 1024);
    {
        let killer = state.killer.clone();
        let exiting = state.exiting.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            killer.flag.load(Ordering::Acquire) || exiting.load(Ordering::Acquire)
        })));
    }
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    let entries = install(&ctx, state)?;
    let code = drive(spawn, state, rx, &rt, &ctx, &entries);
    // Everything JS holds goes before the runtime.
    drop(entries);
    CURRENT.with(|c| c.borrow_mut().take());
    code
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
                move |c: Ctx<'_>, name: String, arg: String, claimed: Option<String>| -> String {
                    if name == "heap.stats" {
                        return heap_stats(&c);
                    }
                    s.op(&name, arg, claimed)
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
                move |name: String, arg: String, id: f64, claimed: Option<String>| {
                    let _ = s.luna.send(Msg::Fork(FromChild::OpAsync {
                        handle: s.handle,
                        name,
                        arg,
                        id,
                        claimed,
                    }));
                },
            )
            .map_err(js)?,
        )
        .map_err(js)?;
        let plugin = state.plugin.clone();
        g.set(
            "__caller",
            Function::new(ctx.clone(), move || -> Option<String> {
                Some(plugin.clone())
            })
            .map_err(js)?,
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
        CURRENT.with(|c| *c.borrow_mut() = Some(state.clone()));
        g.set(
            "__fetch_take",
            Function::new(ctx.clone(), fetch_take).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__fetch_drop",
            Function::new(ctx.clone(), fetch_drop).map_err(js)?,
        )
        .map_err(js)?;
        g.set(
            "__fs_bytes",
            Function::new(ctx.clone(), fs_bytes_js).map_err(js)?,
        )
        .map_err(js)?;
        // The process's own natives, for fork.js.
        let natives = rquickjs::Object::new(ctx.clone()).map_err(js)?;
        let s = state.clone();
        natives
            .set(
                "send",
                Function::new(ctx.clone(), move |data: String| -> bool {
                    if !s.connected.get() || data.len() > MAX_MESSAGE {
                        return false;
                    }
                    s.event("message", data);
                    true
                })
                .map_err(js)?,
            )
            .map_err(js)?;
        let s = state.clone();
        natives
            .set(
                "disconnect",
                Function::new(ctx.clone(), move || {
                    if s.connected.replace(false) {
                        s.event("disconnect", String::new());
                    }
                })
                .map_err(js)?,
            )
            .map_err(js)?;
        let s = state.clone();
        natives
            .set(
                "exit",
                Function::new(ctx.clone(), move |code: f64| {
                    if s.exit.get().is_none() {
                        s.exit.set(Some(code as i32));
                    }
                    s.exiting.store(true, Ordering::Release);
                })
                .map_err(js)?,
            )
            .map_err(js)?;
        let s = state.clone();
        natives
            .set(
                "output",
                Function::new(ctx.clone(), move |kind: String, b64: String| {
                    let kind = if kind == "stderr" { "stderr" } else { "stdout" };
                    s.event(kind, b64);
                })
                .map_err(js)?,
            )
            .map_err(js)?;
        g.set("__fork", natives).map_err(js)?;
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
        for (file, src) in super::runtime::shims_for_fork() {
            eval(src, file)?;
        }
        eval(include_str!("js/fork.js"), "fork.js")?;
        let mut entries = HashMap::new();
        for name in ENTRIES {
            let f: Function = g.get(*name).map_err(|e| format!("{name} missing: {e}"))?;
            entries.insert(*name, Persistent::save(&ctx, f));
        }
        eval(LOCKDOWN, "lockdown.js")?;
        Ok(Entries(entries))
    })
}

thread_local! {
    /// The fork this thread runs (one per thread), for the natives that
    /// hand back the engine's values.
    static CURRENT: std::cell::RefCell<Option<Rc<State>>> = const { std::cell::RefCell::new(None) };
}

fn current<'js>(c: &Ctx<'js>) -> rquickjs::Result<Rc<State>> {
    CURRENT
        .with(|s| s.borrow().clone())
        .ok_or_else(|| rquickjs::Exception::throw_message(c, "EIO: the process has ended"))
}

/// `__fetch_take(token)`: a large fetched body, for the fork's plugin only.
fn fetch_take<'js>(c: Ctx<'js>, token: String) -> rquickjs::Result<ArrayBuffer<'js>> {
    let state = current(&c)?;
    let bytes = super::downloads::take(state.runtime, Some(&state.plugin), &token)
        .map_err(|e| rquickjs::Exception::throw_message(&c, &e))?;
    ArrayBuffer::new(c, bytes)
}

/// `__fetch_drop(token)`: a large fetched body the fork's plugin left
/// unread; its file goes.
fn fetch_drop(c: Ctx<'_>, token: String) -> rquickjs::Result<()> {
    let state = current(&c)?;
    super::downloads::release(state.runtime, Some(&state.plugin), &token);
    Ok(())
}

/// `__fs_bytes(json, plugin, ArrayBuffer?)`: file bytes in and out without
/// base64, in the plugin's sandbox.
fn fs_bytes_js<'js>(
    c: Ctx<'js>,
    arg: String,
    claimed: Option<String>,
    data: Option<ArrayBuffer<'js>>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let state = current(&c)?;
    let data = data.map(|b| super::runtime::bytes_of(&b, 0.0, b.len() as f64));
    match state.fs_bytes(&arg, &claimed, data) {
        Ok(Some(bytes)) => Ok(ArrayBuffer::new(c, bytes)?.into_value()),
        Ok(None) => Ok(rquickjs::Value::new_undefined(c)),
        Err(e) => Err(rquickjs::Exception::throw_message(&c, &e)),
    }
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

fn heap_stats(ctx: &Ctx<'_>) -> String {
    let mut usage = std::mem::MaybeUninit::<rquickjs::qjs::JSMemoryUsage>::zeroed();
    // SAFETY: `ctx` is the live context this call runs in; the runtime
    // behind it fills the plain-data struct and keeps no pointer to it.
    let usage = unsafe {
        let rt = rquickjs::qjs::JS_GetRuntime(ctx.as_raw().as_ptr());
        rquickjs::qjs::JS_ComputeMemoryUsage(rt, usage.as_mut_ptr());
        usage.assume_init()
    };
    json!({ "ok": {
        "limit": usage.malloc_limit,
        "mallocSize": usage.malloc_size,
        "usedSize": usage.memory_used_size,
        "objects": usage.obj_count,
        "codeSize": usage.js_func_code_size,
    } })
    .to_string()
}

/// Runs queued promise jobs; false when the fork must stop.
fn pump(rt: &Runtime, state: &State) -> bool {
    loop {
        if stopping(state) {
            return false;
        }
        match rt.execute_pending_job() {
            Ok(true) => {}
            Ok(false) => return true,
            Err(ex) => {
                if stopping(state) {
                    return false;
                }
                let msg = ex.0.with(|ctx| {
                    let v = ctx.catch();
                    v.as_object()
                        .and_then(|o| o.get::<_, String>("message").ok())
                        .unwrap_or_else(|| "a job failed".into())
                });
                state.log("error", &msg);
            }
        }
    }
}

/// The fork's event loop: the module runs, then timers, promise jobs and
/// what arrives from the Luna thread, until nothing keeps it alive (no
/// timer, no op or program in flight, no channel with a listener), it calls
/// `process.exit`, or it is killed.
fn drive(
    spawn: &Spawn,
    state: &Rc<State>,
    rx: &Receiver<ToChild>,
    rt: &Runtime,
    ctx: &Context,
    entries: &Entries,
) -> Result<i32, String> {
    let start = json!({
        "plugin": spawn.plugin,
        "file": spawn.file,
        "code": spawn.code,
        "args": spawn.args,
        "execArgv": spawn.exec_argv,
        "env": spawn.env,
        "cwd": spawn.cwd,
        "pipes": spawn.pipes,
        "serialization": spawn.serialization,
        "ipc": spawn.ipc,
        "pid": 100_000 + state.handle,
    })
    .to_string();
    let exited = |state: &State| state.exit.get();
    if let Err(e) = entries.call(ctx, "__fork_start", vec![Arg::S(start)]) {
        if let Some(code) = exited(state) {
            return Ok(code);
        }
        if state.killer.flag.load(Ordering::Acquire) {
            return Ok(1);
        }
        // An uncaught exception ends a Node process with code 1.
        state.log("error", &format!("Uncaught {e}"));
        return Ok(1);
    }
    let mut idle_once = false;
    loop {
        if !pump(rt, state) {
            break;
        }
        let next = match entries.call(ctx, "__luna_tick", vec![]) {
            Ok(n) => n,
            Err(e) => {
                if stopping(state) {
                    break;
                }
                return Err(e);
            }
        };
        if !pump(rt, state) {
            break;
        }
        let next = if next >= 0.0 {
            next
        } else {
            entries
                .call(ctx, "__luna_tick", vec![Arg::S("peek".into())])
                .unwrap_or(-1.0)
        };
        if next < 0.0 && !rt.is_job_pending() {
            // `__fork_idle(final)`: 1 while something keeps the process
            // alive; otherwise 'beforeExit' (which may start more work) and,
            // when that started nothing, 'exit'.
            match entries.call(ctx, "__fork_idle", vec![Arg::B(idle_once)]) {
                Ok(alive) if alive > 0.0 => idle_once = false,
                Ok(_) if idle_once => break,
                Ok(_) => {
                    idle_once = true;
                    continue;
                }
                Err(_) if stopping(state) => break,
                Err(e) => return Err(e),
            }
        } else {
            idle_once = false;
        }
        let wait = if next >= 0.0 {
            Duration::from_millis(next.min(3_600_000.0) as u64)
        } else {
            Duration::from_secs(3600)
        };
        let first = match rx.recv_timeout(wait) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            // The Luna thread is gone: so is the parent.
            Err(RecvTimeoutError::Disconnected) => {
                state.killer.kill("SIGTERM");
                break;
            }
        };
        for msg in first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
        {
            if stopping(state) {
                break;
            }
            let r = deliver(state, ctx, entries, msg);
            if let Err(e) = r {
                if stopping(state) {
                    break;
                }
                state.log("error", &format!("Uncaught {e}"));
                return Ok(1);
            }
            if !pump(rt, state) {
                break;
            }
        }
        if stopping(state) {
            break;
        }
    }
    Ok(exited(state).unwrap_or(0))
}

fn deliver(state: &State, ctx: &Context, entries: &Entries, msg: ToChild) -> Result<f64, String> {
    match msg {
        ToChild::Message(data) => {
            if !state.connected.get() {
                return Ok(0.0);
            }
            entries.call(
                ctx,
                "__fork_event",
                vec![Arg::S("message".into()), Arg::S(data)],
            )
        }
        ToChild::Disconnect => {
            if !state.connected.replace(false) {
                return Ok(0.0);
            }
            state.event("disconnect", String::new());
            entries.call(
                ctx,
                "__fork_event",
                vec![Arg::S("disconnect".into()), Arg::S(String::new())],
            )
        }
        ToChild::Wake => Ok(0.0),
        ToChild::OpDone { id, ok, json } => entries.call(
            ctx,
            "__luna_resolve",
            vec![Arg::N(id), Arg::B(ok), Arg::S(json)],
        ),
        ToChild::Child(mut ev) => {
            let args = vec![
                Arg::N(ev.handle as f64),
                Arg::S(ev.kind.into()),
                Arg::S(std::mem::take(&mut ev.data)),
            ];
            drop(ev);
            entries.call(ctx, "__luna_child", args)
        }
        ToChild::Net { handle, kind, data } => entries.call(
            ctx,
            "__luna_net",
            vec![Arg::N(handle as f64), Arg::S(kind.into()), Arg::S(data)],
        ),
        ToChild::Fork { handle, kind, data } => entries.call(
            ctx,
            "__luna_fork",
            vec![Arg::N(handle as f64), Arg::S(kind.into()), Arg::S(data)],
        ),
    }
}
