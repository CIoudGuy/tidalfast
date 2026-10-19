//! The downloads queue: songs kept for offline listening a few at a time,
//! paused, resumed, cancelled and retried, and the albums, playlists,
//! mixes and artists kept as a whole, which follow their changes.
//! Everything stays in the app's own private store.
//!
//! Collections can be huge (hundreds of songs in a playlist, thousands in
//! My Collection), so the queue is gentle: a few songs at once, a pause when
//! TIDAL asks for one, songs TIDAL will not give out are counted and left
//! out instead of failing the collection, the queue is written to disk so a
//! restart carries on, and nothing here walks the whole queue per song.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::*;
use crate::model::{CollectionHead, DownloadState, KeptCollection, QueuedDownload};
use crate::tidal_engine::OfflineEvent;

/// Free space left alone on the disk: downloads pause below it.
const RESERVE: u64 = 256 << 20;
/// How many times a song that failed in a way worth another go starts again
/// before it waits for Try again.
const AUTO_RETRIES: u8 = 2;
/// The pause when TIDAL asks downloads to slow down, doubling up to the
/// longest while it keeps asking.
const COOL_FIRST: u64 = 20;
const COOL_LONGEST: u64 = 300;
/// The queue is written at most this often.
const SAVE_EVERY: Duration = Duration::from_secs(3);
/// The kept list is re-read at most this often while downloads go on and
/// the Downloaded page is open.
const LIST_EVERY: Duration = Duration::from_secs(5);
/// A read of a collection that has not answered by now is given up on.
const READ_GIVE_UP: Duration = Duration::from_secs(120);
/// Kept collections are compared with TIDAL at most this often.
const SYNC_EVERY: Duration = Duration::from_secs(600);

/// What the queue shows of a song.
struct Shown {
    title: String,
    subtitle: String,
    image: Option<String>,
}

/// A song to put in the queue.
struct Wanted {
    uri: String,
    item: Option<PlayableItem>,
    /// The album, playlist or mix it is kept for, and its place there.
    context: Option<(String, u32)>,
}

/// The queue as written to disk.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedQueue {
    paused: bool,
    items: Vec<SavedDownload>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedDownload {
    uri: String,
    title: String,
    subtitle: String,
    image: Option<String>,
    context: Option<(String, u32)>,
    paused: bool,
    failed: Option<String>,
}

/// About how large a song is kept at `quality`, to tell before starting
/// whether a collection fits.
fn song_estimate(quality: crate::api::Quality) -> u64 {
    use crate::api::Quality;
    match quality {
        Quality::Low => 5 << 20,
        Quality::High => 10 << 20,
        Quality::Lossless => 35 << 20,
        Quality::HiResLossless => 90 << 20,
    }
}

/// TIDAL will not give this song out: removed, or not offered here.
fn is_unavailable(error: &str) -> bool {
    let error = error.to_lowercase();
    [
        "isn't available",
        "not offer this",
        "not available for playback",
        "could not find this",
    ]
    .iter()
    .any(|text| error.contains(text))
}

fn is_rate_limited(error: &str) -> bool {
    let error = error.to_lowercase();
    error.contains("rate limiting") || error.contains("quota")
}

/// A failure another go cannot mend.
fn is_final(error: &str) -> bool {
    let error = error.to_lowercase();
    [
        "sign-in expired",
        "not signed in",
        "lossy copy",
        "subscription",
        "monthly streaming limit",
    ]
    .iter()
    .any(|text| error.contains(text))
}

impl App {
    fn shown(&self, item: Option<&PlayableItem>, uri: &str) -> Shown {
        let cached;
        let item = match item {
            Some(item) => Some(item),
            None => {
                cached = util::uri_id(uri)
                    .and_then(|id| self.track_cache.get(id))
                    .cloned()
                    .map(PlayableItem::Track);
                cached.as_ref()
            }
        };
        match item {
            Some(item) => Shown {
                title: item.name().to_string(),
                subtitle: item.subtitle(),
                image: item
                    .image(crate::api::models::SMALL_ART_WIDTH)
                    .map(str::to_string),
            },
            None => Shown {
                title: uri.to_string(),
                subtitle: String::new(),
                image: None,
            },
        }
    }

    /// The quality downloads are kept at.
    fn download_quality(&self) -> crate::api::Quality {
        self.settings
            .download_quality()
            .unwrap_or_else(|| self.settings.quality())
    }

    /// Bytes free on the disk the downloads fill, when it can be told.
    fn free_disk(&self) -> Option<u64> {
        if let Some(free) = self.downloads.free_override {
            return Some(free);
        }
        if cfg!(test) {
            return None;
        }
        crate::diskspace::available(&self.dirs.offline_dir())
    }

    /// Puts songs in the queue, after what is there, unless they are kept
    /// or queued already. `upgrade` queues kept songs again, to replace a
    /// lossy copy.
    fn enqueue_downloads(&mut self, wanted: Vec<Wanted>, upgrade: bool) {
        let mut queued: HashSet<String> = self
            .downloads
            .queue
            .iter()
            .map(|queued| queued.uri.clone())
            .collect();
        let before = self.downloads.queue.len();
        for want in wanted {
            if queued.contains(&want.uri) {
                continue;
            }
            if !upgrade && want.context.is_none() && self.downloads.is_kept(&want.uri) {
                continue;
            }
            let shown = self.shown(want.item.as_ref(), &want.uri);
            queued.insert(want.uri.clone());
            self.downloads.pending.insert(want.uri.clone(), (0, None));
            self.downloads.queue.push(QueuedDownload {
                uri: want.uri,
                title: shown.title,
                subtitle: shown.subtitle,
                image: shown.image,
                context: want.context,
                state: DownloadState::Waiting,
                attempts: 0,
            });
            self.downloads.queue_dirty = true;
        }
        if !upgrade && self.downloads.queue.len() > before {
            self.confirm(
                "download",
                gettext(self.locale, "Download started").into_owned(),
                None,
            );
        }
        self.pump_downloads();
    }

    /// Whether downloads may start now: a connection, and one that is not
    /// metered unless the user allowed that.
    pub(crate) fn downloads_may_run(&self) -> bool {
        self.network_reachable
            && (self.settings.download_metered || !self.power.conditions().metered)
    }

    /// Holds new songs back for `secs` seconds.
    fn cool_downloads(&mut self, secs: u64) {
        let until = Instant::now() + Duration::from_secs(secs);
        if self.downloads.cool_until.is_none_or(|now| now < until) {
            self.downloads.cool_until = Some(until);
        }
    }

