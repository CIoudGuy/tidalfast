//! A small, fair worker pool for plugins' asynchronous host operations
//! (`fetch`, TIDAL lookups).
//!
//! Each call used to get its own OS thread, so `Promise.all` over a few
//! thousand ids started thousands of threads and as many requests at once.
//! A [`Pool`] runs at most `workers` jobs together and at most `per_owner`
//! of one plugin's; the rest wait in order (queued, never refused). Threads
//! start only when there is work and leave after a while without any, so a
//! quiet runtime holds none.

use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Job = Box<dyn FnOnce() + Send>;

/// How long an idle worker waits for work before its thread ends.
const IDLE: Duration = Duration::from_secs(20);

pub struct Pool {
    shared: Arc<Shared>,
}

struct Shared {
    name: &'static str,
    workers: usize,
    per_owner: usize,
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    queue: VecDeque<(String, Job)>,
    /// Jobs running per owner.
    running: HashMap<String, usize>,
    threads: usize,
    idle: usize,
    closed: bool,
}

impl State {
    /// The first queued job whose owner is under its share.
    fn next(&mut self, per_owner: usize) -> Option<(String, Job)> {
        let at = self
            .queue
            .iter()
            .position(|(o, _)| self.running.get(o).copied().unwrap_or(0) < per_owner)?;
        let job = self.queue.remove(at)?;
        *self.running.entry(job.0.clone()).or_default() += 1;
        Some(job)
    }
}

impl Pool {
    pub fn new(name: &'static str, workers: usize, per_owner: usize) -> Self {
        Pool {
            shared: Arc::new(Shared {
                name,
                workers: workers.max(1),
                per_owner: per_owner.clamp(1, workers.max(1)),
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
            }),
        }
    }

    /// Queue `job` for `owner` (a plugin, or "" for the runtime itself).
    pub fn submit(&self, owner: &str, job: impl FnOnce() + Send + 'static) -> Result<(), String> {
        let sh = &self.shared;
        let mut s = sh.state.lock().unwrap_or_else(|e| e.into_inner());
        s.queue.push_back((owner.to_string(), Box::new(job)));
        if s.idle == 0 && s.threads < sh.workers {
            let shared = self.shared.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("luna-{}", sh.name))
                .spawn(move || work(&shared));
            match spawned {
                Ok(_) => s.threads += 1,
                // No thread for it: a worker already running takes it
                // later; with none at all the call fails here.
                Err(e) if s.threads == 0 => {
                    s.queue.pop_back();
                    return Err(e.to_string());
                }
                Err(_) => {}
            }
        }
        drop(s);
        sh.wake.notify_all();
        Ok(())
    }

    /// Worker threads alive now (for tests).
    #[cfg(test)]
    pub fn threads(&self) -> usize {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .threads
    }
}

impl Drop for Pool {
    /// Workers finish the job they run and leave; queued jobs are dropped
    /// (their callers are gone with the runtime).
    fn drop(&mut self) {
        let mut s = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        s.closed = true;
        s.queue.clear();
        drop(s);
        self.shared.wake.notify_all();
    }
}

fn work(sh: &Shared) {
    let mut s = sh.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if s.closed {
            break;
        }
        if let Some((owner, job)) = s.next(sh.per_owner) {
            drop(s);
            let _ = std::panic::catch_unwind(AssertUnwindSafe(job));
            s = sh.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(n) = s.running.get_mut(&owner) {
                *n -= 1;
                if *n == 0 {
                    s.running.remove(&owner);
                }
            }
            // The owner's next job may be runnable now, for another worker.
            sh.wake.notify_all();
            continue;
        }
        s.idle += 1;
        let (next, timeout) = sh
            .wake
            .wait_timeout(s, IDLE)
            .unwrap_or_else(|e| e.into_inner());
        s = next;
        s.idle -= 1;
        if timeout.timed_out() && s.queue.is_empty() {
            break;
        }
    }
    s.threads -= 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// Many calls at once run on a few threads, each plugin within its
    /// share, and another plugin's calls are not stuck behind them.
    #[test]
    fn jobs_are_bounded_per_pool_and_per_plugin() {
        let pool = Pool::new("test", 4, 2);
        let now = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let greedy_now = Arc::new(AtomicUsize::new(0));
        let greedy_most = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..200 {
            let (now, most, gn, gm, done) = (
                now.clone(),
                most.clone(),
                greedy_now.clone(),
                greedy_most.clone(),
                done.clone(),
            );
            pool.submit("greedy", move || {
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(n, Ordering::SeqCst);
                let g = gn.fetch_add(1, Ordering::SeqCst) + 1;
                gm.fetch_max(g, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
                gn.fetch_sub(1, Ordering::SeqCst);
                now.fetch_sub(1, Ordering::SeqCst);
                done.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        }
        // Queued after all of greedy's, yet done long before them.
        let (tx, rx) = std::sync::mpsc::channel();
        let start = Instant::now();
        pool.submit("other", move || {
            let _ = tx.send(start.elapsed());
        })
        .unwrap();
        let waited = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            done.load(Ordering::SeqCst) < 200,
            "the other plugin waited for all of greedy's calls ({waited:?})"
        );
        let t0 = Instant::now();
        while done.load(Ordering::SeqCst) < 200 && t0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(done.load(Ordering::SeqCst), 200);
        assert!(most.load(Ordering::SeqCst) <= 4, "{most:?}");
        assert!(greedy_most.load(Ordering::SeqCst) <= 2, "{greedy_most:?}");
        assert!(pool.threads() <= 4, "{}", pool.threads());
    }

    /// A panicking job does not take its worker's slot with it.
    #[test]
    fn a_panicking_job_frees_its_slot() {
        let pool = Pool::new("test", 1, 1);
        pool.submit("p", || panic!("boom")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        pool.submit("p", move || {
            let _ = tx.send(());
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
