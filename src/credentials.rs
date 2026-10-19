//! The durable TIDAL grant and the proxy password in the platform credential
//! store.
//!
//! All native calls run on one dedicated thread. A locked store cannot hold
//! the UI, command loop, or runtime shutdown hostage. Generation checks reject
//! work from before sign-out, including a write that returns after sign-out.
//! Non-secret revocation markers prevent restoration after a failed deletion.
//!
//! Entries live under Tidalfast's own service name, so nothing another app
//! stored is ever read, migrated or removed. The TIDAL slot has no plaintext
//! predecessor to migrate; only the proxy password keeps its legacy reader.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths::AppDirs;

const SERVICE: &str = "rocks.tidalfast.Tidalfast";
/// How long one store request may take. Past D-Bus's own 25 s method
/// timeout, so a slow Secret Service answers or fails before we give up.
const TIMEOUT: Duration = Duration::from_secs(30);
/// More tries for a sign-in restore whose store did not answer in time, and
/// the pause before each. A Secret Service started on demand (gnome-keyring
/// in a WSLg or fresh session) can take longer than one wait; a store that
/// is late must not read as "no sign-in".
const RESTORE_RETRIES: u32 = 3;
const RESTORE_PAUSE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    /// The TIDAL OAuth grant (device-code or PKCE).
    Tidal,
    Proxy,
}

impl Slot {
    /// The account grants a sign-out removes.
    pub const ACCOUNT: [Self; 1] = [Self::Tidal];
    pub const ALL: [Self; 2] = [Self::Tidal, Self::Proxy];
    pub(crate) fn index(self) -> usize {
        self as usize
    }
    fn name(self) -> &'static str {
        match self {
            Self::Tidal => "tidal",
            Self::Proxy => "proxy-password",
        }
    }
}

/// Deliberately has no Debug implementation: it contains usable secrets.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Grant {
    Tidal(crate::auth::tidal::TidalGrant),
    Proxy(ProxyPassword),
}

/// No Debug implementation: the password is usable, and the username is private.
/// A saved password belongs to one network endpoint and username.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyPassword {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyPassword {
    fn valid(&self) -> bool {
        let settings = crate::settings::Settings {
            proxy_host: self.host.clone(),
            proxy_port: self.port.to_string(),
            proxy_username: self.username.clone(),
            proxy_password: self.password.clone(),
            ..Default::default()
        };
        settings.proxy_password_record().ok().flatten().as_ref() == Some(self)
    }
}

impl Grant {
    fn valid_for(&self, slot: Slot) -> bool {
        match (slot, self) {
            (Slot::Tidal, Self::Tidal(grant)) => grant.valid(),
            (Slot::Proxy, Self::Proxy(password)) => password.valid(),
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    slot: Slot,
    grant: Grant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "The system credential store is unavailable. Unlock or enable it to remember this sign-in."
    )]
    Unavailable,
    #[error(
        "The system credential store is locked or access was denied. Unlock it to remember this sign-in."
    )]
    Locked,
    #[error("The system credential store did not respond. This sign-in cannot be remembered yet.")]
    Timeout,
    #[error("The stored TIDAL sign-in is invalid. Sign in again.")]
    Invalid,
    #[error(
        "Unable to update credential-storage state. Check the application state directory's permissions."
    )]
    Filesystem,
    #[error(
        "The system credential store did not retain the grant. This sign-in cannot be remembered."
    )]
    Verification,
    #[error("The sign-in changed while credential storage was in progress.")]
    Stale,
}

impl Error {
    pub(crate) fn proxy_message(self) -> &'static str {
        match self {
            Self::Unavailable | Self::Locked => {
                "Unlock or enable the system credential store to remember the proxy password."
            }
            Self::Timeout => {
                "The credential store did not respond. The proxy password is available only for this session."
            }
            Self::Invalid => {
                "The stored proxy password or its endpoint is invalid. Enter the proxy settings again."
            }
            Self::Filesystem => {
                "Unable to update proxy-password storage. Check the application state directory permissions."
            }
            Self::Verification => {
                "The credential store did not retain the proxy password. It is available only for this session."
            }
            Self::Stale => "The proxy settings changed while the password was being stored.",
        }
    }
}

