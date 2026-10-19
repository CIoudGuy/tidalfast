//! TIDAL sign-in: the device-code flow and the browser (PKCE) flow, and the
//! grant they produce, refreshed as it ages.
//!
//! The OAuth client identities come from settings (`tidal_client_id`,
//! `tidal_client_secret`, `tidal_pkce_client_id`) and are never written into
//! the source. Tokens never reach a log: [`TidalGrant`] has no `Debug`, and
//! errors carry only classifications, never response text from the token
//! endpoint.
//!
//! - **Device code**: TIDAL shows a short code; the user approves it at
//!   `link.tidal.com` in any browser while this process polls. TIDAL caps
//!   these clients at 16-bit FLAC.
//! - **PKCE**: the user signs in at `login.tidal.com`, lands on a TIDAL page
//!   whose address carries the one-time code, and pastes that address back.
//!   The redirect belongs to TIDAL, so no loopback listener can catch it.
//!   This client class is served hi-res FLAC.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::http::Http;

pub const AUTH_BASE: &str = "https://auth.tidal.com/v1/oauth2";
pub const LOGIN_URL: &str = "https://login.tidal.com/authorize";
/// Where TIDAL sends the browser after a PKCE sign-in. It is TIDAL's own
/// page, so the user copies its address back into the app.
pub const PKCE_REDIRECT_URI: &str = "https://tidal.com/android/login/auth";
pub const SCOPES: &str = "r_usr w_usr w_sub";
/// Refresh this long before the access token expires.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Which sign-in minted a grant. A token must be refreshed with the client
/// that minted it, so the flow is stored beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthFlow {
    #[default]
    Device,
    Pkce,
}

/// An OAuth client identity, from settings.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ClientConfig {
    pub client_id: String,
    /// Empty for public (PKCE) clients.
    pub client_secret: String,
}

/// The public TIDAL clients open-source players sign in with (the same ones
/// python-tidal, Mopidy-Tidal and others ship), so signing in needs nothing
/// but the account, as Tidalfast does with TIDAL's desktop client. Kept in
/// python-tidal's doubly base64-encoded form; settings can override them.
const BUILTIN_DEVICE_ID: [&str; 2] = ["WmxneVNuaGtiVzUw", "V2xkTE1HbDRWQT09"];
const BUILTIN_DEVICE_SECRET: [&str; 2] = [
    "TVU1dU9VRm1SRUZxZUhKblNrWktZa3RPVjB4bFFY",
    "bExSMVpIYlVsT2RWaFFVRXhJVmxoQmRuaEJaejA9",
];
const BUILTIN_PKCE_ID: [&str; 2] = ["TmtKRVUxSmtjRXM=", "NWFIRkZRbFJuVlE9PQ=="];

fn builtin(parts: [&str; 2]) -> String {
    use base64::engine::general_purpose::STANDARD;
    let mut inner = Vec::new();
    for part in parts {
        inner.extend(STANDARD.decode(part).unwrap_or_default());
    }
    STANDARD
        .decode(inner)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

impl ClientConfig {
    /// The device-flow client: the one in settings, else the built-in one.
    pub fn device(settings: &crate::settings::Settings) -> Result<Self, AuthError> {
        let client_id = settings.tidal_client_id.trim();
        if client_id.is_empty() {
            return Ok(Self {
                client_id: builtin(BUILTIN_DEVICE_ID),
                client_secret: builtin(BUILTIN_DEVICE_SECRET),
            });
        }
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: settings.tidal_client_secret.trim().to_string(),
        })
    }

    /// The PKCE client: the one in settings, else the built-in one.
    pub fn pkce(settings: &crate::settings::Settings) -> Result<Self, AuthError> {
        let client_id = settings.tidal_pkce_client_id.trim();
        let client_id = if client_id.is_empty() {
            builtin(BUILTIN_PKCE_ID)
        } else {
            client_id.to_string()
        };
        Ok(Self {
            client_id,
            client_secret: String::new(),
        })
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("client_id", &"<set>")
            .field("client_secret", &!self.client_secret.is_empty())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("{0}")]
    NotConfigured(&'static str),
    /// TIDAL refused the grant; only a new sign-in can mint one.
    #[error("TIDAL rejected the sign-in: {0}")]
    Rejected(&'static str),
    /// The device code ran out before the user approved it.
    #[error("The sign-in code expired before it was approved. Start again.")]
    Expired,
    #[error("Sign-in cancelled.")]
    Cancelled,
    #[error(
        "That address has no sign-in code in it. Paste the whole address TIDAL showed after you signed in."
    )]
    NoCode,
    /// The endpoint could not answer; a stored grant may still be valid.
    #[error("TIDAL sign-in could not be reached: {0}")]
    Unreachable(String),
}

impl AuthError {
    /// Whether the stored grant is dead and the user must sign in again.
    pub fn needs_sign_in(&self) -> bool {
        matches!(self, Self::Rejected(_) | Self::NotConfigured(_))
    }
}

