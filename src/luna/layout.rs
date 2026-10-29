//! Forced layout: a plugin that reads a box after a change that can move
//! boxes reads the box laid out with that change, as a browser lays the
//! page out on such a read.
//!
//! Here the UI lays the page out, in its frames: the frame that starts with
//! the change ([`TreeSlot::drawing`]) draws it, and the tree of the pass
//! after says it was laid out with it ([`HostTree::laid_out`]). So the read
//! waits on that frame, however long the frame takes, not on a fixed time.
//! Two bounds keep the runtime from stalling when no such frame comes: the
//! UI gets [`TreeSlot::pickup`] from when the change went out to start
//! drawing it (a window that does not draw, or a frame stuck long before
//! it), and once drawing it, [`CARRY`] for the tree.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::dom::HostTree;

/// The newest host tree the UI sent, not reconciled yet (trees coalesce),
/// what the UI said about its frames, and the signal that either changed.
pub(crate) type TreeCell = Arc<(Mutex<TreeSlot>, Condvar)>;

/// The longest the UI gets to start drawing a change before a read waiting
/// for it gives up, however late its frames have been.
pub const PICKUP_CAP: Duration = Duration::from_millis(250);

/// The longest a read waits for the tree once the UI is drawing the change.
/// That tree comes with the pass after, which the UI asks for at once; only
/// a window hidden between the two passes (it then rests until its next
/// background pass) or a UI stuck inside one reaches it.
pub const CARRY: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct TreeSlot {
    /// The newest host tree the UI sent, not reconciled yet (the UI keeps
    /// the same tree to compare the next frame's with: shared, not copied).
    pub tree: Option<Arc<HostTree>>,
    /// How many trees the UI has sent; the one in [`TreeSlot::tree`] is
    /// the `sent`th.
    pub sent: u64,
    /// How many input events the UI has sent the plugins' listeners.
    pub events: u64,
    /// How many of those it had sent when it sent [`TreeSlot::tree`]: the
    /// tree reaches the document once they have all been dispatched.
    pub events_before_tree: u64,
    /// The newest overlay that can move boxes, and when it went out.
    published: Option<(u64, Instant)>,
    /// The newest such overlay the UI started a frame with, and when.
    drawing: Option<(u64, Instant)>,
    /// How late the UI started drawing such overlays: the latest wait, or a
    /// longer one before it, fading by a quarter each time.
    late: Duration,
    /// The time, when a test sets it.
    #[cfg(test)]
    clock: Option<Instant>,
    /// While a read waits, the time it last went by.
    #[cfg(test)]
    pub waiting: Option<Instant>,
}

impl TreeSlot {
    pub fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(t) = self.clock {
            return t;
        }
        Instant::now()
    }

    /// The runtime published overlay `version`, which can move boxes.
    pub fn published(&mut self, version: u64) {
        self.published = Some((version, self.now()));
    }

    /// The UI started a frame with overlay `version`, which can move boxes
    /// from the one it drew before: the tree of its next pass says it was
    /// laid out with it.
    pub fn drawing(&mut self, version: u64) {
        let now = self.now();
        if let Some((v, at)) = self.published
            && version >= v
            && self.drawing.is_none_or(|(d, _)| d < v)
        {
            self.late = now.saturating_duration_since(at).max(self.late * 3 / 4);
        }
        if self.drawing.is_none_or(|(d, _)| d < version) {
            self.drawing = Some((version, now));
        }
    }

    /// How long the UI gets to start drawing a change: `floor`, or twice as
    /// long as it has lately taken, up to [`PICKUP_CAP`].
    pub fn pickup(&self, floor: Duration) -> Duration {
        (self.late * 2).max(floor).min(PICKUP_CAP.max(floor))
    }

    /// When a read waiting since `start` for the boxes of overlay `want`
    /// gives up.
    pub fn deadline(&self, want: u64, floor: Duration, start: Instant) -> Instant {
        match self.drawing {
            Some((v, at)) if v >= want => at + CARRY,
            _ => {
                let out = self
                    .published
                    .filter(|(v, _)| *v >= want)
                    .map_or(start, |(_, at)| at);
                out + self.pickup(floor)
            }
        }
    }

    #[cfg(test)]
    pub fn set_clock(&mut self, at: Instant) {
        self.clock = Some(at);
    }
}