fn native_error(error: keyring_core::Error) -> Error {
    // Some provider errors contain the secret or arbitrary platform data.
    // Never propagate their Debug/Display text into application diagnostics.
    match error {
        keyring_core::Error::NoStorageAccess(_) => Error::Locked,
        _ => Error::Unavailable,
    }
}

trait ProtectedStore: Send {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error>;
    fn delete(&mut self, key: &str) -> Result<(), Error>;
}

/// The credential-store service of a profile: a named test profile
/// (`TIDALFAST_PROFILE`) keeps its entries under a service of its own, on top
/// of the per-state-directory key, so it can never read, overwrite or revoke
/// the default profile's sign-in.
pub(crate) fn service_name(profile: Option<&str>) -> String {
    match profile {
        Some(p) => format!("{SERVICE}.profile.{p}"),
        None => SERVICE.to_string(),
    }
}

struct NativeStore {
    service: String,
    store: Option<Arc<keyring_core::api::CredentialStore>>,
}

impl NativeStore {
    fn entry(&mut self, key: &str) -> Result<keyring_core::Entry, Error> {
        if self.store.is_none() {
            #[cfg(target_os = "linux")]
            let store = zbus_secret_service_keyring_store::Store::new();
            #[cfg(target_os = "macos")]
            let store = apple_native_keyring_store::keychain::Store::new();
            #[cfg(windows)]
            let store = windows_native_keyring_store::Store::new();
            self.store = Some(store.map_err(native_error)?);
        }
        self.store
            .as_ref()
            .ok_or(Error::Unavailable)?
            .build(&self.service, key, None)
            .map_err(native_error)
    }
}

impl ProtectedStore for NativeStore {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.entry(key)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(native_error(error)),
        }
    }
    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error> {
        self.entry(key)?.set_secret(secret).map_err(native_error)
    }
    fn delete(&mut self, key: &str) -> Result<(), Error> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(native_error(error)),
        }
    }
}

type Job = Box<dyn FnOnce(&mut dyn ProtectedStore) + Send>;

struct Inner {
    dirs: AppDirs,
    profile: String,
    generations: [AtomicU64; 2],
    // Held only around the tiny local marker files, never a native store call.
    markers: Mutex<()>,
    jobs: mpsc::SyncSender<Job>,
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

/// Permission to read/write one grant for one authorization generation.
#[derive(Clone)]
pub struct Lease {
    store: Store,
    slot: Slot,
    generation: u64,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    version: u32,
    revoked: bool,
}

pub struct Loaded {
    pub grant: Option<Grant>,
    pub warning: Option<Error>,
}

impl Store {
    pub fn new(dirs: AppDirs) -> Self {
        let native = NativeStore {
            service: service_name(crate::paths::profile()),
            store: None,
        };
        Self::with_backend(dirs, Box::new(native))
    }

    #[cfg(test)]
    pub(crate) fn in_memory(dirs: AppDirs) -> Self {
        tests::memory_store(dirs)
    }

    fn with_backend(dirs: AppDirs, mut backend: Box<dyn ProtectedStore>) -> Self {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(16);
        let runtime = tokio::runtime::Handle::try_current().ok();
        let _ = std::thread::Builder::new()
            .name("credentials".into())
            .spawn(move || {
                let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
                while let Ok(job) = receiver.recv() {
                    job(backend.as_mut());
                }
            });
        let profile = std::fs::read_to_string(dirs.state.join("credential-profile"))
            .ok()
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| {
                format!(
                    "{:x}",
                    Sha256::digest(dirs.state.to_string_lossy().as_bytes())
                )
            });
        Self {
            inner: Arc::new(Inner {
                dirs,
                profile,
                generations: std::array::from_fn(|_| AtomicU64::new(0)),
                markers: Mutex::new(()),
                jobs,
            }),
        }
    }