    /// The disk has no room: nothing more starts until the user says so.
    fn pause_for_full_disk(&mut self) {
        let mut stop = Vec::new();
        for queued in &mut self.downloads.queue {
            if matches!(queued.state, DownloadState::Active(..)) {
                stop.push(queued.uri.clone());
                queued.state = DownloadState::Waiting;
            }
        }
        if !stop.is_empty() {
            self.stop_downloads(stop);
        }
        self.downloads.paused = true;
        self.downloads.queue_dirty = true;
        if !self.downloads.disk_full {
            self.downloads.disk_full = true;
            self.confirm_failed(
                "download",
                gettext(
                    self.locale,
                    "The disk is full. Free up space, then resume the downloads.",
                )
                .into_owned(),
            );
        }
    }

    /// Starts waiting songs, up to the number allowed at once.
    pub(crate) fn pump_downloads(&mut self) {
        if self.downloads.paused
            || self.offline
            || !self
                .downloads
                .queue
                .iter()
                .any(|queued| queued.state == DownloadState::Waiting)
            || !self.downloads_may_run()
        {
            return;
        }
        if let Some(until) = self.downloads.cool_until {
            if Instant::now() < until {
                return;
            }
            self.downloads.cool_until = None;
        }
        if self.free_disk().is_some_and(|free| free < RESERVE) {
            self.pause_for_full_disk();
            return;
        }
        let limit = self.settings.download_concurrency();
        let quality = self.settings.download_quality();
        let mut active = self.downloads.active();
        let mut start = Vec::new();
        for queued in &mut self.downloads.queue {
            if active >= limit {
                break;
            }
            if queued.state != DownloadState::Waiting {
                continue;
            }
            queued.state = DownloadState::Active(0, None);
            active += 1;
            start.push((queued.uri.clone(), queued.context.clone()));
        }
        for (uri, context) in start {
            let (context, first) = match context {
                Some((context, first)) => (Some(context), first),
                None => (None, 0),
            };
            self.backend.send(Command::OfflineDownload {
                uris: vec![uri],
                context,
                first,
                quality,
            });
        }
    }

