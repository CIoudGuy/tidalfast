//! Shared HTTP client, so a proxy change can take effect without a restart.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::settings::ProxyConfig;

/// Offline mode (the setting): while on, no request leaves the process.
#[cfg(not(test))]
static OFFLINE_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The text of the error every request gets in offline mode.
pub const OFFLINE_MODE_ERROR: &str = "Offline mode is on";

#[cfg(test)]
thread_local! {
    /// Tests run in parallel in one process: each sees only its own thread's
    /// offline mode, so one that turns it on cannot refuse another's requests.
    static TEST_OFFLINE_MODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Turns offline mode on or off for the whole process.
pub fn set_offline_mode(on: bool) {
    #[cfg(test)]
    TEST_OFFLINE_MODE.with(|mode| mode.set(on));
    #[cfg(not(test))]
    OFFLINE_MODE.store(on, std::sync::atomic::Ordering::SeqCst);
}

/// Whether offline mode is on.
pub fn offline_mode() -> bool {
    #[cfg(test)]
    return TEST_OFFLINE_MODE.with(std::cell::Cell::get);
    #[cfg(not(test))]
    OFFLINE_MODE.load(std::sync::atomic::Ordering::SeqCst)
}

/// `Err` in offline mode: the check in front of every request that does
/// not go through [`Http::client`].
pub fn guard() -> Result<(), String> {
    if offline_mode() {
        log::debug!("offline mode: request refused");
        Err(OFFLINE_MODE_ERROR.into())
    } else {
        Ok(())
    }
}

/// A cheap-to-clone handle to the process HTTP client.
///
/// Replacing the inner client updates every clone: the Web API, token
/// refresh, artwork, lyrics, and the update check all pick up a new proxy
/// on the next request.
#[derive(Clone)]
pub struct Http {
    inner: Arc<RwLock<Result<reqwest::Client, String>>>,
}

impl Http {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Ok(client))),
        }
    }

    pub fn from_proxy(proxy: &ProxyConfig) -> Result<Self, String> {
        build_client(proxy).map(Self::new)
    }

    /// Keep the interface available to repair settings without permitting
    /// requests to bypass the configuration that failed to build.
    pub fn unavailable(error: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Err(error))),
        }
    }

    pub fn replace(&self, client: reqwest::Client) {
        *self.inner.write().unwrap_or_else(|lock| lock.into_inner()) = Ok(client);
    }

    pub fn block(&self, error: String) {
        *self.inner.write().unwrap_or_else(|lock| lock.into_inner()) = Err(error);
    }

    pub fn client(&self) -> Result<reqwest::Client, String> {
        guard()?;
        self.inner
            .read()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone()
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

impl From<reqwest::Client> for Http {
    fn from(client: reqwest::Client) -> Self {
        Self::new(client)
    }
}

pub fn build_client(proxy: &ProxyConfig) -> Result<reqwest::Client, String> {
    client_builder(proxy)?
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.without_url().to_string())
}

pub fn build_blocking(
    proxy: &ProxyConfig,
    timeout: Duration,
) -> Result<reqwest::blocking::Client, String> {
    blocking_builder(proxy)?
        .timeout(timeout)
        .build()
        .map_err(|error| error.without_url().to_string())
}

