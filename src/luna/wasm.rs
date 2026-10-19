//! The `WebAssembly` global for Luna plugins, on the wasmi interpreter.
//!
//! `js/wasm.js` implements the JS API (`WebAssembly.Module`, `Instance`,
//! `Memory`, `Table`, `Global`, `compile`/`instantiate`/`validate`/
//! `*Streaming`) on the natives of the `__wasm` object installed here.
//!
//! - Each plugin gets its own wasmi [`Store`]: its instances, memories,
//!   tables and globals live there, its linear memory is capped by
//!   [`LunaConfig::wasm_memory_limit`](super::LunaConfig) and the store is
//!   dropped when the plugin unloads. The plugin is the one whose module is
//!   on the stack when the object is created (or, for an instance, the store
//!   of the objects it imports).
//! - Wasm runs on fuel in slices; between slices the Luna watchdog deadline
//!   is checked, so a module that never returns is stopped like a JS loop
//!   that never yields.
//! - `Memory.buffer` is an `ArrayBuffer` over the linear memory itself (no
//!   copy). When the memory grows it can move, so the old buffer is
//!   detached before any JavaScript runs again, as the JS API requires.
//! - Calls go both ways and nest (JS -> wasm -> JS -> wasm). JS exceptions
//!   thrown by an import cross the wasm frames unchanged.
//!
//! Everything lives in a thread-local: one Luna runtime per thread.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::fmt::Display;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::{
    Array, ArrayBuffer, ArrayBufferSource, BigInt, Ctx, Exception, Function, IntoJs, Object,
    Persistent, Value,
};
use wasmi::{
    AsContextMut, Caller, Config, Engine, Extern, ExternRef, ExternType, F32, F64, Func, FuncType,
    Global, Memory, MemoryType, Module, Mutability, Nullable, Ref, RefType, ResourceLimiter,
    ResumableCall, Store, StoreContextMut, Table, TableType, Val, ValType,
};
use wasmi_core::LimiterError;

/// Fuel (roughly instructions) a module runs between watchdog checks.
const FUEL_SLICE: u64 = 2_000_000;
/// Fuel for a module's start function, which cannot be paused.
const START_FUEL: u64 = 500_000_000;
/// Largest table, in elements.
const TABLE_LIMIT: usize = 1 << 20;
/// Most wasm objects (instances, memories, tables) one plugin may create.
const OBJECT_LIMIT: usize = 10_000;
/// Error messages starting with this are turned into the matching
/// `WebAssembly.*Error` class by `js/wasm.js`.
const MARK: &str = "\u{1}wasm:";

/// Caps a plugin's linear memory (all its memories together) and notes
/// growth, so stale `Memory.buffer`s get detached.
struct Limiter {
    used: usize,
    limit: usize,
    grown: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        let add = desired.saturating_sub(current);
        if self.used.saturating_add(add) > self.limit {
            return Ok(false);
        }
        self.used += add;
        self.grown = true;
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        Ok(desired <= TABLE_LIMIT)
    }

    fn instances(&self) -> usize {
        OBJECT_LIMIT
    }

    fn tables(&self) -> usize {
        OBJECT_LIMIT
    }

    fn memories(&self) -> usize {
        OBJECT_LIMIT
    }
}

struct Data {
    limiter: Limiter,
}

/// A `Memory.buffer` handed to JavaScript, over `len` bytes at `ptr`.
struct Buf {
    ab: Persistent<ArrayBuffer<'static>>,
    ptr: usize,
    len: usize,
}

/// One plugin's store and the wasm objects JavaScript holds handles to.
struct Slot {
    store: UnsafeCell<Store<Data>>,
    /// Host calls in progress on this store, innermost last: while wasm runs
    /// the store is reachable only through the `Caller` it handed the host.
    callers: RefCell<Vec<*mut ()>>,
    /// Wasm calls in progress on this store.
    depth: Cell<u32>,
    externs: RefCell<Vec<Extern>>,
    /// Index of each extern by identity, so one wasm object keeps one JS
    /// object (`table.get(0) === exports.f`). wasmi handles have no `Eq`;
    /// their `Debug` form names the store and the entity.
    known: RefCell<HashMap<String, u32>>,
    buffers: RefCell<HashMap<usize, Buf>>,
}

struct Wasm {
    engine: Engine,
    modules: RefCell<HashMap<u32, (String, Module)>>,
    slots: RefCell<HashMap<u32, Rc<Slot>>>,
    owners: RefCell<HashMap<String, u32>>,
    next: Cell<u32>,
    deadline: Rc<Cell<Option<Instant>>>,
    /// Longest one wasm call may run (`LunaConfig::wasm_watchdog`).
    budget: Duration,
    /// Where wasm's time since the JS deadline was last moved out began.
    since: Cell<Instant>,
    memory_limit: usize,
    /// JS contexts of the native calls that entered wasm, innermost last.
    ctxs: RefCell<Vec<NonNull<rquickjs::qjs::JSContext>>>,
    /// The JS function that runs imported JS functions (`js/wasm.js`).
    host: RefCell<Option<Persistent<Function<'static>>>>,
    /// A JS exception on its way out through wasm frames.
    pending: RefCell<Option<Persistent<Value<'static>>>>,
}

thread_local! {
    static WASM: RefCell<Option<Rc<Wasm>>> = const { RefCell::new(None) };
}

fn state() -> Option<Rc<Wasm>> {
    WASM.with(|w| w.borrow().clone())
}

impl Wasm {
    /// The time wasm has run since the last call to this (or since the call
    /// began) is not charged to the JS watchdog: move its deadline out by it.
    fn credit(&self, now: Instant) {
        if let Some(d) = self.deadline.get() {
            self.deadline
                .set(Some(d + now.saturating_duration_since(self.since.get())));
        }
        self.since.set(now);
    }

