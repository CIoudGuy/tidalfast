//! `--demo-bench`: repeatable performance numbers from a demo window.
//!
//! A run opens the demo, drives one scenario through the same frames a user
//! would (pages opened through actions, scrolling through wheel events), and
//! prints one report line before closing:
//!
//! - `startup`: process start to the first frame laid out and presented,
//!   then the working set while Home sits idle with its covers loaded.
//! - `scroll`: a 2,000-song playlist scrolled down and back up at a steady
//!   wheel speed; CPU time per frame (eframe's own measure: layout,
//!   tessellation and the GL submission, without waiting for vsync).
//! - `browse`: twenty pages opened one after another, then the working set.
//! - `session`: an hour of use compressed: fifty 550-song playlists opened
//!   and scrolled to the end, the album, artist, library and video pages,
//!   and the now-playing view with lyrics between them, with the memory
//!   after each lap of ten playlists and after a final idle spell.
//!
//! Development only: built with the `demo` feature, never in a release.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::app::App;
use crate::model::{Action, Page};

static START: OnceLock<Instant> = OnceLock::new();

/// Call first thing in `main`, so in-process times start from there.
pub fn mark_start() {
    START.get_or_init(Instant::now);
}

fn since_start() -> Duration {
    START.get_or_init(Instant::now).elapsed()
}

static PHASES: std::sync::Mutex<Vec<(&'static str, Duration)>> = std::sync::Mutex::new(Vec::new());

/// Notes that startup reached `name`, for the `startup` report's breakdown.
pub fn phase(name: &'static str) {
    let at = since_start();
    if let Ok(mut phases) = PHASES.lock() {
        phases.push((name, at));
    }
}

/// `name +ms` for each phase since the one before it.
fn phase_breakdown() -> String {
    let Ok(phases) = PHASES.lock() else {
        return String::new();
    };
    let mut last = Duration::ZERO;
    phases
        .iter()
        .map(|(name, at)| {
            let step = at.saturating_sub(last);
            last = *at;
            format!("{name} +{:.0}", ms(step))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Startup,
    Scroll,
    Browse,
    Session,
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "startup" => Ok(Self::Startup),
            "scroll" => Ok(Self::Scroll),
            "browse" => Ok(Self::Browse),
            "session" => Ok(Self::Session),
            _ => Err(format!(
                "expected startup, scroll, browse or session, got {text}"
            )),
        }
    }

    /// The page and demo surfaces the run opens with.
    pub fn demo_flags(self) -> (Option<&'static str>, Option<&'static str>) {
        match self {
            Self::Startup | Self::Browse | Self::Session => (Some("home"), None),
            Self::Scroll => (Some("playlist:pl1"), Some("big-playlist")),
        }
    }
}

/// Memory as the system counts it, in bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    /// Resident pages: the working set on Windows, RSS elsewhere.
    pub resident: u64,
    /// Private committed memory (Windows) where known.
    pub private: u64,
    /// The largest working set so far (Windows) where known.
    pub peak: u64,
}

#[cfg(windows)]
pub fn memory() -> Memory {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: the pseudo handle needs no closing and `counters` is sized
    // as declared in `cb`.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        return Memory::default();
    }
    Memory {
        resident: counters.working_set_size as u64,
        private: counters.pagefile_usage as u64,
        peak: counters.peak_working_set_size as u64,
    }
}