/// The TIDAL grant, serialized only inside protected credential storage.
/// Deliberately has no `Debug` implementation: it contains usable secrets.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TidalGrant {
    pub flow: AuthFlow,
    pub client_id: String,
    /// The secret of `client_id` at sign-in time, needed to refresh. Empty
    /// for PKCE.
    #[serde(default)]
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds; 0 when TIDAL did not say.
    pub expires_at: u64,
    pub user_id: u64,
    /// The account's store country, sent with every catalogue request.
    #[serde(default)]
    pub country_code: String,
    /// The PKCE `client_unique_key`, kept for the refresh.
    #[serde(default)]
    pub client_unique_key: String,
    #[serde(default)]
    pub scope: String,
}

impl TidalGrant {
    pub fn needs_refresh(&self) -> bool {
        self.expires_at != 0 && now_secs() + REFRESH_MARGIN.as_secs() >= self.expires_at
    }

    pub fn valid(&self) -> bool {
        !self.client_id.is_empty() && !self.refresh_token.is_empty() && self.user_id != 0
    }

    fn client(&self) -> ClientConfig {
        ClientConfig {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
        }
    }

    /// A grant from a token response. A refresh answer usually omits the
    /// refresh token and the user; `previous` supplies them.
    fn from_response(
        flow: AuthFlow,
        client: &ClientConfig,
        response: TokenResponse,
        previous: Option<&TidalGrant>,
    ) -> Result<Self, AuthError> {
        let refresh_token = response
            .refresh_token
            .filter(|token| !token.is_empty())
            .or_else(|| previous.map(|grant| grant.refresh_token.clone()))
            .ok_or(AuthError::Rejected("TIDAL returned no refresh token."))?;
        let user_id = response
            .user_id
            .or(response.user.as_ref().and_then(|user| user.user_id))
            .or(previous.map(|grant| grant.user_id))
            .unwrap_or(0);
        let country_code = response
            .user
            .as_ref()
            .and_then(|user| user.country_code.clone())
            .filter(|code| !code.is_empty())
            .or_else(|| previous.map(|grant| grant.country_code.clone()))
            .unwrap_or_default();
        Ok(Self {
            flow,
            client_id: client.client_id.clone(),
            client_secret: client.client_secret.clone(),
            access_token: response.access_token,
            refresh_token,
            expires_at: response
                .expires_in
                .map_or(0, |seconds| now_secs() + seconds),
            user_id,
            country_code,
            client_unique_key: previous
                .map(|grant| grant.client_unique_key.clone())
                .unwrap_or_default(),
            scope: response
                .scope
                .or_else(|| previous.map(|grant| grant.scope.clone()))
                .unwrap_or_default(),
        })
    }
}

#[derive(Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    user_id: Option<u64>,
    #[serde(default)]
    user: Option<TokenUser>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenUser {
    #[serde(default)]
    user_id: Option<u64>,
    #[serde(default)]
    country_code: Option<String>,
}

/// What TIDAL's token endpoint said, without the parts that could hold
/// secrets.
#[derive(Debug, PartialEq, Eq)]
enum TokenAnswer {
    Granted,
    Pending,
    Expired,
    /// The device poll was too fast; wait longer between requests.
    Slow,
    /// Too many requests (HTTP 429).
    RateLimited,
    Rejected(&'static str),
    Unreachable(String),
}

const TOO_MANY: &str = "too many sign-in attempts. Wait a minute, then try again.";

fn classify(status: u16, body: &str) -> TokenAnswer {
    if (200..300).contains(&status) {
        return TokenAnswer::Granted;
    }
    let error = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|e| e.as_str())
                .map(str::to_owned)
        });
    match (status, error.as_deref()) {
        (429, _) => TokenAnswer::RateLimited,
        (_, Some("authorization_pending")) => TokenAnswer::Pending,
        (_, Some("expired_token")) => TokenAnswer::Expired,
        (_, Some("invalid_grant")) => {
            TokenAnswer::Rejected("the sign-in expired or was revoked. Sign in again.")
        }
        (_, Some("invalid_client" | "unauthorized_client")) => {
            TokenAnswer::Rejected("check the TIDAL client ID and secret in Settings.")
        }
        (_, Some("access_denied")) => TokenAnswer::Rejected("the sign-in was declined."),
        (_, Some("slow_down")) => TokenAnswer::Slow,
        // A request that timed out says nothing about the grant either.
        (408, _) => TokenAnswer::Unreachable(format!("TIDAL answered HTTP {status}")),
        (400..=499, _) => TokenAnswer::Rejected("sign in again."),
        _ => TokenAnswer::Unreachable(format!("TIDAL answered HTTP {status}")),
    }
}

async fn token_request(
    http: &Http,
    url: &str,
    client: &ClientConfig,
    form: &[(&str, &str)],
) -> Result<(TokenAnswer, String), AuthError> {
    let http = http.client().map_err(AuthError::Unreachable)?;
    let mut form: Vec<(&str, &str)> = form.to_vec();
    form.push(("client_id", client.client_id.as_str()));
    if !client.client_secret.is_empty() {
        form.push(("client_secret", client.client_secret.as_str()));
    }
    let response = http
        .post(url)
        .form(&form)
        .send()
        .await
        .map_err(|error| AuthError::Unreachable(error.without_url().to_string()))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    Ok((classify(status, &text), text))
}

