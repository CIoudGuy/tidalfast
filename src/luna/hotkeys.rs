//! System-wide keyboard shortcuts for plugins the user allowed to have them
//! ([`super::Access::GlobalShortcuts`]): Electron's `globalShortcut`, which
//! otherwise fires only while Tidalfast's window has focus.
//!
//! * Windows: `RegisterHotKey` on a thread of its own with a message loop;
//!   a press arrives there as `WM_HOTKEY` and goes to the plugin thread.
//! * Linux and macOS: not available (no hotkey API is wired up there); the
//!   plugin's shortcuts keep working while the window has focus and the
//!   user is never asked.
//! * Every shortcut belongs to the plugin that registered it and is
//!   unregistered when the plugin stops or the permission is revoked.
//!
//! Media keys and Tidalfast's own media controls: the desktop's media
//! controls (`fastframe-now-playing`) already answer the media keys
//! everywhere. A plugin taking a media key system-wide must not make one
//! press count twice (play and pause again at once), so each press is acted
//! on once: [`plugin_media_key`] and [`native_media_command`] share the time
//! of the last press of each kind, and whichever comes second within
//! [`SAME_PRESS`] is dropped.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether this system has system-wide shortcuts for plugins.
pub const SUPPORTED: bool = cfg!(windows);

/// Two media-key reports this close together are one press.
pub const SAME_PRESS: Duration = Duration::from_millis(400);

/// What a media key does, for telling one press from two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    PlayPause,
    Next,
    Previous,
}

/// (kind, who) -> when; who: true for a plugin's hotkey, false for the
/// native media controls.
static LAST_PRESS: Mutex<Option<HashMap<MediaKind, (bool, Instant)>>> = Mutex::new(None);

fn first_of_press(kind: MediaKind, plugin: bool, now: Instant) -> bool {
    let mut last = LAST_PRESS.lock().unwrap_or_else(|e| e.into_inner());
    let map = last.get_or_insert_with(HashMap::new);
    if let Some((by, at)) = map.get(&kind)
        && *by != plugin
        && now.saturating_duration_since(*at) < SAME_PRESS
    {
        // The other side already acted on this press.
        map.remove(&kind);
        return false;
    }
    map.insert(kind, (plugin, now));
    true
}

/// A plugin's system-wide media key was pressed: whether its shortcut
/// should fire (false when Tidalfast's media controls already acted on this
/// press).
pub fn plugin_media_key(kind: MediaKind) -> bool {
    first_of_press(kind, true, Instant::now())
}

/// The native media controls got a command: whether Tidalfast should act
/// on it (false when a plugin's system-wide media key already did).
pub fn native_media_command(kind: MediaKind) -> bool {
    first_of_press(kind, false, Instant::now())
}

/// A parsed accelerator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// Windows virtual-key code.
    pub vk: u32,
}

impl Combo {
    pub fn media(&self) -> Option<MediaKind> {
        match self.vk {
            0xB3 | 0xB2 => Some(MediaKind::PlayPause),
            0xB0 => Some(MediaKind::Next),
            0xB1 => Some(MediaKind::Previous),
            _ => None,
        }
    }
}

/// Electron's accelerator syntax (`CommandOrControl+Shift+P`,
/// `MediaPlayPause`), as far as a key has a virtual-key code.
pub fn parse(accelerator: &str) -> Option<Combo> {
    let mut c = Combo {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        vk: 0,
    };
    // "Ctrl++": the plus key.
    let spelled = match accelerator.strip_suffix("++") {
        Some(rest) => format!("{rest}+Plus"),
        None => accelerator.to_string(),
    };
    for part in spelled.split('+') {
        let p = part.trim().to_ascii_lowercase();
        match p.as_str() {
            "" => return None,
            "commandorcontrol" | "cmdorctrl" | "control" | "ctrl" => c.ctrl = true,
            "command" | "cmd" | "super" | "meta" => c.meta = true,
            "alt" | "option" | "altgr" => c.alt = true,
            "shift" => c.shift = true,
            key => {
                if c.vk != 0 {
                    return None;
                }
                c.vk = vk(key)?;
            }
        }
    }
    (c.vk != 0).then_some(c)
}