#[cfg(target_os = "linux")]
pub fn memory() -> Memory {
    let page = 4096;
    let resident = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0)
        * page;
    Memory {
        resident,
        ..Memory::default()
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn memory() -> Memory {
    Memory::default()
}

/// Time from the process's creation, which includes loading the executable
/// before `main`; falls back to the in-process clock.
#[cfg(windows)]
fn since_process_created() -> Duration {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    let mut times = [FileTime::default(); 4];
    // SAFETY: four valid out-pointers for the current process.
    let ok = unsafe {
        let [creation, exit, kernel, user] = &mut times;
        GetProcessTimes(GetCurrentProcess(), creation, exit, kernel, user)
    };
    let created = (u64::from(times[0].high) << 32) | u64::from(times[0].low);
    // FILETIME counts 100 ns ticks from 1601; the Unix epoch is this many later.
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    if ok == 0 || created < UNIX_EPOCH_TICKS {
        return since_start();
    }
    let created = Duration::from_nanos((created - UNIX_EPOCH_TICKS) * 100);
    now.checked_sub(created).unwrap_or_else(since_start)
}

#[cfg(not(windows))]
fn since_process_created() -> Duration {
    since_start()
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The system allocator, counting the bytes Rust code holds, so a report
/// can tell the application's own heap from what the graphics driver and
/// the system libraries hold. Demo builds only.
struct Counting;

static HELD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Allocations (and reallocations) made so far, for allocations a frame.
static ALLOCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// SAFETY: every call is passed on to the system allocator unchanged.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: as the caller promised.
        let ptr = unsafe { std::alloc::System.alloc(layout) };
        if !ptr.is_null() {
            HELD.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
            ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            count_on_thread();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        HELD.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
        // SAFETY: as the caller promised.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: as the caller promised.
        let ptr = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            HELD.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
            ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            count_on_thread();
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as the caller promised.
        let moved = unsafe { std::alloc::System.realloc(ptr, layout, new_size) };
        if !moved.is_null() {
            ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            count_on_thread();
            HELD.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
            HELD.fetch_add(new_size, std::sync::atomic::Ordering::Relaxed);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

thread_local! {
    /// Allocations made on this thread: a part of a frame, or a test,
    /// counted without the other threads' work.
    static THREAD_ALLOCS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn count_on_thread() {
    // A const thread local without a destructor never allocates or fails.
    let _ = THREAD_ALLOCS.try_with(|count| count.set(count.get() + 1));
}

/// Allocations made so far on this thread.
pub fn thread_allocations() -> u64 {
    THREAD_ALLOCS.try_with(std::cell::Cell::get).unwrap_or(0)
}

/// Whether a [`Part`] measures: while the browse run measures a page.
static PARTS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What each part of the interface cost on the passes measured: its name,
/// allocations, time and the number of passes.
type Parts = Vec<(&'static str, u64, Duration, u32)>;

static PARTS: std::sync::Mutex<Parts> = std::sync::Mutex::new(Vec::new());

/// A part of the interface's pass, from [`Part::start`] until it is
/// dropped: its allocations and time are counted while the browse run
/// measures the page open.
pub struct Part(Option<(&'static str, u64, Instant)>);

impl Part {
    pub fn start(name: &'static str) -> Self {
        Self(
            PARTS_ON
                .load(std::sync::atomic::Ordering::Relaxed)
                .then(|| (name, thread_allocations(), Instant::now())),
        )
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        let Some((name, allocs, started)) = self.0 else {
            return;
        };
        let took = started.elapsed();
        let allocs = thread_allocations() - allocs;
        if let Ok(mut parts) = PARTS.lock() {
            match parts.iter_mut().find(|part| part.0 == name) {
                Some(part) => {
                    part.1 += allocs;
                    part.2 += took;
                    part.3 += 1;
                }
                None => parts.push((name, allocs, took, 1)),
            }
        }
    }
}

/// `name N allocations M us` a pass for each part measured since the last
/// call, and starts over.
fn take_parts() -> String {
    let Ok(mut parts) = PARTS.lock() else {
        return String::new();
    };
    let text = parts
        .iter()
        .map(|(name, allocs, took, passes)| {
            let passes = (*passes).max(1);
            format!(
                "{name} {} allocs {:.0} us",
                allocs / u64::from(passes),
                took.as_secs_f64() * 1e6 / f64::from(passes)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    parts.clear();
    text
}

/// Allocations made so far, on every thread.
pub fn allocations() -> u64 {
    ALLOCS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Bytes Rust code holds on the heap now.
pub fn heap_held() -> u64 {
    HELD.load(std::sync::atomic::Ordering::Relaxed) as u64
}

/// Returns the process heap's free blocks to the system, for telling
/// fragmentation apart from memory in use.
#[cfg(windows)]
fn compact_heap() -> Option<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessHeaps(count: u32, heaps: *mut isize) -> u32;
        fn HeapCompact(heap: isize, flags: u32) -> usize;
    }
    let mut heaps = [0isize; 256];
    // SAFETY: the buffer holds as many handles as it says.
    let count = unsafe { GetProcessHeaps(heaps.len() as u32, heaps.as_mut_ptr()) } as usize;
    for heap in heaps.iter().take(count.min(heaps.len())) {
        // SAFETY: a heap handle the system just listed.
        unsafe { HeapCompact(*heap, 0) };
    }
    Some(())
}

#[cfg(not(windows))]
fn compact_heap() -> Option<()> {
    None
}

/// The textures egui holds and their base levels' bytes, without mipmaps.
fn textures(ctx: &egui::Context) -> (usize, u64) {
    let manager = ctx.tex_manager();
    let manager = manager.read();
    manager
        .allocated()
        .fold((0, 0), |(count, bytes), (_, meta)| {
            (count + 1, bytes + meta.bytes_used() as u64)
        })
}

/// The five largest textures egui holds, by name and size.
fn largest_textures(ctx: &egui::Context) -> String {
    let manager = ctx.tex_manager();
    let manager = manager.read();
    let mut all: Vec<(usize, String)> = manager
        .allocated()
        .map(|(_, meta)| {
            (
                meta.bytes_used(),
                format!("{} {}x{}", meta.name, meta.size[0], meta.size[1]),
            )
        })
        .collect();
    all.sort_by_key(|a| std::cmp::Reverse(a.0));
    all.iter()
        .take(5)
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The value at `fraction` (0..=1) of sorted samples.
fn percentile(sorted: &[f32], fraction: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f32 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// Pages the browse run opens, in order: twenty in all.
const BROWSE: [&str; 20] = [
    "playlist:pl1",
    "album:alb0",
    "artist:art0",
    "mix:mix0",
    "playlist:pl0",
    "liked",
    "albums",
    "artists",
    "playlists",
    "mixes",
    "top-songs",
    "search",
    "downloaded",
    "home",
    "playlist:pl1",
    "album:alb0",
    "artist:art0",
    "liked",
    "playlists",
    "home",
];

const SETTLE: Duration = Duration::from_secs(5);
const SCROLL_WARMUP: Duration = Duration::from_secs(2);
const SCROLL_LEG: Duration = Duration::from_secs(5);
/// Points the wheel moves the list each frame: a fast, steady flick.
const SCROLL_STEP: f32 = 90.0;
const BROWSE_DWELL: Duration = Duration::from_millis(1500);
/// From opening a page to measuring its parts in the browse run.
const BROWSE_SETTLE: Duration = Duration::from_millis(500);

/// One step of the `session` run.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// Open the benchmark's big playlist with this number.
    Playlist(usize),
    /// Open a page by its encoded name.
    Open(&'static str),
    /// Wheel the list in view down this many frames.
    Scroll(u32),
    /// The now-playing view with its lyrics tab, open or closed.
    NowPlaying(bool),
    Wait(Duration),
    /// Report the memory now, under this label.
    Mark(String),
}

/// Laps of the `session` run, each opening ten new big playlists.
const SESSION_LAPS: usize = 5;
/// Points the wheel moves a big playlist each frame in the `session` run:
/// 550 rows reach the end in about two hundred frames.
const SESSION_SCROLL_STEP: f32 = 160.0;
const SESSION_DWELL: Duration = Duration::from_millis(800);

fn session_steps() -> Vec<Step> {
    let mut steps = vec![
        Step::Wait(Duration::from_secs(3)),
        Step::Mark("Home".into()),
    ];
    for lap in 0..SESSION_LAPS {
        // The first lap tells which step costs what.
        let detail = lap == 0;
        for n in 0..10 {
            steps.push(Step::Playlist(lap * 10 + n));
            steps.push(Step::Wait(SESSION_DWELL));
            steps.push(Step::Scroll(200));
            if detail && matches!(n, 0 | 9) {
                steps.push(Step::Wait(SESSION_DWELL));
                steps.push(Step::Mark(format!("{} playlists", n + 1)));
            }
        }
        for page in [
            "album:alb0",
            "artist:art0",
            "home",
            "albums",
            "artists",
            "liked",
            "videos",
            "mixes",
        ] {
            steps.push(Step::Open(page));
            steps.push(Step::Wait(SESSION_DWELL));
            if detail {
                steps.push(Step::Mark(page.into()));
            }
        }
        steps.push(Step::NowPlaying(true));
        steps.push(Step::Wait(Duration::from_secs(3)));
        if detail {
            steps.push(Step::Mark("now playing, lyrics".into()));
        }
        steps.push(Step::NowPlaying(false));
        // Back to a playlist seen earlier in the lap: a hot page.
        steps.push(Step::Playlist(lap * 10 + 9));
        steps.push(Step::Wait(SESSION_DWELL));
        steps.push(Step::Mark(format!("lap {}", lap + 1)));
    }
    steps.push(Step::Open("home"));
    steps.push(Step::Wait(Duration::from_secs(15)));
    steps.push(Step::Mark("idle 15 s on Home".into()));
    steps
}

pub struct Bench {
    scenario: Scenario,
    passes: u32,
    first_ui: Option<Duration>,
    first_presented: Option<Duration>,
    first_presented_at: Option<Instant>,
    phase_started: Option<Instant>,
    /// Scroll: 0 warming up, 1 down, 2 up, 3 done. Browse: next page index.
    step: usize,
    cpu: Vec<f32>,
    intervals: Vec<f32>,
    /// Allocations between one measured pass and the next (every thread).
    allocs: Vec<f32>,
    last_allocs: u64,
    last_pass: Option<Instant>,
    after_first: Memory,
    /// The `session` run's steps, and the memory reported at its marks.
    steps: Vec<Step>,
    scrolling: u32,
    marks: Vec<(String, Memory)>,
    done: bool,
    pub out: Option<std::path::PathBuf>,
    /// The browse run's pages: [`BROWSE`], or `TIDALFAST_BENCH_PAGES`
    /// (encoded pages separated by commas) to measure others.
    pages: Vec<String>,
    /// The browse run's pages and what each part of a pass cost on them.
    parts: Vec<(String, String)>,
}

impl Bench {
    pub fn new(scenario: Scenario, out: Option<std::path::PathBuf>) -> Self {
        Self {
            scenario,
            passes: 0,
            first_ui: None,
            first_presented: None,
            first_presented_at: None,
            phase_started: None,
            step: 0,
            cpu: Vec::new(),
            intervals: Vec::new(),
            allocs: Vec::new(),
            last_allocs: 0,
            last_pass: None,
            after_first: Memory::default(),
            steps: if scenario == Scenario::Session {
                session_steps()
            } else {
                Vec::new()
            },
            scrolling: 0,
            marks: Vec::new(),
            done: false,
            out,
            pages: std::env::var("TIDALFAST_BENCH_PAGES").map_or_else(
                |_| BROWSE.iter().map(|page| page.to_string()).collect(),
                |pages| pages.split(',').map(str::to_string).collect(),
            ),
            parts: Vec::new(),
        }
    }

    /// After the app has drawn its frame.
    pub fn after_ui(&mut self) {
        if self.first_ui.is_none() {
            self.first_ui = Some(since_process_created());
            phase("first frame");
        }
    }

    /// Scripted input for this pass: the wheel over the song list.
    pub fn input(&mut self, raw: &mut egui::RawInput) {
        let session = self.scenario == Scenario::Session && self.scrolling > 0;
        if !session && (self.scenario != Scenario::Scroll || !matches!(self.step, 1 | 2)) {
            return;
        }
        let rect = raw.screen_rect.unwrap_or(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1240.0, 800.0),
        ));
        let pos = egui::pos2(rect.center().x + 80.0, rect.center().y + 60.0);
        let delta = if session {
            -SESSION_SCROLL_STEP
        } else if self.step == 1 {
            -SCROLL_STEP
        } else {
            SCROLL_STEP
        };
        raw.events.push(egui::Event::PointerMoved(pos));
        raw.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
    }

    /// Once per pass, before the app's logic.
    pub fn logic(&mut self, ctx: &egui::Context, frame: &eframe::Frame, app: &mut App) {
        if self.done {
            return;
        }
        ctx.request_repaint();
        let now = Instant::now();
        self.passes += 1;
        if self.passes == 2 && self.first_presented.is_none() {
            // The first frame has been painted and swapped by now.
            self.first_presented = Some(since_process_created());
            self.first_presented_at = Some(now);
            self.after_first = memory();
            self.phase_started = Some(now);
        }
        let Some(started) = self.phase_started else {
            return;
        };
        let measuring = self.scenario == Scenario::Scroll && matches!(self.step, 1 | 2);
        if measuring {
            if let Some(cpu) = frame.info().cpu_usage {
                self.cpu.push(cpu * 1000.0);
            }
            if let Some(last) = self.last_pass {
                self.intervals.push(ms(now - last) as f32);
            }
        }
        let allocs = allocations();
        if measuring && self.last_allocs != 0 {
            self.allocs.push((allocs - self.last_allocs) as f32);
        }
        self.last_allocs = allocs;
        self.last_pass = Some(now);
        let elapsed = now - started;
        match self.scenario {
            Scenario::Startup => {
                if elapsed >= SETTLE {
                    self.finish(ctx);
                }
            }
            Scenario::Scroll => {
                let leg = if self.step == 0 {
                    SCROLL_WARMUP
                } else {
                    SCROLL_LEG
                };
                if elapsed >= leg {
                    self.step += 1;
                    self.phase_started = Some(now);
                    if self.step == 3 {
                        self.finish(ctx);
                    }
                }
            }
            Scenario::Browse => {
                // Each page's parts are measured once it has settled, on
                // the passes the run asks for (every pass redraws it).
                let measure = self.step > 0 && elapsed >= BROWSE_SETTLE;
                PARTS_ON.store(measure, std::sync::atomic::Ordering::Relaxed);
                if self.step == 0 || elapsed >= BROWSE_DWELL {
                    if self.step > 0 && self.parts.len() < self.step {
                        let page = self.pages[self.step - 1].clone();
                        self.parts.push((page, take_parts()));
                    }
                    if let Some(page) = self
                        .pages
                        .get(self.step)
                        .and_then(|page| Page::decode(page))
                    {
                        app.actions.push(Action::Open(page));
                        self.step += 1;
                        self.phase_started = Some(now);
                    } else if elapsed >= SETTLE.min(BROWSE_DWELL * 2) {
                        self.finish(ctx);
                    }
                }
            }
            Scenario::Session => self.session(ctx, now, elapsed, app),
        }
    }

    /// Runs the `session` steps: each starts once the one before is over.
    fn session(&mut self, ctx: &egui::Context, now: Instant, elapsed: Duration, app: &mut App) {
        if self.scrolling > 0 {
            self.scrolling -= 1;
            return;
        }
        if let Some(Step::Wait(wait)) = self.step.checked_sub(1).and_then(|i| self.steps.get(i))
            && elapsed < *wait
        {
            return;
        }
        let Some(step) = self.steps.get(self.step).cloned() else {
            self.finish(ctx);
            return;
        };
        self.step += 1;
        self.phase_started = Some(now);
        match step {
            Step::Playlist(n) => {
                let page = crate::demo::bench_playlist(app, n);
                app.actions.push(Action::Open(page));
            }
            Step::Open(page) => {
                if let Some(page) = Page::decode(page) {
                    app.actions.push(Action::Open(page));
                }
            }
            Step::Scroll(frames) => self.scrolling = frames,
            Step::NowPlaying(true) => {
                crate::demo::apply_flags(app, None, Some("now-playing-lyrics"));
            }
            Step::NowPlaying(false) => app.now_playing_view = false,
            Step::Wait(_) => {}
            Step::Mark(label) => {
                let measured = memory();
                let (textures, texture_bytes) = textures(ctx);
                let loaders = ctx.loaders();
                let loaded: usize = loaders
                    .bytes
                    .lock()
                    .iter()
                    .map(|loader| loader.byte_size())
                    .chain(loaders.image.lock().iter().map(|loader| loader.byte_size()))
                    .sum();
                let compacted = compact_heap().map(|()| memory().private);
                println!(
                    "bench Session {label}: working set {:.1} MB, private {:.1} MB, \
                     {textures} textures of {:.1} MB, loaders {:.1} MB, \
                     private after compacting the heap {:.1} MB, Rust heap {:.1} MB;                      largest textures {}",
                    mb(measured.resident),
                    mb(measured.private),
                    mb(texture_bytes),
                    mb(loaded as u64),
                    compacted.map_or(0.0, mb),
                    mb(heap_held()),
                    largest_textures(ctx),
                );
                self.marks.push((label, measured));
            }
        }
    }

    fn finish(&mut self, ctx: &egui::Context) {
        self.done = true;
        let now = memory();
        let mut report = format!(
            "bench {:?}: first frame laid out {:.0} ms, presented {:.0} ms (from process creation); \
             working set after first frame {:.1} MB, private {:.1} MB",
            self.scenario,
            self.first_ui.map_or(0.0, ms),
            self.first_presented.map_or(0.0, ms),
            mb(self.after_first.resident),
            mb(self.after_first.private),
        );
        if self.scenario == Scenario::Startup {
            report.push_str(&format!(" [in-process ms: {}]", phase_breakdown()));
        }
        let label = match self.scenario {
            Scenario::Startup => "idle on Home",
            Scenario::Scroll => "after scrolling",
            Scenario::Browse => "after 20 pages",
            Scenario::Session => "after the session",
        };
        for (label, memory) in &self.marks {
            report.push_str(&format!(
                "; {label}: working set {:.1} MB, private {:.1} MB",
                mb(memory.resident),
                mb(memory.private),
            ));
        }
        let (texture_count, texture_bytes) = textures(ctx);
        report.push_str(&format!(
            "; {label}: working set {:.1} MB, private {:.1} MB, peak working set {:.1} MB, \
             {texture_count} textures of {:.1} MB, Rust heap {:.1} MB; largest textures {}",
            mb(now.resident),
            mb(now.private),
            mb(now.peak),
            mb(texture_bytes),
            mb(heap_held()),
            largest_textures(ctx),
        ));
        if !self.cpu.is_empty() {
            let mut cpu = self.cpu.clone();
            cpu.sort_by(f32::total_cmp);
            let mut intervals = self.intervals.clone();
            intervals.sort_by(f32::total_cmp);
            let slow = self.intervals.iter().filter(|gap| **gap > 20.0).count();
            report.push_str(&format!(
                "; {} frames, cpu ms p50 {:.2} p95 {:.2} p99 {:.2} max {:.2}; \
                 interval ms p50 {:.1} p99 {:.1}, {} over 20 ms",
                cpu.len(),
                percentile(&cpu, 0.5),
                percentile(&cpu, 0.95),
                percentile(&cpu, 0.99),
                cpu.last().copied().unwrap_or(0.0),
                percentile(&intervals, 0.5),
                percentile(&intervals, 0.99),
                slow,
            ));
        }
        if !self.allocs.is_empty() {
            let mut allocs = self.allocs.clone();
            allocs.sort_by(f32::total_cmp);
            report.push_str(&format!(
                "; allocations a frame p50 {:.0} p99 {:.0}",
                percentile(&allocs, 0.5),
                percentile(&allocs, 0.99),
            ));
        }
        for (page, parts) in &self.parts {
            report.push_str(&format!("; {page} a pass: {parts}"));
        }
        println!("{report}");
        log::info!("{report}");
        if let Some(path) = &self.out {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{report}");
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_pick_from_sorted_samples() {
        let samples: Vec<f32> = (1..=100).map(|n| n as f32).collect();
        assert_eq!(percentile(&samples, 0.5), 51.0);
        assert_eq!(percentile(&samples, 0.99), 99.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn every_browse_page_decodes() {
        for page in BROWSE {
            assert!(Page::decode(page).is_some(), "{page}");
        }
    }

    #[test]
    fn every_session_page_decodes_and_the_run_ends_on_a_mark() {
        let steps = session_steps();
        for step in &steps {
            if let Step::Open(page) = step {
                assert!(Page::decode(page).is_some(), "{page}");
            }
        }
        let playlists = steps
            .iter()
            .filter(|step| matches!(step, Step::Playlist(_)))
            .count();
        assert!(playlists >= 50, "{playlists}");
        assert!(matches!(steps.last(), Some(Step::Mark(_))));
    }

    #[test]
    fn scenarios_parse() {
        assert_eq!(Scenario::parse("scroll"), Ok(Scenario::Scroll));
        assert_eq!(Scenario::parse("session"), Ok(Scenario::Session));
        assert!(Scenario::parse("nope").is_err());
    }

    #[test]
    fn memory_is_measured_here() {
        if cfg!(any(windows, target_os = "linux")) {
            assert!(memory().resident > 0);
        }
    }
}