fn decode_grant(
    answer: TokenAnswer,
    text: &str,
    flow: AuthFlow,
    client: &ClientConfig,
    previous: Option<&TidalGrant>,
) -> Result<Option<TidalGrant>, AuthError> {
    match answer {
        TokenAnswer::Granted => {
            let response: TokenResponse = serde_json::from_str(text).map_err(|_| {
                AuthError::Unreachable("TIDAL returned an unreadable token response".into())
            })?;
            TidalGrant::from_response(flow, client, response, previous).map(Some)
        }
        TokenAnswer::Pending | TokenAnswer::Slow => Ok(None),
        TokenAnswer::RateLimited => Err(AuthError::Rejected(TOO_MANY)),
        TokenAnswer::Expired => Err(AuthError::Expired),
        TokenAnswer::Rejected(why) => Err(AuthError::Rejected(why)),
        TokenAnswer::Unreachable(why) => Err(AuthError::Unreachable(why)),
    }
}

// ---- device code ------------------------------------------------------------

/// A started device sign-in: show `user_code`, open `verification_uri_complete`.
#[derive(Clone, Debug)]
pub struct DeviceCode {
    device_code: String,
    pub user_code: String,
    /// With scheme, ready to open.
    pub verification_uri: String,
    /// With scheme and the code filled in, ready to open.
    pub verification_uri_complete: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    #[serde(default)]
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

fn with_scheme(uri: &str) -> String {
    if uri.is_empty() || uri.starts_with("http://") || uri.starts_with("https://") {
        uri.to_string()
    } else {
        format!("https://{uri}")
    }
}

fn decode_device(text: &str) -> Result<DeviceCode, AuthError> {
    let answer: DeviceAuthorization = serde_json::from_str(text)
        .map_err(|_| AuthError::Unreachable("TIDAL returned an unreadable sign-in code".into()))?;
    let verification_uri = with_scheme(&answer.verification_uri);
    let complete = if answer.verification_uri_complete.is_empty() {
        format!(
            "{}/{}",
            verification_uri.trim_end_matches('/'),
            answer.user_code
        )
    } else {
        with_scheme(&answer.verification_uri_complete)
    };
    Ok(DeviceCode {
        device_code: answer.device_code,
        user_code: answer.user_code,
        verification_uri,
        verification_uri_complete: complete,
        expires_in: Duration::from_secs(answer.expires_in.unwrap_or(300)),
        interval: Duration::from_secs(answer.interval.unwrap_or(2).max(1)),
    })
}

/// Asks TIDAL for a device code.
pub async fn device_start(http: &Http, client: &ClientConfig) -> Result<DeviceCode, AuthError> {
    device_start_at(AUTH_BASE, http, client).await
}

pub(crate) async fn device_start_at(
    base: &str,
    http: &Http,
    client: &ClientConfig,
) -> Result<DeviceCode, AuthError> {
    let (answer, text) = token_request(
        http,
        &format!("{base}/device_authorization"),
        &ClientConfig {
            // The secret is not needed to start, and some clients reject it.
            client_id: client.client_id.clone(),
            client_secret: String::new(),
        },
        &[("scope", SCOPES)],
    )
    .await?;
    match answer {
        TokenAnswer::Granted => decode_device(&text),
        TokenAnswer::Rejected(why) => Err(AuthError::Rejected(why)),
        TokenAnswer::RateLimited | TokenAnswer::Slow => Err(AuthError::Rejected(TOO_MANY)),
        TokenAnswer::Unreachable(why) => Err(AuthError::Unreachable(why)),
        TokenAnswer::Pending | TokenAnswer::Expired => Err(AuthError::Unreachable(
            "unexpected answer to a device code request".into(),
        )),
    }
}

/// Polls until the user approves `code`, it expires, or `cancel` turns true.
/// Network failures while polling are retried until the code expires.
pub async fn device_poll(
    http: &Http,
    client: &ClientConfig,
    code: &DeviceCode,
    cancel: watch::Receiver<bool>,
) -> Result<TidalGrant, AuthError> {
    device_poll_at(AUTH_BASE, http, client, code, cancel).await
}

pub(crate) async fn device_poll_at(
    base: &str,
    http: &Http,
    client: &ClientConfig,
    code: &DeviceCode,
    mut cancel: watch::Receiver<bool>,
) -> Result<TidalGrant, AuthError> {
    let deadline = tokio::time::Instant::now() + code.expires_in;
    let mut interval = code.interval;
    loop {
        if *cancel.borrow() {
            return Err(AuthError::Cancelled);
        }
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Err(AuthError::Cancelled);
                }
                continue;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::Expired);
        }
        let polled = token_request(
            http,
            &format!("{base}/token"),
            client,
            &[
                ("device_code", code.device_code.as_str()),
                ("grant_type", DEVICE_GRANT),
                ("scope", SCOPES),
            ],
        )
        .await;
        let (answer, text) = match polled {
            Ok(polled) => polled,
            Err(AuthError::Unreachable(why)) => {
                log::debug!("TIDAL device poll failed, retrying: {why}");
                continue;
            }
            Err(error) => return Err(error),
        };
        match &answer {
            TokenAnswer::Unreachable(why) => {
                log::debug!("TIDAL device poll failed, retrying: {why}");
                continue;
            }
            // RFC 8628: back off by five seconds on slow_down; do the same
            // when rate limited, rather than failing the sign-in.
            TokenAnswer::Slow | TokenAnswer::RateLimited => {
                interval += Duration::from_secs(5);
                continue;
            }
            _ => {}
        }
        if let Some(grant) = decode_grant(answer, &text, AuthFlow::Device, client, None)? {
            return Ok(grant);
        }
    }
}

