//! Draws Luna plugins' `<video>` elements (see `src/luna/video.rs`).
//!
//! The first pass that draws a clip asks for its frames at the drawn pixel
//! size; they are decoded once on a thread of their own. Each pass then
//! shows the frame for the clip's clock, uploading it into the element's
//! texture only when the frame changed, and asks for the next pass when the
//! next frame falls due: at most 30 a second, 15 while the window is not
//! focused, none while the clip is paused or nobody can see the window
//! (covered, hidden). With reduced motion the first frame stands still.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Rect, Sense, TextureHandle, pos2, vec2};

use crate::luna::dom::VNode;
use crate::luna::video;

/// Frames of clips not drawn for this long are let go (decoding them
/// again takes a moment, so a short absence keeps them).
const IDLE: Duration = Duration::from_secs(30);
const FRAME: Duration = Duration::from_micros(33_333);
const UNFOCUSED_FRAME: Duration = Duration::from_micros(66_667);

/// The clip id a VNode carries (`markup` = `video:<id>`).
fn key(node: &VNode) -> Option<u64> {
    node.markup.as_deref()?.strip_prefix("video:")?.parse().ok()
}

/// Whether `node` is a video with a loaded clip (others are not drawn).
pub fn has_clip(node: &VNode) -> bool {
    key(node).is_some_and(|id| video::clip(id).is_some())
}

struct Tex {
    handle: TextureHandle,
    /// Which frames (by their decode) and which frame of them it holds.
    decoded: usize,
    index: usize,
    pass: u64,
}

thread_local! {
    static TEXTURES: RefCell<HashMap<u64, Tex>> = RefCell::default();
}

/// The drawn size: the CSS size, one side from the other by the clip's
/// shape, else the `width`/`height` attributes, else the clip's own size.
fn size(node: &VNode, clip: [u32; 2], width: Option<f32>, height: Option<f32>) -> egui::Vec2 {
    let attr = |n: &str| {
        node.attrs
            .iter()
            .find(|(k, _)| k == n)
            .and_then(|(_, v)| v.trim().trim_end_matches("px").parse::<f32>().ok())
    };
    let (cw, ch) = (clip[0].max(1) as f32, clip[1].max(1) as f32);
    let width = width.or_else(|| attr("width"));
    let height = height.or_else(|| attr("height"));
    match (width, height) {
        (Some(w), Some(h)) => vec2(w, h),
        (Some(w), None) => vec2(w, w * ch / cw),
        (None, Some(h)) => vec2(h * cw / ch, h),
        (None, None) => vec2(cw, ch),
    }
}

/// Draws the video. `true` when it took room.
pub fn show(ui: &mut egui::Ui, node: &VNode, width: Option<f32>, height: Option<f32>) -> bool {
    let Some(id) = key(node) else { return false };
    let Some(clip) = video::clip(id) else {
        return false;
    };
    let size = size(node, clip.size(), width, height);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) || size.x <= 0.0 || size.y <= 0.0 {
        return true;
    }
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let want = [
        (size.x * ppp).ceil().max(1.0) as usize,
        (size.y * ppp).ceil().max(1.0) as usize,
    ];
    let frames = clip.frames(want);
    let reduced = crate::motion::reduced(&ctx);
    let now = Instant::now();
    let (time, runs) = clip.time(now);
    let time = if reduced { 0.0 } else { time };
    let Some((index, image)) = frames.frame_at(time) else {
        if !frames.done() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        return true;
    };
    let decoded = Arc::as_ptr(&frames) as usize;
    let pass = ctx.cumulative_pass_nr();
    let tex = TEXTURES.with(|t| {
        let mut t = t.borrow_mut();
        let handle = match t.get_mut(&id) {
            Some(e) if e.decoded == decoded && e.index == index => {
                e.pass = pass;
                e.handle.clone()
            }
            Some(e) => {
                e.handle
                    .set(image.as_ref().clone(), egui::TextureOptions::LINEAR);
                e.decoded = decoded;
                e.index = index;
                e.pass = pass;
                e.handle.clone()
            }
            None => {
                let handle = ctx.load_texture(
                    format!("luna-video-{id}"),
                    image.as_ref().clone(),
                    egui::TextureOptions::LINEAR,
                );
                t.insert(
                    id,
                    Tex {
                        handle: handle.clone(),
                        decoded,
                        index,
                        pass,
                    },
                );
                handle
            }
        };
        // Videos no longer drawn give their textures back.
        t.retain(|_, e| pass.saturating_sub(e.pass) < 600);
        handle
    });
    ui.painter().image(
        tex.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    if !frames.done() {
        // Frames still arriving: the newest is shown as it comes.
        ctx.request_repaint_after(Duration::from_millis(100));
    } else if runs && !reduced && !crate::theme::unseen(&ctx) {
        // Nobody sees a covered or hidden window: the clip's clock runs on,
        // and the pass that shows the window again draws the frame it is at.
        let rate = clip.rate().max(0.01);
        let next = frames.next_time(index).unwrap_or(clip.duration());
        let due = Duration::from_secs_f64(((next - time) / rate).clamp(0.0, 10.0));
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let floor = if focused { FRAME } else { UNFOCUSED_FRAME };
        ctx.request_repaint_after(due.max(floor));
    }
    true
}

/// Once a pass: frames of clips no longer drawn are let go. Nothing to do
/// while no plugin has a clip.
pub fn end_pass() {
    video::release_idle(IDLE);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether drawing the clip asked for the next pass.
    fn asks_for_frames(budget: crate::power::Budget) -> bool {
        let id = video::test_clip([52, 40], 25, 25.0);
        let node = VNode {
            tag: "video".into(),
            markup: Some(format!("video:{id}")),
            ..VNode::default()
        };
        let ctx = egui::Context::default();
        crate::theme::set_budget(&ctx, budget);
        for step in 0..3 {
            ctx.run_ui(
                egui::RawInput {
                    time: Some(f64::from(step) / 10.0),
                    ..Default::default()
                },
                |ui| {
                    assert!(show(ui, &node, Some(52.0), Some(40.0)));
                },
            )
            .textures_delta
            .clear();
        }
        // egui reports a pass's causes during the pass after it.
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        video::free(id);
        ctx.repaint_causes().iter().any(|cause| {
            cause
                .file
                .replace('\\', "/")
                .ends_with("src/ui/luna_video.rs")
        })
    }

    /// A playing clip (CatJam's cat) asks for its next frame while the
    /// window can be seen, and for none while it is covered or hidden, as
    /// the app's own animations do.
    #[test]
    fn a_clip_asks_for_frames_only_while_the_window_is_seen() {
        assert!(asks_for_frames(crate::power::Budget::Normal));
        assert!(!asks_for_frames(crate::power::Budget::Background));
    }
}
