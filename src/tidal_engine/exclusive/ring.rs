//! The queue between the engine and an exclusive device's render thread.
//!
//! The engine appends bytes in the device's format and waits while the
//! queue is full and the device plays; the render thread takes them a
//! period at a time and plays silence where the queue runs dry.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// What the engine and the render thread share.
#[derive(Default)]
pub(super) struct Shared {
    ring: Mutex<Ring>,
    pub changed: Condvar,
}

#[derive(Default)]
pub(super) struct Ring {
    pub data: VecDeque<u8>,
    pub playing: bool,
    pub closing: bool,
    /// A gain for the device's own volume control, waiting to be set.
    pub volume: Option<f64>,
    pub failed: Option<String>,
    /// Bytes queued before the device starts asking for sound, so it does
    /// not begin on a queue that holds a packet and then run dry.
    pub start_at: usize,
    /// A drain is waiting: start even on less than `start_at`.
    pub draining: bool,
}

/// How much a device with a queue of `capacity` bytes waits for before it
/// starts: half the queue, in whole frames.
pub(super) fn start_at(capacity: usize, block: usize) -> usize {
    capacity / 2 / block.max(1) * block.max(1)
}

impl Ring {
    /// Whether a device that has not started yet should start now.
    pub fn ready_to_start(&self) -> bool {
        self.playing && (self.draining || self.data.len() >= self.start_at)
    }
}

impl Shared {
    /// A queue that starts its device once `start_at` bytes are queued.
    #[cfg(test)]
    pub fn starting_at(start_at: usize) -> Self {
        let shared = Self::default();
        shared.lock().start_at = start_at;
        shared
    }

    pub fn lock(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn change(&self, change: impl FnOnce(&mut Ring)) {
        change(&mut self.lock());
        self.changed.notify_all();
    }

    /// Queues `bytes`, then waits while more than `capacity` is queued and
    /// the device plays. An error means the device was lost.
    pub fn write(&self, bytes: &[u8], capacity: usize) -> Result<(), String> {
        let mut ring = self.lock();
        if let Some(failed) = &ring.failed {
            return Err(failed.clone());
        }
        ring.data.extend(bytes);
        // The render thread may be waiting for enough to start on.
        self.changed.notify_all();
        while ring.data.len() > capacity && ring.playing && ring.failed.is_none() {
            ring = self
                .changed
                .wait_timeout(ring, Duration::from_millis(50))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        match &ring.failed {
            Some(failed) => Err(failed.clone()),
            None => Ok(()),
        }
    }

    /// Waits until the queue has gone to the device, at most `timeout`.
    /// True when it did and the device is still playing, so the caller
    /// should wait for the device's own buffer too.
    pub fn drain(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut ring = self.lock();
        ring.draining = true;
        self.changed.notify_all();
        while !ring.data.is_empty() && ring.playing && ring.failed.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // Given up: a later start waits for a proper queue again.
                ring.draining = false;
                return false;
            }
            ring = self
                .changed
                .wait_timeout(ring, left.min(Duration::from_millis(50)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        ring.draining = false;
        ring.playing && ring.failed.is_none()
    }

    /// Fills `out` from the front of the queue, whole frames of `block`
    /// bytes, and silence after. Returns how many bytes were music.
    pub fn take_into(&self, out: &mut [u8], block: usize) -> usize {
        let take = {
            let mut ring = self.lock();
            let take = ring.data.len().min(out.len()) / block * block;
            for (to, from) in out.iter_mut().zip(ring.data.drain(..take)) {
                *to = from;
            }
            take
        };
        out[take..].fill(0);
        self.changed.notify_all();
        take
    }

    /// Records that the device is gone and drops what was queued for it.
    pub fn fail(&self, message: String) {
        self.change(|ring| {
            ring.failed = Some(message);
            ring.data.clear();
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn take_fills_whole_frames_then_silence() {
        let shared = Shared::default();
        shared.write(&[1, 2, 3, 4, 5, 6, 7], 100).unwrap();
        let mut out = [9u8; 8];
        assert_eq!(shared.take_into(&mut out, 2), 6);
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 0, 0]);
        assert_eq!(shared.lock().data.len(), 1);
    }

    #[test]
    fn a_failed_device_refuses_writes() {
        let shared = Shared::default();
        shared.fail("gone".into());
        assert_eq!(shared.write(&[0; 4], 100), Err("gone".into()));
        assert!(!shared.drain(Duration::from_millis(10)));
    }

    /// A device waits for a proper queue before it starts, so it does not
    /// begin on one packet and play silence between the next ones; a
    /// drain starts it on whatever is left.
    #[test]
    fn a_device_starts_on_a_full_enough_queue_or_a_drain() {
        let shared = Arc::new(Shared::starting_at(8));
        shared.change(|ring| ring.playing = true);
        shared.write(&[1; 4], 100).unwrap();
        assert!(!shared.lock().ready_to_start());
        shared.write(&[1; 4], 100).unwrap();
        assert!(shared.lock().ready_to_start());
        shared.change(|ring| ring.data.truncate(2));
        assert!(!shared.lock().ready_to_start());
        let render = Arc::clone(&shared);
        let device = std::thread::spawn(move || {
            let mut ring = render.lock();
            while !ring.ready_to_start() {
                ring = render.changed.wait(ring).unwrap();
            }
            drop(ring);
            let mut out = [0u8; 4];
            render.take_into(&mut out, 2);
        });
        assert!(shared.drain(Duration::from_secs(5)));
        device.join().unwrap();
        assert!(!shared.lock().draining);
    }

    /// A drain that runs out of time (the device stopped taking sound)
    /// does not leave the queue marked as draining, which would start the
    /// device later on whatever little was queued and play silence after.
    #[test]
    fn a_drain_that_gives_up_leaves_the_start_threshold_in_place() {
        let shared = Shared::starting_at(8);
        shared.change(|ring| ring.playing = true);
        shared.write(&[1; 4], 100).unwrap();
        assert!(!shared.drain(Duration::from_millis(20)));
        assert!(!shared.lock().draining);
        assert!(!shared.lock().ready_to_start());
    }
}