    fn id(&self) -> u32 {
        let n = self.next.get() + 1;
        self.next.set(n);
        n
    }

    fn slot(&self, s: u32) -> Option<Rc<Slot>> {
        self.slots.borrow().get(&s).cloned()
    }

    fn slot_for(&self, owner: &str) -> (u32, Rc<Slot>) {
        if let Some(&s) = self.owners.borrow().get(owner)
            && let Some(slot) = self.slot(s)
        {
            return (s, slot);
        }
        let mut store = Store::new(
            &self.engine,
            Data {
                limiter: Limiter {
                    used: 0,
                    limit: self.memory_limit,
                    grown: false,
                },
            },
        );
        store.limiter(|d| &mut d.limiter);
        let slot = Rc::new(Slot {
            store: UnsafeCell::new(store),
            callers: RefCell::default(),
            depth: Cell::new(0),
            externs: RefCell::default(),
            known: RefCell::default(),
            buffers: RefCell::default(),
        });
        let s = self.id();
        self.slots.borrow_mut().insert(s, slot.clone());
        self.owners.borrow_mut().insert(owner.to_string(), s);
        (s, slot)
    }
}

/// Runs `f` with the slot's store: through the innermost host call's
/// `Caller` while wasm is running on it, else directly.
fn with_store<R>(slot: &Slot, f: impl FnOnce(StoreContextMut<'_, Data>) -> R) -> R {
    let top = slot.callers.borrow().last().copied();
    match top {
        // SAFETY: the pointer was pushed by `host_call` for a `Caller` that
        // lives until it pops it again, and nothing else touches the store
        // meanwhile (single thread, strictly nested calls).
        Some(p) => f(unsafe { &mut *(p as *mut Caller<'static, Data>) }.as_context_mut()),
        // SAFETY: no wasm is running on this store (no caller pushed), so no
        // other reference to it is alive.
        None => f(unsafe { &mut *slot.store.get() }.as_context_mut()),
    }
}

impl Slot {
    fn push(&self, e: Extern) -> u32 {
        let key = format!("{e:?}");
        if let Some(&i) = self.known.borrow().get(&key) {
            return i;
        }
        let mut x = self.externs.borrow_mut();
        x.push(e);
        let i = (x.len() - 1) as u32;
        self.known.borrow_mut().insert(key, i);
        i
    }

    fn ext(&self, i: u32) -> Option<Extern> {
        self.externs.borrow().get(i as usize).copied()
    }

    /// Detach `Memory.buffer`s whose memory has grown (and maybe moved).
    fn sync(&self, ctx: &Ctx<'_>) {
        let grown = with_store(self, |mut sc| {
            std::mem::replace(&mut sc.data_mut().limiter.grown, false)
        });
        if grown {
            self.refresh(ctx, None);
        }
    }

    /// Detach stale buffers (all of them with `None`... of one memory with
    /// `Some`); returns nothing, callers make new ones on demand.
    fn refresh(&self, ctx: &Ctx<'_>, only: Option<usize>) {
        let stale: Vec<Buf> = {
            let mut b = self.buffers.borrow_mut();
            let keys: Vec<usize> = b
                .iter()
                .filter(|(k, buf)| {
                    only.is_none_or(|o| o == **k) && {
                        let now = self.ext(**k as u32).and_then(Extern::into_memory).map(|m| {
                            with_store(self, |sc| (m.data_ptr(&sc) as usize, m.data_size(&sc)))
                        });
                        now != Some((buf.ptr, buf.len))
                    }
                })
                .map(|(k, _)| *k)
                .collect();
            keys.into_iter().filter_map(|k| b.remove(&k)).collect()
        };
        for buf in stale {
            if let Ok(mut ab) = buf.ab.restore(ctx) {
                ab.detach();
            }
        }
    }

    fn detach_all(&self, ctx: &Ctx<'_>) {
        let all: Vec<Buf> = self.buffers.borrow_mut().drain().map(|(_, b)| b).collect();
        for buf in all {
            if let Ok(mut ab) = buf.ab.restore(ctx) {
                ab.detach();
            }
        }
    }
}

/// Bytes wasm owns, lent to an `ArrayBuffer` (detached before they go).
struct Lent {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: `Slot::refresh`/`detach_all` detach the buffer before the memory
// moves (grows) or is freed (store dropped), so the pointer stays valid for
// as long as JavaScript can reach it.
unsafe impl ArrayBufferSource for Lent {
    fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
    fn len(&self) -> usize {
        self.len
    }
}

fn throw<'js>(ctx: &Ctx<'js>, kind: &str, msg: impl Display) -> rquickjs::Error {
    Exception::throw_message(ctx, &format!("{MARK}{kind}:{msg}"))
}

fn no_state<'js>(ctx: &Ctx<'js>) -> rquickjs::Error {
    throw(ctx, "Error", "WebAssembly is not running")
}

fn val_name(t: ValType) -> &'static str {
    match t {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::V128 => "v128",
        ValType::FuncRef => "anyfunc",
        ValType::ExternRef => "externref",
    }
}

fn val_type(s: &str) -> Option<ValType> {
    Some(match s {
        "i32" => ValType::I32,
        "i64" => ValType::I64,
        "f32" => ValType::F32,
        "f64" => ValType::F64,
        "anyfunc" | "funcref" => ValType::FuncRef,
        "externref" => ValType::ExternRef,
        _ => return None,
    })
}

fn names<'js>(ctx: &Ctx<'js>, tys: &[ValType]) -> rquickjs::Result<Array<'js>> {
    let a = Array::new(ctx.clone())?;
    for (k, t) in tys.iter().enumerate() {
        a.set(k, val_name(*t))?;
    }
    Ok(a)
}

