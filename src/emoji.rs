//! Colour emoji in every text, in the platform's own style.
//!
//! fastframe-emoji finds the platform's colour emoji font (Apple Color Emoji,
//! Segoe UI Emoji, the desktop's font on Linux) and its egui plugin, added
//! in [`crate::theme::install`], paints each emoji's picture over the
//! monochrome glyph egui laid out. Layout, truncation and right-to-left
//! reordering stay egui's own.
//!
//! Without a colour emoji font (a Linux desktop with none installed) the
//! bundled monochrome face draws the emoji, as it always has.

/// Chooses the platform's emoji font and starts finding it off this thread.
/// `synchronous` draws each picture in the frame that first shows it, for
/// demo captures that must show every emoji in their first frame.
pub fn install(synchronous: bool) {
    fastframe_emoji::EmojiSetup::default()
        .system(true)
        .synchronous(synchronous)
        .install();
    std::thread::spawn(fastframe_emoji::warm_up);
}
