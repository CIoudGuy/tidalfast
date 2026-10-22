//! What every catalogue request shares: errors, the play request handed to
//! the engine, and the network activity the interface shows.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;

use super::ApiSource;

#[derive(Clone, Debug, Error)]
pub enum ApiError {
    #[error("not signed in")]
    NotSignedIn,
    #[error("{message}")]
    Status { status: u16, message: String },
    #[error("TIDAL is rate limiting requests; try again in a moment")]
    RateLimited,
    #[error("TIDAL's request quota is exhausted; try again later")]
    QuotaExhausted,
    #[error("your TIDAL sign-in expired; please sign in again")]
    SignInExpired { api_source: ApiSource },
    /// A folder still holds items after its playlists were moved out, so it
    /// is not deleted.
    #[error("the folder still holds playlists")]
    FolderNotEmpty,
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response from TIDAL: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_network(&self) -> bool {
        matches!(self, Self::Network(_))
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_decode() {
            Self::Decode(error.to_string())
        } else {
            Self::Network(error.to_string())
        }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// What to start playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayRequest {
    pub context_uri: Option<String>,
    pub uris: Vec<String>,
    pub offset_uri: Option<String>,
    /// The clicked playlist row `offset_uri` is in, by its index among the
    /// playlist's entries: a song listed twice starts at that entry.
    pub offset_row: Option<u32>,
    pub offset_position: Option<u32>,
    pub position_ms: u32,
    /// A shuffled play order to resume: the context songs after the chosen
    /// one, as they were due to play. Songs missing from it follow shuffled.
    pub shuffle_order: Vec<String>,
    /// Songs to play instead when the context cannot be read again or no
    /// longer holds the chosen one: the chosen song and those after it.
    pub fallback_uris: Vec<String>,
}

impl PlayRequest {
    pub fn context(uri: impl Into<String>) -> Self {
        Self {
            context_uri: Some(uri.into()),
            ..Self::default()
        }
    }

    pub fn tracks(uris: Vec<String>) -> Self {
        Self {
            uris,
            ..Self::default()
        }
    }

    pub fn starting_at_uri(mut self, uri: impl Into<String>) -> Self {
        self.offset_uri = Some(uri.into());
        self
    }

    /// Starts at the playlist row `row`, which holds `uri`: that entry,
    /// also when the song is listed more than once.
    pub fn starting_at_row(mut self, uri: impl Into<String>, row: u32) -> Self {
        self.offset_uri = Some(uri.into());
        self.offset_row = Some(row);
        self
    }

    pub fn starting_at_index(mut self, index: u32) -> Self {
        self.offset_position = Some(index);
        self
    }
}

/// Live view of the client's traffic, shared with the interface so it can
/// show that the app is talking to TIDAL rather than being slow itself.
pub struct NetActivity {
    started_at: Instant,
    in_flight: AtomicUsize,
    /// Milliseconds since `started_at` when the oldest current burst began.
    busy_since_ms: AtomicU64,
}

impl Default for NetActivity {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            in_flight: AtomicUsize::new(0),
            busy_since_ms: AtomicU64::new(0),
        }
    }
}

impl NetActivity {
    fn now_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    fn begin(&self) {
        if self.in_flight.fetch_add(1, Ordering::SeqCst) == 0 {
            self.busy_since_ms.store(self.now_ms(), Ordering::SeqCst);
        }
    }

    fn end(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }

    /// Requests have been in flight continuously for at least `for_at_least`.
    pub fn busy(&self, for_at_least: Duration) -> bool {
        self.in_flight.load(Ordering::SeqCst) > 0
            && self
                .now_ms()
                .saturating_sub(self.busy_since_ms.load(Ordering::SeqCst))
                >= for_at_least.as_millis() as u64
    }
}

tokio::task_local! {
    static UNSEEN: ();
}

/// Runs requests nothing on screen waits for without showing them as
/// activity. Checking a grant in the background retries through rate
/// limits for as long as TIDAL keeps refusing, and would otherwise hold
/// the "Waiting for TIDAL" spinner up over an app that is ready to use.
pub async fn unseen<F: std::future::Future>(future: F) -> F::Output {
    UNSEEN.scope((), future).await
}

/// Decrements the in-flight count even if the request future is dropped.
pub(crate) struct ActivityGuard<'a>(&'a NetActivity);

impl<'a> ActivityGuard<'a> {
    /// Counts a request as activity, unless it runs [`unseen`].
    pub(crate) fn begin(activity: &'a NetActivity) -> Option<Self> {
        if UNSEEN.try_with(|_| ()).is_ok() {
            return None;
        }
        activity.begin();
        Some(Self(activity))
    }
}

impl Drop for ActivityGuard<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}