    pub fn lease(&self, slot: Slot) -> Lease {
        Lease {
            store: self.clone(),
            slot,
            generation: self.inner.generations[slot.index()].load(Ordering::SeqCst),
        }
    }

    /// Invalidate in-flight work before canceling workers or deleting grants.
    pub fn invalidate(&self, slot: Slot) {
        self.inner.generations[slot.index()].fetch_add(1, Ordering::SeqCst);
    }

    /// Persist revocation before contacting the native store. Even a locked
    /// keychain or an interrupted deletion must not sign the user back in.
    pub fn revoke(&self, slot: Slot) -> Result<(), Error> {
        self.invalidate(slot);
        let _guard = self.inner.markers.lock().unwrap_or_else(|p| p.into_inner());
        let marker = self.write_marker(slot, true);
        let legacy = self.remove_legacy(slot);
        marker.and(legacy)
    }

    /// Sign-out: revoke every account grant, keeping the proxy password.
    pub fn revoke_account(&self) -> Result<(), Error> {
        let mut error = None;
        for slot in Slot::ACCOUNT {
            if let Err(failure) = self.revoke(slot) {
                error = Some(failure);
            }
        }
        error.map_or(Ok(()), Err)
    }

    /// Transitional name for [`Self::revoke_account`].
    #[doc(hidden)]
    pub fn revoke_tidal(&self) -> Result<(), Error> {
        self.revoke_account()
    }

    fn marker_path(&self, slot: Slot) -> PathBuf {
        self.inner
            .dirs
            .state
            .join("credential-storage")
            .join(format!("{}.json", slot.name()))
    }

    fn marker(&self, slot: Slot) -> Result<Option<Marker>, Error> {
        match std::fs::read(self.marker_path(slot)) {
            Ok(bytes) => {
                let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
                if marker.version != 1 {
                    return Err(Error::Invalid);
                }
                Ok(Some(marker))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(Error::Filesystem),
        }
    }

    fn write_marker(&self, slot: Slot, revoked: bool) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&Marker {
            version: 1,
            revoked,
        })
        .map_err(|_| Error::Invalid)?;
        // It holds no secret, but it lives with the credentials, which only
        // their owner reads.
        fastframe_file::write_private(&self.marker_path(slot), bytes).map_err(|_| Error::Filesystem)
    }

    /// Plaintext predecessors of a slot. The TIDAL grant never had one, and
    /// Other apps' token files are not Tidalfast's to read or delete.
    fn legacy_paths(&self, slot: Slot) -> Vec<PathBuf> {
        match slot {
            Slot::Proxy => {
                let path = self.inner.dirs.proxy_secret_file();
                vec![path.clone(), path.with_extension("tmp")]
            }
            Slot::Tidal => Vec::new(),
        }
    }

    fn remove_legacy(&self, slot: Slot) -> Result<(), Error> {
        let mut result = Ok(());
        for path in self.legacy_paths(slot) {
            if let Err(error) = remove_file(&path) {
                result = Err(error);
            }
        }
        result
    }

    fn legacy_proxy(&self) -> Result<Option<Grant>, Error> {
        let text = match std::fs::read_to_string(self.inner.dirs.settings_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return match self.inner.dirs.proxy_secret_file().try_exists() {
                    Ok(false) => Ok(None),
                    Ok(true) => Err(Error::Invalid),
                    Err(_) => Err(Error::Filesystem),
                };
            }
            Err(_) => return Err(Error::Filesystem),
        };
        let mut settings: crate::settings::Settings =
            serde_json::from_str(&text).map_err(|_| Error::Invalid)?;
        settings.migrate_proxy(&text);
        match std::fs::read_to_string(self.inner.dirs.proxy_secret_file()) {
            Ok(password) => settings.proxy_password = password,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Filesystem),
        }
        settings
            .proxy_password_record()
            .map(|password| password.map(Grant::Proxy))
            .map_err(|_| Error::Invalid)
    }

    fn legacy(&self, slot: Slot) -> Result<Option<Grant>, Error> {
        match slot {
            Slot::Proxy => self.legacy_proxy(),
            Slot::Tidal => Ok(None),
        }
    }
}