/// Resolves once `cancel` is true or its sender is gone.
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() || cancel.changed().await.is_err() {
            return;
        }
    }
}

/// How many times an expired code is swapped for a fresh one before the
/// user is asked to start again.
const CODE_REFRESHES: u32 = 3;

/// The whole device sign-in: wait `wait` (so rapid start/cancel clicks
/// reach TIDAL as one request), ask for a code, hand it to `on_code`
/// (`true` when it replaces an expired one), then poll. An expired code is
/// replaced up to [`CODE_REFRESHES`] times. Every step ends at once when
/// `cancel` turns true, dropping any request in flight.
pub async fn device_sign_in(
    base: &str,
    http: &Http,
    client: &ClientConfig,
    wait: Duration,
    cancel: watch::Receiver<bool>,
    mut on_code: impl FnMut(&DeviceCode, bool),
) -> Result<TidalGrant, AuthError> {
    let mut gate = cancel.clone();
    tokio::select! {
        _ = tokio::time::sleep(wait) => {}
        _ = cancelled(&mut gate) => return Err(AuthError::Cancelled),
    }
    let mut refreshed = 0;
    loop {
        let code = tokio::select! {
            code = device_start_at(base, http, client) => code?,
            _ = cancelled(&mut gate) => return Err(AuthError::Cancelled),
        };
        on_code(&code, refreshed > 0);
        match device_poll_at(base, http, client, &code, cancel.clone()).await {
            Err(AuthError::Expired) if refreshed < CODE_REFRESHES => refreshed += 1,
            other => return other,
        }
    }
}

// ---- PKCE ---------------------------------------------------------------------

/// A started browser sign-in: open `url`, then hand the pasted address to
/// [`pkce_finish`] with this flow.
#[derive(Clone)]
pub struct PkceFlow {
    pub url: String,
    verifier: String,
    client_unique_key: String,
}

impl std::fmt::Debug for PkceFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PkceFlow").finish_non_exhaustive()
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buffer = [0u8; N];
    rand::rng().fill_bytes(&mut buffer);
    buffer
}

/// A 43-character base64url verifier and its S256 challenge.
pub fn pkce_pair() -> (String, String) {
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes::<32>());
    let challenge = pkce_challenge(&verifier);
    (verifier, challenge)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn pkce_begin(client: &ClientConfig) -> PkceFlow {
    let (verifier, challenge) = pkce_pair();
    let client_unique_key = format!("{:016x}", u64::from_le_bytes(random_bytes::<8>()));
    let url = format!(
        "{LOGIN_URL}?response_type=code&redirect_uri={}&client_id={}&lang=EN&appMode=android&client_unique_key={client_unique_key}&code_challenge={challenge}&code_challenge_method=S256&restrict_signup=true",
        urlencoding::encode(PKCE_REDIRECT_URI),
        urlencoding::encode(&client.client_id),
    );
    PkceFlow {
        url,
        verifier,
        client_unique_key,
    }
}

/// The authorization code in the address the user pasted (or the bare code).
pub fn parse_redirect(pasted: &str) -> Result<String, AuthError> {
    let pasted = pasted.trim();
    if pasted.is_empty() {
        return Err(AuthError::NoCode);
    }
    let Some((_, query)) = pasted.split_once('?') else {
        // A bare code, copied on its own.
        if pasted.contains(['/', ':', ' ', '&', '=']) {
            return Err(AuthError::NoCode);
        }
        return Ok(pasted.to_string());
    };
    let query = query.split('#').next().unwrap_or_default();
    let mut code = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" if !value.is_empty() => {
                code = Some(
                    urlencoding::decode(value)
                        .map(|value| value.into_owned())
                        .unwrap_or_else(|_| value.to_string()),
                )
            }
            "error" => return Err(AuthError::Rejected("the sign-in was declined.")),
            _ => {}
        }
    }
    code.ok_or(AuthError::NoCode)
}

/// Exchanges the pasted redirect for a grant.
pub async fn pkce_finish(
    http: &Http,
    client: &ClientConfig,
    flow: &PkceFlow,
    pasted: &str,
) -> Result<TidalGrant, AuthError> {
    let code = parse_redirect(pasted)?;
    let (answer, text) = token_request(
        http,
        &format!("{AUTH_BASE}/token"),
        client,
        &[
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", PKCE_REDIRECT_URI),
            ("scope", SCOPES),
            ("code_verifier", flow.verifier.as_str()),
            ("client_unique_key", flow.client_unique_key.as_str()),
        ],
    )
    .await?;
    let mut grant = decode_grant(answer, &text, AuthFlow::Pkce, client, None)?
        .ok_or(AuthError::Rejected("sign in again."))?;
    grant.client_unique_key = flow.client_unique_key.clone();
    Ok(grant)
}

// ---- refresh ------------------------------------------------------------------

