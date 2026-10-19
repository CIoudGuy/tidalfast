//! The Windows check: the main window's client area against the windows
//! above it in z-order.
//!
//! While the window shows, a pass checks at most every [`CHECK_EVERY`]; a
//! check walks `GetWindow(GW_HWNDPREV)` up from the window and stops as soon
//! as the client area is all covered or the windows run out. While it is
//! covered, every pass checks (they are few: the clock and playback), and
//! WinEvent hooks on window moves, shows, hides, minimising, cloaking,
//! z-order and the foreground window wake the window at once, so one that
//! comes back into sight draws in the next pass instead of showing its last
//! frame. The hooks are out-of-context (`WINEVENT_OUTOFCONTEXT`): Windows
//! queues each event to this thread, whose message loop is winit's, and
//! nothing is loaded into other processes. They exist only while covered.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetWindowRgnBox};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_REORDER, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
    EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, GA_ROOT, GW_HWNDPREV, GWL_EXSTYLE,
    GetAncestor, GetClientRect, GetLayeredWindowAttributes, GetWindow, GetWindowLongPtrW,
    GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, LWA_ALPHA, LWA_COLORKEY, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

use super::{Above, Area, Uncovered};

/// How often a showing window looks again.
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// Windows 11 rounds a window's corners by 8 pixels at 100%.
const CORNER_AT_96_DPI: u32 = 8;
/// Walking more windows than this answers "showing".
const MAX_WINDOWS: usize = 4096;