fn remove_file(path: &Path) -> Result<(), Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::Filesystem),
    }
}

impl Lease {
    pub fn current(&self) -> bool {
        self.store.inner.generations[self.slot.index()].load(Ordering::SeqCst) == self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    fn check(&self) -> Result<(), Error> {
        if self.current() {
            Ok(())
        } else {
            Err(Error::Stale)
        }
    }
    fn key(&self) -> String {
        format!("{}:{}", self.store.inner.profile, self.slot.name())
    }

    fn request<T: Send + 'static>(
        &self,
        guard_generation: bool,
        operation: impl FnOnce(Self, &mut dyn ProtectedStore) -> Result<T, Error> + Send + 'static,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, Error>> + Send>> {
        let pending = (|| {
            if guard_generation {
                self.check()?;
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let lease = self.clone();
            self.store
                .inner
                .jobs
                .try_send(Box::new(move |backend| {
                    let result = (if guard_generation {
                        lease.check()
                    } else {
                        Ok(())
                    })
                    .and_then(|()| operation(lease.clone(), backend));
                    let result = result.and_then(|value| {
                        if guard_generation {
                            lease.check()?;
                        }
                        Ok(value)
                    });
                    let _ = sender.send(result);
                }))
                .map_err(|_| Error::Unavailable)?;
            Ok::<_, Error>(receiver)
        })();
        Box::pin(async move {
            tokio::time::timeout(TIMEOUT, pending?)
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Unavailable)?
        })
    }

    /// [`Self::load`], tried again while the store does not answer in time.
    pub async fn load_patiently(&self) -> Result<Loaded, Error> {
        let mut result = self.load().await;
        for attempt in 1..=RESTORE_RETRIES {
            if !matches!(result, Err(Error::Timeout)) {
                break;
            }
            log::warn!(
                "The credential store did not answer; trying again ({attempt}/{RESTORE_RETRIES})"
            );
            tokio::time::sleep(RESTORE_PAUSE).await;
            result = self.load().await;
        }
        result
    }

    pub async fn load(&self) -> Result<Loaded, Error> {
        self.request(true, |lease, backend| {
            let marker = lease.store.marker(lease.slot)?;
            if marker.as_ref().is_some_and(|marker| marker.revoked) {
                return Ok(Loaded {
                    grant: None,
                    warning: None,
                });
            }
            let existing = backend.read(&lease.key());
            if let Ok(Some(bytes)) = &existing {
                let record: Record = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
                if record.version != 1
                    || record.slot != lease.slot
                    || !record.grant.valid_for(lease.slot)
                {
                    return Err(Error::Invalid);
                }
                let _guard = lease
                    .store
                    .inner
                    .markers
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                lease.check()?;
                lease.store.write_marker(lease.slot, false)?;
                let warning = lease.store.remove_legacy(lease.slot).err();
                return Ok(Loaded {
                    grant: Some(record.grant),
                    warning,
                });
            }
            if marker.is_some() {
                return existing.map(|_| Loaded {
                    grant: None,
                    warning: None,
                });
            }
            let Some(grant) = lease.store.legacy(lease.slot)? else {
                return existing.map(|_| Loaded {
                    grant: None,
                    warning: None,
                });
            };
            // Keep a failed migration recoverable. No new plaintext is written,
            // and the caller must show the warning about persistence.
            let warning = match existing {
                Err(error) => Some(error),
                Ok(_) => lease.save_inner(backend, grant.clone()).err(),
            };
            Ok(Loaded {
                grant: Some(grant),
                warning,
            })
        })
        .await
    }

