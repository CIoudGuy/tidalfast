//! Keeps a scrolled page's view in place while content above it changes
//! height (a row added to a list at the top, an update bar that goes away,
//! a card removed), and remembers where a page's tabs were scrolled to.
//!
//! The page reports its blocks (a card, a row, a grid) as it draws them,
//! with the vertical span each took and whether the user acted on it this
//! frame. At the end of the page, [`settle`] finds the block the user is
//! looking at (the one just acted on, else the first one in view) in the
//! frame before and in this one; the distance it moved is the height that
//! changed above it. The host's scroll area then moves by that distance
//! ([`finish`]) and the pass is thrown away and drawn again, so the shifted
//! frame is never shown.
//!
//! Positions are kept relative to where the page began, so a user's own
//! scrolling never counts as a change.

use egui::{Context, Id, Ui};
use std::hash::Hash;

/// A block moving by less than this is not a change (float noise).
const EPSILON: f32 = 0.5;

/// How many times a frame may be drawn before it is shown.
const PASSES: usize = 3;

/// One block reported by the page: its span from the page's start.
#[derive(Clone, Copy)]
struct Block {
    id: Id,
    top: f32,
    bottom: f32,
}

/// What this pass has drawn so far.
#[derive(Clone, Default)]
struct Pass {
    /// Where the page began on screen.
    origin: f32,
    /// The scroll area's viewport, relative to `origin`.
    view: (f32, f32),
    blocks: Vec<Block>,
    /// The block the user acted on this frame.
    acted: Option<Id>,
}

/// What the last finished pass saw, and the block the user last acted on.
#[derive(Clone, Default)]
struct Memory {
    blocks: Vec<Block>,
    view: (f32, f32),
    acted: Option<Id>,
}

/// What the page asks of the scroll area.
#[derive(Clone, Copy, Default)]
struct Wish {
    /// Move by this much.
    shift: f32,
    /// Go to this offset instead.
    goto: Option<f32>,
}

fn id(name: &str) -> Id {
    Id::new(("scroll-anchor", name))
}

/// Start of the page's content: call first, before anything the page draws.
pub fn begin(ui: &Ui) {
    let origin = ui.cursor().top();
    let clip = ui.clip_rect();
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            id("pass"),
            Pass {
                origin,
                view: (clip.top() - origin, clip.bottom() - origin),
                ..Pass::default()
            },
        );
    });
}

/// A block of the page, from `top` to `bottom` on screen; `acted` when the
/// user did something in it this frame.
pub fn block(ui: &Ui, key: impl Hash + std::fmt::Debug, top: f32, bottom: f32, acted: bool) {
    let block_id = Id::new(key);
    ui.ctx().data_mut(|d| {
        let pass = d.get_temp_mut_or_default::<Pass>(id("pass"));
        let at = pass.origin;
        pass.blocks.push(Block {
            id: block_id,
            top: top - at,
            bottom: bottom - at,
        });
        if acted {
            pass.acted = Some(block_id);
        }
    });
}

/// Forget what the page looked like: its content is about to be another.
pub fn reset(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(id("memory"), Memory::default()));
}

/// The end of the page: works out how far the view has to move to keep
/// what the user is looking at where it was.
pub fn settle(ui: &Ui) {
    let ctx = ui.ctx();
    let offset = offset(ctx);
    let (pass, memory) = ctx.data_mut(|d| {
        (
            d.get_temp::<Pass>(id("pass")).unwrap_or_default(),
            d.get_temp::<Memory>(id("memory")).unwrap_or_default(),
        )
    });
    let acted = pass.acted.or(memory.acted);
    let shift = anchor_shift(&memory, &pass, acted, offset > 0.0);
    // The block acted on stays the anchor while it is in view.
    let acted = acted.filter(|a| {
        pass.blocks
            .iter()
            .find(|b| b.id == *a)
            .is_some_and(|b| in_view(b, pass.view))
    });
    ctx.data_mut(|d| {
        d.insert_temp(
            id("memory"),
            Memory {
                blocks: pass.blocks,
                view: pass.view,
                acted,
            },
        );
        if shift.abs() > EPSILON {
            let wish = d.get_temp_mut_or_default::<Wish>(id("wish"));
            wish.shift += shift;
        }
    });
}

fn in_view(block: &Block, view: (f32, f32)) -> bool {
    block.bottom > view.0 && block.top < view.1
}

/// How far the block the user is looking at moved since the last frame.
fn anchor_shift(memory: &Memory, pass: &Pass, acted: Option<Id>, scrolled: bool) -> f32 {
    // Only what was in view counts: a block out of sight above or below
    // says nothing of where the page the user is looking at went.
    let seen: Vec<&Block> = memory
        .blocks
        .iter()
        .filter(|b| in_view(b, memory.view))
        .collect();
    // The block acted on first, then the ones after it (they lie below, as
    // it did), then those before it, nearest first; without one, the blocks
    // in view, from the top of the page, and only when it is scrolled (at
    // its very top the page just grows downwards).
    let mut order: Vec<&Block> = Vec::new();
    match acted.and_then(|a| seen.iter().position(|b| b.id == a)) {
        Some(at) => {
            order.extend(&seen[at..]);
            order.extend(seen[..at].iter().rev());
        }
        None if scrolled => order = seen,
        None => {}
    }
    order
        .into_iter()
        .find_map(|old| {
            pass.blocks
                .iter()
                .find(|b| b.id == old.id)
                .map(|now| now.top - old.top)
        })
        .unwrap_or(0.0)
}

