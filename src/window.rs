//! Checks saved window positions before restoring them.
//!
//! eframe already places the window on-screen. The session can still refer
//! to a monitor that was unplugged, so check before moving the window there.

/// Says why no window opened on Windows on ARM and offers the fix.
///
/// Snapdragon graphics drivers have no OpenGL of their own. Windows provides
/// it on ARM through Microsoft's compatibility pack, a Store app that
/// translates it to Direct3D 12, so a missing or broken pack leaves eframe
/// without a context and the app would otherwise close without a word.
#[cfg(all(windows, target_arch = "aarch64"))]
pub fn report_missing_opengl(locale: crate::i18n::Locale) {
    use crate::i18n::gettext;
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONERROR, MB_YESNO, MessageBoxW};
    /// The Store page of the OpenCL, OpenGL & Vulkan Compatibility Pack.
    const STORE_PAGE: &str = "ms-windows-store://pdp/?ProductId=9NQPSL29BFFF";

    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let title = wide(&gettext(locale, "Tidalfast cannot open its window"));
    let text = wide(&gettext(
        locale,
        "On Windows on ARM, the OpenGL that Tidalfast draws with comes from Microsoft's OpenCL, OpenGL & Vulkan Compatibility Pack. Open its Microsoft Store page to install or repair it?",
    ));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call,
    // and the box has no owner window.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONERROR,
        )
    };
    if answer == IDYES
        && let Err(error) = open::that(STORE_PAGE)
    {
        log::warn!("cannot open the Microsoft Store: {error}");
    }
}