    fn save_inner(&self, backend: &mut dyn ProtectedStore, grant: Grant) -> Result<(), Error> {
        self.check()?;
        if !grant.valid_for(self.slot) {
            return Err(Error::Invalid);
        }
        let bytes = serde_json::to_vec(&Record {
            version: 1,
            slot: self.slot,
            grant,
        })
        .map_err(|_| Error::Invalid)?;
        backend.write(&self.key(), &bytes)?;
        if !self.current() {
            // Operations on this store are serialized. No newer write can be
            // erased here; it is still queued behind this one.
            let _ = backend.delete(&self.key());
            return Err(Error::Stale);
        }
        if backend.read(&self.key())?.as_deref() != Some(bytes.as_slice()) {
            return Err(Error::Verification);
        }
        let _guard = self
            .store
            .inner
            .markers
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.check()?;
        self.store.write_marker(self.slot, false)?;
        self.store.remove_legacy(self.slot)
    }

    /// Enqueue immediately so refreshes preserve write order even when callers
    /// await completion outside their token mutex.
    pub fn save(
        &self,
        grant: Grant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send>> {
        self.request(true, move |lease, backend| lease.save_inner(backend, grant))
    }

    /// Call `Store::revoke` before this. Deletion failure remains visible while
    /// the local revocation marker prevents restoration on the next launch.
    pub fn delete(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send>> {
        // Deletion is enqueued before any replacement write. Run it even if a
        // subsequent authorization increments the generation while it waits.
        self.request(false, |lease, backend| backend.delete(&lease.key()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        values: HashMap<String, Vec<u8>>,
        read_error: Option<Error>,
        write_error: Option<Error>,
        delete_error: Option<Error>,
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        release: Option<mpsc::Receiver<()>>,
    }
    pub(super) fn memory_store(dirs: AppDirs) -> Store {
        Store::with_backend(
            dirs,
            Box::new(Backend(Arc::new(Mutex::new(Fake::default())))),
        )
    }
    struct Backend(Arc<Mutex<Fake>>);
    impl ProtectedStore for Backend {
        fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
            let fake = self.0.lock().unwrap();
            if let Some(error) = fake.read_error {
                return Err(error);
            }
            Ok(fake.values.get(key).cloned())
        }
        fn write(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
            let (entered, release) = {
                let mut fake = self.0.lock().unwrap();
                if let Some(error) = fake.write_error {
                    return Err(error);
                }
                (fake.entered.take(), fake.release.take())
            };
            if let Some(entered) = entered {
                let _ = entered.send(());
            }
            if let Some(release) = release {
                release.recv().unwrap();
            }
            self.0
                .lock()
                .unwrap()
                .values
                .insert(key.to_owned(), value.to_vec());
            Ok(())
        }
        fn delete(&mut self, key: &str) -> Result<(), Error> {
            let mut fake = self.0.lock().unwrap();
            if let Some(error) = fake.delete_error {
                return Err(error);
            }
            fake.values.remove(key);
            Ok(())
        }
    }

    struct Fixture {
        dirs: AppDirs,
        fake: Arc<Mutex<Fake>>,
        store: Store,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "tidalfast-credential-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let dirs = AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            };
            dirs.ensure().unwrap();
            let fake = Arc::new(Mutex::new(Fake::default()));
            let store = Store::with_backend(dirs.clone(), Box::new(Backend(fake.clone())));
            Self { dirs, fake, store }
        }
        fn restart(&self) -> Store {
            Store::with_backend(self.dirs.clone(), Box::new(Backend(self.fake.clone())))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.dirs.state.parent().unwrap());
        }
    }

    fn tidal(access: &str) -> Grant {
        Grant::Tidal(crate::auth::tidal::TidalGrant {
            flow: crate::auth::tidal::AuthFlow::Pkce,
            client_id: "dummy-client".into(),
            access_token: access.into(),
            refresh_token: "dummy-refresh".into(),
            expires_at: u64::MAX,
            user_id: 7,
            country_code: "SE".into(),
            client_unique_key: "0123456789abcdef".into(),
            ..Default::default()
        })
    }

    fn proxy_password() -> Grant {
        Grant::Proxy(ProxyPassword {
            host: "127.0.0.1".into(),
            port: 8080,
            username: "dummy-user".into(),
            password: " dummy-private-password\n".into(),
        })
    }

    #[test]
    fn a_test_profile_never_shares_the_default_sign_in() {
        assert_eq!(service_name(None), SERVICE);
        let profile = service_name(Some("lunatest"));
        assert_ne!(profile, SERVICE);
        assert!(profile.starts_with(SERVICE));
        // Only read: neither store is written to.
        let key = |dirs: AppDirs| {
            let fake = Arc::new(Mutex::new(Fake::default()));
            let store = Store::with_backend(dirs, Box::new(Backend(fake)));
            Slot::ALL.map(|slot| store.lease(slot).key())
        };
        let default = key(AppDirs::for_profile(None));
        let test = key(AppDirs::for_profile(Some("lunatest")));
        for (a, b) in default.iter().zip(&test) {
            assert_ne!(a, b);
        }
    }

    #[tokio::test]
    async fn a_tidal_grant_survives_a_restart_whole() {
        let f = Fixture::new();
        let grant = tidal("dummy-access");
        f.store
            .lease(Slot::Tidal)
            .save(grant.clone())
            .await
            .unwrap();
        let loaded = f.restart().lease(Slot::Tidal).load().await.unwrap();
        assert!(loaded.grant == Some(grant));
        assert_eq!(loaded.warning, None);
        let key = f.store.lease(Slot::Tidal).key();
        assert!(key.ends_with(":tidal"));
        let marker = std::fs::read_to_string(f.store.marker_path(Slot::Tidal)).unwrap();
        assert!(!marker.contains("dummy"));
    }

    #[tokio::test]
    async fn an_incomplete_tidal_grant_is_refused() {
        let f = Fixture::new();
        let Grant::Tidal(mut grant) = tidal("dummy-access") else {
            unreachable!()
        };
        grant.refresh_token.clear();
        assert_eq!(
            f.store.lease(Slot::Tidal).save(Grant::Tidal(grant)).await,
            Err(Error::Invalid)
        );
        assert_eq!(
            f.store.lease(Slot::Tidal).save(proxy_password()).await,
            Err(Error::Invalid)
        );
    }

    #[tokio::test]
    async fn a_recorded_credential_profile_keeps_grants_and_revocation_markers() {
        let fixture = Fixture::new();
        let grant = tidal("dummy-access");
        fixture
            .store
            .lease(Slot::Tidal)
            .save(grant.clone())
            .await
            .unwrap();
        let parent = fixture.dirs.state.parent().unwrap();
        let renamed = AppDirs {
            config: parent.join("new-config"),
            state: parent.join("new-state"),
            cache: parent.join("new-cache"),
        };
        std::fs::rename(&fixture.dirs.state, &renamed.state).unwrap();
        std::fs::write(
            renamed.state.join("credential-profile"),
            &fixture.store.inner.profile,
        )
        .unwrap();
        let store = Store::with_backend(renamed, Box::new(Backend(fixture.fake.clone())));
        assert_eq!(store.inner.profile, fixture.store.inner.profile);
        assert!(store.lease(Slot::Tidal).load().await.unwrap().grant == Some(grant));
    }

    fn write_proxy_legacy(f: &Fixture, combined: bool) -> Grant {
        let Grant::Proxy(password) = proxy_password() else {
            unreachable!()
        };
        let settings = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Http,
            proxy_host: password.host.clone(),
            proxy_port: password.port.to_string(),
            proxy_username: password.username.clone(),
            ..Default::default()
        };
        let mut json = serde_json::to_value(settings).unwrap();
        if combined {
            json["proxy_password"] = password.password.clone().into();
        }
        std::fs::write(f.dirs.settings_file(), serde_json::to_vec(&json).unwrap()).unwrap();
        if !combined {
            std::fs::write(f.dirs.proxy_secret_file(), password.password.as_bytes()).unwrap();
        }
        Grant::Proxy(password)
    }