/// Where the scroll area was left, as the host last saw it.
pub fn offset(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(id("offset"))).unwrap_or(0.0)
}

/// Ask for the scroll area to be put at `y`.
pub fn goto(ctx: &Context, y: f32) {
    ctx.data_mut(|d| {
        d.insert_temp(
            id("wish"),
            Wish {
                shift: 0.0,
                goto: Some(y),
            },
        );
    });
}

/// The first thing in the host's scroll area (called inside it, before
/// anything is drawn): notes where its content begins, which with the
/// area's top says how far it is scrolled in this pass, before egui has
/// clamped that to the content's length.
pub fn content_start(ui: &Ui) {
    let top = ui.cursor().top();
    ui.ctx().data_mut(|d| d.insert_temp(id("start"), top));
}

/// After the host's scroll area (`area`, now at `offset`, with `max` the
/// furthest it can go and `top` the top of its view) has been shown:
/// applies what the page asked for. The pass is redrawn when the view
/// moved, so the frame before the move is never shown.
pub fn finish(ctx: &Context, area: Id, offset: f32, max: f32, top: f32) {
    // A row that learns its height after it is first drawn moves the view
    // again in the pass that follows the first move: room for a third.
    if ctx.options(|o| o.max_passes.get()) < PASSES {
        ctx.options_mut(|o| {
            o.max_passes = std::num::NonZeroUsize::new(PASSES).unwrap_or(o.max_passes)
        });
    }
    let wish = ctx
        .data_mut(|d| d.remove_temp::<Wish>(id("wish")))
        .unwrap_or_default();
    let drawn = top - ctx.data(|d| d.get_temp::<f32>(id("start"))).unwrap_or(top);
    let wanted = wish
        .goto
        .or_else(|| (wish.shift.abs() > EPSILON).then_some(drawn + wish.shift));
    let target = wanted.map_or(offset, |y| y.clamp(0.0, max.max(0.0)));
    ctx.data_mut(|d| d.insert_temp(id("offset"), target));
    // What was drawn this pass is what the target must match, not the
    // offset egui has since clamped to the content's new length.
    if wanted.is_none() || (target - drawn).abs() <= EPSILON {
        return;
    }
    if let Some(mut state) = egui::scroll_area::State::load(ctx, area) {
        state.offset.y = target;
        state.store(ctx, area);
        ctx.request_discard("scroll anchor");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_at(key: &str, top: f32, bottom: f32) -> Block {
        Block {
            id: Id::new(key),
            top,
            bottom,
        }
    }

    /// What the last frame saw, with the view from 50 to 500.
    fn memory(blocks: Vec<Block>) -> Memory {
        Memory {
            blocks,
            view: (50.0, 500.0),
            acted: None,
        }
    }

    fn pass(blocks: Vec<Block>) -> Pass {
        Pass {
            blocks,
            ..Pass::default()
        }
    }

    #[test]
    fn the_first_block_in_view_keeps_its_place() {
        let before = memory(vec![
            block_at("above", 0.0, 40.0),
            block_at("first", 90.0, 300.0),
            block_at("second", 310.0, 600.0),
        ]);
        // A row of 120 appeared at the top.
        let now = pass(vec![
            block_at("new", 0.0, 120.0),
            block_at("above", 120.0, 160.0),
            block_at("first", 210.0, 420.0),
            block_at("second", 430.0, 720.0),
        ]);
        assert_eq!(anchor_shift(&before, &now, None, true), 120.0);
        // At the very top of the page it grows downwards instead.
        assert_eq!(anchor_shift(&before, &now, None, false), 0.0);
    }

    #[test]
    fn the_block_acted_on_wins_and_its_neighbours_stand_in_when_it_goes() {
        let before = memory(vec![
            block_at("a", 60.0, 160.0),
            block_at("b", 170.0, 270.0),
        ]);
        let a = Some(Id::new("a"));
        let moved = pass(vec![
            block_at("a", 100.0, 200.0),
            block_at("b", 300.0, 400.0),
        ]);
        assert_eq!(anchor_shift(&before, &moved, a, true), 40.0);
        // Removed: the block below it stays instead.
        let gone = pass(vec![block_at("b", 80.0, 180.0)]);
        assert_eq!(anchor_shift(&before, &gone, a, true), -90.0);
        // Nothing left to anchor to.
        assert_eq!(anchor_shift(&before, &pass(vec![]), a, true), 0.0);
    }

    #[test]
    fn blocks_out_of_sight_are_no_anchor() {
        let before = memory(vec![
            block_at("far above", 0.0, 40.0),
            block_at("seen", 60.0, 200.0),
        ]);
        // The one out of view moved by a lot; the one in view did not.
        let now = pass(vec![
            block_at("far above", 500.0, 540.0),
            block_at("seen", 60.0, 200.0),
        ]);
        assert_eq!(anchor_shift(&before, &now, None, true), 0.0);
    }
}