fn func_type_js<'js>(ctx: &Ctx<'js>, t: &FuncType) -> rquickjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    o.set("params", names(ctx, t.params())?)?;
    o.set("results", names(ctx, t.results())?)?;
    Ok(o)
}

fn extern_type_js<'js>(
    ctx: &Ctx<'js>,
    t: &ExternType,
) -> rquickjs::Result<(&'static str, Object<'js>)> {
    let o = Object::new(ctx.clone())?;
    let kind = match t {
        ExternType::Func(f) => return Ok(("function", func_type_js(ctx, f)?)),
        ExternType::Memory(m) => {
            o.set("minimum", m.minimum() as f64)?;
            if let Some(x) = m.maximum() {
                o.set("maximum", x as f64)?;
            }
            "memory"
        }
        ExternType::Table(t) => {
            o.set("element", val_name(t.element().into()))?;
            o.set("minimum", t.minimum() as f64)?;
            if let Some(x) = t.maximum() {
                o.set("maximum", x as f64)?;
            }
            "table"
        }
        ExternType::Global(g) => {
            o.set("value", val_name(g.content()))?;
            o.set("mutable", g.mutability().is_mut())?;
            "global"
        }
    };
    Ok((kind, o))
}

fn handle<'js>(ctx: &Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    o.set("s", s)?;
    o.set("i", i)?;
    Ok(o)
}

/// A JS value (already coerced by `js/wasm.js`) as a wasm value of type `t`.
fn to_val<'js>(
    ctx: &Ctx<'js>,
    s: u32,
    slot: &Slot,
    t: ValType,
    v: &Value<'js>,
) -> rquickjs::Result<Val> {
    let num = || v.as_number().unwrap_or(f64::NAN);
    Ok(match t {
        ValType::I32 => Val::I32(v.as_int().unwrap_or_else(|| num() as i64 as i32)),
        ValType::I64 => Val::I64(match v.as_big_int() {
            Some(b) => b.clone().to_i64()?,
            None => num() as i64,
        }),
        ValType::F32 => Val::F32(F32::from_float(num() as f32)),
        ValType::F64 => Val::F64(F64::from_float(num())),
        ValType::FuncRef => Val::FuncRef(match v.as_object() {
            None => Nullable::Null,
            Some(o) => {
                let (fs, fi): (u32, u32) = (o.get("s")?, o.get("i")?);
                if fs != s {
                    return Err(throw(
                        ctx,
                        "TypeError",
                        "a function from another plugin's WebAssembly cannot be used here",
                    ));
                }
                match slot.ext(fi).and_then(Extern::into_func) {
                    Some(f) => Nullable::Val(f),
                    None => return Err(throw(ctx, "TypeError", "not a WebAssembly function")),
                }
            }
        }),
        ValType::ExternRef => Val::ExternRef(if v.is_null() {
            Nullable::Null
        } else {
            let id = num() as u32;
            Nullable::Val(with_store(slot, |sc| ExternRef::new(sc, id)))
        }),
        ValType::V128 => {
            return Err(throw(
                ctx,
                "TypeError",
                "v128 values cannot cross into JavaScript",
            ));
        }
    })
}

/// A wasm value for `js/wasm.js` (`{s, i}` for a function, `{x}` for an
/// externref; it turns those into the JS values).
fn from_val<'js>(ctx: &Ctx<'js>, s: u32, slot: &Slot, v: &Val) -> rquickjs::Result<Value<'js>> {
    Ok(match v {
        Val::I32(n) => Value::new_int(ctx.clone(), *n),
        Val::I64(n) => BigInt::from_i64(ctx.clone(), *n)?.into_value(),
        Val::F32(f) => Value::new_float(ctx.clone(), f.to_float() as f64),
        Val::F64(f) => Value::new_float(ctx.clone(), f.to_float()),
        Val::FuncRef(Nullable::Null) | Val::ExternRef(Nullable::Null) => {
            Value::new_null(ctx.clone())
        }
        Val::FuncRef(Nullable::Val(f)) => handle(ctx, s, slot.push(Extern::Func(*f)))?.into_value(),
        Val::ExternRef(Nullable::Val(e)) => {
            let id = with_store(slot, |sc| e.data(&sc).downcast_ref::<u32>().copied());
            let o = Object::new(ctx.clone())?;
            o.set("x", id.unwrap_or(0))?;
            o.into_value()
        }
        Val::V128(_) => {
            return Err(throw(
                ctx,
                "TypeError",
                "v128 values cannot cross into JavaScript",
            ));
        }
    })
}

fn zero(t: ValType) -> Val {
    match t {
        ValType::I32 => Val::I32(0),
        ValType::I64 => Val::I64(0),
        ValType::F32 => Val::F32(F32::from_float(0.0)),
        ValType::F64 => Val::F64(F64::from_float(0.0)),
        ValType::V128 => Val::V128(wasmi::V128::from(0u128)),
        ValType::FuncRef => Val::FuncRef(Nullable::Null),
        ValType::ExternRef => Val::ExternRef(Nullable::Null),
    }
}

/// Keeps a JS context reachable for host calls while a native runs wasm,
/// and counts the call on its store.
struct Entered<'a> {
    w: &'a Wasm,
    slot: &'a Slot,
}

impl<'a> Entered<'a> {
    fn new(w: &'a Wasm, slot: &'a Slot, ctx: &Ctx<'_>) -> Self {
        w.ctxs.borrow_mut().push(ctx.as_raw());
        slot.depth.set(slot.depth.get() + 1);
        Entered { w, slot }
    }
}

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.w.ctxs.borrow_mut().pop();
        self.slot.depth.set(self.slot.depth.get() - 1);
    }
}

