//! Writes state files on a thread of their own, so the synced write and
//! move that make a save atomic never hold up the interface.
//!
//! Writes reach the disk in the order they were asked for; a file asked
//! for twice before the first write starts is written once, with the newer
//! text. [`Writer::flush`] waits until everything asked for so far is on
//! disk: the app calls it before it quits, sleeps or ends the session, so
//! nothing saved is lost.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

/// Who may read the file written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// As the file it replaces allowed.
    Default,
    /// Its owner only (on Unix).
    Private,
}

struct Job {
    path: PathBuf,
    text: String,
    access: Access,
    /// Set again when the write fails, so the owner writes it next time.
    failed: Option<Arc<AtomicBool>>,
    what: &'static str,
}

enum Message {
    Write(Job),
    Flush(SyncSender<()>),
    /// Tests: the thread waits until this sender is dropped.
    #[cfg(test)]
    Hold(Receiver<()>),
}

/// A handle to the writer thread; clones share it.
#[derive(Clone)]
pub struct Writer {
    sender: Sender<Message>,
}

impl Writer {
    /// Starts the writer thread. It ends once every handle is dropped.
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let started = std::thread::Builder::new()
            .name("tidalfast-files".into())
            .spawn(move || run(&receiver));
        if let Err(error) = started {
            log::warn!("could not start the file writer: {error}");
        }
        Self { sender }
    }

    /// Writes `text` to `path`, replacing the file whole, behind the
    /// caller. `what` names the file in a warning; a failed write sets
    /// `failed`.
    pub fn write(
        &self,
        what: &'static str,
        path: PathBuf,
        text: String,
        access: Access,
        failed: Option<Arc<AtomicBool>>,
    ) {
        let job = Job {
            path,
            text,
            access,
            failed,
            what,
        };
        // No thread to take it: write it here rather than lose it.
        if let Err(std::sync::mpsc::SendError(Message::Write(job))) =
            self.sender.send(Message::Write(job))
        {
            log::warn!("the file writer is gone; writing {what} here");
            write_now(job);
        }
    }

    /// Waits until every write asked for before this call is on disk.
    pub fn flush(&self) {
        let (done, wait) = sync_channel(1);
        if self.sender.send(Message::Flush(done)).is_ok() {
            let _ = wait.recv();
        }
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

fn run(receiver: &Receiver<Message>) {
    while let Ok(first) = receiver.recv() {
        // Everything already waiting goes in one batch: a file asked for
        // more than once is written with its newest text only.
        let mut batch = vec![first];
        batch.extend(receiver.try_iter());
        let mut jobs: Vec<Job> = Vec::new();
        let mut flushes = Vec::new();
        for message in batch {
            match message {
                Message::Write(job) => {
                    jobs.retain(|older| older.path != job.path);
                    jobs.push(job);
                }
                Message::Flush(done) => flushes.push(done),
                #[cfg(test)]
                Message::Hold(release) => {
                    let _ = release.recv();
                }
            }
        }
        for job in jobs {
            write_now(job);
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

fn write_now(job: Job) {
    let written = match job.access {
        Access::Default => fastframe_file::write(&job.path, job.text),
        Access::Private => fastframe_file::write_private(&job.path, job.text),
    };
    if let Err(error) = written {
        log::warn!(
            "could not write {} to {}: {error}",
            job.what,
            job.path.display()
        );
        if let Some(failed) = job.failed {
            failed.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = crate::paths::unique_test_dir(&format!("writer-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    impl Writer {
        /// Holds the thread until the returned sender is dropped.
        fn hold(&self) -> SyncSender<()> {
            let (release, wait) = sync_channel(0);
            self.sender.send(Message::Hold(wait)).unwrap();
            release
        }
    }

    /// Saving the play history hands the write to the writer and returns:
    /// the interface thread never waits for the disk.
    #[test]
    fn a_history_save_does_not_wait_for_the_disk() {
        let dir = dir("history");
        let path = dir.join("history.json");
        let writer = Writer::new();
        let mut history = crate::history::History::default();
        history.record(
            crate::api::models::Track {
                uri: "tidal:track:a".into(),
                ..Default::default()
            },
            "2026-09-01T09:00:00Z".parse().unwrap(),
        );
        // The writer is busy: a save that wrote here would wait for it.
        let release = writer.hold();
        history.save(&path, &writer);
        assert!(!path.exists(), "written on the caller's thread");
        drop(release);
        writer.flush();
        assert_eq!(crate::history::History::load(&path).plays().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Writes land in order, a file asked for twice is written with the
    /// newer text, and a flush waits for all of them.
    #[test]
    fn writes_land_in_order_and_flush_waits_for_them() {
        let dir = dir("order");
        let a = dir.join("a.json");
        let b = dir.join("b.json");
        let writer = Writer::new();
        let release = writer.hold();
        writer.write("a", a.clone(), "1".into(), Access::Default, None);
        writer.write("b", b.clone(), "x".into(), Access::Default, None);
        writer.write("a", a.clone(), "2".into(), Access::Default, None);
        drop(release);
        writer.flush();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "2");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x");
        writer.write("a", a.clone(), "3".into(), Access::Default, None);
        writer.flush();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "3");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A write that fails says so to its owner.
    #[test]
    fn a_failed_write_is_reported() {
        let dir = dir("failed");
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "").unwrap();
        let failed = Arc::new(AtomicBool::new(false));
        let writer = Writer::new();
        writer.write(
            "x",
            blocked.join("x.json"),
            "1".into(),
            Access::Default,
            Some(Arc::clone(&failed)),
        );
        writer.flush();
        assert!(failed.load(Ordering::Relaxed));
        let _ = std::fs::remove_dir_all(dir);
    }
}