/// Mints a new access token for a grant. A trait so tests can count and
/// script refreshes without a network.
pub trait Refresher: Send + Sync {
    fn refresh<'a>(
        &'a self,
        grant: &'a TidalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<TidalGrant, AuthError>> + Send + 'a>>;
}

/// Refreshes against TIDAL's token endpoint.
pub struct HttpRefresher {
    pub http: Http,
}

impl Refresher for HttpRefresher {
    fn refresh<'a>(
        &'a self,
        grant: &'a TidalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<TidalGrant, AuthError>> + Send + 'a>> {
        Box::pin(refresh(&self.http, grant))
    }
}

pub async fn refresh(http: &Http, grant: &TidalGrant) -> Result<TidalGrant, AuthError> {
    refresh_at(AUTH_BASE, http, grant).await
}

pub(crate) async fn refresh_at(
    base: &str,
    http: &Http,
    grant: &TidalGrant,
) -> Result<TidalGrant, AuthError> {
    let client = grant.client();
    let (answer, text) = token_request(
        http,
        &format!("{base}/token"),
        &client,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", grant.refresh_token.as_str()),
            ("scope", SCOPES),
        ],
    )
    .await?;
    if matches!(answer, TokenAnswer::RateLimited | TokenAnswer::Slow) {
        // A busy token endpoint says nothing about the grant. Treating it
        // as a refusal signed the listener out and deleted a good grant.
        return Err(AuthError::Unreachable(
            "TIDAL is rate limiting sign-in requests".into(),
        ));
    }
    decode_grant(answer, &text, grant.flow, &client, Some(grant))?
        .ok_or(AuthError::Rejected("sign in again."))
}

/// Ends the session at TIDAL. Best effort: the local grant is deleted
/// whatever this answers.
pub async fn logout(http: &Http, access_token: &str) {
    let Ok(http) = http.client() else {
        return;
    };
    let _ = http
        .post("https://api.tidal.com/v1/logout")
        .bearer_auth(access_token)
        .send()
        .await;
}

/// Called with the refreshed grant so it can be persisted.
pub type Persist = Arc<dyn Fn(TidalGrant) + Send + Sync>;

/// The live grant behind every request: hands out the access token,
/// refreshing it once when it nears expiry or TIDAL rejects it.
pub struct TidalTokens {
    grant: tokio::sync::Mutex<TidalGrant>,
    refresher: Arc<dyn Refresher>,
    persist: Option<Persist>,
    user_id: u64,
    country_code: std::sync::RwLock<String>,
}

impl TidalTokens {
    pub fn new(
        grant: TidalGrant,
        refresher: Arc<dyn Refresher>,
        persist: Option<Persist>,
    ) -> Arc<Self> {
        Arc::new(Self {
            user_id: grant.user_id,
            country_code: std::sync::RwLock::new(grant.country_code.clone()),
            grant: tokio::sync::Mutex::new(grant),
            refresher,
            persist,
        })
    }