/// Why a wasm call ended early, for JavaScript.
fn call_error<'js>(ctx: &Ctx<'js>, w: &Wasm, e: wasmi::Error) -> rquickjs::Error {
    if let Some(p) = w.pending.borrow_mut().take()
        && let Ok(v) = p.restore(ctx)
    {
        return ctx.throw(v);
    }
    let msg = e.to_string();
    if msg.contains("interrupted") {
        // Like QuickJS's own watchdog interrupt: no `catch` may swallow it,
        // so the plugin is stopped and disabled (the one whose code called
        // into this wasm, read from the live frames).
        super::caller::note_interrupt(ctx);
        return match Exception::from_message(ctx.clone(), &msg) {
            Ok(exc) => {
                // SAFETY: `exc` is a live error object of this context.
                unsafe {
                    rquickjs::qjs::JS_SetUncatchableError(
                        ctx.as_raw().as_ptr(),
                        exc.as_value().as_raw(),
                    )
                };
                ctx.throw(exc.into_value())
            }
            Err(e) => e,
        };
    }
    throw(ctx, "RuntimeError", msg)
}

/// Run `f` on fuel slices, checking the wasm budget between slices. The
/// time spent in wasm is not charged to the JavaScript watchdog: after every
/// slice (and when the call ends) the JS deadline moves out by the time the
/// slice took, so the JS that wasm calls into, and the JS that called wasm,
/// is not stopped for a module that is merely slow on an interpreter (and a
/// JS loop that calls wasm still runs out of time on its own steps). A call
/// over the wasm budget is stopped and the plugin disabled, as with a JS
/// loop.
fn run(w: &Wasm, slot: &Slot, f: Func, args: &[Val], out: &mut [Val]) -> Result<(), wasmi::Error> {
    let limit = Instant::now() + w.budget;
    w.since.set(Instant::now());
    let credit = |now: Instant| w.credit(now);
    let first = with_store(slot, |mut sc| {
        sc.set_fuel(FUEL_SLICE)?;
        f.call_resumable(&mut sc, args, out)
    });
    credit(Instant::now());
    let mut call = first?;
    loop {
        match call {
            ResumableCall::Finished => return Ok(()),
            ResumableCall::HostTrap(t) => return Err(t.into_host_error()),
            ResumableCall::OutOfFuel(r) => {
                if Instant::now() > limit {
                    return Err(wasmi::Error::new(format!(
                        "interrupted: WebAssembly ran for more than {:.0}s without returning",
                        w.budget.as_secs_f32()
                    )));
                }
                let next = with_store(slot, |mut sc| {
                    sc.set_fuel(FUEL_SLICE.max(r.required_fuel()))?;
                    r.resume(&mut sc, out)
                });
                credit(Instant::now());
                call = next?;
            }
        }
    }
}

/// An imported JS function called from wasm.
fn host_call(
    mut caller: Caller<'_, Data>,
    s: u32,
    host: u32,
    ty: &FuncType,
    params: &[Val],
    results: &mut [Val],
) -> Result<(), wasmi::Error> {
    let gone = || wasmi::Error::new("WebAssembly host call outside of a JavaScript call");
    let w = state().ok_or_else(gone)?;
    let slot = w.slot(s).ok_or_else(gone)?;
    let raw = *w.ctxs.borrow().last().ok_or_else(gone)?;
    // SAFETY: `raw` belongs to a native call further up this thread's
    // stack (pushed by `Entered`), which holds the runtime lock.
    let ctx = unsafe { Ctx::from_raw(raw) };
    slot.callers
        .borrow_mut()
        .push(&mut caller as *mut Caller<'_, Data> as *mut ());
    struct Pop<'a>(&'a Slot);
    impl Drop for Pop<'_> {
        fn drop(&mut self) {
            self.0.callers.borrow_mut().pop();
        }
    }
    let _pop = Pop(&slot);
    slot.sync(&ctx);
    // The wasm time before this call is not charged to the JS watchdog, and
    // nor is a call's own time once it returns (below): wasm that calls JS
    // thousands of times is bounded by the wasm budget. One call that never
    // returns is still stopped, at the watchdog's time from here.
    w.credit(Instant::now());
    let r = (|| -> rquickjs::Result<()> {
        let f = w
            .host
            .borrow()
            .clone()
            .ok_or_else(|| throw(&ctx, "Error", "WebAssembly host missing"))?
            .restore(&ctx)?;
        let args = Array::new(ctx.clone())?;
        for (k, p) in params.iter().enumerate() {
            args.set(k, from_val(&ctx, s, &slot, p)?)?;
        }
        let out: Array = f.call((host, args, names(&ctx, ty.results())?))?;
        for (k, (r, t)) in results.iter_mut().zip(ty.results()).enumerate() {
            let v: Value = out.get(k)?;
            *r = to_val(&ctx, s, &slot, *t, &v)?;
        }
        Ok(())
    })();
    w.credit(Instant::now());
    match r {
        Ok(()) => Ok(()),
        Err(rquickjs::Error::Exception) => {
            let exc = ctx.catch();
            let interrupted = exc
                .as_exception()
                .and_then(|e| e.message())
                .is_some_and(|m| m.contains("interrupted"));
            *w.pending.borrow_mut() = Some(Persistent::save(&ctx, exc));
            Err(wasmi::Error::new(if interrupted {
                "interrupted"
            } else {
                "JavaScript exception"
            }))
        }
        Err(e) => Err(wasmi::Error::new(e.to_string())),
    }
}

fn bytes(buf: &ArrayBuffer<'_>, off: f64, len: f64) -> Vec<u8> {
    super::runtime::bytes_of(buf, off, len)
}

fn validate(buf: ArrayBuffer<'_>, off: f64, len: f64) -> bool {
    state().is_some_and(|w| Module::validate(&w.engine, &bytes(&buf, off, len)).is_ok())
}