    #[tokio::test]
    async fn proxy_password_migration_verifies_before_removing_legacy_data() {
        for combined in [false, true] {
            let f = Fixture::new();
            let expected = write_proxy_legacy(&f, combined);
            let settings_before = std::fs::read(f.dirs.settings_file()).unwrap();
            let loaded = f.store.lease(Slot::Proxy).load().await.unwrap();
            assert!(loaded.grant == Some(expected.clone()));
            assert_eq!(loaded.warning, None);
            assert!(!f.dirs.proxy_secret_file().exists());
            assert_eq!(
                std::fs::read(f.dirs.settings_file()).unwrap(),
                settings_before
            );
            assert!(f.restart().lease(Slot::Proxy).load().await.unwrap().grant == Some(expected));
        }
    }

    #[tokio::test]
    async fn a_failed_proxy_migration_retains_the_exact_original_and_can_retry() {
        for combined in [false, true] {
            let f = Fixture::new();
            let expected = write_proxy_legacy(&f, combined);
            let path = if combined {
                f.dirs.settings_file()
            } else {
                f.dirs.proxy_secret_file()
            };
            let before = std::fs::read(&path).unwrap();
            f.fake.lock().unwrap().write_error = Some(Error::Locked);
            let loaded = f.store.lease(Slot::Proxy).load().await.unwrap();
            assert!(loaded.grant == Some(expected.clone()));
            assert_eq!(loaded.warning, Some(Error::Locked));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            f.fake.lock().unwrap().write_error = None;
            let loaded = f.restart().lease(Slot::Proxy).load().await.unwrap();
            assert!(loaded.grant == Some(expected));
            assert_eq!(loaded.warning, None);
        }
    }