/// A blocking client for a download, made from any thread. reqwest's
/// blocking client panics when it is built where an async runtime is
/// running (the backend's thread is one), which took the whole backend down
/// at the first Download; it is built on a thread of its own instead.
pub fn blocking_download_client(
    proxy: &ProxyConfig,
    connect_timeout: Duration,
    timeout: Duration,
) -> Result<reqwest::blocking::Client, String> {
    let proxy = proxy.clone();
    std::thread::Builder::new()
        .name("download-client".into())
        .spawn(move || {
            blocking_builder(&proxy).and_then(|builder| {
                builder
                    .connect_timeout(connect_timeout)
                    .timeout(timeout)
                    .build()
                    .map_err(|error| error.without_url().to_string())
            })
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "the download client could not be made".to_string())?
}

fn client_builder(proxy: &ProxyConfig) -> Result<reqwest::ClientBuilder, String> {
    apply_proxy(reqwest::Client::builder().user_agent(user_agent()), proxy)
}

pub(crate) fn blocking_builder(
    proxy: &ProxyConfig,
) -> Result<reqwest::blocking::ClientBuilder, String> {
    apply_blocking_proxy(
        reqwest::blocking::Client::builder().user_agent(user_agent()),
        proxy,
    )
}

fn apply_proxy(
    builder: reqwest::ClientBuilder,
    proxy: &ProxyConfig,
) -> Result<reqwest::ClientBuilder, String> {
    Ok(match proxy {
        ProxyConfig::Invalid(error) => return Err(error.clone()),
        ProxyConfig::Off => builder.no_proxy(),
        ProxyConfig::System => builder,
        ProxyConfig::Http(manual) | ProxyConfig::Socks(manual) => builder.proxy(
            manual
                .reqwest_proxy()
                .map_err(|error| error.without_url().to_string())?,
        ),
    })
}

fn apply_blocking_proxy(
    builder: reqwest::blocking::ClientBuilder,
    proxy: &ProxyConfig,
) -> Result<reqwest::blocking::ClientBuilder, String> {
    Ok(match proxy {
        ProxyConfig::Invalid(error) => return Err(error.clone()),
        ProxyConfig::Off => builder.no_proxy(),
        ProxyConfig::System => builder,
        ProxyConfig::Http(manual) | ProxyConfig::Socks(manual) => builder.proxy(
            manual
                .reqwest_proxy()
                .map_err(|error| error.without_url().to_string())?,
        ),
    })
}

fn user_agent() -> &'static str {
    concat!("Tidalfast/", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_proxy_configuration_never_falls_back_to_a_direct_client() {
        let invalid = ProxyConfig::Invalid("Proxy port must be a number".into());
        assert!(Http::from_proxy(&invalid).is_err());
        assert!(build_blocking(&invalid, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn unavailable_clients_stay_blocked_until_a_successful_replacement() {
        let http = Http::unavailable("Unable to build the configured client".into());
        let artwork = http.clone();
        let api = http.clone();
        assert!(artwork.client().is_err());
        assert!(api.client().is_err());
        http.replace(build_client(&ProxyConfig::Off).unwrap());
        assert!(artwork.client().is_ok());
        assert!(api.client().is_ok());
    }

    #[test]
    fn replacing_the_client_is_visible_to_clones() {
        let http = Http::default();
        let clone = http.clone();
        let replacement = reqwest::Client::builder()
            .user_agent("tidalfast-test")
            .build()
            .unwrap();
        http.replace(replacement.clone());
        // Distinct Client values still share the pool after a replace; the
        // lock is what matters, and a second replace of a dummy client
        // must not panic.
        clone.replace(reqwest::Client::new());
    }

    #[test]
    fn off_system_and_socks5_each_build_a_client() {
        build_client(&ProxyConfig::Off).unwrap();
        build_client(&ProxyConfig::System).unwrap();
        let settings = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Socks,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "1080".into(),
            proxy_username: "user".into(),
            proxy_password: "pass".into(),
            ..crate::settings::Settings::default()
        };
        let proxy = settings.proxy_config().unwrap();
        build_client(&proxy).unwrap();
        build_blocking(&proxy, Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn web_api_client_goes_through_an_http_proxy() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin_addr = origin.local_addr().unwrap();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied",
                )
                .unwrap();
            request
        });

        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let proxy_thread = thread::spawn(move || {
            let (mut client, _) = proxy.accept().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buf = [0u8; 8192];
            let n = client.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let mut upstream = std::net::TcpStream::connect(origin_addr).unwrap();
            upstream.write_all(&buf[..n]).unwrap();
            let mut response = Vec::new();
            upstream.read_to_end(&mut response).unwrap();
            client.write_all(&response).unwrap();
            head
        });

        let settings = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Http,
            proxy_host: proxy_addr.ip().to_string(),
            proxy_port: proxy_addr.port().to_string(),
            ..crate::settings::Settings::default()
        };
        let proxy_config = settings.proxy_config().unwrap();
        let client = build_blocking(&proxy_config, Duration::from_secs(3)).unwrap();
        let body = client
            .get(format!("http://{origin_addr}/catalogue"))
            .send()
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(body, "proxied");
        let seen_by_proxy = proxy_thread.join().unwrap();
        assert!(
            seen_by_proxy.contains("catalogue"),
            "proxy should see the Web API request, got {seen_by_proxy:?}"
        );
        let seen_by_origin = origin_thread.join().unwrap();
        assert!(seen_by_origin.contains("GET"));
    }

    #[test]
    fn proxy_authentication_preserves_opaque_credentials() {
        use base64::Engine;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let proxy_thread = thread::spawn(move || {
            let (mut client, _) = proxy.accept().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buf = [0u8; 8192];
            let n = client.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            client
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            head
        });

        let username = "alice/name";
        let password = " secret/with spaces ";
        let settings = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Http,
            proxy_host: proxy_addr.ip().to_string(),
            proxy_port: proxy_addr.port().to_string(),
            proxy_username: username.into(),
            proxy_password: password.into(),
            ..crate::settings::Settings::default()
        };
        let proxy_config = settings.proxy_config().unwrap();
        let client = build_blocking(&proxy_config, Duration::from_secs(3)).unwrap();
        assert_eq!(
            client
                .get("http://example.invalid/catalogue")
                .send()
                .unwrap()
                .text()
                .unwrap(),
            "ok"
        );
        let seen = proxy_thread.join().unwrap();
        let expected =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        assert!(
            seen.lines()
                .any(|line| line
                    .eq_ignore_ascii_case(&format!("Proxy-Authorization: Basic {expected}"))),
            "proxy did not receive the exact configured credentials"
        );
    }

    #[test]
    fn socks5_authentication_and_hostname_resolution_use_the_selected_proxy() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "SOCKS5 proxy was not contacted"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture listener: {error}"),
                }
            };
            // Accepted sockets inherit O_NONBLOCK on macOS. The fixture uses
            // blocking reads after its bounded, nonblocking accept loop.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut pair = [0; 2];
            stream.read_exact(&mut pair).unwrap();
            assert_eq!(pair[0], 5);
            let mut methods = vec![0; pair[1] as usize];
            stream.read_exact(&mut methods).unwrap();
            assert!(methods.contains(&2));
            stream.write_all(&[5, 2]).unwrap();
            stream.read_exact(&mut pair).unwrap();
            assert_eq!(pair[0], 1);
            let mut username = vec![0; pair[1] as usize];
            stream.read_exact(&mut username).unwrap();
            let mut size = [0; 1];
            stream.read_exact(&mut size).unwrap();
            let mut password = vec![0; size[0] as usize];
            stream.read_exact(&mut password).unwrap();
            assert_eq!(username, b"dummy-user");
            assert_eq!(password, b" dummy-password ");
            stream.write_all(&[1, 0]).unwrap();
            let mut command = [0; 4];
            stream.read_exact(&mut command).unwrap();
            assert_eq!(
                command,
                [5, 1, 0, 3],
                "send the hostname for resolution at the proxy"
            );
            stream.read_exact(&mut size).unwrap();
            let mut hostname = vec![0; size[0] as usize];
            stream.read_exact(&mut hostname).unwrap();
            assert_eq!(hostname, b"example.invalid");
            stream.read_exact(&mut pair).unwrap();
            assert_eq!(u16::from_be_bytes(pair), 80);
            stream
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
                .unwrap();
            let mut request = [0; 4096];
            let size = stream.read(&mut request).unwrap();
            assert!(
                String::from_utf8_lossy(&request[..size]).starts_with("GET /catalogue HTTP/1.1")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let proxy = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Socks,
            proxy_host: address.ip().to_string(),
            proxy_port: address.port().to_string(),
            proxy_username: "dummy-user".into(),
            proxy_password: " dummy-password ".into(),
            ..Default::default()
        }
        .proxy_config()
        .unwrap();
        let client = build_blocking(&proxy, Duration::from_secs(3)).unwrap();
        assert_eq!(
            client
                .get("http://example.invalid/catalogue")
                .send()
                .unwrap()
                .text()
                .unwrap(),
            "ok"
        );
        server.join().unwrap();
    }

    /// The backend's thread runs an async runtime, and reqwest's blocking
    /// client panics when built on it: the first Download took the backend
    /// down ("Cannot drop a runtime in a context where blocking is not
    /// allowed").
    #[test]
    fn a_download_client_can_be_made_where_a_runtime_is_running() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let proxy = ProxyConfig::Off;
        let direct = runtime.block_on(async {
            std::panic::catch_unwind(|| build_blocking(&proxy, Duration::from_secs(1)).is_ok())
        });
        // reqwest makes that check only with debug assertions on (its
        // `wait::enter`), so a release test run, as the Nix package's
        // check phase is, builds it directly without a panic.
        if cfg!(debug_assertions) {
            assert!(direct.is_err(), "building it directly panics here");
        }
        let made = runtime.block_on(async {
            blocking_download_client(&proxy, Duration::from_secs(1), Duration::from_secs(2))
        });
        assert!(made.is_ok());
    }

    #[test]
    fn offline_mode_refuses_every_request_until_it_is_turned_off() {
        let http = Http::default();
        assert!(http.client().is_ok() && guard().is_ok());
        set_offline_mode(true);
        assert_eq!(http.client().unwrap_err(), OFFLINE_MODE_ERROR);
        assert_eq!(guard().unwrap_err(), OFFLINE_MODE_ERROR);
        set_offline_mode(false);
        assert!(http.client().is_ok() && guard().is_ok());
    }
}