fn compile<'js>(
    ctx: Ctx<'js>,
    owner: String,
    buf: ArrayBuffer<'js>,
    off: f64,
    len: f64,
) -> rquickjs::Result<u32> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let code = bytes(&buf, off, len);
    if !code.starts_with(b"\0asm") {
        return Err(throw(
            &ctx,
            "CompileError",
            "not a WebAssembly module (bad magic)",
        ));
    }
    let m = Module::new(&w.engine, &code).map_err(|e| throw(&ctx, "CompileError", e))?;
    let id = w.id();
    w.modules.borrow_mut().insert(id, (owner, m));
    Ok(id)
}

fn module(w: &Wasm, ctx: &Ctx<'_>, id: u32) -> rquickjs::Result<Module> {
    w.modules
        .borrow()
        .get(&id)
        .map(|(_, m)| m.clone())
        .ok_or_else(|| throw(ctx, "TypeError", "this WebAssembly.Module was unloaded"))
}

fn imports<'js>(ctx: Ctx<'js>, id: u32) -> rquickjs::Result<Array<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let m = module(&w, &ctx, id)?;
    let a = Array::new(ctx.clone())?;
    for (k, i) in m.imports().enumerate() {
        let (kind, ty) = extern_type_js(&ctx, i.ty())?;
        let o = Object::new(ctx.clone())?;
        o.set("module", i.module())?;
        o.set("name", i.name())?;
        o.set("kind", kind)?;
        o.set("type", ty)?;
        a.set(k, o)?;
    }
    Ok(a)
}

fn exports<'js>(ctx: Ctx<'js>, id: u32) -> rquickjs::Result<Array<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let m = module(&w, &ctx, id)?;
    let a = Array::new(ctx.clone())?;
    for (k, e) in m.exports().enumerate() {
        let (kind, ty) = extern_type_js(&ctx, e.ty())?;
        let o = Object::new(ctx.clone())?;
        o.set("name", e.name())?;
        o.set("kind", kind)?;
        o.set("type", ty)?;
        a.set(k, o)?;
    }
    Ok(a)
}

fn custom_sections<'js>(ctx: Ctx<'js>, id: u32, name: String) -> rquickjs::Result<Array<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let m = module(&w, &ctx, id)?;
    let a = Array::new(ctx.clone())?;
    let mut k = 0;
    for c in m.custom_sections() {
        if c.name() == name {
            a.set(k, ArrayBuffer::new_copy(ctx.clone(), c.data())?)?;
            k += 1;
        }
    }
    Ok(a)
}

/// Instantiate module `id`. `list` has one entry per module import:
/// `{s, i}` (a wasm object), `{h}` (a JS function registered with
/// `js/wasm.js`) or `{v}` (a global's value).
fn instantiate<'js>(
    ctx: Ctx<'js>,
    owner: String,
    id: u32,
    list: Array<'js>,
) -> rquickjs::Result<Object<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let m = module(&w, &ctx, id)?;
    // The store: the one of the objects it imports, else the caller's.
    let mut store: Option<u32> = None;
    for v in list.iter::<Object>() {
        if let Ok(Some(s)) = v?.get::<_, Option<u32>>("s") {
            if store.is_some_and(|x| x != s) {
                return Err(throw(
                    &ctx,
                    "LinkError",
                    "imports come from different plugins' WebAssembly",
                ));
            }
            store = Some(s);
        }
    }
    let (s, slot) = match store {
        Some(s) => (
            s,
            w.slot(s)
                .ok_or_else(|| throw(&ctx, "LinkError", "imported object was unloaded"))?,
        ),
        None => w.slot_for(&owner),
    };
    let mut externs = Vec::new();
    for (k, imp) in m.imports().enumerate() {
        let o: Object = list.get(k)?;
        let what = || format!("import {}.{}", imp.module(), imp.name());
        let e = if let Ok(Some(i)) = o.get::<_, Option<u32>>("i") {
            slot.ext(i)
                .ok_or_else(|| throw(&ctx, "LinkError", format!("{}: unknown object", what())))?
        } else if let Ok(Some(h)) = o.get::<_, Option<u32>>("h") {
            let ExternType::Func(ty) = imp.ty() else {
                return Err(throw(
                    &ctx,
                    "LinkError",
                    format!("{} must be a function", what()),
                ));
            };
            let t = ty.clone();
            Extern::Func(with_store(&slot, |sc| {
                Func::new(sc, ty.clone(), move |caller, params, results| {
                    host_call(caller, s, h, &t, params, results)
                })
            }))
        } else {
            let ExternType::Global(g) = imp.ty() else {
                return Err(throw(&ctx, "LinkError", format!("{}: bad import", what())));
            };
            if g.mutability().is_mut() {
                return Err(throw(
                    &ctx,
                    "LinkError",
                    format!("{} is mutable: pass a WebAssembly.Global", what()),
                ));
            }
            let v: Value = o.get("v")?;
            let val = to_val(&ctx, s, &slot, g.content(), &v)?;
            Extern::Global(with_store(&slot, |sc| {
                Global::new(sc, val, Mutability::Const)
            }))
        };
        // Check the type here: wasmi would otherwise refuse with a terse error.
        let got = with_store(&slot, |sc| e.ty(&sc));
        if !import_matches(&got, imp.ty()) {
            return Err(throw(
                &ctx,
                "LinkError",
                format!("{}: incompatible import type", what()),
            ));
        }
        externs.push(e);
    }
    let instance = {
        let _in = Entered::new(&w, &slot, &ctx);
        let r = with_store(&slot, |mut sc| {
            sc.set_fuel(START_FUEL)?;
            wasmi::Instance::new(&mut sc, &m, &externs)
        });
        slot.sync(&ctx);
        r.map_err(|e| {
            if w.pending.borrow().is_some() {
                call_error(&ctx, &w, e)
            } else {
                let msg = e.to_string();
                let kind = if msg.contains("trap") || msg.contains("fuel") {
                    "RuntimeError"
                } else {
                    "LinkError"
                };
                throw(&ctx, kind, msg)
            }
        })?
    };
    let out = Array::new(ctx.clone())?;
    let list: Vec<(String, Extern)> = with_store(&slot, |sc| {
        instance
            .exports(&sc)
            .map(|e| (e.name().to_string(), e.into_extern()))
            .collect()
    });
    for (k, (name, e)) in list.into_iter().enumerate() {
        let ty = with_store(&slot, |sc| e.ty(&sc));
        let (kind, t) = extern_type_js(&ctx, &ty)?;
        let o = handle(&ctx, s, slot.push(e))?;
        o.set("name", name)?;
        o.set("kind", kind)?;
        o.set("type", t)?;
        out.set(k, o)?;
    }
    let r = Object::new(ctx.clone())?;
    r.set("store", s)?;
    r.set("exports", out)?;
    Ok(r)
}

