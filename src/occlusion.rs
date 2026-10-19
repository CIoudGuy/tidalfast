//! Whether the main window is fully covered by other windows.
//!
//! eframe draws only the logic of a window that winit reports occluded,
//! like a minimised one, but winit has no occlusion event on Windows: a
//! window under others kept drawing the now-playing background thirty
//! times a second for nobody. On Windows `Watch` works it out from the
//! windows above the main one in z-order, and the pass's logic records the
//! answer here (`set_covered`) before anything is drawn. A covered window
//! gets the drawing budget of a window nobody can see
//! (`power::Budget::Background`, see `App::sync_power`), so animations and
//! timed frames ask for nothing; playback, the media controls, the tray,
//! plugins and the network run as before, and the passes they need still
//! run and draw.
//!
//! Covered means every pixel of the client area is under a visible, opaque
//! window. Anything unsure counts as showing: a partly covered window, one
//! under a transparent, layered or click-through window, a tool window or
//! a tooltip, a window with a shaped region, and a window under another
//! one's rounded corners all keep drawing. Other platforms never report
//! covered here (Wayland, X11 and macOS tell eframe through winit).

/// Whether the last pass found the main window fully covered. Always false
/// outside Windows.
pub fn covered(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp(covered_id()))
        .unwrap_or(false)
}

/// Records what the main window's [`Watch`] found this pass.
#[cfg(any(windows, test))]
pub fn set_covered(ctx: &egui::Context, covered: bool) {
    ctx.data_mut(|data| data.insert_temp(covered_id(), covered));
}

fn covered_id() -> egui::Id {
    egui::Id::new("tidalfast-window-covered")
}

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::Watch;

/// A rectangle in screen pixels, right and bottom exclusive.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg(any(windows, test))]
impl Area {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }
}

/// The most pieces the uncovered part may break into before the check
/// gives up and answers "showing" (a mosaic of small windows).
#[cfg(any(windows, test))]
const MAX_PIECES: usize = 64;

/// The part of the window no window above it covers yet, as disjoint
/// rectangles.
#[cfg(any(windows, test))]
#[derive(Clone, Debug)]
pub(crate) struct Uncovered {
    pieces: Vec<Area>,
    /// Too many pieces to follow: counts as showing.
    gave_up: bool,
}

#[cfg(any(windows, test))]
impl Uncovered {
    pub fn new(area: Area) -> Self {
        Self {
            pieces: if area.is_empty() {
                Vec::new()
            } else {
                vec![area]
            },
            gave_up: false,
        }
    }

    /// Whether nothing of the window is left showing.
    pub fn is_empty(&self) -> bool {
        !self.gave_up && self.pieces.is_empty()
    }

    /// Whether `area` overlaps anything still showing.
    #[cfg(windows)]
    pub fn touches(&self, area: &Area) -> bool {
        self.gave_up || self.pieces.iter().any(|piece| piece.intersects(area))
    }

    /// Takes `cover` away from what is showing.
    pub fn subtract(&mut self, cover: &Area) {
        if self.gave_up || cover.is_empty() {
            return;
        }
        let mut left = Vec::with_capacity(self.pieces.len() + 4);
        for piece in &self.pieces {
            if !piece.intersects(cover) {
                left.push(*piece);
                continue;
            }
            // Above, below, then left and right of the cover, within the
            // cover's band.
            let top = cover.top.max(piece.top);
            let bottom = cover.bottom.min(piece.bottom);
            for part in [
                Area::new(piece.left, piece.top, piece.right, top),
                Area::new(piece.left, bottom, piece.right, piece.bottom),
                Area::new(piece.left, top, cover.left.max(piece.left), bottom),
                Area::new(cover.right.min(piece.right), top, piece.right, bottom),
            ] {
                if !part.is_empty() {
                    left.push(part);
                }
            }
        }
        if left.len() > MAX_PIECES {
            self.gave_up = true;
            self.pieces.clear();
        } else {
            self.pieces = left;
        }
    }

    /// Takes away a window's `frame` whose corners are rounded by up to
    /// `corner` pixels: the frame less a `corner` square at each corner.
    pub fn subtract_window(&mut self, frame: &Area, corner: i32) {
        let corner = corner.max(0);
        self.subtract(&Area::new(
            frame.left,
            frame.top + corner,
            frame.right,
            frame.bottom - corner,
        ));
        self.subtract(&Area::new(
            frame.left + corner,
            frame.top,
            frame.right - corner,
            frame.bottom,
        ));
    }
}

/// What the check knows of a window above the main one.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Above {
    pub visible: bool,
    pub minimised: bool,
    /// DWM hides it (another virtual desktop, a suspended app).
    pub cloaked: bool,
    /// `WS_EX_TRANSPARENT`: input and drawing fall through it.
    pub click_through: bool,
    /// `WS_EX_TOOLWINDOW`: tooltips, menus, tray flyouts and overlays.
    pub tool: bool,
    /// `WS_EX_LAYERED`, and whether it is known to be fully opaque (a
    /// constant alpha of 255 and no colour key).
    pub layered: bool,
    pub layered_opaque: bool,
    /// `WS_EX_NOREDIRECTIONBITMAP` and `WS_EX_TOPMOST`: a composition
    /// surface above everything, which overlays use to draw over the
    /// desktop with transparency.
    pub composited_topmost: bool,
    /// It has a window region (a shaped window).
    pub shaped: bool,
    pub maximised: bool,
    /// Its visible frame (DWM's extended frame bounds), if known.
    pub frame: Option<Area>,
    /// How far its corners may be rounded, in pixels.
    pub corner: i32,
}

