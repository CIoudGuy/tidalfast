//! The transport for TIDAL's event producer (play reporting).
//!
//! Events go to `ec.tidal.com`, not the API hosts, as an SQS
//! `SendMessageBatch` form (docs/_reference/tidal-activity.md, 2.2). The
//! form carries the access token inside each event's headers attribute, so
//! the caller builds it from the token handed to `build`; nothing here logs
//! the form or the token.

use std::time::Duration;

use super::{TidalClient, auth_error};
use crate::api::client::{ActivityGuard, ApiError, Result};

pub const EVENT_BATCH_URL: &str = "https://ec.tidal.com/api/event-batch";
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the event collector answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventReply {
    pub status: u16,
    pub body: String,
}

impl TidalClient {
    /// Posts one event batch to `url`. `build` gets the access token and
    /// returns the form fields. A 401 refreshes the token once and builds
    /// the form again. Network failures are errors; any answer, whatever its
    /// status, is a reply for the caller to judge.
    pub async fn post_event_batch(
        &self,
        url: &str,
        build: impl Fn(&str) -> Vec<(String, String)>,
    ) -> Result<EventReply> {
        let tokens = self.tokens()?;
        let http = self.http.client().map_err(ApiError::Network)?;
        let _activity = ActivityGuard::begin(&self.activity);
        let mut refreshed = false;
        loop {
            let token = tokens.access_token(None).await.map_err(auth_error)?;
            let form = build(&token);
            let response = http
                .post(url)
                .bearer_auth(&token)
                .form(&form)
                .timeout(TIMEOUT)
                .send()
                .await
                .map_err(|error| ApiError::Network(error.without_url().to_string()))?;
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            if status == 401 && !refreshed {
                refreshed = true;
                log::debug!("event collector refused the token; refreshing once");
                tokens
                    .access_token(Some(&token))
                    .await
                    .map_err(auth_error)?;
                continue;
            }
            return Ok(EventReply { status, body });
        }
    }

    /// The OAuth client the signed-in grant belongs to, for the events'
    /// `client-id` header. Never log it.
    pub async fn oauth_client_id(&self) -> Result<String> {
        Ok(self.tokens()?.client_id().await)
    }
}