fn import_matches(got: &ExternType, want: &ExternType) -> bool {
    match (got, want) {
        (ExternType::Func(a), ExternType::Func(b)) => a == b,
        (ExternType::Global(a), ExternType::Global(b)) => {
            a.content() == b.content() && a.mutability() == b.mutability()
        }
        (ExternType::Memory(a), ExternType::Memory(b)) => {
            a.minimum() >= b.minimum()
                && match (a.maximum(), b.maximum()) {
                    (_, None) => true,
                    (Some(x), Some(y)) => x <= y,
                    (None, Some(_)) => false,
                }
        }
        (ExternType::Table(a), ExternType::Table(b)) => {
            a.element() == b.element()
                && a.minimum() >= b.minimum()
                && match (a.maximum(), b.maximum()) {
                    (_, None) => true,
                    (Some(x), Some(y)) => x <= y,
                    (None, Some(_)) => false,
                }
        }
        _ => false,
    }
}

fn lookup<'js>(ctx: &Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<(Rc<Wasm>, Rc<Slot>, Extern)> {
    let w = state().ok_or_else(|| no_state(ctx))?;
    let slot = w.slot(s).ok_or_else(|| {
        throw(
            ctx,
            "RuntimeError",
            "this plugin's WebAssembly was unloaded",
        )
    })?;
    let e = slot
        .ext(i)
        .ok_or_else(|| throw(ctx, "TypeError", "unknown WebAssembly object"))?;
    Ok((w, slot, e))
}

fn func_type<'js>(ctx: Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<Object<'js>> {
    let (_, slot, e) = lookup(&ctx, s, i)?;
    let f = e
        .into_func()
        .ok_or_else(|| throw(&ctx, "TypeError", "not a function"))?;
    let t = with_store(&slot, |sc| f.ty(&sc));
    func_type_js(&ctx, &t)
}

/// Call an exported function; `args` are already coerced to its types.
fn call<'js>(ctx: Ctx<'js>, s: u32, i: u32, args: Array<'js>) -> rquickjs::Result<Value<'js>> {
    let (w, slot, e) = lookup(&ctx, s, i)?;
    let f = e
        .into_func()
        .ok_or_else(|| throw(&ctx, "TypeError", "not a function"))?;
    let ty = with_store(&slot, |sc| f.ty(&sc));
    let mut params = Vec::with_capacity(ty.params().len());
    for (k, t) in ty.params().iter().enumerate() {
        let v: Value = args.get(k)?;
        params.push(to_val(&ctx, s, &slot, *t, &v)?);
    }
    let mut out: Vec<Val> = ty.results().iter().map(|t| zero(*t)).collect();
    let r = {
        let _in = Entered::new(&w, &slot, &ctx);
        let r = run(&w, &slot, f, &params, &mut out);
        slot.sync(&ctx);
        r
    };
    r.map_err(|e| call_error(&ctx, &w, e))?;
    match out.len() {
        0 => Ok(Value::new_undefined(ctx.clone())),
        1 => from_val(&ctx, s, &slot, &out[0]),
        _ => {
            let a = Array::new(ctx.clone())?;
            for (k, v) in out.iter().enumerate() {
                a.set(k, from_val(&ctx, s, &slot, v)?)?;
            }
            Ok(a.into_value())
        }
    }
}

fn pages<'js>(ctx: &Ctx<'js>, n: f64, what: &str) -> rquickjs::Result<u32> {
    if !(0.0..=65536.0).contains(&n) || n.fract() != 0.0 {
        return Err(throw(
            ctx,
            "RangeError",
            format!("{what} must be 0..65536 pages"),
        ));
    }
    Ok(n as u32)
}

fn mem_new<'js>(
    ctx: Ctx<'js>,
    owner: String,
    initial: f64,
    maximum: Option<f64>,
) -> rquickjs::Result<Object<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let min = pages(&ctx, initial, "initial")?;
    let max = maximum.map(|m| pages(&ctx, m, "maximum")).transpose()?;
    if max.is_some_and(|m| m < min) {
        return Err(throw(&ctx, "RangeError", "maximum is below initial"));
    }
    let (s, slot) = w.slot_for(&owner);
    let m = with_store(&slot, |sc| Memory::new(sc, MemoryType::new(min, max))).map_err(|e| {
        throw(
            &ctx,
            "RangeError",
            format!("could not allocate memory: {e}"),
        )
    })?;
    slot.sync(&ctx);
    handle(&ctx, s, slot.push(Extern::Memory(m)))
}

fn memory<'js>(ctx: &Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<(Rc<Slot>, Memory)> {
    let (_, slot, e) = lookup(ctx, s, i)?;
    let m = e
        .into_memory()
        .ok_or_else(|| throw(ctx, "TypeError", "not a memory"))?;
    Ok((slot, m))
}

