//! Development only (demo builds): the plugin record. With
//! `TIDALFAST_DEMO_PLUGREC=<file>` the runtime runs `js/plugrec.js` after
//! its shims and writes what the plugins did every second to that file: the
//! DOM changes their own calls made, the redux actions they dispatched, the
//! requests they started, the errors they logged, the elements they put in
//! the page with their computed style, which of those elements the UI drew,
//! and the context-menu entries they added (drawn natively here, elements
//! in TIDAL's menu there). The same script records the same plugin in TIDAL's page in a
//! browser, so the two records can be compared.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use rquickjs::context::EvalOptions;
use rquickjs::{Ctx, Function};

/// The recorder, shared with the browser side.
pub const SCRIPT: &str = include_str!("js/plugrec.js");

/// Starts it in the runtime: the plugin is whoever the engine's frames
/// name, and the host tree's own mutations are not counted.
const START: &str = r#"(() => {
	const L = globalThis.__lunaInternal;
	const write = globalThis.__plugrec_write;
	delete globalThis.__plugrec_write;
	globalThis.__plugrecInstall({ owner: () => L.pluginFromStack() !== undefined, write, hostFlag: true, store: () => L.reduxStore });
})();"#;

/// The most drawn element ids kept.
const MAX_DRAWN: usize = 100_000;

static ON: AtomicBool = AtomicBool::new(false);
static DRAWN: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
/// (plugin, words) of every context-menu entry shown.
static MENU: Mutex<BTreeSet<(String, String)>> = Mutex::new(BTreeSet::new());

/// A plugin element (its virtual DOM id) was drawn by the UI.
pub fn note_drawn(id: u64) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let mut drawn = DRAWN.lock().unwrap_or_else(|p| p.into_inner());
    if drawn.len() < MAX_DRAWN {
        drawn.insert(id);
    }
}

/// Plugins' context-menu entries were shown: the visible ones are kept.
pub(super) fn note_menu(items: &[super::ContextMenuItem]) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let mut menu = MENU.lock().unwrap_or_else(|p| p.into_inner());
    for item in items.iter().filter(|i| !i.hidden && !i.text.is_empty()) {
        if menu.len() < 1000 {
            menu.insert((item.plugin.clone().unwrap_or_default(), item.text.clone()));
        }
    }
}

/// The record (the script's JSON), the drawn ids and the menu entries, written to `path`
/// through a temporary file.
fn write(path: &Path, record: &str) -> std::io::Result<()> {
    let record: serde_json::Value = serde_json::from_str(record).map_err(std::io::Error::other)?;
    let drawn: Vec<u64> = DRAWN
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .copied()
        .collect();
    let menu: Vec<serde_json::Value> = MENU
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|(plugin, text)| serde_json::json!({ "plugin": plugin, "text": text }))
        .collect();
    let out = serde_json::json!({ "record": record, "drawn": drawn, "menu": menu });
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec(&out).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(&tmp, path)
}

/// Runs the recorder in `ctx`, after the shims and before the lockdown
/// takes their internals away.
pub(super) fn install(ctx: &Ctx<'_>, path: PathBuf) -> rquickjs::Result<()> {
    ON.store(true, Ordering::Relaxed);
    ctx.globals().set(
        "__plugrec_write",
        Function::new(ctx.clone(), move |json: String| {
            if let Err(e) = write(&path, &json) {
                log::warn!("plugin record: {e}");
            }
        })?,
    )?;
    for (file, src) in [("plugrec.js", SCRIPT), ("plugrec-start.js", START)] {
        let mut opts = EvalOptions::default();
        opts.strict = false;
        opts.filename = Some(format!("luna:{file}"));
        ctx.eval_with_options::<(), _>(src, opts)?;
    }
    Ok(())
}
