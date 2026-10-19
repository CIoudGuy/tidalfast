//! Letting go of the tray item when the app ends.
//!
//! On Linux the item is ksni's StatusNotifierItem (through fastframe-tray).
//! ksni 0.3's service thread waits in a `select!` on two things, the session
//! bus's NameOwnerChanged stream and the handle's channel, with no `else`
//! arm. When the session bus is already gone (WSLg restarting, a logout
//! taking the bus down before the app), the first has ended; dropping the
//! handle ends the second, and the thread panics with "all branches are
//! disabled and there is no else branch". The release build aborts on any
//! panic, so quitting then crashed. The handle is kept alive instead: the
//! item goes away with the process's bus connection anyway, and while the
//! bus lives a dropped handle did not remove it before exit either.
//!
//! Elsewhere the tray is dropped normally, with a panic in its teardown
//! logged rather than unwound into the app's own exit.

/// Release the tray at exit (see the module docs).
pub(crate) fn release(tray: Option<fastframe_tray::Tray>) {
    let Some(tray) = tray else { return };
    #[cfg(target_os = "linux")]
    std::mem::forget(tray);
    #[cfg(not(target_os = "linux"))]
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(tray))).is_err() {
        log::warn!("The tray item panicked while being removed; ignored at exit");
    }
}