    #[tokio::test]
    async fn sign_out_retains_the_network_password() {
        let f = Fixture::new();
        let password = proxy_password();
        f.store
            .lease(Slot::Proxy)
            .save(password.clone())
            .await
            .unwrap();
        f.store
            .lease(Slot::Tidal)
            .save(tidal("dummy-access"))
            .await
            .unwrap();
        f.store.revoke_account().unwrap();
        let restarted = f.restart();
        assert!(restarted.lease(Slot::Proxy).load().await.unwrap().grant == Some(password));
        assert!(
            restarted
                .lease(Slot::Tidal)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_failed_proxy_delete_cannot_restore_the_forgotten_password() {
        let f = Fixture::new();
        let password = write_proxy_legacy(&f, true);
        f.store.lease(Slot::Proxy).save(password).await.unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke(Slot::Proxy).unwrap();
        assert_eq!(
            f.store.lease(Slot::Proxy).delete().await,
            Err(Error::Locked)
        );
        assert!(
            f.restart()
                .lease(Slot::Proxy)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires an unlocked platform credential store; uses dummy grants only"]
    async fn native_store_round_trip() {
        let f = Fixture::new();
        let store = Store::new(f.dirs.clone());
        let grants = [tidal("dummy-access"), proxy_password()];
        for (slot, grant) in Slot::ALL.into_iter().zip(grants.iter()) {
            store.lease(slot).save(grant.clone()).await.unwrap();
        }
        let restarted = Store::new(f.dirs.clone());
        for (slot, grant) in Slot::ALL.into_iter().zip(grants) {
            assert!(restarted.lease(slot).load().await.unwrap().grant == Some(grant));
        }
        store.revoke_account().unwrap();
        store.revoke(Slot::Proxy).unwrap();
        for slot in Slot::ALL {
            let lease = store.lease(slot);
            lease.delete().await.unwrap();
            assert!(
                lease
                    .request(false, |lease, backend| backend.read(&lease.key()))
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn queued_deletion_cannot_erase_a_new_authorization() {
        let f = Fixture::new();
        let old = tidal("dummy-old");
        let new = tidal("dummy-new");
        f.store.lease(Slot::Tidal).save(old).await.unwrap();
        f.store.revoke(Slot::Tidal).unwrap();
        let deletion = f.store.lease(Slot::Tidal).delete();
        f.store.invalidate(Slot::Tidal);
        let saving = f.store.lease(Slot::Tidal).save(new.clone());
        deletion.await.unwrap();
        saving.await.unwrap();
        assert!(f.restart().lease(Slot::Tidal).load().await.unwrap().grant == Some(new));
    }

    #[tokio::test]
    async fn late_write_after_signout_cannot_recreate_grant() {
        let f = Fixture::new();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut fake = f.fake.lock().unwrap();
            fake.entered = Some(entered_tx);
            fake.release = Some(release_rx);
        }
        let lease = f.store.lease(Slot::Tidal);
        let pending_lease = lease.clone();
        let pending = tokio::spawn(async move { pending_lease.save(tidal("dummy-late")).await });
        entered_rx.await.unwrap();
        f.store.revoke_account().unwrap();
        assert!(!lease.current());
        release_tx.send(()).unwrap();
        assert_eq!(pending.await.unwrap(), Err(Error::Stale));
        assert!(f.fake.lock().unwrap().values.is_empty());
        assert!(
            f.restart()
                .lease(Slot::Tidal)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn failed_native_deletion_remains_revoked_across_restart() {
        let f = Fixture::new();
        f.store
            .lease(Slot::Tidal)
            .save(tidal("dummy-access"))
            .await
            .unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke_account().unwrap();
        assert_eq!(
            f.store.lease(Slot::Tidal).delete().await,
            Err(Error::Locked)
        );
        assert_eq!(f.fake.lock().unwrap().values.len(), 1);
        // A revoked load must not even require access to the locked service.
        f.fake.lock().unwrap().read_error = Some(Error::Locked);
        assert!(
            f.restart()
                .lease(Slot::Tidal)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn tidalfast_token_files_are_neither_read_nor_removed() {
        let f = Fixture::new();
        let path = f.dirs.shared_web_token_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"client_id":"x","access_token":"a","refresh_token":"r","expires_at":1}"#,
        )
        .unwrap();
        assert!(
            f.store
                .lease(Slot::Tidal)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
        f.store.revoke_account().unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn profiles_are_isolated_and_invalid_records_are_rejected() {
        let f = Fixture::new();
        let other = Fixture::new();
        let isolated = Store::with_backend(other.dirs.clone(), Box::new(Backend(f.fake.clone())));
        f.store
            .lease(Slot::Tidal)
            .save(tidal("dummy-access"))
            .await
            .unwrap();
        assert!(
            isolated
                .lease(Slot::Tidal)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
        for bytes in [
            b"not-json".to_vec(),
            serde_json::to_vec(&Record {
                version: 2,
                slot: Slot::Tidal,
                grant: tidal("dummy-access"),
            })
            .unwrap(),
            serde_json::to_vec(&Record {
                version: 1,
                slot: Slot::Proxy,
                grant: proxy_password(),
            })
            .unwrap(),
        ] {
            f.fake
                .lock()
                .unwrap()
                .values
                .insert(f.store.lease(Slot::Tidal).key(), bytes);
            assert!(matches!(
                f.store.lease(Slot::Tidal).load().await,
                Err(Error::Invalid)
            ));
        }
    }
}