fn mem_buffer<'js>(ctx: Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<ArrayBuffer<'js>> {
    let (slot, m) = memory(&ctx, s, i)?;
    slot.refresh(&ctx, Some(i as usize));
    if let Some(b) = slot.buffers.borrow().get(&(i as usize)) {
        return b.ab.clone().restore(&ctx);
    }
    let (ptr, len) = with_store(&slot, |sc| (m.data_ptr(&sc), m.data_size(&sc)));
    let ab = ArrayBuffer::from_source(ctx.clone(), Lent { ptr, len })?;
    slot.buffers.borrow_mut().insert(
        i as usize,
        Buf {
            ab: Persistent::save(&ctx, ab.clone()),
            ptr: ptr as usize,
            len,
        },
    );
    Ok(ab)
}

fn mem_grow<'js>(ctx: Ctx<'js>, s: u32, i: u32, delta: f64) -> rquickjs::Result<f64> {
    let (slot, m) = memory(&ctx, s, i)?;
    let delta = pages(&ctx, delta, "delta")?;
    let r = with_store(&slot, |sc| m.grow(sc, delta as u64));
    // Growing by 0 still detaches the old buffer (JS API).
    slot.detach_one(&ctx, i as usize);
    slot.sync(&ctx);
    r.map(|old| old as f64).map_err(|_| {
        throw(
            &ctx,
            "RangeError",
            "WebAssembly.Memory.grow(): maximum memory size exceeded",
        )
    })
}

impl Slot {
    fn detach_one(&self, ctx: &Ctx<'_>, i: usize) {
        let b = self.buffers.borrow_mut().remove(&i);
        if let Some(b) = b
            && let Ok(mut ab) = b.ab.restore(ctx)
        {
            ab.detach();
        }
    }
}

fn ref_type<'js>(ctx: &Ctx<'js>, element: &str) -> rquickjs::Result<RefType> {
    match element {
        "anyfunc" | "funcref" => Ok(RefType::Func),
        "externref" => Ok(RefType::Extern),
        _ => Err(throw(
            ctx,
            "TypeError",
            "table element must be \"anyfunc\" or \"externref\"",
        )),
    }
}

fn to_ref<'js>(
    ctx: &Ctx<'js>,
    s: u32,
    slot: &Slot,
    t: RefType,
    v: &Value<'js>,
) -> rquickjs::Result<Ref> {
    Ok(match to_val(ctx, s, slot, t.into(), v)? {
        Val::FuncRef(f) => Ref::Func(f),
        Val::ExternRef(e) => Ref::Extern(e),
        _ => Ref::Func(Nullable::Null),
    })
}

fn ref_val(r: Ref) -> Val {
    match r {
        Ref::Func(f) => Val::FuncRef(f),
        Ref::Extern(e) => Val::ExternRef(e),
    }
}

fn table_new<'js>(
    ctx: Ctx<'js>,
    owner: String,
    element: String,
    initial: f64,
    maximum: Option<f64>,
    init: Value<'js>,
) -> rquickjs::Result<Object<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let t = ref_type(&ctx, &element)?;
    let bad = |n: f64| !(0.0..=TABLE_LIMIT as f64).contains(&n) || n.fract() != 0.0;
    if bad(initial) || maximum.is_some_and(|m| bad(m) || m < initial) {
        return Err(throw(&ctx, "RangeError", "bad table size"));
    }
    let (s, slot) = w.slot_for(&owner);
    let r = to_ref(&ctx, s, &slot, t, &init)?;
    let table = with_store(&slot, |sc| {
        Table::new(
            sc,
            TableType::new(t, initial as u32, maximum.map(|m| m as u32)),
            r,
        )
    })
    .map_err(|e| throw(&ctx, "RangeError", e))?;
    handle(&ctx, s, slot.push(Extern::Table(table)))
}

fn table<'js>(ctx: &Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<(Rc<Slot>, Table)> {
    let (_, slot, e) = lookup(ctx, s, i)?;
    let t = e
        .into_table()
        .ok_or_else(|| throw(ctx, "TypeError", "not a table"))?;
    Ok((slot, t))
}

fn table_size<'js>(ctx: Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<f64> {
    let (slot, t) = table(&ctx, s, i)?;
    Ok(with_store(&slot, |sc| t.size(&sc)) as f64)
}

fn table_get<'js>(ctx: Ctx<'js>, s: u32, i: u32, index: f64) -> rquickjs::Result<Value<'js>> {
    let (slot, t) = table(&ctx, s, i)?;
    let r = with_store(&slot, |sc| t.get(&sc, index as u64))
        .filter(|_| index >= 0.0)
        .ok_or_else(|| throw(&ctx, "RangeError", "table index out of bounds"))?;
    from_val(&ctx, s, &slot, &ref_val(r))
}

fn table_set<'js>(
    ctx: Ctx<'js>,
    s: u32,
    i: u32,
    index: f64,
    v: Value<'js>,
) -> rquickjs::Result<()> {
    let (slot, t) = table(&ctx, s, i)?;
    let ty = with_store(&slot, |sc| t.ty(&sc)).element();
    let r = to_ref(&ctx, s, &slot, ty, &v)?;
    if index < 0.0 {
        return Err(throw(&ctx, "RangeError", "table index out of bounds"));
    }
    with_store(&slot, |sc| t.set(sc, index as u64, r))
        .map_err(|_| throw(&ctx, "RangeError", "table index out of bounds"))
}

