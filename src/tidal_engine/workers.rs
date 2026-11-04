//! Threads kept for the engine's per-song work.
//!
//! Every song is fetched and decoded on threads of its own. Started anew for
//! each song, those threads cost the process memory it did not get back: on
//! Windows 2,000 thread starts left 35 MB behind, and a long session of
//! skipping through songs starts thousands ("Memory over many song changes"
//! in performance.md). Here a thread that is done waits a while for the
//! next job of its kind instead of ending.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

/// How long a thread with nothing to do waits for another job.
const IDLE: Duration = Duration::from_secs(120);

type Job = Box<dyn FnOnce() + Send>;

/// A thread waiting for a job.
struct Idle {
    id: u64,
    thread: thread::Thread,
    jobs: Sender<Job>,
}

#[derive(Default)]
struct Pool {
    idle: HashMap<&'static str, Vec<Idle>>,
    next_id: u64,
}

static POOL: Mutex<Option<Pool>> = Mutex::new(None);

fn pool<T>(f: impl FnOnce(&mut Pool) -> T) -> T {
    let mut pool = POOL.lock().unwrap_or_else(PoisonError::into_inner);
    f(pool.get_or_insert_with(Pool::default))
}

/// Runs `job` on a thread called `name`: one of that name waiting for work,
/// or a new one. Returns the thread, for waking it.
pub fn run(name: &'static str, job: impl FnOnce() + Send + 'static) -> io::Result<thread::Thread> {
    let mut job: Job = Box::new(job);
    while let Some(idle) = pool(|pool| pool.idle.get_mut(name).and_then(Vec::pop)) {
        match idle.jobs.send(job) {
            Ok(()) => return Ok(idle.thread),
            // It ended just now: the next one, or a new thread.
            Err(mpsc::SendError(back)) => job = back,
        }
    }
    let (sender, receiver) = mpsc::channel::<Job>();
    let id = pool(|pool| {
        pool.next_id += 1;
        pool.next_id
    });
    let handle = thread::Builder::new().name(name.into()).spawn(move || {
        job();
        loop {
            pool(|pool| {
                pool.idle.entry(name).or_default().push(Idle {
                    id,
                    thread: thread::current(),
                    jobs: sender.clone(),
                });
            });
            let job = match receiver.recv_timeout(IDLE) {
                Ok(job) => job,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    // Off the idle list, unless a job is already on its
                    // way here.
                    let taken = pool(|pool| {
                        let list = pool.idle.entry(name).or_default();
                        let before = list.len();
                        list.retain(|idle| idle.id != id);
                        list.len() == before
                    });
                    if !taken {
                        break;
                    }
                    match receiver.recv() {
                        Ok(job) => job,
                        Err(_) => break,
                    }
                }
            };
            job();
        }
    })?;
    Ok(handle.thread().clone())
}

/// Threads of `name` waiting for work now.
#[cfg(test)]
pub(crate) fn waiting(name: &'static str) -> usize {
    pool(|pool| pool.idle.get(name).map_or(0, Vec::len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    #[test]
    fn a_finished_thread_takes_the_next_job() {
        let name = "workers-test-reuse";
        let (done, finished) = channel();
        let first = {
            let done = done.clone();
            run(name, move || done.send(thread::current().id()).unwrap()).unwrap()
        };
        let first_ran = finished.recv().unwrap();
        assert_eq!(first.id(), first_ran);
        // Back on the idle list once its job is done.
        while waiting(name) == 0 {
            thread::sleep(Duration::from_millis(5));
        }
        let second = run(name, move || done.send(thread::current().id()).unwrap()).unwrap();
        assert_eq!(finished.recv().unwrap(), first_ran);
        assert_eq!(second.id(), first_ran);
    }

    #[test]
    fn jobs_at_once_get_threads_of_their_own() {
        let name = "workers-test-parallel";
        let (go, wait) = channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let (done, finished) = channel();
        let first = {
            let (wait, done) = (Arc::clone(&wait), done.clone());
            run(name, move || {
                let _ = wait.lock().unwrap().recv();
                done.send(()).unwrap();
            })
            .unwrap()
        };
        // The first is still busy: the second job gets another thread.
        let second = run(name, move || done.send(()).unwrap()).unwrap();
        assert_ne!(first.id(), second.id());
        finished.recv().unwrap();
        go.send(()).unwrap();
        finished.recv().unwrap();
    }
}
