//! A development aid for keeping the idle app idle.
//!
//! With `TIDALFAST_REPAINT_TRACE=1` in the environment, the log gets one line
//! a second (or, after a quiet spell, one line covering it) with how many
//! passes ran, how many of them drew, and which call sites asked for them.
//! egui records the file and line of every `request_repaint*` call, so a
//! site that keeps the window busy shows up by name. A pass with no recorded
//! cause came from the window itself: input, a resize, focus or a wake-up.
//! Without the variable this costs one cached check per pass.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ENV: &str = "TIDALFAST_REPAINT_TRACE";
const WINDOW: Duration = Duration::from_secs(1);

struct Trace {
    since: Instant,
    passes: u32,
    paints: u32,
    causes: BTreeMap<String, u32>,
    /// Allocations counted when the window began (demo builds count them).
    #[cfg(feature = "demo")]
    allocs: u64,
}

impl Trace {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            passes: 0,
            paints: 0,
            causes: BTreeMap::new(),
            #[cfg(feature = "demo")]
            allocs: crate::bench::allocations(),
        }
    }

    fn line(&self, elapsed: Duration) -> String {
        let mut causes: Vec<_> = self.causes.iter().collect();
        causes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let causes = causes
            .into_iter()
            .map(|(site, count)| format!("{site} x{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        #[cfg(feature = "demo")]
        let allocs = format!(
            ", {} allocations",
            crate::bench::allocations().saturating_sub(self.allocs)
        );
        #[cfg(not(feature = "demo"))]
        let allocs = "";
        format!(
            "repaint trace: {} passes, {} paints in {:.1}s{allocs}; {}",
            self.passes,
            self.paints,
            elapsed.as_secs_f32(),
            if causes.is_empty() {
                "no causes"
            } else {
                &causes
            },
        )
    }
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os(ENV).is_some_and(|value| !value.is_empty() && value != "0"))
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

/// Counts one pass and the call sites that asked for it. Runs with the
/// logic, which eframe runs on every pass, drawn or not.
pub(super) fn note_pass(ctx: &egui::Context) {
    if !enabled() {
        return;
    }
    let sites: HashSet<String> = ctx
        .repaint_causes()
        .into_iter()
        .map(|cause| {
            // Shorten registry paths to the crate and file.
            let file = cause.file.replace('\\', "/");
            let file = file
                .rsplit_once("/src/")
                .map_or(file.as_str(), |(_, tail)| tail)
                .to_string();
            if cause.reason.is_empty() {
                format!("{file}:{}", cause.line)
            } else {
                format!("{file}:{} ({})", cause.line, cause.reason)
            }
        })
        .collect();
    let mut guard = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let trace = guard.get_or_insert_with(Trace::new);
    let elapsed = trace.since.elapsed();
    if elapsed >= WINDOW && trace.passes > 0 {
        log::info!("{}", trace.line(elapsed));
        *trace = Trace::new();
    } else if trace.passes == 0 {
        *trace = Trace::new();
    }
    trace.passes += 1;
    if sites.is_empty() {
        *trace.causes.entry("window event".into()).or_default() += 1;
    }
    for site in sites {
        *trace.causes.entry(site).or_default() += 1;
    }
}

/// Counts a pass that drew the window.
pub(super) fn note_paint() {
    if !enabled() {
        return;
    }
    let mut guard = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(Trace::new).paints += 1;
}

/// Records the app's own next pass and what asked for it, since egui's
/// causes name the call site but not the delay.
pub(super) fn note_scheduled(why: &'static str, due: Duration) {
    if !enabled() {
        return;
    }
    let mut guard = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = format!("next pass: {why} {}ms", due.as_millis());
    *guard
        .get_or_insert_with(Trace::new)
        .causes
        .entry(key)
        .or_default() += 1;
}
