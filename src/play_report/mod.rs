//! Play reporting: tells TIDAL what was played here, the way TIDAL's own
//! apps do, so Tidalfast plays count in the account's history, My Activity
//! and recommendations (docs/_reference/tidal-activity.md, section 2).
//!
//! - [`listen`] follows the engine's state reports and cuts them into
//!   listens, one per played track, with TIDAL's start/stop actions.
//! - [`event`] turns a listen into a `playback_session` event (group
//!   `play_log`, version 2) that says honestly it comes from a desktop app.
//! - [`queue`] keeps unsent events on disk with retry and backoff.
//! - [`sqs`] is the event collector's SQS `SendMessageBatch` form and XML.
//!
//! [`PlayReporting`] ties them together on the backend's runtime: every
//! finished listen is written to the local statistics ([`crate::play_stats`])
//! and, while "Report plays to TIDAL" is on, queued; the queue is sent in
//! batches of up to ten every 30 seconds and on quit. Events refused as
//! malformed (`SenderFault=true`) are dropped; other failures wait and try
//! again. Tokens are never logged.

pub mod event;
pub mod listen;
pub mod queue;
pub mod sqs;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::api::client::ApiError;
use crate::api::tidal::TidalClient;
use crate::api::tidal::events::EVENT_BATCH_URL;
use crate::play_stats::{ListenLog, ListenRecord, StatsQuery, StatsReport};
use crate::tidal_engine::LocalState;
use listen::{Listen, ListenTracker};
use queue::{EventQueue, Pending};

/// How often the queue is sent, as TIDAL's web SDK does.
pub const FLUSH_EVERY: Duration = Duration::from_secs(30);
/// Batches sent per flush at most.
const BATCHES_PER_FLUSH: usize = 5;
/// The wait for the last flush when the app quits.
const SHUTDOWN_FLUSH: Duration = Duration::from_secs(4);
const FIRST_NETWORK_BACKOFF_MS: i64 = 30_000;
const MAX_NETWORK_BACKOFF_MS: i64 = 30 * 60 * 1000;

/// A random (version 4) UUID.
pub fn uuid_v4() -> String {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

enum Msg {
    Listen(Box<Listen>),
    Enabled(bool),
    Account(Option<Arc<TidalClient>>),
    Flush,
    Shutdown(oneshot::Sender<()>),
}

/// The reporting and local statistics, owned by the backend. See the
/// module docs.
#[derive(Clone)]
pub struct PlayReporting {
    tracker: Arc<Mutex<ListenTracker>>,
    log: Arc<Mutex<ListenLog>>,
    sender: mpsc::UnboundedSender<Msg>,
}

impl PlayReporting {
    /// Starts the reporter on the current Tokio runtime.
    pub fn spawn(queue_file: PathBuf, listens_file: PathBuf, enabled: bool) -> Self {
        Self::spawn_with(
            queue_file,
            listens_file,
            enabled,
            EVENT_BATCH_URL.to_string(),
        )
    }

    fn spawn_with(queue_file: PathBuf, listens_file: PathBuf, enabled: bool, url: String) -> Self {
        let log = Arc::new(Mutex::new(ListenLog::new(listens_file)));
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut queue = EventQueue::load(&queue_file);
        if !enabled {
            // Turned off while plays waited: they stay unsent.
            queue.clear();
            queue.save();
        }
        let core = Reporter {
            queue,
            log: Arc::clone(&log),
            enabled,
            client: None,
            url,
            network_hold_until: 0,
            network_failures: 0,
        };
        tokio::spawn(core.run(receiver));
        Self {
            tracker: Arc::default(),
            log,
            sender,
        }
    }

    /// Takes one engine report. Cheap; called on the engine's thread.
    pub fn observe(&self, state: &LocalState) {
        let finished = self
            .tracker
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .observe(state, now_ms());
        if let Some(listen) = finished {
            let _ = self.sender.send(Msg::Listen(Box::new(listen)));
        }
    }

    /// Ends the listen in progress, if any (sign-out, quit, engine gone).
    pub fn finish_listen(&self) {
        let finished = self
            .tracker
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .finish(now_ms());
        if let Some(listen) = finished {
            let _ = self.sender.send(Msg::Listen(Box::new(listen)));
        }
    }

    /// "Report plays to TIDAL". Turning it off drops what waits unsent.
    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.sender.send(Msg::Enabled(enabled));
    }

    /// The signed-in account's client, or `None` when signed out. Events
    /// are only ever sent with the token of the account they were played on.
    pub fn set_account(&self, client: Option<Arc<TidalClient>>) {
        let _ = self.sender.send(Msg::Account(client));
    }

    pub fn flush(&self) {
        let _ = self.sender.send(Msg::Flush);
    }

    /// Ends the listen in progress and sends what waits, for a few seconds
    /// at most.
    pub async fn shutdown(&self) {
        self.finish_listen();
        let (done, wait) = oneshot::channel();
        if self.sender.send(Msg::Shutdown(done)).is_ok() {
            let _ = tokio::time::timeout(SHUTDOWN_FLUSH + Duration::from_secs(1), wait).await;
        }
    }

    /// The local statistics for `query`. Blocking file work: run it off the
    /// async workers (`spawn_blocking`).
    pub fn stats_blocking(&self, query: &StatsQuery) -> Result<StatsReport, String> {
        let mut log = self.log.lock().unwrap_or_else(|lock| lock.into_inner());
        crate::play_stats::query(&mut log, query).map_err(|error| {
            log::warn!("unable to read the listening statistics: {error}");
            format!("Couldn't read the listening statistics: {error}")
        })
    }
}