/// Wait for the tree laid out with overlay `want` (see the module), at
/// least `floor` for the UI to start drawing it; returns the newest tree
/// sent, laid out with it or not.
pub(crate) fn wait(cell: &TreeCell, want: u64, floor: Duration) -> Option<Arc<HostTree>> {
    let (slot, arrived) = &**cell;
    let mut slot = slot.lock().ok()?;
    let start = slot.now();
    loop {
        if slot.tree.as_ref().is_some_and(|t| t.laid_out >= want) {
            break;
        }
        let now = slot.now();
        let left = slot
            .deadline(want, floor, start)
            .saturating_duration_since(now);
        if left.is_zero() {
            break;
        }
        #[cfg(test)]
        {
            slot.waiting = Some(now);
        }
        slot = arrived.wait_timeout(slot, left).ok()?.0;
    }
    #[cfg(test)]
    {
        slot.waiting = None;
    }
    slot.tree.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: Duration = Duration::from_millis(64);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn slot(t0: Instant) -> TreeSlot {
        let mut s = TreeSlot::default();
        s.set_clock(t0);
        s
    }

    /// Before the UI draws a change, the read gives up `floor` after it went
    /// out; once the UI draws it, [`CARRY`] after that frame started,
    /// however late that was.
    #[test]
    fn the_wait_follows_the_frame_that_draws_the_change() {
        let t0 = Instant::now();
        let mut s = slot(t0);
        s.published(3);
        assert_eq!(s.deadline(3, FLOOR, t0 + ms(20)), t0 + FLOOR);
        s.set_clock(t0 + ms(200));
        s.drawing(3);
        assert_eq!(s.deadline(3, FLOOR, t0 + ms(20)), t0 + ms(200) + CARRY);
        // An older overlay drawn does not carry this one.
        s.published(5);
        assert_eq!(s.deadline(5, FLOOR, t0), t0 + ms(200) + s.pickup(FLOOR));
    }

    /// A UI whose frames start late gets longer to start drawing the next
    /// change, up to [`PICKUP_CAP`]; the extra time fades once it is quick
    /// again.
    #[test]
    fn the_time_to_start_drawing_follows_late_frames() {
        let t0 = Instant::now();
        let mut s = slot(t0);
        assert_eq!(s.pickup(FLOOR), FLOOR);
        s.published(1);
        s.set_clock(t0 + ms(100));
        s.drawing(1);
        assert_eq!(s.pickup(FLOOR), ms(200));
        // Measured once per change: a later frame with it is no new wait.
        s.set_clock(t0 + ms(400));
        s.drawing(1);
        assert_eq!(s.pickup(FLOOR), ms(200));
        s.published(2);
        s.set_clock(t0 + ms(1400));
        s.drawing(2);
        assert_eq!(s.pickup(FLOOR), PICKUP_CAP);
        let mut at = t0 + ms(1400);
        for v in 3..20 {
            s.published(v);
            at += ms(5);
            s.set_clock(at);
            s.drawing(v);
        }
        assert_eq!(s.pickup(FLOOR), FLOOR);
    }

    /// The wait ends with the tree laid out with the change, and gives up
    /// at its deadline without one; it never blocks past it.
    #[test]
    fn the_wait_ends_with_the_tree_or_at_its_deadline() {
        let t0 = Instant::now();
        let cell: TreeCell = Arc::default();
        {
            let mut s = cell.0.lock().unwrap();
            s.set_clock(t0);
            s.published(2);
            s.tree = Some(Arc::new(HostTree {
                laid_out: 1,
                ..Default::default()
            }));
            // The deadline is long past.
            s.set_clock(t0 + CARRY + PICKUP_CAP);
        }
        let tree = wait(&cell, 2, FLOOR).expect("the newest tree");
        assert_eq!(tree.laid_out, 1);
        cell.0.lock().unwrap().tree = Some(Arc::new(HostTree {
            laid_out: 2,
            ..Default::default()
        }));
        assert_eq!(wait(&cell, 2, FLOOR).map(|t| t.laid_out), Some(2));
        assert!(wait(&cell, 2, FLOOR).is_none(), "taken");
    }
}