#[cfg(any(windows, test))]
impl Above {
    /// The frame it hides everything under, if it surely hides it, and the
    /// corner to leave out.
    pub fn cover(&self) -> Option<(Area, i32)> {
        if !self.visible
            || self.minimised
            || self.cloaked
            || self.click_through
            || self.tool
            || (self.layered && !self.layered_opaque)
            || self.composited_topmost
            || self.shaped
        {
            return None;
        }
        let frame = self.frame?;
        // A maximised window has square corners.
        let corner = if self.maximised { 0 } else { self.corner };
        Some((frame, corner))
    }
}

/// Whether the windows in `above` (any order) cover all of `client`.
#[cfg(test)]
pub(crate) fn fully_covered(client: Area, above: &[Above]) -> bool {
    let mut uncovered = Uncovered::new(client);
    if uncovered.is_empty() {
        return false;
    }
    for window in above {
        if let Some((frame, corner)) = window.cover() {
            uncovered.subtract_window(&frame, corner);
            if uncovered.is_empty() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: Area = Area {
        left: 100,
        top: 100,
        right: 900,
        bottom: 700,
    };

    fn window(left: i32, top: i32, right: i32, bottom: i32) -> Above {
        Above {
            visible: true,
            frame: Some(Area::new(left, top, right, bottom)),
            corner: 8,
            ..Above::default()
        }
    }

    fn maximised(left: i32, top: i32, right: i32, bottom: i32) -> Above {
        Above {
            maximised: true,
            ..window(left, top, right, bottom)
        }
    }

    #[test]
    fn a_maximised_window_over_it_covers_it() {
        assert!(fully_covered(CLIENT, &[maximised(0, 0, 1920, 1040)]));
        assert!(!fully_covered(CLIENT, &[]), "nothing above");
    }

    #[test]
    fn a_partly_covered_window_is_showing() {
        // A strip of one pixel on the right stays out.
        assert!(!fully_covered(CLIENT, &[maximised(0, 0, 899, 1040)]));
        // Two windows side by side with a one-pixel gap between them.
        assert!(!fully_covered(
            CLIENT,
            &[maximised(0, 0, 500, 1040), maximised(501, 0, 1920, 1040)]
        ));
    }

    #[test]
    fn windows_that_together_cover_it_cover_it() {
        assert!(fully_covered(
            CLIENT,
            &[
                maximised(0, 0, 500, 1040),
                maximised(500, 0, 1920, 400),
                maximised(500, 400, 1920, 1040),
            ]
        ));
    }

    /// The rounded corners of a window the same size as the client area
    /// leave its corner pixels showing; a bigger one hides them.
    #[test]
    fn rounded_corners_leave_the_corners_showing() {
        assert!(!fully_covered(CLIENT, &[window(100, 100, 900, 700)]));
        assert!(fully_covered(CLIENT, &[window(92, 92, 908, 708)]));
        assert!(!fully_covered(CLIENT, &[window(93, 93, 908, 708)]));
    }

    #[test]
    fn see_through_and_hidden_windows_cover_nothing() {
        let big = maximised(0, 0, 1920, 1040);
        let cases = [
            Above {
                visible: false,
                ..big
            },
            Above {
                minimised: true,
                ..big
            },
            Above {
                cloaked: true,
                ..big
            },
            Above {
                click_through: true,
                ..big
            },
            Above { tool: true, ..big },
            Above {
                layered: true,
                ..big
            },
            Above {
                composited_topmost: true,
                ..big
            },
            Above {
                shaped: true,
                ..big
            },
            Above { frame: None, ..big },
        ];
        for case in cases {
            assert!(!fully_covered(CLIENT, &[case]), "{case:?}");
        }
        let opaque_layered = Above {
            layered: true,
            layered_opaque: true,
            ..big
        };
        assert!(fully_covered(CLIENT, &[opaque_layered]));
    }

    #[test]
    fn an_empty_client_area_is_not_covered() {
        let empty = Area::new(100, 100, 100, 700);
        assert!(!fully_covered(empty, &[maximised(0, 0, 1920, 1040)]));
    }

    /// A mosaic of tiny windows breaks the uncovered part into more pieces
    /// than the check follows; it then answers "showing".
    #[test]
    fn too_many_pieces_count_as_showing() {
        let mut above = Vec::new();
        for i in 0..40 {
            let x = 110 + i * 19;
            above.push(maximised(x, 300, x + 2, 302));
            above.push(maximised(x, 500, x + 2, 502));
        }
        above.push(maximised(0, 0, 1920, 1040));
        assert!(!fully_covered(CLIENT, &above));
    }

    #[test]
    fn the_answer_is_shared_through_the_context() {
        let ctx = egui::Context::default();
        assert!(!covered(&ctx));
        set_covered(&ctx, true);
        assert!(covered(&ctx));
        set_covered(&ctx, false);
        assert!(!covered(&ctx));
    }
}