fn vk(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return Some(b[0].to_ascii_uppercase() as u32);
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    if let Some(n) = key.strip_prefix("num").and_then(|n| n.parse::<u32>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    Some(match key {
        "space" | " " => 0x20,
        "tab" => 0x09,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "insert" => 0x2D,
        "return" | "enter" => 0x0D,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "escape" | "esc" => 0x1B,
        "volumeup" => 0xAF,
        "volumedown" => 0xAE,
        "volumemute" => 0xAD,
        "medianexttrack" => 0xB0,
        "mediaprevioustrack" => 0xB1,
        "mediastop" => 0xB2,
        "mediaplaypause" => 0xB3,
        "printscreen" => 0x2C,
        "plus" => 0xBB,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "/" => 0xBF,
        "`" => 0xC0,
        ";" => 0xBA,
        "=" => 0xBB,
        "[" => 0xDB,
        "\\" => 0xDC,
        "]" => 0xDD,
        "'" => 0xDE,
        _ => return None,
    })
}

/// Where a press goes: (plugin, accelerator).
pub type Fired = std::sync::Arc<dyn Fn(String, String) + Send + Sync>;

/// The shortcuts plugins hold system-wide. Lives on the plugin thread.
pub struct Hotkeys {
    /// (plugin, accelerator) -> id.
    held: HashMap<(String, String), i32>,
    next: i32,
    fired: Fired,
    #[cfg(windows)]
    thread: Option<win::Thread>,
}

impl Hotkeys {
    pub fn new(fired: Fired) -> Self {
        Hotkeys {
            held: HashMap::new(),
            next: 0,
            fired,
            #[cfg(windows)]
            thread: None,
        }
    }

    /// Take `accelerator` system-wide for `plugin` (the permission was
    /// checked). An error says why it cannot be (another app holds it, a
    /// key without a code, no support here).
    pub fn register(&mut self, plugin: &str, accelerator: &str) -> Result<(), String> {
        let key = (plugin.to_string(), accelerator.to_string());
        if self.held.contains_key(&key) {
            return Ok(());
        }
        let combo = parse(accelerator)
            .ok_or_else(|| format!("'{accelerator}' cannot be a system-wide shortcut"))?;
        self.next += 1;
        let id = self.next;
        self.platform_register(id, plugin, accelerator, combo)?;
        self.held.insert(key, id);
        Ok(())
    }

    pub fn unregister(&mut self, plugin: &str, accelerator: &str) {
        if let Some(id) = self
            .held
            .remove(&(plugin.to_string(), accelerator.to_string()))
        {
            self.platform_unregister(id);
        }
    }

    /// The plugin stopped or its permission was revoked.
    pub fn close_plugin(&mut self, plugin: &str) {
        let ids: Vec<i32> = self
            .held
            .iter()
            .filter(|((p, _), _)| p == plugin)
            .map(|(_, id)| *id)
            .collect();
        self.held.retain(|(p, _), _| p != plugin);
        for id in ids {
            self.platform_unregister(id);
        }
    }

    pub fn held_by(&self, plugin: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .held
            .keys()
            .filter(|(p, _)| p == plugin)
            .map(|(_, a)| a.clone())
            .collect();
        v.sort();
        v
    }

    /// The app is closing.
    pub fn stop_all(&mut self) {
        let ids: Vec<i32> = self.held.drain().map(|(_, id)| id).collect();
        for id in ids {
            self.platform_unregister(id);
        }
        #[cfg(windows)]
        if let Some(t) = self.thread.take() {
            t.quit();
        }
    }

    #[cfg(windows)]
    fn platform_register(
        &mut self,
        id: i32,
        plugin: &str,
        accelerator: &str,
        combo: Combo,
    ) -> Result<(), String> {
        if self.thread.is_none() {
            self.thread = Some(win::Thread::start()?);
        }
        let fired = self.fired.clone();
        let (plugin, accelerator) = (plugin.to_string(), accelerator.to_string());
        let media = combo.media();
        let on_press: win::OnPress = Box::new(move || {
            if media.is_some_and(|kind| !plugin_media_key(kind)) {
                return;
            }
            fired(plugin.clone(), accelerator.clone());
        });
        self.thread
            .as_ref()
            .ok_or("no hotkey thread")?
            .register(id, combo, on_press)
    }

    #[cfg(not(windows))]
    fn platform_register(
        &mut self,
        _id: i32,
        _plugin: &str,
        _accelerator: &str,
        _combo: Combo,
    ) -> Result<(), String> {
        let _ = &self.fired;
        Err("system-wide shortcuts are not available on this system".into())
    }

    #[cfg(windows)]
    fn platform_unregister(&mut self, id: i32) {
        if let Some(t) = &self.thread {
            t.unregister(id);
        }
    }

    #[cfg(not(windows))]
    fn platform_unregister(&mut self, _id: i32) {}
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(windows)]
mod win {
    use super::Combo;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY,
    };

    pub type OnPress = Box<dyn Fn() + Send>;

    enum Cmd {
        Register(i32, Combo, OnPress, mpsc::Sender<Result<(), String>>),
        Unregister(i32),
        Quit,
    }

    /// The thread that owns the hotkeys (`RegisterHotKey` posts to the
    /// queue of the thread that called it).
    pub struct Thread {
        tid: u32,
        tx: mpsc::Sender<Cmd>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Thread {
        pub fn start() -> Result<Thread, String> {
            let (tx, rx) = mpsc::channel::<Cmd>();
            let (ready_tx, ready_rx) = mpsc::channel();
            let handle = std::thread::Builder::new()
                .name("luna-hotkeys".into())
                .spawn(move || {
                    // SAFETY: plain Win32 calls on this thread's own queue.
                    unsafe {
                        let mut msg: MSG = std::mem::zeroed();
                        // Makes the queue exist before anyone posts to it.
                        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                        let _ = ready_tx.send(GetCurrentThreadId());
                        let mut presses: HashMap<i32, OnPress> = HashMap::new();
                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                            if msg.message == WM_HOTKEY {
                                if let Some(f) = presses.get(&(msg.wParam as i32)) {
                                    f();
                                }
                                continue;
                            }
                            if msg.message != WM_APP {
                                continue;
                            }
                            while let Ok(cmd) = rx.try_recv() {
                                match cmd {
                                    Cmd::Register(id, c, f, reply) => {
                                        let mut mods = MOD_NOREPEAT;
                                        if c.ctrl {
                                            mods |= MOD_CONTROL;
                                        }
                                        if c.alt {
                                            mods |= MOD_ALT;
                                        }
                                        if c.shift {
                                            mods |= MOD_SHIFT;
                                        }
                                        if c.meta {
                                            mods |= MOD_WIN;
                                        }
                                        let ok =
                                            RegisterHotKey(std::ptr::null_mut(), id, mods, c.vk)
                                                != 0;
                                        if ok {
                                            presses.insert(id, f);
                                        }
                                        let _ = reply.send(if ok {
                                            Ok(())
                                        } else {
                                            Err("another app already uses this shortcut".into())
                                        });
                                    }
                                    Cmd::Unregister(id) => {
                                        if presses.remove(&id).is_some() {
                                            UnregisterHotKey(std::ptr::null_mut(), id);
                                        }
                                    }
                                    Cmd::Quit => {
                                        for id in presses.keys() {
                                            UnregisterHotKey(std::ptr::null_mut(), *id);
                                        }
                                        return;
                                    }
                                }
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            let tid = ready_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|_| "the hotkey thread did not start".to_string())?;
            Ok(Thread {
                tid,
                tx,
                handle: Some(handle),
            })
        }

        fn post(&self, cmd: Cmd) -> bool {
            if self.tx.send(cmd).is_err() {
                return false;
            }
            // SAFETY: posting to a thread id this process owns.
            unsafe { PostThreadMessageW(self.tid, WM_APP, 0, 0) != 0 }
        }

        pub fn register(&self, id: i32, combo: Combo, f: OnPress) -> Result<(), String> {
            let (reply, answer) = mpsc::channel();
            if !self.post(Cmd::Register(id, combo, f, reply)) {
                return Err("the hotkey thread is gone".into());
            }
            answer
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|_| "the hotkey thread did not answer".to_string())?
        }

        pub fn unregister(&self, id: i32) {
            self.post(Cmd::Unregister(id));
        }

        /// Lets every key go and ends the thread, waiting for it.
        pub fn quit(mut self) {
            if self.post(Cmd::Quit)
                && let Some(h) = self.handle.take()
            {
                let _ = h.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_parse_like_electron() {
        let c = parse("CommandOrControl+Shift+P").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.meta);
        assert_eq!(c.vk, 'P' as u32);
        assert_eq!(
            parse("MediaPlayPause").unwrap().media(),
            Some(MediaKind::PlayPause)
        );
        assert_eq!(
            parse("MediaStop").unwrap().media(),
            Some(MediaKind::PlayPause)
        );
        assert_eq!(
            parse("MediaNextTrack").unwrap().media(),
            Some(MediaKind::Next)
        );
        assert_eq!(parse("F12").unwrap().vk, 0x7B);
        assert_eq!(parse("Ctrl++").unwrap().vk, 0xBB);
        assert!(parse("Ctrl+A+B").is_none());
        assert!(parse("Ctrl").is_none());
        assert!(parse("Hyper+Q").is_none());
    }

    /// One media-key press reported by both a plugin's hotkey and the
    /// native media controls is acted on once, whichever comes first; two
    /// presses apart are both acted on.
    #[test]
    fn one_media_key_press_counts_once() {
        let t = Instant::now();
        assert!(first_of_press(MediaKind::Previous, true, t));
        assert!(!first_of_press(
            MediaKind::Previous,
            false,
            t + Duration::from_millis(30)
        ));
        assert!(first_of_press(
            MediaKind::Previous,
            false,
            t + Duration::from_secs(2)
        ));
        assert!(!first_of_press(
            MediaKind::Previous,
            true,
            t + Duration::from_millis(2100)
        ));
        // The same side twice is two presses.
        assert!(first_of_press(
            MediaKind::Previous,
            true,
            t + Duration::from_secs(5)
        ));
        assert!(first_of_press(
            MediaKind::Previous,
            true,
            t + Duration::from_millis(5100)
        ));
    }

    #[test]
    fn closing_a_plugin_lets_its_shortcuts_go() {
        let mut h = Hotkeys::new(std::sync::Arc::new(|_, _| {}));
        // A combination nobody uses, so the real registration succeeds on
        // Windows; elsewhere it is refused and nothing is held.
        let r = h.register("p", "Ctrl+Alt+Shift+F23");
        assert_eq!(r.is_ok(), SUPPORTED, "{r:?}");
        assert_eq!(h.held_by("p").len(), usize::from(SUPPORTED));
        h.close_plugin("p");
        assert!(h.held_by("p").is_empty());
        if SUPPORTED {
            // Let go for real: it can be taken again.
            assert!(h.register("q", "Ctrl+Alt+Shift+F23").is_ok());
        }
        h.stop_all();
    }
}