    /// Refreshes against TIDAL, and saves each refreshed grant to `lease`.
    pub fn with_http(
        http: Http,
        grant: TidalGrant,
        lease: Option<crate::credentials::Lease>,
        storage_error: Arc<dyn Fn(crate::credentials::Error) + Send + Sync>,
    ) -> Arc<Self> {
        let persist: Option<Persist> = lease.map(|lease| {
            Arc::new(move |grant: TidalGrant| {
                let pending = lease.save(crate::credentials::Grant::Tidal(grant));
                let storage_error = storage_error.clone();
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move {
                        if let Err(error) = pending.await {
                            storage_error(error);
                        }
                    });
                }
            }) as Persist
        });
        Self::new(grant, Arc::new(HttpRefresher { http }), persist)
    }

    pub fn user_id(&self) -> u64 {
        self.user_id
    }

    pub fn country_code(&self) -> String {
        self.country_code
            .read()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone()
    }

    /// Records the store country learned from `sessions`.
    pub async fn set_country_code(&self, code: &str) {
        *self
            .country_code
            .write()
            .unwrap_or_else(|lock| lock.into_inner()) = code.to_string();
        let mut grant = self.grant.lock().await;
        if grant.country_code != code {
            grant.country_code = code.to_string();
            if let Some(persist) = &self.persist {
                persist(grant.clone());
            }
        }
    }

    /// The OAuth client the grant was minted for. Never log it.
    pub async fn client_id(&self) -> String {
        self.grant.lock().await.client_id.clone()
    }

    /// The current grant, for persistence after sign-in.
    pub async fn snapshot(&self) -> TidalGrant {
        self.grant.lock().await.clone()
    }

    /// A usable access token. `rejected` is a token TIDAL just refused as
    /// expired: it is refreshed unless another caller already replaced it,
    /// so a burst of 401s costs one refresh.
    pub async fn access_token(&self, rejected: Option<&str>) -> Result<String, AuthError> {
        let mut grant = self.grant.lock().await;
        let refused = rejected.is_some_and(|token| token == grant.access_token);
        if !refused && !grant.needs_refresh() && !grant.access_token.is_empty() {
            return Ok(grant.access_token.clone());
        }
        let refreshed = self.refresher.refresh(&grant).await?;
        *grant = refreshed;
        if let Some(persist) = &self.persist {
            persist(grant.clone());
        }
        Ok(grant.access_token.clone())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) fn grant(access: &str, expires_at: u64) -> TidalGrant {
        TidalGrant {
            flow: AuthFlow::Device,
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
            access_token: access.into(),
            refresh_token: "test-refresh".into(),
            expires_at,
            user_id: 42,
            country_code: "US".into(),
            ..Default::default()
        }
    }

    /// Hands out `fresh-1`, `fresh-2`, ... and counts calls.
    pub(crate) struct FakeRefresher {
        pub calls: AtomicUsize,
        pub fail: bool,
    }

    impl Refresher for FakeRefresher {
        fn refresh<'a>(
            &'a self,
            grant: &'a TidalGrant,
        ) -> Pin<Box<dyn Future<Output = Result<TidalGrant, AuthError>> + Send + 'a>> {
            Box::pin(async move {
                let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::task::yield_now().await;
                if self.fail {
                    return Err(AuthError::Rejected("sign in again."));
                }
                let mut next = grant.clone();
                next.access_token = format!("fresh-{n}");
                next.expires_at = now_secs() + 3600;
                Ok(next)
            })
        }
    }

    fn fake(fail: bool) -> Arc<FakeRefresher> {
        Arc::new(FakeRefresher {
            calls: AtomicUsize::new(0),
            fail,
        })
    }

    #[tokio::test]
    async fn a_fresh_token_is_used_as_is() {
        let refresher = fake(false);
        let tokens = TidalTokens::new(grant("old", now_secs() + 3600), refresher.clone(), None);
        assert_eq!(tokens.access_token(None).await.unwrap(), "old");
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_token_near_expiry_is_refreshed_and_persisted() {
        let refresher = fake(false);
        let saved = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = saved.clone();
        let persist: Persist = Arc::new(move |grant: TidalGrant| {
            sink.lock().unwrap().push(grant.access_token);
        });
        let tokens = TidalTokens::new(
            grant("old", now_secs() + 60),
            refresher.clone(),
            Some(persist),
        );
        assert_eq!(tokens.access_token(None).await.unwrap(), "fresh-1");
        assert_eq!(tokens.access_token(None).await.unwrap(), "fresh-1");
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*saved.lock().unwrap(), vec!["fresh-1".to_string()]);
        // The refresh token and identity survive a refresh.
        let snapshot = tokens.snapshot().await;
        assert_eq!(snapshot.refresh_token, "test-refresh");
        assert_eq!(snapshot.user_id, 42);
    }

    #[tokio::test]
    async fn a_burst_of_rejections_costs_one_refresh() {
        let refresher = fake(false);
        let tokens = TidalTokens::new(grant("old", now_secs() + 3600), refresher.clone(), None);
        let (a, b, c) = tokio::join!(
            tokens.access_token(Some("old")),
            tokens.access_token(Some("old")),
            tokens.access_token(Some("old")),
        );
        assert_eq!(
            (a.unwrap(), b.unwrap(), c.unwrap()),
            ("fresh-1".into(), "fresh-1".into(), "fresh-1".into())
        );
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        // A later rejection of the new token refreshes again.
        assert_eq!(
            tokens.access_token(Some("fresh-1")).await.unwrap(),
            "fresh-2"
        );
    }

    #[tokio::test]
    async fn a_refused_refresh_asks_for_sign_in() {
        let tokens = TidalTokens::new(grant("old", 1), fake(true), None);
        let error = tokens.access_token(None).await.unwrap_err();
        assert!(error.needs_sign_in());
    }

    #[test]
    fn token_endpoint_answers_are_classified_without_their_text() {
        assert_eq!(classify(200, "{}"), TokenAnswer::Granted);
        assert_eq!(
            classify(
                400,
                r#"{"status":400,"error":"authorization_pending","sub_status":1002,"error_description":"x"}"#
            ),
            TokenAnswer::Pending
        );
        assert_eq!(
            classify(400, r#"{"error":"expired_token"}"#),
            TokenAnswer::Expired
        );
        assert!(
            matches!(classify(400, r#"{"error":"invalid_grant","error_description":"secret-ish"}"#), TokenAnswer::Rejected(why) if !why.contains("secret"))
        );
        assert!(matches!(classify(503, ""), TokenAnswer::Unreachable(_)));
        assert_eq!(classify(429, ""), TokenAnswer::RateLimited);
        assert_eq!(classify(400, r#"{"error":"slow_down"}"#), TokenAnswer::Slow);
    }

    /// A scripted auth server on a local port: each path answers its
    /// (status, body) list in order, repeating the last. Returns the base
    /// URL and the paths requested so far.
    pub(crate) async fn fake_server(
        routes: Vec<(&'static str, Vec<(u16, String)>)>,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = log.clone();
        tokio::spawn(async move {
            let mut counts = std::collections::HashMap::<&str, usize>::new();
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0u8; 8192];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                seen.lock().unwrap().push(path.clone());
                let (status, body) = routes
                    .iter()
                    .find(|(route, _)| path.ends_with(route))
                    .map(|(route, answers)| {
                        let count = counts.entry(route).or_default();
                        let answer = answers[(*count).min(answers.len() - 1)].clone();
                        *count += 1;
                        answer
                    })
                    .unwrap_or((404, "{}".into()));
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (base, log)
    }

    fn test_client() -> ClientConfig {
        ClientConfig {
            client_id: "cid".into(),
            client_secret: "cs".into(),
        }
    }

    pub(crate) const DEVICE_JSON: &str = r#"{"deviceCode":"d1","userCode":"ABCDE","verificationUri":"link.tidal.com","verificationUriComplete":"link.tidal.com/ABCDE","expiresIn":300,"interval":1}"#;
    pub(crate) const GRANT_JSON: &str = r#"{"access_token":"a","refresh_token":"r","expires_in":3600,"user_id":1,"user":{"userId":1,"countryCode":"NO"}}"#;

    #[tokio::test]
    async fn a_rate_limited_start_says_so_plainly() {
        let (base, _) =
            fake_server(vec![("/device_authorization", vec![(429, "{}".into())])]).await;
        let http = Http::new(reqwest::Client::new());
        let error = device_start_at(&base, &http, &test_client())
            .await
            .unwrap_err();
        assert_eq!(error, AuthError::Rejected(TOO_MANY));
    }

    /// A rate-limited or timed-out refresh keeps the grant: only TIDAL
    /// refusing the grant itself asks for a new sign-in.
    #[tokio::test]
    async fn a_busy_token_endpoint_does_not_end_the_sign_in() {
        for (status, body) in [(429, "{}"), (400, r#"{"error":"slow_down"}"#), (408, "")] {
            let (base, _) = fake_server(vec![("/token", vec![(status, body.into())])]).await;
            let http = Http::new(reqwest::Client::new());
            let error = refresh_at(&base, &http, &grant("old", 1))
                .await
                .err()
                .expect("no grant");
            assert!(
                matches!(error, AuthError::Unreachable(_)) && !error.needs_sign_in(),
                "HTTP {status}: {error:?}"
            );
        }
        let (base, _) = fake_server(vec![(
            "/token",
            vec![(400, r#"{"error":"invalid_grant"}"#.into())],
        )])
        .await;
        let http = Http::new(reqwest::Client::new());
        let error = refresh_at(&base, &http, &grant("old", 1))
            .await
            .err()
            .expect("no grant");
        assert!(error.needs_sign_in());
    }

    #[tokio::test]
    async fn polling_waits_through_pending_then_grants() {
        let (base, log) = fake_server(vec![(
            "/token",
            vec![
                (400, r#"{"error":"authorization_pending"}"#.into()),
                (200, GRANT_JSON.into()),
            ],
        )])
        .await;
        let http = Http::new(reqwest::Client::new());
        let mut code = decode_device(DEVICE_JSON).unwrap();
        code.interval = Duration::from_millis(10);
        let (_keep, cancelled) = watch::channel(false);
        let grant = device_poll_at(&base, &http, &test_client(), &code, cancelled).await;
        assert!(grant.is_ok());
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn only_the_last_of_a_burst_of_attempts_reaches_tidal() {
        let (base, log) = fake_server(vec![
            ("/device_authorization", vec![(200, DEVICE_JSON.into())]),
            (
                "/token",
                vec![(400, r#"{"error":"authorization_pending"}"#.into())],
            ),
        ])
        .await;
        let http = Http::new(reqwest::Client::new());
        let client = test_client();
        let codes = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let mut senders = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..5 {
            let (cancel, cancelled) = watch::channel(false);
            senders.push(cancel);
            let (base, http, client, codes) =
                (base.clone(), http.clone(), client.clone(), codes.clone());
            tasks.push(tokio::spawn(async move {
                device_sign_in(
                    &base,
                    &http,
                    &client,
                    Duration::from_millis(150),
                    cancelled,
                    |code, _| codes.lock().unwrap().push(code.user_code.clone()),
                )
                .await
            }));
            tokio::time::sleep(Duration::from_millis(20)).await;
            if senders.len() < 5 {
                senders.last().unwrap().send(true).unwrap();
            }
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        senders[4].send(true).unwrap();
        for task in tasks {
            let result = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("every attempt ends")
                .unwrap();
            assert_eq!(result.err(), Some(AuthError::Cancelled));
        }
        let paths = log.lock().unwrap().clone();
        let starts = paths
            .iter()
            .filter(|p| p.ends_with("/device_authorization"))
            .count();
        assert_eq!(starts, 1, "{paths:?}");
        assert_eq!(codes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_expired_code_is_replaced_by_a_fresh_one() {
        let (base, log) = fake_server(vec![
            (
                "/device_authorization",
                vec![
                    (
                        200,
                        DEVICE_JSON.replace("\"expiresIn\":300", "\"expiresIn\":1"),
                    ),
                    (200, DEVICE_JSON.into()),
                ],
            ),
            ("/token", vec![(200, GRANT_JSON.into())]),
        ])
        .await;
        let http = Http::new(reqwest::Client::new());
        let (_keep, cancel) = watch::channel(false);
        let seen = std::cell::RefCell::new(Vec::<bool>::new());
        let grant = device_sign_in(
            &base,
            &http,
            &test_client(),
            Duration::ZERO,
            cancel,
            |_, again| seen.borrow_mut().push(again),
        )
        .await;
        assert!(grant.is_ok());
        assert_eq!(*seen.borrow(), vec![false, true]);
        let starts = log
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ends_with("/device_authorization"))
            .count();
        assert_eq!(starts, 2);
    }

    #[tokio::test]
    async fn cancelling_stops_the_poller() {
        let (base, log) = fake_server(vec![(
            "/token",
            vec![(400, r#"{"error":"authorization_pending"}"#.into())],
        )])
        .await;
        let http = Http::new(reqwest::Client::new());
        let client = test_client();
        let mut code = decode_device(DEVICE_JSON).unwrap();
        code.interval = Duration::from_millis(20);
        let (cancel, cancelled) = watch::channel(false);
        let poller =
            tokio::spawn(
                async move { device_poll_at(&base, &http, &client, &code, cancelled).await },
            );
        tokio::time::sleep(Duration::from_millis(80)).await;
        cancel.send(true).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), poller)
            .await
            .expect("poller must stop")
            .unwrap();
        assert_eq!(result.err(), Some(AuthError::Cancelled));
        let after = log.lock().unwrap().len();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(log.lock().unwrap().len(), after, "no polls after cancel");
    }

    #[test]
    fn a_token_response_becomes_a_grant() {
        let text = r#"{"access_token":"eyJ.a","refresh_token":"eyJ.r","token_type":"Bearer","expires_in":86400,
            "scope":"r_usr w_usr w_sub","user_id":12345678,"clientName":"x",
            "user":{"userId":12345678,"countryCode":"NO","username":"u"}}"#;
        let client = ClientConfig {
            client_id: "cid".into(),
            client_secret: "cs".into(),
        };
        let grant = decode_grant(TokenAnswer::Granted, text, AuthFlow::Device, &client, None)
            .unwrap()
            .unwrap();
        assert_eq!(grant.user_id, 12345678);
        assert_eq!(grant.country_code, "NO");
        assert_eq!(grant.refresh_token, "eyJ.r");
        assert!(grant.expires_at > now_secs() + 80000);
        assert!(grant.valid());
        // A refresh answer without refresh token or user keeps the old ones.
        let refreshed = decode_grant(
            TokenAnswer::Granted,
            r#"{"access_token":"eyJ.b","expires_in":3600}"#,
            AuthFlow::Device,
            &client,
            Some(&grant),
        )
        .unwrap()
        .unwrap();
        assert_eq!(refreshed.refresh_token, "eyJ.r");
        assert_eq!(refreshed.user_id, 12345678);
        assert_eq!(refreshed.country_code, "NO");
        assert_eq!(refreshed.access_token, "eyJ.b");
    }

    #[test]
    fn device_codes_get_a_scheme() {
        let code = decode_device(
            r#"{"deviceCode":"d","userCode":"ABCDE","verificationUri":"link.tidal.com","verificationUriComplete":"link.tidal.com/ABCDE","expiresIn":300,"interval":2}"#,
        )
        .unwrap();
        assert_eq!(code.user_code, "ABCDE");
        assert_eq!(
            code.verification_uri_complete,
            "https://link.tidal.com/ABCDE"
        );
        assert_eq!(code.interval, Duration::from_secs(2));
    }

    #[test]
    fn pkce_verifier_and_challenge_follow_rfc_7636() {
        let (verifier, challenge) = pkce_pair();
        assert_eq!(verifier.len(), 43);
        assert!(
            verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_eq!(challenge, pkce_challenge(&verifier));
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let other = pkce_pair().0;
        assert_ne!(verifier, other);
    }

    #[test]
    fn pkce_login_url_carries_the_challenge() {
        let client = ClientConfig {
            client_id: "pkce-test".into(),
            client_secret: String::new(),
        };
        let flow = pkce_begin(&client);
        assert!(
            flow.url
                .starts_with("https://login.tidal.com/authorize?response_type=code&")
        );
        assert!(flow.url.contains("client_id=pkce-test"));
        assert!(
            flow.url
                .contains("redirect_uri=https%3A%2F%2Ftidal.com%2Fandroid%2Flogin%2Fauth")
        );
        assert!(flow.url.contains(&format!(
            "code_challenge={}",
            pkce_challenge(&flow.verifier)
        )));
        assert!(flow.url.contains("code_challenge_method=S256"));
        assert!(
            flow.url
                .contains(&format!("client_unique_key={}", flow.client_unique_key))
        );
        assert_eq!(flow.client_unique_key.len(), 16);
    }

    #[test]
    fn the_code_is_read_from_the_pasted_address() {
        assert_eq!(
            parse_redirect("https://tidal.com/android/login/auth?code=abc%2Fdef&state=na").unwrap(),
            "abc/def"
        );
        assert_eq!(parse_redirect("  rawcode123  ").unwrap(), "rawcode123");
        assert_eq!(
            parse_redirect("https://tidal.com/android/login/auth"),
            Err(AuthError::NoCode)
        );
        assert!(matches!(
            parse_redirect("https://x/?error=access_denied"),
            Err(AuthError::Rejected(_))
        ));
        assert_eq!(parse_redirect(""), Err(AuthError::NoCode));
    }
}