/// Handles a macOS title-bar double-click, or leaves a first click to drag.
#[cfg(target_os = "macos")]
pub fn macos_titlebar_should_drag() -> bool {
    use fastframe_macos::DoubleClick;
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSApplication, NSEventType};
    use objc2_foundation::NSObjectNSDelayedPerforming;

    let Some(mtm) = MainThreadMarker::new() else {
        return true;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(event) = app.currentEvent() else {
        return true;
    };
    if event.r#type() != NSEventType::LeftMouseDown || event.clickCount() != 2 {
        return true;
    }

    if let Some(window) = event.window(mtm) {
        // Let egui finish this frame before AppKit starts resizing the window.
        // SAFETY: Both NSWindow selectors take one optional sender argument.
        unsafe {
            match fastframe_macos::double_click_action() {
                // AppKit already fills the screen as part of the native
                // window drag.
                DoubleClick::Fill | DoubleClick::Nothing => {}
                DoubleClick::Minimize => window.performSelector_withObject_afterDelay(
                    sel!(performMiniaturize:),
                    None,
                    0.0,
                ),
                DoubleClick::Zoom => {
                    window.performSelector_withObject_afterDelay(sel!(performZoom:), None, 0.0)
                }
            }
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
pub fn macos_titlebar_should_drag() -> bool {
    true
}

/// Minimize the active window without leaving egui's macOS viewport flag stale
/// when the user later restores it from the Dock.
pub fn minimize_window(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    {
        use objc2::{MainThreadMarker, sel};
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSObjectNSDelayedPerforming;

        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let window = app.currentEvent().and_then(|event| event.window(mtm));
        if let Some(window) = window.or_else(|| app.keyWindow()) {
            // Let egui finish this frame before AppKit minimizes the window.
            // SAFETY: NSWindow's selector takes one optional sender argument.
            unsafe {
                window.performSelector_withObject_afterDelay(sel!(miniaturize:), None, 0.0);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    #[cfg(target_os = "macos")]
    let _ = ctx;
}

/// The main window's smallest size with no side panel open. The page and
/// its top bar fit beside the narrowest sidebar from here.
pub const MAIN_MIN_SIZE: [f32; 2] = [760.0, 520.0];

static FIXED_SIZE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the main window keeps one size it was given, as demo shots ask,
/// so the interface must not raise its minimum.
pub fn fixed_size() -> bool {
    FIXED_SIZE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pins the main window to the size it is created with.
pub fn set_fixed_size(on: bool) {
    FIXED_SIZE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A title-bar action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

#[cfg(target_os = "linux")]
pub struct LinuxWindowControls {
    pub left: Vec<WindowButton>,
    pub right: Vec<WindowButton>,
}

#[cfg(target_os = "linux")]
static LINUX_CONTROLS: std::sync::OnceLock<LinuxWindowControls> = std::sync::OnceLock::new();

#[cfg(target_os = "linux")]
pub(crate) static LINUX_ICONS: std::sync::OnceLock<[Option<Vec<u8>>; 4]> =
    std::sync::OnceLock::new();

/// Read desktop preferences at startup, before any UI is drawn. They come
/// through the desktop portal, which answers inside a Flatpak too, and each
/// read gives up after about a second. No GTK dependency or desktop
/// subprocess is needed while rendering a frame.
#[cfg(target_os = "linux")]
pub fn init_linux_window_controls() {
    LINUX_CONTROLS.get_or_init(|| {
        if fastframe_desktop::is_gnome() {
            let layout =
                fastframe_desktop::setting("org.gnome.desktop.wm.preferences", "button-layout")
                    .and_then(|layout| parse_linux_controls(&layout))
                    .filter(|controls| !controls.left.is_empty() || !controls.right.is_empty());
            layout.unwrap_or_else(|| parse_linux_controls(":close").unwrap())
        } else {
            parse_linux_controls(":minimize,maximize,close").unwrap()
        }
    });
    LINUX_ICONS.get_or_init(|| {
        let theme = fastframe_desktop::setting("org.gnome.desktop.interface", "icon-theme")
            .map(|theme| theme.trim().trim_matches('\'').to_owned())
            .filter(|theme| !theme.contains('/') && theme != "." && theme != "..")
            .unwrap_or_else(|| "Adwaita".into());
        let mut roots = Vec::new();
        if let Some(dirs) = directories::BaseDirs::new() {
            roots.push(dirs.data_dir().join("icons"));
            roots.push(dirs.home_dir().join(".icons"));
        }
        roots.extend(
            std::env::split_paths(
                &std::env::var_os("XDG_DATA_DIRS")
                    .unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
            )
            .map(|root| root.join("icons")),
        );
        [
            "window-minimize-symbolic",
            "window-maximize-symbolic",
            "window-close-symbolic",
            "window-restore-symbolic",
        ]
        .map(|name| {
            for theme in [theme.as_str(), "Adwaita"] {
                for root in &roots {
                    for folder in [
                        "symbolic/ui",
                        "symbolic/actions",
                        "scalable/actions",
                        "16x16/actions",
                    ] {
                        if let Ok(svg) = std::fs::read_to_string(
                            root.join(theme).join(folder).join(format!("{name}.svg")),
                        ) && svg.contains("</svg>")
                        {
                            // SVG loaders do not apply GTK's symbolic colour palette.
                            // Set monochrome fills to white so egui can tint them.
                            let fill = concat!(
                                "<style>path,rect,circle,ellipse,polygon,polyline",
                                " { fill: white !important; }</style></svg>",
                            );
                            return Some(svg.replace("</svg>", fill).into_bytes());
                        }
                    }
                }
            }
            None
        })
    });
}

#[cfg(target_os = "linux")]
pub fn linux_window_controls() -> &'static LinuxWindowControls {
    LINUX_CONTROLS.get_or_init(|| parse_linux_controls(":minimize,maximize,close").unwrap())
}

#[cfg(target_os = "linux")]
fn parse_linux_controls(layout: &str) -> Option<LinuxWindowControls> {
    use WindowButton::*;
    let layout = layout.trim().trim_matches('\'');
    let (left, right) = layout.split_once(':')?;
    let mut seen = Vec::new();
    let mut buttons = |side: &str| {
        side.split(',')
            .filter_map(|name| match name.trim() {
                "minimize" => Some(Minimize),
                "maximize" => Some(Maximize),
                "close" => Some(Close),
                _ => None,
            })
            .filter(|button| {
                if seen.contains(button) {
                    false
                } else {
                    seen.push(*button);
                    true
                }
            })
            .collect()
    };
    Some(LinuxWindowControls {
        left: buttons(left),
        right: buttons(right),
    })
}

#[cfg(not(test))]
static CUSTOM_TITLEBAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// Each test draws its own windows on its own thread. A process-wide choice
// let a test that turns the title bar on move every row of a test drawing at
// the same time, which then clicked beside its target.
#[cfg(test)]
thread_local! {
    static CUSTOM_TITLEBAR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the main window draws its own title bar and window buttons
/// instead of the platform's frame. Windows and Linux offer the choice, and it
/// is off unless the listener turns it on in Settings.
pub fn custom_titlebar() -> bool {
    #[cfg(not(test))]
    let chosen = CUSTOM_TITLEBAR.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    let chosen = CUSTOM_TITLEBAR.with(std::cell::Cell::get);
    custom_titlebar_for(cfg!(any(windows, target_os = "linux")), chosen)
}

/// Sets the title bar choice the next main window is created with.
pub fn set_custom_titlebar(on: bool) {
    #[cfg(not(test))]
    CUSTOM_TITLEBAR.store(on, std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    CUSTOM_TITLEBAR.with(|chosen| chosen.set(on));
}

const fn custom_titlebar_for(supported: bool, chosen: bool) -> bool {
    supported && chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The portal gives GNOME's button layout as a plain string, where
    /// gsettings quoted it; both read the same, and a side may be empty.
    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_button_layouts_read_with_or_without_quotes() {
        use WindowButton::*;
        for layout in ["appmenu:minimize,close", "'appmenu:minimize,close'\n"] {
            let controls = parse_linux_controls(layout).unwrap();
            assert!(controls.left.is_empty());
            assert_eq!(controls.right, [Minimize, Close]);
        }
        let controls = parse_linux_controls("close,maximize,close:").unwrap();
        assert_eq!(controls.left, [Close, Maximize], "each button once");
        assert!(controls.right.is_empty());
        assert!(parse_linux_controls("close").is_none(), "no side marker");
    }

    #[test]
    fn supported_platforms_draw_the_custom_title_bar_only_when_chosen() {
        assert!(custom_titlebar_for(true, true));
        assert!(!custom_titlebar_for(true, false));
        assert!(!custom_titlebar_for(false, true));
        assert!(!custom_titlebar_for(false, false));
    }
}