    /// Once a frame: carries on after a pause TIDAL asked for, and writes
    /// the queue to disk now and then.
    pub(crate) fn tick_downloads(&mut self, ctx: &egui::Context) {
        if let Some(until) = self.downloads.cool_until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.pump_downloads();
            } else {
                ctx.request_repaint_after(left);
            }
        }
        if self
            .downloads
            .reading
            .is_some_and(|since| since.elapsed() > READ_GIVE_UP)
        {
            self.read_next_collection();
        }
        self.flush_download_queue(false);
    }

    /// Writes the queue to disk if it changed, at most every few seconds
    /// unless `force`.
    pub(crate) fn flush_download_queue(&mut self, force: bool) {
        if !self.downloads.queue_dirty {
            return;
        }
        if !force
            && self
                .downloads
                .queue_saved
                .is_some_and(|saved| saved.elapsed() < SAVE_EVERY)
        {
            return;
        }
        self.downloads.queue_dirty = false;
        self.downloads.queue_saved = Some(Instant::now());
        let saved = SavedQueue {
            paused: self.downloads.paused,
            items: self
                .downloads
                .queue
                .iter()
                .map(|queued| SavedDownload {
                    uri: queued.uri.clone(),
                    title: queued.title.clone(),
                    subtitle: queued.subtitle.clone(),
                    image: queued.image.clone(),
                    context: queued.context.clone(),
                    paused: queued.state == DownloadState::Paused,
                    failed: match &queued.state {
                        DownloadState::Failed(error) => Some(error.clone()),
                        _ => None,
                    },
                })
                .collect(),
        };
        let path = self.dirs.download_queue_file();
        if saved.items.is_empty() && !saved.paused {
            let _ = std::fs::remove_file(&path);
            return;
        }
        match serde_json::to_string(&saved) {
            Ok(text) => {
                if let Err(error) = fastframe_file::write_private(&path, text) {
                    log::warn!("could not write the download queue: {error}");
                }
            }
            Err(error) => log::warn!("could not save the download queue: {error}"),
        }
    }

    /// Takes back the queue of the last run: what was waiting or on its way
    /// waits again. Songs kept since, and ones of collections let go, are
    /// left out.
    fn restore_download_queue(&mut self) {
        let path = self.dirs.download_queue_file();
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let saved: SavedQueue = match serde_json::from_slice(&bytes) {
            Ok(saved) => saved,
            Err(error) => {
                log::warn!("unreadable {}: {error}", path.display());
                return;
            }
        };
        self.downloads.paused = saved.paused;
        let mut queued: HashSet<String> = self
            .downloads
            .queue
            .iter()
            .map(|queued| queued.uri.clone())
            .collect();
        for item in saved.items {
            if item.uri.is_empty()
                || self.downloads.is_kept(&item.uri)
                || !queued.insert(item.uri.clone())
            {
                continue;
            }
            if let Some((context, _)) = &item.context
                && self.downloads.collection(context).is_none()
            {
                continue;
            }
            let state = match item.failed {
                Some(error) => DownloadState::Failed(error),
                None if item.paused => DownloadState::Paused,
                None => DownloadState::Waiting,
            };
            self.downloads.pending.insert(item.uri.clone(), (0, None));
            self.downloads.queue.push(QueuedDownload {
                uri: item.uri,
                title: item.title,
                subtitle: item.subtitle,
                image: item.image,
                context: item.context,
                state,
                attempts: 0,
            });
        }
        self.pump_downloads();
    }

    /// Takes songs out of the queue, stopping those downloading.
    fn drop_from_queue(&mut self, keep: impl Fn(&QueuedDownload) -> bool) {
        let mut stop = Vec::new();
        self.downloads.queue.retain(|queued| {
            if keep(queued) {
                return true;
            }
            if matches!(queued.state, DownloadState::Active(..)) {
                stop.push(queued.uri.clone());
            }
            false
        });
        let queued: HashSet<&str> = self
            .downloads
            .queue
            .iter()
            .map(|queued| queued.uri.as_str())
            .collect();
        self.downloads
            .pending
            .retain(|uri, _| queued.contains(uri.as_str()));
        self.downloads.queue_dirty = true;
        if !stop.is_empty() {
            self.stop_downloads(stop);
        }
    }

    /// Cancels running downloads. Each job answers once (Done, Failed or
    /// Cancelled); that answer is the stopped job's, not news about a song
    /// queued again since.
    fn stop_downloads(&mut self, uris: Vec<String>) {
        self.downloads.stopped.extend(uris.iter().cloned());
        self.backend.send(Command::OfflineCancel(uris));
    }

    pub(super) fn download_songs(&mut self, uris: Vec<String>, context: Option<String>) {
        let wanted = uris
            .into_iter()
            .enumerate()
            .map(|(index, uri)| Wanted {
                uri,
                item: None,
                context: context.clone().map(|context| (context, index as u32)),
            })
            .collect();
        self.enqueue_downloads(wanted, false);
    }

    pub(super) fn remove_songs(&mut self, uris: Vec<String>) {
        let gone: HashSet<&String> = uris.iter().collect();
        self.drop_from_queue(|queued| !gone.contains(&queued.uri));
        self.confirm(
            "download-removed",
            gettext(self.locale, "Download removed").into_owned(),
            None,
        );
        self.backend.send(Command::OfflineRemove(uris));
    }

    /// Kept songs that are lossy copies are fetched again when downloads are
    /// Lossless or Max: an earlier version kept TIDAL's AAC answer to those
    /// requests.
    pub(super) fn upgrade_lossy_downloads(&mut self, uris: Vec<String>) {
        if uris.is_empty() || self.download_quality() < crate::api::Quality::Lossless {
            return;
        }
        let wanted = uris
            .into_iter()
            .map(|uri| Wanted {
                uri,
                item: None,
                context: None,
            })
            .collect();
        self.enqueue_downloads(wanted, true);
    }

    pub(super) fn download_collection(&mut self, head: CollectionHead) {
        self.load_kept_collections();
        if head.uri.is_empty() || self.downloads.collection(&head.uri).is_some() {
            return;
        }
        let uri = head.uri.clone();
        self.downloads.collections.push(KeptCollection {
            head,
            ..KeptCollection::default()
        });
        self.save_kept_collections();
        self.backend.send(Command::OfflineContextTracks(uri));
    }

    pub(super) fn remove_collection(&mut self, uri: String) {
        self.load_kept_collections();
        let Some(index) = self
            .downloads
            .collections
            .iter()
            .position(|kept| kept.head.uri == uri)
        else {
            return;
        };
        let kept = self.downloads.collections.remove(index);
        self.save_kept_collections();
        let leaving: Vec<String> = self
            .downloads
            .queue
            .iter()
            .filter(|queued| {
                queued
                    .context
                    .as_ref()
                    .is_some_and(|(context, _)| *context == uri)
            })
            .map(|queued| queued.uri.clone())
            .collect();
        self.drop_from_queue(|queued| {
            queued
                .context
                .as_ref()
                .is_none_or(|(context, _)| *context != uri)
        });
        // A song another kept collection lists stays queued, for that one.
        let wanted: Vec<Wanted> = leaving
            .into_iter()
            .filter_map(|song| {
                let context = self.other_collections(&song, None).into_iter().next()?;
                Some(Wanted {
                    uri: song,
                    item: None,
                    context: Some(context),
                })
            })
            .collect();
        if !wanted.is_empty() {
            // Quietly: nothing new was asked for.
            self.enqueue_downloads(wanted, true);
        }
        if !kept.tracks.is_empty() {
            self.backend.send(Command::OfflineRelease {
                uris: kept.tracks,
                context: uri,
            });
        }
        self.pump_downloads();
    }

    pub(super) fn apply_download_queue(&mut self, action: Action) {
        match action {
            Action::PauseDownload(uri) => {
                let mut stop = None;
                if let Some(queued) = self.downloads.queue.iter_mut().find(|q| q.uri == uri) {
                    if matches!(queued.state, DownloadState::Active(..)) {
                        stop = Some(uri.clone());
                    }
                    if matches!(
                        queued.state,
                        DownloadState::Active(..) | DownloadState::Waiting
                    ) {
                        queued.state = DownloadState::Paused;
                    }
                }
                if let Some(uri) = stop {
                    self.stop_downloads(vec![uri]);
                }
            }
            Action::ResumeDownload(uri) | Action::RetryDownload(uri) => {
                if let Some(queued) = self.downloads.queue.iter_mut().find(|q| q.uri == uri)
                    && matches!(
                        queued.state,
                        DownloadState::Paused | DownloadState::Failed(_)
                    )
                {
                    queued.state = DownloadState::Waiting;
                    queued.attempts = 0;
                }
                self.downloads.cool_until = None;
                self.pump_downloads();
            }
            Action::CancelDownload(uri) => self.drop_from_queue(|queued| queued.uri != uri),
            Action::PauseAllDownloads => {
                self.downloads.paused = true;
                let mut stop = Vec::new();
                for queued in &mut self.downloads.queue {
                    if matches!(queued.state, DownloadState::Active(..)) {
                        stop.push(queued.uri.clone());
                        // Starts again with the queue.
                        queued.state = DownloadState::Waiting;
                    }
                }
                if !stop.is_empty() {
                    self.stop_downloads(stop);
                }
            }
            Action::RetryFailedDownloads => {
                for queued in &mut self.downloads.queue {
                    if matches!(queued.state, DownloadState::Failed(_)) {
                        queued.state = DownloadState::Waiting;
                        queued.attempts = 0;
                    }
                }
                self.downloads.cool_until = None;
                self.pump_downloads();
            }
            Action::ResumeAllDownloads => {
                self.downloads.paused = false;
                self.downloads.disk_full = false;
                self.downloads.cool_until = None;
                for queued in &mut self.downloads.queue {
                    if matches!(
                        queued.state,
                        DownloadState::Paused | DownloadState::Failed(_)
                    ) {
                        queued.state = DownloadState::Waiting;
                        queued.attempts = 0;
                    }
                }
                self.pump_downloads();
            }
            Action::CancelAllDownloads => self.drop_from_queue(|_| false),
            Action::RemoveAllDownloads => {
                self.drop_from_queue(|_| false);
                self.downloads.collections.clear();
                self.save_kept_collections();
                self.backend.send(Command::OfflineRemoveAll);
            }
            _ => {}
        }
        self.downloads.queue_dirty = true;
    }

    pub(super) fn receive_offline(&mut self, event: OfflineEvent) {
        match event {
            OfflineEvent::Progress {
                uri,
                received,
                total,
            } => {
                if let Some(queued) = self.downloads.queue.iter_mut().find(|q| q.uri == uri)
                    && matches!(queued.state, DownloadState::Active(..))
                {
                    queued.state = DownloadState::Active(received, total);
                    self.downloads.pending.insert(uri, (received, total));
                }
            }
            OfflineEvent::Done { uri } => {
                let stopped = self.downloads.stopped.remove(&uri);
                let Some(position) = self.downloads.queue.iter().position(|q| q.uri == uri) else {
                    if stopped {
                        // Finished as it was cancelled or removed: whether
                        // it stayed is for the disk to say.
                        self.downloads.list_asked = Some(Instant::now());
                        self.backend.send(Command::OfflineList);
                        return;
                    }
                    // A song noted for another collection (below).
                    self.downloads.kept.insert(uri);
                    self.settle_downloads();
                    return;
                };
                let done = self.downloads.queue.remove(position);
                // The queue holds a song once, for the first collection that
                // asked; the others that list it are noted on it now, or
                // letting go of the first would remove it from them too.
                let queued_for = done.context.as_ref().map(|(context, _)| context.as_str());
                for (context, first) in self.other_collections(&uri, queued_for) {
                    self.backend.send(Command::OfflineDownload {
                        uris: vec![uri.clone()],
                        context: Some(context),
                        first,
                        quality: self.settings.download_quality(),
                    });
                }
                self.downloads.pending.remove(&uri);
                self.downloads.kept.insert(uri);
                // TIDAL let this one through: ease off the pause it asked for.
                self.downloads.cool_secs /= 2;
                self.downloads.queue_dirty = true;
                self.settle_downloads();
                if self.downloads.queue.is_empty() {
                    self.confirm_done("download");
                    self.confirm(
                        "download-done",
                        gettext(self.locale, "Downloaded").into_owned(),
                        None,
                    );
                }
                self.pump_downloads();
            }
            OfflineEvent::Failed { uri, error } => {
                // A stopped job's failure says nothing of the queue now.
                if !self.downloads.stopped.remove(&uri) {
                    self.download_failed(uri, error);
                }
                self.pump_downloads();
            }
            OfflineEvent::Cancelled { uri } if self.downloads.stopped.remove(&uri) => {
                // Paused, cancelled or the whole queue paused: the queue
                // already says so, even if the song was queued again since.
                self.pump_downloads();
            }
            OfflineEvent::Cancelled { uri } => {
                // Paused, cancelled or the whole queue paused: the queue
                // already says so. One cut off any other way starts again.
                if let Some(queued) = self.downloads.queue.iter_mut().find(|q| q.uri == uri)
                    && matches!(queued.state, DownloadState::Active(..))
                {
                    queued.state = DownloadState::Waiting;
                }
                self.pump_downloads();
            }
            OfflineEvent::Finished => {}
        }
    }

    /// What to do with a song that could not be kept: leave out one TIDAL
    /// will not give out, wait out a rate limit, try a flaky one again, or
    /// stop at a full disk; any other waits for Try again.
    fn download_failed(&mut self, uri: String, error: String) {
        log::warn!("download of {uri} failed: {error}");
        let Some(position) = self.downloads.queue.iter().position(|q| q.uri == uri) else {
            return;
        };
        if !matches!(
            self.downloads.queue[position].state,
            DownloadState::Active(..)
        ) {
            return;
        }
        self.downloads.queue_dirty = true;
        if error.contains(crate::tidal_engine::DISK_FULL) {
            self.downloads.queue[position].state = DownloadState::Waiting;
            self.pause_for_full_disk();
        } else if is_unavailable(&error) {
            let gone = self.downloads.queue.remove(position);
            self.downloads.pending.remove(&uri);
            self.downloads.skipped_run += 1;
            if let Some((context, _)) = gone.context
                && let Some(kept) = self
                    .downloads
                    .collections
                    .iter_mut()
                    .find(|kept| kept.head.uri == context)
            {
                kept.tracks.retain(|track| *track != uri);
                kept.unavailable.push(uri);
                self.save_kept_collections();
            }
            self.settle_downloads();
        } else if is_rate_limited(&error) {
            self.downloads.queue[position].state = DownloadState::Waiting;
            self.downloads.cool_secs =
                (self.downloads.cool_secs.max(COOL_FIRST / 2) * 2).clamp(COOL_FIRST, COOL_LONGEST);
            self.cool_downloads(self.downloads.cool_secs);
        } else if !is_final(&error) && self.downloads.queue[position].attempts < AUTO_RETRIES {
            let queued = &mut self.downloads.queue[position];
            queued.attempts += 1;
            queued.state = DownloadState::Waiting;
            let wait = 5 * u64::from(queued.attempts);
            self.cool_downloads(wait);
        } else {
            self.downloads.queue[position].state = DownloadState::Failed(error);
            // One notice for a run of failures, not one for each song.
            if self.downloads.failed() == 1 {
                self.confirm_failed(
                    "download",
                    gettext(self.locale, "A download failed").into_owned(),
                );
            }
        }
    }

    /// After a song leaves the queue: when it empties, says how many songs
    /// could not be had, and has the kept list read once; while it fills
    /// the open Downloaded page now and then.
    fn settle_downloads(&mut self) {
        let page_open = matches!(self.page(), Page::Downloaded);
        if self.downloads.queue.is_empty() {
            if std::mem::take(&mut self.downloads.skipped_run) > 0 {
                let count = self
                    .downloads
                    .collections
                    .iter()
                    .map(KeptCollection::unavailable_count)
                    .sum::<usize>();
                let message = gettext(self.locale, "Songs not available: {count}")
                    .replace("{count}", &count.to_string());
                self.toast(message);
            }
            self.downloads.list_asked = Some(Instant::now());
            self.backend.send(Command::OfflineList);
        } else if page_open
            && self
                .downloads
                .list_asked
                .is_none_or(|asked| asked.elapsed() > LIST_EVERY)
        {
            self.downloads.list_asked = Some(Instant::now());
            self.backend.send(Command::OfflineList);
        }
    }

    /// A kept collection's songs as TIDAL lists them now: new ones are
    /// queued, ones it no longer has are let go.
    pub(super) fn receive_offline_context(
        &mut self,
        uri: String,
        result: Result<(Vec<PlayableItem>, u32), String>,
    ) {
        self.apply_offline_context(&uri, result);
        self.read_next_collection();
    }

    /// Reads the next kept collection waiting to be compared with TIDAL.
    fn read_next_collection(&mut self) {
        self.downloads.reading = None;
        if self.offline || !self.network_reachable {
            self.downloads.to_read.clear();
            return;
        }
        if let Some(uri) = self.downloads.to_read.pop_front() {
            self.downloads.reading = Some(Instant::now());
            self.backend.send(Command::OfflineContextTracks(uri));
        }
    }

    fn apply_offline_context(
        &mut self,
        uri: &str,
        result: Result<(Vec<PlayableItem>, u32), String>,
    ) {
        let (items, unplayable) = match result {
            Ok(found) => found,
            Err(error) => {
                log::warn!("unable to read {uri} for downloading: {error}");
                if let Some(kept) = self.downloads.collection(uri)
                    && kept.tracks.is_empty()
                {
                    let name = kept.head.name.clone();
                    // Not kept as an empty shell: that would stop it being
                    // added again once the cause is gone.
                    self.downloads
                        .collections
                        .retain(|kept| kept.head.uri != uri);
                    self.save_kept_collections();
                    let title =
                        gettext(self.locale, "Couldn't download {name}").replace("{name}", &name);
                    self.confirm_failed(
                        "download",
                        format!("{title}: {}", download_failure_reason(&error)),
                    );
                }
                return;
            }
        };
        let quality = self.download_quality();
        let free = self.free_disk();
        let queued: HashSet<String> = self
            .downloads
            .queue
            .iter()
            .map(|queued| queued.uri.clone())
            .collect();
        let Some(index) = self
            .downloads
            .collections
            .iter()
            .position(|kept| kept.head.uri == uri)
        else {
            return;
        };
        let unavailable: HashSet<String> = self.downloads.collections[index]
            .unavailable
            .iter()
            .cloned()
            .collect();
        let items: Vec<PlayableItem> = items
            .into_iter()
            .filter(crate::ui::widgets::row_playable)
            .filter(|item| !unavailable.contains(item.uri()))
            .collect();
        let had_songs = !self.downloads.collections[index].tracks.is_empty();
        if items.is_empty() && had_songs {
            // A list that came back empty is more likely a glitch than the
            // playlist emptied: do not let go of everything kept.
            log::warn!("{uri} came back empty; leaving its songs alone");
            return;
        }
        // Whether what is still to download fits the disk.
        let missing = items
            .iter()
            .filter(|item| !self.downloads.is_kept(item.uri()) && !queued.contains(item.uri()))
            .count() as u64;
        let needed = missing.saturating_mul(song_estimate(quality));
        let fits = free.is_none_or(|free| needed.saturating_add(RESERVE) <= free);
        if !fits {
            let name = self.downloads.collections[index].head.name.clone();
            let message = gettext(
                self.locale,
                "There isn't enough disk space for {name}. It needs about {needed}, and {free} is free.",
            )
            .replace("{name}", &name)
            .replace("{needed}", &util::format_bytes(needed))
            .replace("{free}", &util::format_bytes(free.unwrap_or_default()));
            self.confirm_failed("download", message);
            if !had_songs {
                self.downloads.collections.remove(index);
                self.save_kept_collections();
                return;
            }
        }

        let kept = &mut self.downloads.collections[index];
        if unplayable > kept.skipped {
            self.downloads.skipped_run += unplayable - kept.skipped;
        }
        kept.skipped = unplayable;
        let now: Vec<String> = items.iter().map(|item| item.uri().to_string()).collect();
        let current: HashSet<&String> = now.iter().collect();
        let before: HashSet<String> = kept.tracks.iter().cloned().collect();
        let gone: Vec<String> = kept
            .tracks
            .iter()
            .filter(|track| !current.contains(track))
            .cloned()
            .collect();
        kept.tracks = now;
        self.save_kept_collections();
        if !gone.is_empty() {
            let leaving: HashSet<&String> = gone.iter().collect();
            self.drop_from_queue(|queued| {
                !(leaving.contains(&queued.uri)
                    && queued
                        .context
                        .as_ref()
                        .is_some_and(|(context, _)| context == uri))
            });
            self.backend.send(Command::OfflineRelease {
                uris: gone,
                context: uri.to_string(),
            });
        }
        // Everything new to the collection, and anything of it not kept (a
        // download that failed before). The store notes the collection on
        // songs it keeps already without fetching them again.
        if fits {
            let wanted: Vec<Wanted> = items
                .into_iter()
                .enumerate()
                .filter(|(_, item)| {
                    !before.contains(item.uri()) || !self.downloads.is_kept(item.uri())
                })
                .map(|(index, item)| Wanted {
                    uri: item.uri().to_string(),
                    context: Some((uri.to_string(), index as u32)),
                    item: Some(item),
                })
                .collect();
            self.enqueue_downloads(wanted, false);
        }
        self.settle_downloads();
    }

    /// Reads the kept collections and the queue once, then asks TIDAL
    /// whether the collections changed.
    pub(super) fn load_kept_collections(&mut self) {
        if self.downloads.synced {
            return;
        }
        self.downloads.synced = true;
        let path = self.dirs.downloads_file();
        if let Ok(text) = std::fs::read(&path) {
            // One by one: a collection this version cannot read is left out,
            // not every other one, which the next save would forget.
            match serde_json::from_slice::<Vec<serde_json::Value>>(&text) {
                Ok(collections) => {
                    self.downloads.collections = collections
                        .into_iter()
                        .filter_map(|kept| match serde_json::from_value(kept) {
                            Ok(kept) => Some(kept),
                            Err(error) => {
                                log::warn!("unreadable kept collection: {error}");
                                None
                            }
                        })
                        .collect();
                }
                Err(error) => log::warn!("unreadable {}: {error}", path.display()),
            }
        }
        self.restore_download_queue();
        self.sync_kept_collections();
    }

    /// Kept collections follow TIDAL's changes, one read at a time: a
    /// playlist, mix or artist always, an album only while some of its songs
    /// are not kept (it does not change, but a restart may have cut it
    /// short).
    pub(super) fn sync_kept_collections(&mut self) {
        if self.offline || !self.network_reachable {
            return;
        }
        let reading = self
            .downloads
            .reading
            .is_some_and(|since| since.elapsed() < READ_GIVE_UP);
        if reading || !self.downloads.to_read.is_empty() {
            return;
        }
        if self
            .downloads
            .last_sync
            .is_some_and(|synced| synced.elapsed() < SYNC_EVERY)
        {
            return;
        }
        self.downloads.last_sync = Some(Instant::now());
        let changing: Vec<String> = self
            .downloads
            .collections
            .iter()
            .filter(|kept| {
                util::uri_kind(&kept.head.uri) != Some("album")
                    || kept.tracks.is_empty()
                    || kept
                        .tracks
                        .iter()
                        .any(|track| !self.downloads.is_kept(track))
            })
            .map(|kept| kept.head.uri.clone())
            .collect();
        self.downloads.to_read.extend(changing);
        self.read_next_collection();
    }

    /// The kept collections other than `except` that list `uri`, with its
    /// place in each.
    fn other_collections(&self, uri: &str, except: Option<&str>) -> Vec<(String, u32)> {
        self.downloads
            .collections
            .iter()
            .filter(|kept| Some(kept.head.uri.as_str()) != except)
            .filter_map(|kept| {
                let place = kept.tracks.iter().position(|track| track == uri)?;
                Some((kept.head.uri.clone(), place as u32))
            })
            .collect()
    }

    fn save_kept_collections(&self) {
        match serde_json::to_string(&self.downloads.collections) {
            Ok(text) => {
                if let Err(error) = fastframe_file::write_private(&self.dirs.downloads_file(), text)
                {
                    log::warn!("could not write the kept downloads: {error}");
                }
            }
            Err(error) => log::warn!("could not save the kept downloads: {error}"),
        }
    }
}

