//! Which plugin's code is running, read from the engine's own call stack.
//!
//! Every plugin module is loaded under the name `plugin:<name>@<generation>`
//! and every `.native` file under `native:<name>@<file>`; QuickJS records
//! that name in each function's bytecode. Walking the live frames (innermost
//! first) and taking the first one that is not the runtime's own shim code
//! names the plugin whose code made the current call. Unlike an
//! `Error().stack` string, no plugin can forge this: function names,
//! `Error.prepareStackTrace` and `Error.stackTraceLimit` do not reach it, and
//! the only way to give code another plugin's module name went with the
//! hidden `__eval`.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use rquickjs::Ctx;

/// Frames looked at, innermost first. Native (C/Rust) frames have no
/// module name and are passed over.
const MAX_LEVELS: isize = 128;

/// Who is running the current JavaScript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Caller {
    /// Only the runtime's own code is on the stack.
    Host,
    /// A plugin's module or `.native` code is the innermost non-shim code.
    Plugin(String),
    /// Code with no plugin's name is innermost (`eval`, `new Function`):
    /// nobody can be held responsible, so nothing is granted for it.
    Unknown,
}

impl Caller {
    pub(super) fn plugin(&self) -> Option<&str> {
        match self {
            Caller::Plugin(p) => Some(p),
            _ => None,
        }
    }
}

/// The plugin a module name belongs to (`plugin:<enc>@<gen>`,
/// `native:<enc>@<file>`).
pub(super) fn module_plugin(name: &str) -> Option<String> {
    let rest = name
        .strip_prefix("plugin:")
        .or_else(|| name.strip_prefix("native:"))?;
    let (enc, _) = rest.split_once('@')?;
    if enc.is_empty() {
        return None;
    }
    urlencoding::decode(enc).ok().map(|s| s.into_owned())
}

fn is_shim(name: &str) -> bool {
    name.starts_with("luna:") || name.starts_with("luna-shim:")
}

/// The innermost plugin on the live call stack.
pub(super) fn caller(ctx: &Ctx<'_>) -> Caller {
    for level in 0..MAX_LEVELS {
        let Some(atom) = ctx.script_or_module_name(level) else {
            continue;
        };
        let Ok(name) = atom.to_string() else {
            continue;
        };
        if is_shim(&name) {
            continue;
        }
        return match module_plugin(&name) {
            Some(p) => Caller::Plugin(p),
            None => Caller::Unknown,
        };
    }
    Caller::Host
}

/// Whom a runaway is blamed on: the innermost plugin on the stack, also
/// through code of its own `eval` (it ran that code).
fn blamed(ctx: &Ctx<'_>) -> Caller {
    for level in 0..MAX_LEVELS {
        let Some(name) = ctx
            .script_or_module_name(level)
            .and_then(|a| a.to_string().ok())
        else {
            continue;
        };
        if let Some(p) = module_plugin(&name) {
            return Caller::Plugin(p);
        }
    }
    Caller::Host
}

/// Interrupt-handler calls in a row, past the deadline, that found no
/// plugin on the stack before the runaway is stopped anyway.
const MAX_DEFERRALS: u32 = 200;

thread_local! {
    /// The Luna context on this thread, for the interrupt handler (which
    /// gets no context of its own).
    static CONTEXT: Cell<Option<NonNull<rquickjs::qjs::JSContext>>> = const { Cell::new(None) };
    /// Who was running when the watchdog last fired, until the failure it
    /// caused is handled.
    static BLAME: RefCell<Option<Caller>> = const { RefCell::new(None) };
    static DEFERRALS: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn set_context(ctx: Option<NonNull<rquickjs::qjs::JSContext>>) {
    CONTEXT.with(|c| c.set(ctx));
}

fn note(who: Caller) {
    DEFERRALS.with(|d| d.set(0));
    BLAME.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_none() {
            *b = Some(who);
        }
    });
}

/// The watchdog stops what `ctx` runs: remember who is running (the first
/// interrupt counts; unwinding may call the handler again).
pub(super) fn note_interrupt(ctx: &Ctx<'_>) {
    note(blamed(ctx));
}

/// The interrupt handler found the deadline passed: whether to stop now.
/// QuickJS also polls as a function is entered, before its frame is on the
/// stack (a promise job starting); then the stack shows no plugin, so the
/// stop waits for a poll that does (the next ones come within microseconds
/// of running JavaScript), for at most [`MAX_DEFERRALS`] polls.
pub(super) fn interrupt_now() -> bool {
    let Some(raw) = CONTEXT.with(Cell::get) else {
        return true;
    };
    // SAFETY: the pointer is the live Luna context of this thread (set
    // after it was created, cleared before it goes); the handler runs on
    // this thread while that context executes JavaScript.
    let ctx = unsafe { Ctx::from_raw(raw) };
    let who = blamed(&ctx);
    if who == Caller::Host {
        let n = DEFERRALS.with(|d| {
            let n = d.get() + 1;
            d.set(n);
            n
        });
        if n <= MAX_DEFERRALS {
            return false;
        }
    }
    note(who);
    true
}

/// A new watchdog budget starts.
pub(super) fn new_turn() {
    DEFERRALS.with(|d| d.set(0));
}

/// Who the last watchdog interrupt caught, if one happened since the last
/// call.
pub(super) fn take_blame() -> Option<Caller> {
    BLAME.with(|b| b.borrow_mut().take())
}

/// The plugin a stack string names (first frame with a plugin module), for
/// log attribution only: a stack string can be shaped by the code that
/// threw, so nothing is granted or blamed on it.
pub(super) fn plugin_from_stack(stack: &str) -> Option<String> {
    for line in stack.lines() {
        let Some(inner) = line.trim_end().strip_suffix(')') else {
            continue;
        };
        let Some((_, file)) = inner.rsplit_once(" (") else {
            continue;
        };
        // FILE:LINE:COL
        let file = file
            .rsplit_once(':')
            .and_then(|(f, _)| f.rsplit_once(':'))
            .map_or(file, |(f, _)| f);
        if let Some(p) = module_plugin(file) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names_name_their_plugin() {
        assert_eq!(module_plugin("plugin:A%20b@3").as_deref(), Some("A b"));
        assert_eq!(
            module_plugin("native:%40scope%2Fx@x/y.native.ts").as_deref(),
            Some("@scope/x")
        );
        assert_eq!(module_plugin("luna:core.js"), None);
        assert_eq!(module_plugin("<input>"), None);
        assert_eq!(module_plugin("plugin:@1"), None);
    }

    #[test]
    fn stack_strings_are_read_from_the_file_part() {
        let s = "    at f (luna:prelude.js:10:3)\n    at plugin:B@1 (plugin:A@2:5:7)\n";
        assert_eq!(plugin_from_stack(s).as_deref(), Some("A"));
        assert_eq!(plugin_from_stack("    at x (native)\n"), None);
    }
}
