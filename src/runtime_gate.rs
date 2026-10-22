//! Lends the backend's tokio runtime to threads outside it, and takes it
//! back in order when the app closes.
//!
//! The engine's catalogue workers ("tidal-catalog"), the Luna plugins'
//! lookups ("luna-lookup") and the cover workers block on the backend's
//! runtime from their own threads. A timer polled on a runtime that is
//! shutting down panics ("A Tokio 1.x context was found, but it is being
//! shutdown"), and release builds abort on a panic: quitting while such a
//! request was in flight crashed the app. Every such call goes through a
//! [`RuntimeGate`] instead, and the runtime is shut down only through
//! [`shut_down`], which first refuses new calls, ends those in flight and
//! waits for their threads to let go.

use std::future::Future;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::runtime::{Handle, Runtime};
use tokio::sync::watch;

/// The runtime is closing: the call was refused, or ended unanswered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closing;

impl std::fmt::Display for Closing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the app is closing")
    }
}

impl std::error::Error for Closing {}

#[derive(Default)]
struct Users {
    /// Threads inside [`RuntimeGate::block_on`] now.
    count: usize,
    closing: bool,
}

struct Shared {
    users: Mutex<Users>,
    idle: Condvar,
    closed: watch::Sender<bool>,
}

/// A handle to a runtime for threads outside it, which refuses work once
/// the runtime closes. Clones share the gate.
#[derive(Clone)]
pub struct RuntimeGate {
    handle: Handle,
    shared: Arc<Shared>,
}

impl From<Handle> for RuntimeGate {
    fn from(handle: Handle) -> Self {
        Self::new(handle)
    }
}

impl RuntimeGate {
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            shared: Arc::new(Shared {
                users: Mutex::new(Users::default()),
                idle: Condvar::new(),
                closed: watch::Sender::new(false),
            }),
        }
    }

    /// The runtime, to spawn on. Spawning on a closed runtime is safe (the
    /// task never runs); blocking on it is not, so block through
    /// [`Self::block_on`].
    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    /// Runs `future` to completion on the runtime, blocking this thread,
    /// which must be outside the runtime. Once the runtime closes this
    /// returns [`Closing`] at once, and a call in flight then ends with it
    /// (its future dropped) rather than outliving the runtime.
    pub fn block_on<F: Future>(&self, future: F) -> Result<F::Output, Closing> {
        let _user = self.enter()?;
        let mut closed = self.shared.closed.subscribe();
        self.handle.block_on(async move {
            tokio::select! {
                biased;
                _ = closed.wait_for(|closing| *closing) => Err(Closing),
                output = future => Ok(output),
            }
        })
    }

    /// Whether the runtime is closing or closed.
    pub fn is_closing(&self) -> bool {
        self.lock().closing
    }

    fn enter(&self) -> Result<User<'_>, Closing> {
        let mut users = self.lock();
        if users.closing {
            return Err(Closing);
        }
        users.count += 1;
        Ok(User(&self.shared))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Users> {
        self.shared
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Refuses new calls, ends those in flight, and waits up to `patience`
    /// for their threads to leave. True when none is left inside.
    pub fn close(&self, patience: Duration) -> bool {
        let mut users = self.lock();
        users.closing = true;
        self.shared.closed.send_replace(true);
        let deadline = Instant::now() + patience;
        while users.count > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            users = self
                .shared
                .idle
                .wait_timeout(users, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        true
    }
}

/// One thread inside [`RuntimeGate::block_on`].
struct User<'a>(&'a Shared);

impl Drop for User<'_> {
    fn drop(&mut self) {
        let mut users = self
            .0
            .users
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        users.count -= 1;
        if users.count == 0 {
            self.0.idle.notify_all();
        }
    }
}

/// Shuts `runtime` down in order: `gate` first refuses new calls from
/// outside threads and ends those in flight, and only once they have let
/// go does the runtime stop, giving its own tasks up to `patience`. A
/// thread that will not let go keeps the runtime alive (it is left to
/// the process's exit) rather than have it shut down under the thread.
pub fn shut_down(runtime: Runtime, gate: &RuntimeGate, patience: Duration) {
    if gate.close(patience) {
        runtime.shutdown_timeout(patience);
    } else {
        log::warn!("a request outlived closing; its runtime is left to the exit");
        std::mem::forget(runtime);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    /// A thread outside the runtime, blocked on a timer, while the runtime
    /// shuts down: before, the timer panicked (and the release build
    /// aborted, a crash at quit). Now the call ends with `Closing`.
    #[test]
    fn closing_ends_a_timer_in_flight_without_a_panic() {
        let runtime = runtime();
        let gate = RuntimeGate::new(runtime.handle().clone());
        let (started, wait) = mpsc::channel();
        let lookup = std::thread::Builder::new()
            .name("luna-lookup".into())
            .spawn({
                let gate = gate.clone();
                move || {
                    gate.block_on(async move {
                        let _ = started.send(());
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    })
                }
            })
            .unwrap();
        wait.recv().unwrap();
        // Give the thread time to park in the timer.
        std::thread::sleep(Duration::from_millis(50));
        let begun = Instant::now();
        shut_down(runtime, &gate, Duration::from_secs(2));
        let answer = lookup.join().expect("the waiting thread panicked");
        assert_eq!(answer, Err(Closing));
        assert!(
            begun.elapsed() < Duration::from_secs(2),
            "{:?}",
            begun.elapsed()
        );
    }

    /// After closing, a call never touches the runtime: it is refused at
    /// once, even with the runtime gone.
    #[test]
    fn a_call_after_closing_is_refused() {
        let runtime = runtime();
        let gate = RuntimeGate::new(runtime.handle().clone());
        assert_eq!(gate.block_on(async { 7 }), Ok(7));
        shut_down(runtime, &gate, Duration::from_secs(2));
        assert!(gate.is_closing());
        let answer = std::thread::spawn(move || {
            gate.block_on(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                7
            })
        })
        .join()
        .expect("a call after closing panicked");
        assert_eq!(answer, Err(Closing));
    }

    /// Closing waits for a thread inside to leave, and says when one would
    /// not.
    #[test]
    fn closing_waits_for_threads_inside() {
        let runtime = runtime();
        let gate = RuntimeGate::new(runtime.handle().clone());
        let (started, wait) = mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let stuck = std::thread::spawn({
            let gate = gate.clone();
            move || {
                gate.block_on(async move {
                    let _ = started.send(());
                    // Blocks the thread itself: a future that cannot be
                    // ended from outside.
                    let _ = held.recv();
                })
            }
        });
        wait.recv().unwrap();
        assert!(!gate.close(Duration::from_millis(100)));
        release.send(()).unwrap();
        assert!(gate.close(Duration::from_secs(2)));
        assert_eq!(stuck.join().unwrap(), Ok(()));
        runtime.shutdown_timeout(Duration::from_secs(1));
    }
}