/// Why a collection could not be read, in plain words for a toast. The
/// error is the whole chain (`loading tidal:playlist:..: <cause>`).
fn download_failure_reason(error: &str) -> String {
    let cause = error.rsplit(": ").next().unwrap_or(error).trim();
    let lower = error.to_lowercase();
    if lower.contains("sign-in expired") || lower.contains("not signed in") {
        "sign in to TIDAL again and retry".into()
    } else if lower.contains("rate limit") || lower.contains("quota") {
        "TIDAL is limiting requests, try again in a minute".into()
    } else if lower.contains("network error") {
        "TIDAL could not be reached, check the connection".into()
    } else if lower.contains("cannot play this kind") || lower.contains("not a tidal context") {
        "TIDAL has no song list for this".into()
    } else if cause.is_empty() {
        "TIDAL did not return its songs".into()
    } else {
        cause.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Track;

    fn app(name: &str) -> App {
        let root =
            std::env::temp_dir().join(format!("tidalfast-downloads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        app_at(&root)
    }

    /// An app over the folders of an earlier one, as after a restart.
    fn app_at(root: &std::path::Path) -> App {
        let mut app = App::new(
            &Waker::default(),
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.offline = false;
        app.network_reachable = true;
        app.settings.download_metered = true;
        app.settings.download_concurrency = 2;
        app.downloads.synced = true;
        app.backend.take_offline_commands();
        app
    }

    fn uri(n: u32) -> String {
        format!("tidal:track:{n}")
    }

    fn song(n: u32) -> PlayableItem {
        PlayableItem::Track(Track {
            uri: uri(n),
            id: Some(n.to_string()),
            name: format!("Song {n}"),
            ..Track::default()
        })
    }

    fn states(app: &App) -> Vec<(String, DownloadState)> {
        app.downloads
            .queue
            .iter()
            .map(|queued| (queued.uri.clone(), queued.state.clone()))
            .collect()
    }

    /// Songs download a few at a time; the next starts when one is kept.
    #[test]
    fn the_queue_keeps_to_its_limit_and_moves_on() {
        let mut app = app("limit");
        app.download_songs(vec![uri(1), uri(2), uri(3)], None);
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1", "download tidal:track:2"]
        );
        assert_eq!(states(&app)[2].1, DownloadState::Waiting);
        app.receive_offline(OfflineEvent::Done { uri: uri(1) });
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:3"]
        );
        assert!(app.downloads.is_kept(&uri(1)));
        assert_eq!(app.downloads.queued(), 2);
        app.backend.shutdown();
    }

    /// Pausing stops a song, resuming queues it again, cancelling drops it;
    /// a failure waits for Try again.
    #[test]
    fn a_song_pauses_resumes_cancels_and_retries() {
        let mut app = app("controls");
        app.settings.download_concurrency = 1;
        app.download_songs(vec![uri(1), uri(2)], None);
        app.backend.take_offline_commands();
        app.apply_download_queue(Action::PauseDownload(uri(1)));
        assert_eq!(
            app.backend.take_offline_commands(),
            ["cancel tidal:track:1"]
        );
        app.receive_offline(OfflineEvent::Cancelled { uri: uri(1) });
        // The paused song stays paused; the next one starts.
        assert_eq!(states(&app)[0].1, DownloadState::Paused);
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:2"]
        );
        app.receive_offline(OfflineEvent::Failed {
            uri: uri(2),
            error: "sign-in expired".into(),
        });
        assert_eq!(
            states(&app)[1].1,
            DownloadState::Failed("sign-in expired".into())
        );
        app.apply_download_queue(Action::ResumeDownload(uri(1)));
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1"]
        );
        app.apply_download_queue(Action::CancelDownload(uri(1)));
        assert_eq!(
            app.backend.take_offline_commands(),
            ["cancel tidal:track:1"]
        );
        app.apply_download_queue(Action::RetryDownload(uri(2)));
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:2"]
        );
        assert_eq!(app.downloads.queued(), 1);
        app.apply_download_queue(Action::CancelAllDownloads);
        assert!(app.downloads.queue.is_empty());
        assert!(app.downloads.pending.is_empty());
        app.backend.shutdown();
    }

    /// The answer of a job stopped here belongs to that job: a Done that
    /// crossed a cancel is checked on disk rather than shown kept, and a
    /// paused song resumed at once is not cut off by its old job's
    /// Cancelled (which started it a second time).
    #[test]
    fn a_stopped_jobs_late_answer_changes_nothing() {
        let mut app = app("stopped");
        app.settings.download_concurrency = 1;
        app.download_songs(vec![uri(1)], None);
        app.backend.take_offline_commands();
        app.apply_download_queue(Action::CancelDownload(uri(1)));
        app.backend.take_offline_commands();
        app.receive_offline(OfflineEvent::Done { uri: uri(1) });
        assert!(!app.downloads.is_kept(&uri(1)), "a cancelled song");
        assert!(
            app.backend
                .take_offline_commands()
                .iter()
                .all(|line| !line.starts_with("download")),
            "nothing starts again"
        );

        app.download_songs(vec![uri(2)], None);
        app.apply_download_queue(Action::PauseDownload(uri(2)));
        app.apply_download_queue(Action::ResumeDownload(uri(2)));
        assert_eq!(
            app.backend.take_offline_commands(),
            [
                "download tidal:track:2",
                "cancel tidal:track:2",
                "download tidal:track:2"
            ]
        );
        app.receive_offline(OfflineEvent::Cancelled { uri: uri(2) });
        assert!(matches!(states(&app)[0].1, DownloadState::Active(..)));
        assert!(
            app.backend.take_offline_commands().is_empty(),
            "the running job is left to run"
        );
        app.receive_offline(OfflineEvent::Done { uri: uri(2) });
        assert!(app.downloads.is_kept(&uri(2)));
        assert!(app.downloads.stopped.is_empty());
        app.backend.shutdown();
    }

    /// Without a connection nothing starts; it does once one is back.
    #[test]
    fn the_queue_waits_for_a_connection() {
        let mut app = app("offline");
        app.network_reachable = false;
        app.download_songs(vec![uri(1)], None);
        assert!(app.backend.take_offline_commands().is_empty());
        app.network_reachable = true;
        app.pump_downloads();
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1"]
        );
        app.backend.shutdown();
    }

    /// A kept playlist follows TIDAL: new songs are queued in their place,
    /// songs it no longer has are let go.
    #[test]
    fn a_kept_playlist_follows_its_changes() {
        let mut app = app("sync");
        app.settings.download_concurrency = 6;
        let playlist = "tidal:playlist:p".to_string();
        app.download_collection(CollectionHead {
            uri: playlist.clone(),
            name: "Road trip".into(),
            ..CollectionHead::default()
        });
        assert_eq!(
            app.backend.take_offline_commands(),
            ["read tidal:playlist:p"]
        );
        app.receive_offline_context(playlist.clone(), Ok((vec![song(1), song(2)], 0)));
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1", "download tidal:track:2"]
        );
        assert_eq!(
            app.downloads.queue[1].context,
            Some((playlist.clone(), 1)),
            "each song keeps its place"
        );
        app.receive_offline(OfflineEvent::Done { uri: uri(1) });
        app.receive_offline(OfflineEvent::Done { uri: uri(2) });
        app.receive_offline_context(playlist.clone(), Ok((vec![song(2), song(3)], 0)));
        assert_eq!(
            app.backend.take_offline_commands(),
            [
                "release tidal:track:1 tidal:playlist:p",
                "download tidal:track:3"
            ]
        );
        assert_eq!(app.downloads.queue[0].context, Some((playlist.clone(), 1)));
        assert_eq!(
            app.downloads.collection(&playlist).unwrap().tracks,
            [uri(2), uri(3)]
        );
        // Letting it go stops what is on its way and lets its songs go.
        app.remove_collection(playlist.clone());
        assert_eq!(
            app.backend.take_offline_commands(),
            [
                "cancel tidal:track:3",
                "release tidal:track:2 tidal:playlist:p",
                "release tidal:track:3 tidal:playlist:p",
            ]
        );
        assert!(app.downloads.collections.is_empty());
        app.backend.shutdown();
    }

    /// A kept collection this version cannot read costs that one only.
    #[test]
    fn one_unreadable_kept_collection_keeps_the_others() {
        let mut app = app("lenient");
        let good = KeptCollection {
            head: CollectionHead {
                uri: "tidal:album:a".into(),
                name: "Kept".into(),
                ..CollectionHead::default()
            },
            tracks: vec![uri(1)],
            ..KeptCollection::default()
        };
        let saved = serde_json::json!([{"head": 5}, serde_json::to_value(&good).unwrap()]);
        std::fs::create_dir_all(&app.dirs.state).unwrap();
        std::fs::write(app.dirs.downloads_file(), saved.to_string()).unwrap();
        app.downloads.synced = false;
        app.network_reachable = false;
        app.load_kept_collections();
        assert_eq!(app.downloads.collections.len(), 1);
        assert_eq!(app.downloads.collections[0].head.uri, "tidal:album:a");
        app.backend.shutdown();
    }

    /// A song two kept collections share downloads once and is noted for
    /// both, so letting one go does not take it from the other.
    #[test]
    fn a_song_two_collections_share_is_kept_for_both() {
        let mut app = app("shared");
        app.settings.download_concurrency = 6;
        let album = "tidal:album:a".to_string();
        let playlist = "tidal:playlist:p".to_string();
        for uri in [&album, &playlist] {
            app.download_collection(CollectionHead {
                uri: uri.clone(),
                name: uri.clone(),
                ..CollectionHead::default()
            });
        }
        app.backend.take_offline_commands();
        app.receive_offline_context(album.clone(), Ok((vec![song(1), song(2)], 0)));
        app.receive_offline_context(playlist.clone(), Ok((vec![song(2), song(3)], 0)));
        assert_eq!(
            app.backend.take_offline_commands(),
            [
                "download tidal:track:1",
                "download tidal:track:2",
                "download tidal:track:3"
            ],
            "the shared song is fetched once"
        );
        app.receive_offline(OfflineEvent::Done { uri: uri(2) });
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:2"],
            "noted for the playlist too, which the store does without fetching"
        );
        // That note finishing is not a second song done.
        app.receive_offline(OfflineEvent::Done { uri: uri(2) });
        assert_eq!(app.downloads.queued(), 2);

        // Letting the album go while its song 1 is on its way: the playlist
        // now lists song 1 too, so it stays queued, for the playlist.
        app.receive_offline_context(playlist.clone(), Ok((vec![song(1), song(2), song(3)], 0)));
        app.backend.take_offline_commands();
        app.remove_collection(album.clone());
        let queued: Vec<_> = app
            .downloads
            .queue
            .iter()
            .map(|queued| (queued.uri.clone(), queued.context.clone()))
            .collect();
        assert_eq!(
            queued,
            [
                (uri(3), Some((playlist.clone(), 1))),
                (uri(1), Some((playlist.clone(), 0))),
            ]
        );
        app.backend.shutdown();
    }

    /// A collection TIDAL will not list says why, and is not left behind
    /// as an empty entry that could never be added again.
    #[test]
    fn a_collection_that_cannot_be_read_says_why_and_can_be_added_again() {
        let mut app = app("unreadable");
        let playlist = "tidal:playlist:p".to_string();
        let head = CollectionHead {
            uri: playlist.clone(),
            name: "Road trip".into(),
            ..CollectionHead::default()
        };
        app.download_collection(head.clone());
        app.backend.take_offline_commands();
        app.receive_offline_context(
            playlist.clone(),
            Err("loading tidal:playlist:p: network error: connection reset".into()),
        );
        assert!(app.downloads.collection(&playlist).is_none());
        let toast = app.toasts.last().unwrap().message.clone();
        assert_eq!(
            toast,
            "Couldn't download Road trip: TIDAL could not be reached, check the connection"
        );
        app.download_collection(head);
        assert_eq!(
            app.backend.take_offline_commands(),
            ["read tidal:playlist:p"]
        );
        app.backend.shutdown();
    }

    fn playlist(app: &mut App, count: u32) -> String {
        let playlist = "tidal:playlist:big".to_string();
        app.download_collection(CollectionHead {
            uri: playlist.clone(),
            name: "Big".into(),
            ..CollectionHead::default()
        });
        app.backend.take_offline_commands();
        let songs = (1..=count).map(song).collect();
        app.receive_offline_context(playlist.clone(), Ok((songs, 0)));
        playlist
    }

    /// A 1,200-song playlist queues whole but starts only a few songs at a
    /// time, and the count of what is kept rises as they finish.
    #[test]
    fn a_huge_playlist_downloads_a_few_songs_at_a_time() {
        let mut app = app("huge");
        app.settings.download_concurrency = 3;
        let playlist = playlist(&mut app, 1200);
        assert_eq!(app.downloads.queue.len(), 1200);
        assert_eq!(app.downloads.active(), 3);
        assert_eq!(app.backend.take_offline_commands().len(), 3);
        for n in 1..=300 {
            app.receive_offline(OfflineEvent::Done { uri: uri(n) });
        }
        assert_eq!(app.downloads.active(), 3, "the pool stays full");
        assert_eq!(app.downloads.queue.len(), 900);
        let kept = app.downloads.collection(&playlist).unwrap();
        assert_eq!(app.downloads.collection_counts(kept), (300, 1200));
        app.backend.shutdown();
    }

    /// TIDAL asking for a pause holds every song back, none fails, and
    /// they carry on once the pause is over.
    #[test]
    fn a_rate_limit_pauses_the_queue_and_it_carries_on() {
        let mut app = app("limited");
        app.download_songs(vec![uri(1), uri(2), uri(3)], None);
        app.backend.take_offline_commands();
        app.receive_offline(OfflineEvent::Failed {
            uri: uri(1),
            error: "TIDAL is rate limiting requests; try again in a moment".into(),
        });
        assert_eq!(states(&app)[0].1, DownloadState::Waiting);
        assert!(app.downloads.cool_until.is_some());
        assert!(
            app.backend.take_offline_commands().is_empty(),
            "nothing starts during the pause"
        );
        app.downloads.cool_until = Some(Instant::now() - Duration::from_secs(1));
        app.pump_downloads();
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1"]
        );
        assert!(app.downloads.cool_until.is_none());
        app.backend.shutdown();
    }

    /// A flaky song is tried again on its own; one that keeps failing waits
    /// for Try again.
    #[test]
    fn a_song_that_keeps_failing_waits_for_try_again() {
        let mut app = app("flaky");
        app.settings.download_concurrency = 1;
        app.download_songs(vec![uri(1)], None);
        for _ in 0..AUTO_RETRIES {
            app.receive_offline(OfflineEvent::Failed {
                uri: uri(1),
                error: "network error: reset".into(),
            });
            assert_eq!(states(&app)[0].1, DownloadState::Waiting);
            app.downloads.cool_until = None;
            app.pump_downloads();
        }
        app.receive_offline(OfflineEvent::Failed {
            uri: uri(1),
            error: "network error: reset".into(),
        });
        assert!(matches!(states(&app)[0].1, DownloadState::Failed(_)));
        app.apply_download_queue(Action::RetryFailedDownloads);
        assert_eq!(app.downloads.active(), 1);
        app.backend.shutdown();
    }

    /// Songs TIDAL will not give out are left out and counted, and the
    /// rest of the collection downloads.
    #[test]
    fn unavailable_songs_are_skipped_and_counted() {
        let mut app = app("skipped");
        app.settings.download_concurrency = 1;
        let playlist = "tidal:playlist:p".to_string();
        app.download_collection(CollectionHead {
            uri: playlist.clone(),
            name: "Mine".into(),
            ..CollectionHead::default()
        });
        app.receive_offline_context(playlist.clone(), Ok((vec![song(1), song(2)], 2)));
        app.receive_offline(OfflineEvent::Failed {
            uri: uri(1),
            error: "This track isn't available: tidal:track:1".into(),
        });
        let kept = app.downloads.collection(&playlist).unwrap();
        assert_eq!(kept.tracks, [uri(2)]);
        assert_eq!(
            kept.unavailable_count(),
            3,
            "two unplayable and one refused"
        );
        assert_eq!(app.downloads.queue.len(), 1);
        app.receive_offline(OfflineEvent::Done { uri: uri(2) });
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message == "Songs not available: 3")
        );
        // Read again, the refused song is not tried again.
        app.receive_offline_context(playlist.clone(), Ok((vec![song(1), song(2)], 2)));
        assert!(app.downloads.queue.is_empty());
        app.backend.shutdown();
    }

    /// Without room nothing starts, and a collection that cannot fit is
    /// refused up front with the sizes.
    #[test]
    fn a_full_disk_stops_downloads_and_says_so() {
        let mut app = app("full");
        app.downloads.free_override = Some(1 << 20);
        app.download_songs(vec![uri(1)], None);
        assert!(app.backend.take_offline_commands().is_empty());
        assert!(app.downloads.paused);
        assert!(app.toasts.last().unwrap().message.contains("disk is full"));
        app.apply_download_queue(Action::CancelAllDownloads);

        // Room for the reserve but not for a playlist of 50 songs.
        app.downloads.free_override = Some(RESERVE + (100 << 20));
        app.downloads.paused = false;
        let playlist = playlist(&mut app, 50);
        assert!(app.downloads.collection(&playlist).is_none());
        assert!(app.downloads.queue.is_empty());
        assert!(
            app.toasts
                .last()
                .unwrap()
                .message
                .starts_with("There isn't enough disk space for Big.")
        );

        // The disk filling up under a running download pauses the queue.
        app.downloads.free_override = None;
        app.settings.download_concurrency = 2;
        app.download_songs(vec![uri(7), uri(8)], None);
        app.backend.take_offline_commands();
        app.receive_offline(OfflineEvent::Failed {
            uri: uri(7),
            error: crate::tidal_engine::DISK_FULL.into(),
        });
        assert!(app.downloads.paused && app.downloads.disk_full);
        assert_eq!(app.downloads.active(), 0);
        app.backend.shutdown();
    }

    /// A restart takes the queue back where it was: what was waiting or on
    /// its way waits again, what was kept meanwhile is not queued twice.
    #[test]
    fn a_restart_resumes_the_queue() {
        let mut first = app("restart");
        first.settings.download_concurrency = 2;
        let playlist = playlist(&mut first, 10);
        for n in 1..=4 {
            first.receive_offline(OfflineEvent::Done { uri: uri(n) });
        }
        first.flush_download_queue(true);
        first.backend.shutdown();
        let root = first.dirs.state.parent().unwrap().to_path_buf();

        let mut second = app_at(&root);
        second.downloads.synced = false;
        // The backend lists what is kept before the queue is read.
        second.downloads.kept = (1..=4).map(uri).collect();
        second.load_kept_collections();
        assert_eq!(second.downloads.queue.len(), 6);
        assert_eq!(second.downloads.queue[0].uri, uri(5));
        assert_eq!(second.downloads.active(), 2);
        assert_eq!(
            second.backend.take_offline_commands(),
            [
                "download tidal:track:5",
                "download tidal:track:6",
                "read tidal:playlist:big"
            ]
        );
        assert!(second.downloads.collection(&playlist).is_some());
        second.backend.shutdown();
    }

    /// A list that comes back empty does not empty a kept playlist.
    #[test]
    fn an_empty_answer_does_not_empty_a_kept_playlist() {
        let mut app = app("empty");
        let playlist = playlist(&mut app, 3);
        app.backend.take_offline_commands();
        app.receive_offline_context(playlist.clone(), Ok((Vec::new(), 0)));
        assert_eq!(app.downloads.collection(&playlist).unwrap().tracks.len(), 3);
        assert!(
            !app.backend
                .take_offline_commands()
                .iter()
                .any(|line| line.starts_with("release"))
        );
        app.backend.shutdown();
    }

    /// Lossy copies kept by an earlier version are fetched again, but only
    /// when downloads are Lossless or Max.
    #[test]
    fn lossy_copies_are_replaced_when_downloads_are_lossless() {
        let mut app = app("lossy");
        app.settings.download_quality = "HIGH".into();
        app.upgrade_lossy_downloads(vec![uri(1)]);
        assert!(app.backend.take_offline_commands().is_empty());
        app.settings.download_quality = "LOSSLESS".into();
        app.downloads.kept.insert(uri(1));
        app.upgrade_lossy_downloads(vec![uri(1)]);
        assert_eq!(
            app.backend.take_offline_commands(),
            ["download tidal:track:1"]
        );
        app.backend.shutdown();
    }

    #[test]
    fn failure_reasons_are_plain() {
        assert_eq!(
            download_failure_reason("loading x: your TIDAL sign-in expired; please sign in again"),
            "sign in to TIDAL again and retry"
        );
        assert_eq!(
            download_failure_reason("loading x: TIDAL could not find this."),
            "TIDAL could not find this."
        );
    }
}