/// The event ranges the hooks follow while covered.
const HOOKS: [(u32, u32); 6] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    // Destroy, show, hide and reorder.
    (EVENT_OBJECT_DESTROY, EVENT_OBJECT_REORDER),
    (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

thread_local! {
    /// The context the hooks wake, on the thread that owns the window.
    static WAKE: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    /// When the first event since the last check came in.
    static STIRRED: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Events seen while covered, for the log.
    static EVENTS: Cell<u64> = const { Cell::new(0) };
}

/// Follows whether the main window is fully covered. Lives with the window,
/// on its thread.
pub struct Watch {
    hwnd: HWND,
    covered: bool,
    next_check: Instant,
    hooks: Vec<HWINEVENTHOOK>,
    covered_since: Option<Instant>,
}

impl Watch {
    /// Watches `hwnd`, the main window, which this thread owns.
    pub fn new(hwnd: isize) -> Self {
        Self {
            hwnd: hwnd as HWND,
            covered: false,
            next_check: Instant::now(),
            hooks: Vec::new(),
            covered_since: None,
        }
    }

    /// Checks when due and records the answer on `ctx` for this pass.
    /// Runs at the start of the pass's logic, before anything is drawn.
    pub fn sync(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let stirred = STIRRED.with(|stirred| stirred.take());
        if self.covered || stirred.is_some() || now >= self.next_check {
            self.next_check = now + CHECK_EVERY;
            // SAFETY: the window belongs to this thread and outlives the
            // watch (the shell drops it with the window).
            let covered = unsafe { fully_covered(self.hwnd) };
            if covered != self.covered {
                self.covered = covered;
                if covered {
                    self.hook(ctx);
                    // A window that moved away before the hooks were in
                    // place sent its events to nobody: look once more.
                    // SAFETY: as above.
                    if self.covered && !unsafe { fully_covered(self.hwnd) } {
                        self.covered = false;
                        self.unhook();
                    }
                } else {
                    if let Some(at) = stirred {
                        log::debug!(
                            "main window showing: drawn {:.1} ms after the window event",
                            at.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                    self.unhook();
                }
            }
        }
        super::set_covered(ctx, self.covered);
    }

    fn hook(&mut self, ctx: &egui::Context) {
        WAKE.with(|wake| *wake.borrow_mut() = Some(ctx.clone()));
        EVENTS.with(|events| events.set(0));
        for (min, max) in HOOKS {
            // SAFETY: an out-of-context hook with a plain function; the
            // handle is unhooked in `unhook` or on drop, on this thread.
            let hook = unsafe {
                SetWinEventHook(
                    min,
                    max,
                    std::ptr::null_mut(),
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_null() {
                // Without a hook nothing would wake a covered window that
                // comes back into sight: treat it as showing.
                log::warn!("cannot watch window changes (event {min:#x}); drawing as usual");
                self.unhook();
                self.covered = false;
                return;
            }
            self.hooks.push(hook);
        }
        self.covered_since = Some(Instant::now());
        log::debug!("main window covered: drawing only what the logic needs");
    }

    fn unhook(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: a handle from `SetWinEventHook` on this thread.
            unsafe { UnhookWinEvent(hook) };
        }
        WAKE.with(|wake| wake.borrow_mut().take());
        if let Some(since) = self.covered_since.take() {
            log::debug!(
                "main window showing again after {:.1} s covered ({} window events)",
                since.elapsed().as_secs_f32(),
                EVENTS.with(Cell::get),
            );
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.unhook();
    }
}

/// A window event while covered: anything that can uncover the window
/// wakes it for a check.
unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    // Object events come for the caret and the cursor too. Moves come
    // often, from child windows as well: only a top-level window moving
    // matters there, and a tool window never counts as covering. A window
    // shown, hidden or destroyed may be gone before the event arrives, so
    // those count whatever the window was.
    let window = object == OBJID_WINDOW && child == CHILDID_SELF as i32 && !hwnd.is_null();
    let relevant = match event {
        EVENT_OBJECT_LOCATIONCHANGE => {
            window
                // SAFETY: plain queries on a window handle, which fail
                // harmlessly if it is gone.
                && unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd
                && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW == 0
        }
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE => window,
        _ => true,
    };
    if !relevant {
        return;
    }
    EVENTS.with(|events| events.set(events.get() + 1));
    STIRRED.with(|stirred| {
        if stirred.get().is_none() {
            stirred.set(Some(Instant::now()));
        }
    });
    WAKE.with(|wake| {
        if let Some(ctx) = wake.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}

/// Whether every pixel of `hwnd`'s client area is under an opaque window.
///
/// # Safety
/// `hwnd` must be a window handle; a stale one answers "showing".
unsafe fn fully_covered(hwnd: HWND) -> bool {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            // Hidden and minimised windows are eframe's to handle.
            return false;
        }
        if cloaked(hwnd) {
            // On another virtual desktop: nothing of it shows.
            return true;
        }
        let mut client: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client) == 0 {
            return false;
        }
        let mut origin = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return false;
        }
        let mut uncovered = Uncovered::new(Area::new(
            origin.x,
            origin.y,
            origin.x + client.right,
            origin.y + client.bottom,
        ));
        if uncovered.is_empty() {
            return false;
        }
        let mut above = GetWindow(hwnd, GW_HWNDPREV);
        let mut walked = 0;
        while !above.is_null() {
            walked += 1;
            if walked > MAX_WINDOWS {
                return false;
            }
            if let Some((frame, corner)) = cover_of(above, &uncovered) {
                uncovered.subtract_window(&frame, corner);
                if uncovered.is_empty() {
                    return true;
                }
            }
            above = GetWindow(above, GW_HWNDPREV);
        }
        false
    }
}

/// What `hwnd` hides, if it surely hides something still showing. Cheap
/// tests first: most windows above are invisible or elsewhere.
unsafe fn cover_of(hwnd: HWND, uncovered: &Uncovered) -> Option<(Area, i32)> {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return None;
        }
        let mut outer: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut outer) == 0
            || !uncovered.touches(&Area::new(outer.left, outer.top, outer.right, outer.bottom))
        {
            return None;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let layered = style & WS_EX_LAYERED != 0;
        let layered_opaque = layered && {
            let mut key = 0;
            let mut alpha = 0u8;
            let mut flags = 0;
            GetLayeredWindowAttributes(hwnd, &mut key, &mut alpha, &mut flags) != 0
                && flags & LWA_COLORKEY == 0
                && (flags & LWA_ALPHA == 0 || alpha == 255)
        };
        let mut region: RECT = std::mem::zeroed();
        let window = Above {
            visible: true,
            minimised: false,
            cloaked: cloaked(hwnd),
            click_through: style & WS_EX_TRANSPARENT != 0,
            tool: style & WS_EX_TOOLWINDOW != 0,
            layered,
            layered_opaque,
            composited_topmost: style & WS_EX_NOREDIRECTIONBITMAP != 0
                && style & WS_EX_TOPMOST != 0,
            // ERROR (0) means no region.
            shaped: GetWindowRgnBox(hwnd, &mut region) != 0,
            maximised: IsZoomed(hwnd) != 0,
            frame: frame(hwnd),
            corner: (CORNER_AT_96_DPI * GetDpiForWindow(hwnd).max(96)).div_ceil(96) as i32,
        };
        window.cover()
    }
}

/// Whether DWM hides `hwnd`.
unsafe fn cloaked(hwnd: HWND) -> bool {
    let mut value = 0u32;
    // SAFETY: the attribute is a DWORD.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&raw mut value).cast(),
            size_of::<u32>() as u32,
        )
    };
    result >= 0 && value != 0
}

/// `hwnd`'s visible frame: its window rectangle less the invisible resize
/// borders.
unsafe fn frame(hwnd: HWND) -> Option<Area> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: the attribute is a RECT.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    (result >= 0).then(|| Area::new(rect.left, rect.top, rect.right, rect.bottom))
}