struct Reporter {
    queue: EventQueue,
    log: Arc<Mutex<ListenLog>>,
    enabled: bool,
    client: Option<Arc<TidalClient>>,
    url: String,
    /// After a network failure, nothing is sent before this (Unix ms).
    network_hold_until: i64,
    network_failures: u32,
}

impl Reporter {
    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Msg>) {
        let mut tick = tokio::time::interval(FLUSH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                message = receiver.recv() => match message {
                    None => break,
                    Some(Msg::Listen(listen)) => self.on_listen(&listen, now_ms()),
                    Some(Msg::Enabled(enabled)) => {
                        self.enabled = enabled;
                        if !enabled {
                            self.queue.clear();
                            self.queue.save();
                        }
                    }
                    Some(Msg::Account(client)) => {
                        self.client = client;
                        self.network_hold_until = 0;
                    }
                    Some(Msg::Flush) => self.flush(now_ms()).await,
                    Some(Msg::Shutdown(done)) => {
                        self.network_hold_until = 0;
                        let _ = tokio::time::timeout(SHUTDOWN_FLUSH, self.flush(now_ms())).await;
                        self.queue.save();
                        let _ = done.send(());
                        break;
                    }
                },
                _ = tick.tick() => self.flush(now_ms()).await,
            }
        }
    }

    fn on_listen(&mut self, listen: &Listen, now_ms: i64) {
        let record = ListenRecord::from_listen(listen);
        if let Err(error) = self
            .log
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .append(&record)
        {
            log::warn!("unable to record a listen: {error}");
        }
        // How TIDAL's apps report a video play (its product type and
        // fields) is not known, so videos stay in the local statistics only.
        if !self.enabled || crate::tidal_engine::video::is_video_uri(&listen.track.uri) {
            return;
        }
        let Some(user_id) = self
            .client
            .as_ref()
            .and_then(|client| client.user_id().ok())
        else {
            return;
        };
        let id = uuid_v4();
        self.queue.push(Pending {
            body: event::queued_body(listen, &id, now_ms),
            id,
            name: event::NAME.to_string(),
            user_id,
            created_ms: now_ms,
            attempts: 0,
            next_ms: 0,
        });
        self.queue.save();
    }

    async fn flush(&mut self, now_ms: i64) {
        self.queue.prune(now_ms);
        if !self.enabled || now_ms < self.network_hold_until {
            self.queue.save();
            return;
        }
        let Some(client) = self.client.clone() else {
            self.queue.save();
            return;
        };
        let Ok(user_id) = client.user_id() else {
            return;
        };
        for _ in 0..BATCHES_PER_FLUSH {
            let batch = self.queue.due(user_id, now_ms, sqs::MAX_BATCH);
            if batch.is_empty() || !self.send(&client, &batch, now_ms).await {
                break;
            }
        }
        self.queue.save();
    }

    /// Sends one batch; `false` stops this flush.
    async fn send(&mut self, client: &TidalClient, batch: &[Pending], now_ms: i64) -> bool {
        let ids: Vec<String> = batch.iter().map(|event| event.id.clone()).collect();
        let client_id = client.oauth_client_id().await.unwrap_or_default();
        let reply = client
            .post_event_batch(&self.url, |token| {
                let claims = event::claims(token);
                let entries: Vec<sqs::Entry<'_>> = batch
                    .iter()
                    .map(|pending| sqs::Entry {
                        id: &pending.id,
                        name: &pending.name,
                        body: event::message_body(&pending.body, &claims),
                        headers: event::headers_attribute(token, &client_id, now_ms),
                    })
                    .collect();
                sqs::encode(&entries)
            })
            .await;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                let signed_out = matches!(
                    error,
                    ApiError::NotSignedIn | ApiError::SignInExpired { .. }
                );
                self.network_failures += 1;
                let wait = (FIRST_NETWORK_BACKOFF_MS
                    << self.network_failures.saturating_sub(1).min(10))
                .min(MAX_NETWORK_BACKOFF_MS);
                self.network_hold_until = now_ms + wait;
                log::debug!(
                    "play reports not sent ({}): {error}; next try in {} s",
                    if signed_out { "signed out" } else { "network" },
                    wait / 1000
                );
                return false;
            }
        };
        self.network_failures = 0;
        match sqs::judge(reply.status, &reply.body, &ids) {
            Ok(outcome) => {
                if !outcome.accepted.is_empty() {
                    log::info!("reported {} plays to TIDAL", outcome.accepted.len());
                }
                if !outcome.rejected.is_empty() {
                    log::warn!(
                        "TIDAL refused {} play reports as malformed; dropped",
                        outcome.rejected.len()
                    );
                }
                self.queue.remove(&outcome.accepted);
                self.queue.remove(&outcome.rejected);
                self.queue.failed(&outcome.retry, now_ms);
                outcome.retry.is_empty()
            }
            Err(sqs::Failure::Reject(reason)) => {
                log::warn!("TIDAL refused a batch of play reports ({reason}); dropped");
                self.queue.remove(&ids);
                true
            }
            Err(sqs::Failure::Retry(reason)) => {
                log::debug!("play reports not accepted yet ({reason}); will retry");
                self.queue.failed(&ids, now_ms);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::NetActivity;
    use crate::auth::tidal::TidalTokens;
    use crate::auth::tidal::tests::{FakeRefresher, grant};
    use crate::tidal_engine::LocalTrack;
    use listen::{Action, ActionType};
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn uuids_are_version_four() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(id, uuid_v4());
    }

    /// Answers each connection with the next reply and keeps the requests.
    async fn collector(replies: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/event-batch", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = vec![0u8; 64 * 1024];
                // Read the head, then the body by Content-Length.
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let length = text[..head_end]
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= head_end + 4 + length || read == 0 {
                            break;
                        }
                    }
                    if read == 0 {
                        break;
                    }
                }
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                socket.write_all(reply.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        (url, seen)
    }

    fn reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    fn listen(id: &str) -> Listen {
        Listen {
            session_id: uuid_v4(),
            track: LocalTrack {
                uri: format!("tidal:track:{id}"),
                duration_ms: 100_000,
                ..LocalTrack::default()
            },
            context_uri: Some("tidal:playlist:p".into()),
            quality: None,
            actions: vec![
                Action {
                    kind: ActionType::Start,
                    position_ms: 0,
                    timestamp_ms: 1,
                },
                Action {
                    kind: ActionType::Stop,
                    position_ms: 60_000,
                    timestamp_ms: 60_001,
                },
            ],
            listened_ms: 60_000,
            completed: false,
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tidalfast-reporter-{}-{}",
            std::process::id(),
            uuid_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn reporter(dir: &std::path::Path, url: String, enabled: bool) -> Reporter {
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let client = TidalClient::new(http, Arc::new(NetActivity::default()));
        let far = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let refresher = Arc::new(FakeRefresher {
            calls: AtomicUsize::new(0),
            fail: false,
        });
        client.set_tokens(Some(TidalTokens::new(
            grant("token-x", far),
            refresher,
            None,
        )));
        Reporter {
            queue: EventQueue::load(&dir.join("play-reports.json")),
            log: Arc::new(Mutex::new(ListenLog::new(dir.join("listens.jsonl")))),
            enabled,
            client: Some(Arc::new(client)),
            url,
            network_hold_until: 0,
            network_failures: 0,
        }
    }

    #[tokio::test]
    async fn listens_are_recorded_queued_sent_and_retried() {
        let dir = temp_dir();
        let (url, seen) = collector(vec![
            reply("503 Service Unavailable", ""),
            reply("200 OK", "<SendMessageBatchResponse/>"),
        ])
        .await;
        let mut reporter = reporter(&dir, url, true);
        for id in 0..12 {
            reporter.on_listen(&listen(&id.to_string()), 1_000);
        }
        assert_eq!(reporter.queue.len(), 12);
        assert_eq!(reporter.log.lock().unwrap().read().unwrap().len(), 12);
        // The collector is down: the first batch waits with a backoff.
        reporter.flush(2_000).await;
        assert_eq!(reporter.queue.len(), 12);
        assert_eq!(
            reporter
                .queue
                .events()
                .iter()
                .filter(|event| event.attempts == 1)
                .count(),
            10
        );
        // The ten held back are not due yet; the other two are.
        reporter.flush(3_000).await;
        assert_eq!(reporter.queue.len(), 10);
        let requests = seen.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer token-x")
        );
        assert!(requests[0].contains("SendMessageBatchRequestEntry.10.Id"));
        assert!(!requests[1].contains("SendMessageBatchRequestEntry.3.Id"));
        assert!(requests[1].contains("playback_session"));
        assert!(requests[1].contains("PLAYBACK_START"));
        // Survives a restart.
        let again = EventQueue::load(&dir.join("play-reports.json"));
        assert_eq!(again.len(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn malformed_events_are_dropped_and_off_means_nothing_queued() {
        let dir = temp_dir();
        let (url, _) = collector(vec![]).await;
        let mut reporter = reporter(&dir, url.clone(), true);
        reporter.on_listen(&listen("1"), 1_000);
        let id = reporter.queue.events()[0].id.clone();
        let xml = format!(
            "<SendMessageBatchResponse><SendMessageBatchResult><BatchResultErrorEntry><Id>{id}</Id><SenderFault>true</SenderFault></BatchResultErrorEntry></SendMessageBatchResult></SendMessageBatchResponse>"
        );
        let (url, _) = collector(vec![reply("200 OK", &xml)]).await;
        reporter.url = url;
        reporter.flush(2_000).await;
        assert!(reporter.queue.is_empty());

        let mut off = super::tests::reporter(&dir, String::new(), false);
        off.on_listen(&listen("2"), 1_000);
        assert!(off.queue.is_empty());
        // Still counted locally.
        assert_eq!(off.log.lock().unwrap().read().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_network_failure_holds_every_send_back() {
        let dir = temp_dir();
        // Nothing listens on this port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        let mut reporter = reporter(&dir, url, true);
        reporter.on_listen(&listen("1"), 1_000);
        reporter.flush(2_000).await;
        assert_eq!(reporter.queue.events()[0].attempts, 0);
        assert_eq!(
            reporter.network_hold_until,
            2_000 + FIRST_NETWORK_BACKOFF_MS
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
