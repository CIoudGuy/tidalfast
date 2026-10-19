//! The signed-in TIDAL client and whether it is ready for requests.

use std::sync::{Arc, RwLock};

use super::client::{ApiError, NetActivity};
use super::tidal::TidalClient;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Unavailable,
    Authorizing,
    Ready { account: AccountId },
}

impl SessionState {
    pub fn account(&self) -> Option<&AccountId> {
        match self {
            Self::Ready { account } => Some(account),
            Self::Unavailable | Self::Authorizing => None,
        }
    }
}

pub struct ApiGateway {
    // The counter remembers a sign-out even if a new sign-in becomes ready
    // before a waiting request wakes up.
    state: tokio::sync::watch::Sender<(u64, SessionState)>,
    /// The one TIDAL client: `begin_tidal` marks it authorizing, `install`
    /// ready, `clear` signed out.
    tidal: RwLock<Arc<TidalClient>>,
}

impl ApiGateway {
    pub fn new(http: impl Into<crate::http::Http>, activity: Arc<NetActivity>) -> Self {
        Self {
            state: tokio::sync::watch::channel((0, SessionState::Unavailable)).0,
            tidal: RwLock::new(Arc::new(TidalClient::new(http.into(), activity))),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state.borrow().1.clone()
    }

    /// Marks the verified account ready.
    pub fn install(&self, account: AccountId) {
        self.state
            .send_modify(|current| current.1 = SessionState::Ready { account });
    }

    /// Signs out: waiting requests fail and the client drops its tokens.
    pub fn clear(&self) {
        self.tidal().set_tokens(None);
        self.state.send_modify(|current| {
            current.0 += 1;
            current.1 = SessionState::Unavailable;
        });
    }

    pub fn account(&self) -> Option<AccountId> {
        self.state().account().cloned()
    }

    /// Installs a new TIDAL grant on a fresh client and withdraws the old
    /// account. Call `install` once `me()` or `session()` has verified it.
    pub fn begin_tidal(&self, tokens: Arc<crate::auth::tidal::TidalTokens>) {
        self.state
            .send_modify(|current| current.1 = SessionState::Authorizing);
        let client = self.tidal().for_tokens(tokens);
        *self.tidal.write().unwrap_or_else(|lock| lock.into_inner()) = Arc::new(client);
    }

    /// The TIDAL client as it is now, ready or not (verification, sign-out).
    pub fn tidal(&self) -> Arc<TidalClient> {
        Arc::clone(&self.tidal.read().unwrap_or_else(|lock| lock.into_inner()))
    }

    /// The TIDAL client once the account is verified, waiting while a
    /// sign-in is being verified.
    pub async fn tidal_ready(&self) -> Result<Arc<TidalClient>, ApiError> {
        let mut state = self.state.subscribe();
        let generation = state.borrow().0;
        loop {
            let (current, status) = state.borrow_and_update().clone();
            if current != generation {
                return Err(ApiError::NotSignedIn);
            }
            match status {
                SessionState::Ready { .. } => return Ok(self.tidal()),
                SessionState::Unavailable => return Err(ApiError::NotSignedIn),
                SessionState::Authorizing => {
                    state.changed().await.map_err(|_| ApiError::NotSignedIn)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gateway() -> ApiGateway {
        let http =
            crate::http::Http::from_proxy(&crate::settings::ProxyConfig::Off).expect("a client");
        ApiGateway::new(http, Arc::new(NetActivity::default()))
    }

    #[tokio::test]
    async fn requests_wait_for_verification_and_fail_when_signed_out() {
        let api = Arc::new(gateway());
        assert!(matches!(
            api.tidal_ready().await,
            Err(ApiError::NotSignedIn)
        ));

        api.state
            .send_modify(|current| current.1 = SessionState::Authorizing);
        let waiting = tokio::spawn({
            let api = Arc::clone(&api);
            async move { api.tidal_ready().await.is_ok() }
        });
        tokio::task::yield_now().await;
        api.install(AccountId::new("42"));
        assert!(waiting.await.unwrap());
        assert_eq!(api.account(), Some(AccountId::new("42")));

        api.state
            .send_modify(|current| current.1 = SessionState::Authorizing);
        let waiting = tokio::spawn({
            let api = Arc::clone(&api);
            async move { api.tidal_ready().await.is_ok() }
        });
        tokio::task::yield_now().await;
        api.clear();
        api.install(AccountId::new("43"));
        assert!(
            !waiting.await.unwrap(),
            "a sign-out in between fails the request"
        );
    }
}