fn table_grow<'js>(
    ctx: Ctx<'js>,
    s: u32,
    i: u32,
    delta: f64,
    v: Value<'js>,
) -> rquickjs::Result<f64> {
    let (slot, t) = table(&ctx, s, i)?;
    let ty = with_store(&slot, |sc| t.ty(&sc)).element();
    let r = to_ref(&ctx, s, &slot, ty, &v)?;
    if !(0.0..=TABLE_LIMIT as f64).contains(&delta) {
        return Err(throw(&ctx, "RangeError", "bad table grow delta"));
    }
    with_store(&slot, |sc| t.grow(sc, delta as u64, r))
        .map(|old| old as f64)
        .map_err(|_| {
            throw(
                &ctx,
                "RangeError",
                "WebAssembly.Table.grow(): failed to grow table",
            )
        })
}

fn global_new<'js>(
    ctx: Ctx<'js>,
    owner: String,
    ty: String,
    mutable: bool,
    v: Value<'js>,
) -> rquickjs::Result<Object<'js>> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    let t = val_type(&ty).ok_or_else(|| throw(&ctx, "TypeError", "bad global value type"))?;
    let (s, slot) = w.slot_for(&owner);
    let val = to_val(&ctx, s, &slot, t, &v)?;
    let m = if mutable {
        Mutability::Var
    } else {
        Mutability::Const
    };
    let g = with_store(&slot, |sc| Global::new(sc, val, m));
    handle(&ctx, s, slot.push(Extern::Global(g)))
}

fn global<'js>(ctx: &Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<(Rc<Slot>, Global)> {
    let (_, slot, e) = lookup(ctx, s, i)?;
    let g = e
        .into_global()
        .ok_or_else(|| throw(ctx, "TypeError", "not a global"))?;
    Ok((slot, g))
}

fn global_get<'js>(ctx: Ctx<'js>, s: u32, i: u32) -> rquickjs::Result<Value<'js>> {
    let (slot, g) = global(&ctx, s, i)?;
    let v = with_store(&slot, |sc| g.get(&sc));
    from_val(&ctx, s, &slot, &v)
}

fn global_set<'js>(ctx: Ctx<'js>, s: u32, i: u32, v: Value<'js>) -> rquickjs::Result<()> {
    let (slot, g) = global(&ctx, s, i)?;
    let t = with_store(&slot, |sc| g.ty(&sc));
    if !t.mutability().is_mut() {
        return Err(throw(
            &ctx,
            "TypeError",
            "can't set the value of an immutable global",
        ));
    }
    let val = to_val(&ctx, s, &slot, t.content(), &v)?;
    with_store(&slot, |sc| g.set(sc, val)).map_err(|e| throw(&ctx, "TypeError", e))
}

fn set_host<'js>(ctx: Ctx<'js>, f: Function<'js>) -> rquickjs::Result<()> {
    let w = state().ok_or_else(|| no_state(&ctx))?;
    *w.host.borrow_mut() = Some(Persistent::save(&ctx, f));
    Ok(())
}

/// Install `__wasm` (the natives `js/wasm.js` builds `WebAssembly` on) for
/// the runtime on this thread.
pub(super) fn install(
    ctx: &Ctx<'_>,
    deadline: Rc<Cell<Option<Instant>>>,
    budget: Duration,
    memory_limit: usize,
) -> rquickjs::Result<()> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let w = Rc::new(Wasm {
        engine: Engine::new(&config),
        modules: RefCell::default(),
        slots: RefCell::default(),
        owners: RefCell::default(),
        next: Cell::new(0),
        deadline,
        budget,
        since: Cell::new(Instant::now()),
        memory_limit,
        ctxs: RefCell::default(),
        host: RefCell::default(),
        pending: RefCell::default(),
    });
    WASM.with(|s| *s.borrow_mut() = Some(w));
    let o = Object::new(ctx.clone())?;
    macro_rules! native {
        ($($name:literal => $f:expr),* $(,)?) => {
            $( o.set($name, Function::new(ctx.clone(), $f)?)?; )*
        };
    }
    native! {
        "validate" => validate,
        "compile" => compile,
        "imports" => imports,
        "exports" => exports,
        "customSections" => custom_sections,
        "instantiate" => instantiate,
        "funcType" => func_type,
        "call" => call,
        "memNew" => mem_new,
        "memBuffer" => mem_buffer,
        "memGrow" => mem_grow,
        "tableNew" => table_new,
        "tableSize" => table_size,
        "tableGet" => table_get,
        "tableSet" => table_set,
        "tableGrow" => table_grow,
        "globalNew" => global_new,
        "globalGet" => global_get,
        "globalSet" => global_set,
        "setHost" => set_host,
    }
    ctx.globals().set("__wasm", o.into_js(ctx)?)?;
    Ok(())
}

/// Drop a plugin's store and modules (it was unloaded). Its buffers are
/// detached first; its JS objects that remain throw when used.
pub(super) fn drop_owner(ctx: &Ctx<'_>, owner: &str) {
    let Some(w) = state() else { return };
    let s = w.owners.borrow().get(owner).copied();
    if let Some(s) = s
        && let Some(slot) = w.slot(s)
        && slot.depth.get() == 0
    {
        slot.detach_all(ctx);
        w.owners.borrow_mut().remove(owner);
        w.slots.borrow_mut().remove(&s);
    }
    w.modules.borrow_mut().retain(|_, (o, _)| o != owner);
}

/// Drop everything before the JS runtime goes away.
pub(super) fn teardown(ctx: &Ctx<'_>) {
    let Some(w) = state() else { return };
    let slots: Vec<Rc<Slot>> = w.slots.borrow_mut().drain().map(|(_, s)| s).collect();
    for s in &slots {
        s.detach_all(ctx);
    }
    w.owners.borrow_mut().clear();
    w.modules.borrow_mut().clear();
    w.host.borrow_mut().take();
    w.pending.borrow_mut().take();
    drop(slots);
    WASM.with(|s| s.borrow_mut().take());
}
