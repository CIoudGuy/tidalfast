//! Sockets for plugin `.native` code (`net.createConnection`): TCP, and local
//! IPC to Discord (Windows named pipes `\\?\pipe\discord-ipc-N`, Unix
//! sockets named `discord-ipc-N`), or to another program's own pipe or
//! socket once the user allowed that plugin that path (`permits.rs`; the
//! runtime checks it before [`NetHub::connect`]). Each connection runs on its own thread
//! with a small current-thread Tokio runtime and reports back to the Luna
//! thread as [`NetEvent`]s.

use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// What happened on a socket, for `__luna_net(handle, kind, data)`.
#[derive(Debug)]
pub struct NetEvent {
    pub handle: u32,
    /// "connect", "data" (base64 `data`), "end", "error" (message), "close";
    /// for listeners also "listening" (`data` = port) and "connection"
    /// (`data` = "<new handle> <peer ip> <peer port>").
    pub kind: &'static str,
    pub data: String,
    /// For "connection": the new socket, to register with [`NetHub::adopt`].
    pub accepted: Option<Accepted>,
}

/// A socket a listener accepted, not yet known to the [`NetHub`].
#[derive(Debug)]
pub struct Accepted {
    pub handle: u32,
    tx: mpsc::UnboundedSender<Cmd>,
}

impl NetEvent {
    fn new(handle: u32, kind: &'static str, data: String) -> Self {
        NetEvent {
            handle,
            kind,
            data,
            accepted: None,
        }
    }
}

#[derive(Debug)]
enum Cmd {
    Write(Vec<u8>),
    Close,
}

struct Conn {
    plugin: String,
    tx: mpsc::UnboundedSender<Cmd>,
}

/// Where to connect.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Tcp { host: String, port: u16 },
    Ipc(String),
}

/// Only Discord's IPC endpoints on this computer are reachable as local
/// sockets/pipes.
pub fn check_ipc_path(path: &str) -> Result<(), String> {
    if is_discord_ipc_path(path) {
        Ok(())
    } else {
        Err(format!(
            "EACCES: plugins may only open Discord IPC pipes on this computer, not '{path}'"
        ))
    }
}

/// `discord-ipc-N`.
fn is_discord_ipc_name(name: &str) -> bool {
    name.strip_prefix("discord-ipc-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()))
}

/// A Discord IPC endpoint on this computer. On Windows: a pipe in the local
/// pipe namespace (`\\.\pipe\discord-ipc-N` or `\\?\pipe\...`); a UNC path
/// such as `\\host\pipe\discord-ipc-0` never is one, since opening it makes
/// Windows sign in to that host with the user's credentials. On Unix: a
/// socket directly in the runtime or temp directory, or in a sandboxed
/// Discord's folder there.
pub fn is_discord_ipc_path(path: &str) -> bool {
    #[cfg(windows)]
    {
        let p = path.replace('/', "\\").to_ascii_lowercase();
        [r"\\.\pipe\", r"\\?\pipe\"]
            .iter()
            .any(|prefix| p.strip_prefix(prefix).is_some_and(is_discord_ipc_name))
    }
    #[cfg(not(windows))]
    {
        use std::path::{Component, Path};
        let p = Path::new(path);
        if !p.is_absolute()
            || p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return false;
        }
        let (Some(parent), Some(name)) = (p.parent(), p.file_name().and_then(|n| n.to_str()))
        else {
            return false;
        };
        is_discord_ipc_name(name) && super::fsops::ipc_dirs().iter().any(|d| d == parent)
    }
}

#[derive(Default)]
pub struct NetHub {
    next: std::sync::Arc<std::sync::atomic::AtomicU32>,
    conns: HashMap<u32, Conn>,
    /// Connections to another program's pipe, by handle: (plugin, the
    /// pipe's key as allowed), so a revoke closes exactly those.
    pipes: HashMap<u32, (String, String)>,
}

/// Why a free-looking port cannot be bound: on Windows, error 10013
/// (WSAEACCES) on bind means the port is in a range Windows reserved
/// (`netsh int ipv4 show excludedportrange protocol=tcp`; Hyper-V, WSL and
/// Docker reserve such ranges). TIDAL's plugins fail the same way there.
fn reserved_port_hint(e: &std::io::Error) -> &'static str {
    if cfg!(windows) && e.raw_os_error() == Some(10013) {
        " (Windows reserves this port for another use, often Hyper-V, WSL or Docker; see `netsh int ipv4 show excludedportrange protocol=tcp`. Pick another port in the plugin's settings if it has one)"
    } else {
        ""
    }
}

/// Plugin servers only ever listen on the loopback interface.
pub const LISTEN_HOST: &str = "127.0.0.1";

/// Sockets and servers one plugin may have open at once (each holds a
/// thread). Discord RPC needs one; a local server a handful.
pub const MAX_SOCKETS_PER_PLUGIN: usize = 64;

impl NetHub {
    fn open_by(&self, plugin: &str) -> usize {
        self.conns.values().filter(|c| c.plugin == plugin).count()
    }

    fn room_for(&self, plugin: &str) -> Result<(), String> {
        if self.open_by(plugin) >= MAX_SOCKETS_PER_PLUGIN {
            return Err(format!(
                "EMFILE: too many open sockets (a plugin may have {MAX_SOCKETS_PER_PLUGIN})"
            ));
        }
        Ok(())
    }

    /// Start connecting; events arrive through `notify`.
    /// `pipe`: the key of another program's pipe the user allowed (the
    /// caller checked it); without one only Discord's IPC paths connect.
    pub fn connect(
        &mut self,
        plugin: &str,
        target: Target,
        pipe: Option<String>,
        notify: impl Fn(NetEvent) + Send + 'static,
    ) -> Result<u32, String> {
        if let Target::Ipc(p) = &target
            && pipe.is_none()
        {
            check_ipc_path(p)?;
        }
        self.room_for(plugin)?;
        let handle = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::unbounded_channel();
        self.conns.insert(
            handle,
            Conn {
                plugin: plugin.to_string(),
                tx,
            },
        );
        if let Some(key) = pipe {
            self.pipes.insert(handle, (plugin.to_string(), key));
        }
        std::thread::Builder::new()
            .name(format!("luna-net-{handle}"))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        notify(NetEvent::new(handle, "error", e.to_string()));
                        notify(NetEvent::new(handle, "close", String::new()));
                        return;
                    }
                };
                rt.block_on(run(handle, target, rx, &notify));
                notify(NetEvent::new(handle, "close", String::new()));
            })
            .map_err(|e| e.to_string())?;
        Ok(handle)
    }

    /// Listen on `127.0.0.1:port` (0 = any free port). Accepted sockets
    /// arrive as "connection" events; register them with [`NetHub::adopt`].
    pub fn listen(
        &mut self,
        plugin: &str,
        port: u16,
        notify: impl Fn(NetEvent) + Send + Sync + Clone + 'static,
    ) -> Result<u32, String> {
        self.room_for(plugin)?;
        let handle = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let next = self.next.clone();
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.conns.insert(
            handle,
            Conn {
                plugin: plugin.to_string(),
                tx,
            },
        );
        std::thread::Builder::new()
            .name(format!("luna-listen-{handle}"))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        notify(NetEvent::new(handle, "error", e.to_string()));
                        notify(NetEvent::new(handle, "close", String::new()));
                        return;
                    }
                };
                rt.block_on(async {
                    let l = match tokio::net::TcpListener::bind((LISTEN_HOST, port)).await {
                        Ok(l) => l,
                        Err(e) => {
                            let code = if e.kind() == std::io::ErrorKind::AddrInUse {
                                "EADDRINUSE"
                            } else {
                                "EACCES"
                            };
                            notify(NetEvent::new(
                                handle,
                                "error",
                                format!(
                                    "{code}: listen {LISTEN_HOST}:{port}: {e}{}",
                                    reserved_port_hint(&e)
                                ),
                            ));
                            return;
                        }
                    };
                    let port = l.local_addr().map(|a| a.port()).unwrap_or(port);
                    notify(NetEvent::new(handle, "listening", port.to_string()));
                    loop {
                        tokio::select! {
                            a = l.accept() => match a {
                                Ok((stream, peer)) => {
                                    let h = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                    let (ctx, crx) = mpsc::unbounded_channel();
                                    notify(NetEvent {
                                        handle,
                                        kind: "connection",
                                        data: format!("{h} {} {}", peer.ip(), peer.port()),
                                        accepted: Some(Accepted { handle: h, tx: ctx }),
                                    });
                                    let n2 = notify.clone();
                                    let _ = std::thread::Builder::new()
                                        .name(format!("luna-net-{h}"))
                                        .spawn(move || {
                                            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                                                .enable_io()
                                                .enable_time()
                                                .build()
                                            {
                                                rt.block_on(pump(h, Box::new(stream), crx, &n2));
                                            }
                                            n2(NetEvent::new(h, "close", String::new()));
                                        });
                                }
                                Err(e) => {
                                    notify(NetEvent::new(handle, "error", e.to_string()));
                                    break;
                                }
                            },
                            cmd = rx.recv() => match cmd {
                                Some(Cmd::Close) | None => break,
                                Some(Cmd::Write(_)) => {}
                            },
                        }
                    }
                });
                notify(NetEvent::new(handle, "close", String::new()));
            })
            .map_err(|e| e.to_string())?;
        Ok(handle)
    }

    /// Register a socket a listener accepted (owned by the listener's
    /// plugin). When the listener was closed meanwhile (its plugin stopped),
    /// the socket is closed: owned by nobody, nothing would ever close it.
    pub fn adopt(&mut self, listener: u32, a: Accepted) {
        match self.conns.get(&listener).map(|c| c.plugin.clone()) {
            // Over the plugin's share: refused like a full backlog.
            Some(plugin) if self.open_by(&plugin) >= MAX_SOCKETS_PER_PLUGIN => {
                let _ = a.tx.send(Cmd::Close);
            }
            Some(plugin) => {
                self.conns.insert(a.handle, Conn { plugin, tx: a.tx });
            }
            None => {
                let _ = a.tx.send(Cmd::Close);
            }
        }
    }

    /// The socket is open and belongs to `plugin`.
    pub fn owned_by(&self, handle: u32, plugin: &str) -> bool {
        self.conns.get(&handle).is_some_and(|c| c.plugin == plugin)
    }

    pub fn write(&mut self, handle: u32, data: Vec<u8>) -> Result<(), String> {
        let c = self.conns.get(&handle).ok_or("EPIPE: socket is closed")?;
        c.tx.send(Cmd::Write(data))
            .map_err(|_| "EPIPE: socket is closed".to_string())
    }

    pub fn close(&mut self, handle: u32) {
        if let Some(c) = self.conns.remove(&handle) {
            let _ = c.tx.send(Cmd::Close);
        }
    }

    /// The socket's thread finished.
    pub fn forget(&mut self, handle: u32) {
        self.conns.remove(&handle);
        self.pipes.remove(&handle);
    }

    /// The user took back `plugin`'s permission for pipe `key`: its
    /// connections there close.
    pub fn close_pipe(&mut self, plugin: &str, key: &str) {
        let handles: Vec<u32> = self
            .pipes
            .iter()
            .filter(|(_, (p, k))| p == plugin && k == key)
            .map(|(h, _)| *h)
            .collect();
        for h in handles {
            self.close(h);
        }
    }

    pub fn close_plugin(&mut self, plugin: &str) {
        let handles: Vec<u32> = self
            .conns
            .iter()
            .filter(|(_, c)| c.plugin == plugin)
            .map(|(h, _)| *h)
            .collect();
        for h in handles {
            self.close(h);
        }
    }
}

trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

async fn open(target: &Target) -> std::io::Result<Box<dyn Stream>> {
    match target {
        Target::Tcp { host, port } => {
            let s = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                tokio::net::TcpStream::connect((host.as_str(), *port)),
            )
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timed out")
            })??;
            Ok(Box::new(s))
        }
        Target::Ipc(path) => open_ipc(path).await,
    }
}

#[cfg(windows)]
async fn open_ipc(path: &str) -> std::io::Result<Box<dyn Stream>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let path = path.replace('/', "\\");
    // ERROR_PIPE_BUSY: all instances busy, try again shortly.
    for _ in 0..20 {
        match ClientOptions::new().open(&path) {
            Ok(c) => return Ok(Box::new(c)),
            Err(e) if e.raw_os_error() == Some(231) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("pipe busy"))
}

#[cfg(unix)]
async fn open_ipc(path: &str) -> std::io::Result<Box<dyn Stream>> {
    let s = tokio::net::UnixStream::connect(std::path::Path::new(path)).await?;
    Ok(Box::new(s))
}

#[cfg(not(any(windows, unix)))]
async fn open_ipc(_path: &str) -> std::io::Result<Box<dyn Stream>> {
    Err(std::io::Error::other(
        "IPC is not supported on this platform",
    ))
}

async fn run(
    handle: u32,
    target: Target,
    rx: mpsc::UnboundedReceiver<Cmd>,
    notify: &(impl Fn(NetEvent) + Send),
) {
    let stream = match open(&target).await {
        Ok(s) => s,
        Err(e) => {
            notify(NetEvent::new(handle, "error", format!("connect {e}")));
            return;
        }
    };
    notify(NetEvent::new(handle, "connect", String::new()));
    pump(handle, stream, rx, notify).await;
}

/// Move bytes between a connected stream and the plugin until either side
/// closes.
async fn pump(
    handle: u32,
    stream: Box<dyn Stream>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    notify: &(impl Fn(NetEvent) + Send),
) {
    use base64::Engine;
    let (mut r, mut w) = tokio::io::split(stream);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            n = r.read(&mut buf) => match n {
                Ok(0) => {
                    notify(NetEvent::new(handle, "end", String::new()));
                    break;
                }
                Ok(n) => notify(NetEvent::new(
                    handle,
                    "data",
                    base64::engine::general_purpose::STANDARD.encode(&buf[..n]),
                )),
                Err(e) => {
                    notify(NetEvent::new(handle, "error", e.to_string()));
                    break;
                }
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Write(d)) => {
                    if let Err(e) = w.write_all(&d).await {
                        notify(NetEvent::new(handle, "error", e.to_string()));
                        break;
                    }
                    let _ = w.flush().await;
                }
                Some(Cmd::Close) | None => {
                    let _ = w.shutdown().await;
                    break;
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discord_pipes_are_allowed() {
        assert!(check_ipc_path("/var/run/docker.sock").is_err());
        assert!(check_ipc_path(r"\\.\pipe\discord-ipc-x").is_err());
        assert!(check_ipc_path(r"\\.\pipe\discord-ipc-").is_err());
        if cfg!(windows) {
            assert!(check_ipc_path(r"\\?\pipe\discord-ipc-0").is_ok());
            assert!(check_ipc_path(r"\\.\pipe\discord-ipc-9").is_ok());
            assert!(check_ipc_path("//./pipe/discord-ipc-1").is_ok());
        } else {
            let dir = std::env::temp_dir();
            let ok = dir.join("discord-ipc-3");
            assert!(check_ipc_path(&ok.to_string_lossy()).is_ok());
            let flatpak = dir.join("app/com.discordapp.Discord/discord-ipc-0");
            assert!(check_ipc_path(&flatpak.to_string_lossy()).is_ok());
            let elsewhere = dir.join("x/discord-ipc-0");
            assert!(check_ipc_path(&elsewhere.to_string_lossy()).is_err());
            assert!(check_ipc_path("/home/u/discord-ipc-0").is_err());
        }
    }

    /// A remote pipe (UNC) would make Windows authenticate to that host with
    /// the user's credentials: it is refused before anything is opened,
    /// whatever its last component says.
    #[test]
    fn network_paths_are_never_ipc_endpoints() {
        for p in [
            r"\\evil.example\pipe\discord-ipc-0",
            "//evil.example/pipe/discord-ipc-0",
            r"\\?\UNC\evil.example\pipe\discord-ipc-0",
            r"\\.\pipe\..\..\evil.example\pipe\discord-ipc-0",
            r"\\.\pipe\x\discord-ipc-0",
            r"\\evil.example\share\discord-ipc-0",
            "discord-ipc-0",
        ] {
            assert!(!is_discord_ipc_path(p), "{p}");
            assert!(check_ipc_path(p).is_err(), "{p}");
        }
    }

    /// A connection accepted just as its listener closed is closed too, not
    /// left open with no owner (`close_plugin` would never find it).
    #[test]
    fn a_socket_accepted_after_its_listener_closed_is_closed() {
        let mut hub = NetHub::default();
        let (ltx, _lrx) = mpsc::unbounded_channel();
        hub.conns.insert(
            1,
            Conn {
                plugin: "P".into(),
                tx: ltx,
            },
        );
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.adopt(1, Accepted { handle: 2, tx });
        assert_eq!(hub.conns.get(&2).map(|c| c.plugin.as_str()), Some("P"));
        hub.close_plugin("P");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Close)));

        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.adopt(1, Accepted { handle: 3, tx });
        assert!(!hub.conns.contains_key(&3));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Close)));
    }

    /// M4: a plugin's open sockets are capped (each holds a thread):
    /// connecting, listening and accepting past the cap are refused, and
    /// another plugin is not affected.
    #[test]
    fn a_plugins_open_sockets_are_capped() {
        let mut hub = NetHub::default();
        let mut keep = Vec::new();
        for h in 0..MAX_SOCKETS_PER_PLUGIN as u32 {
            let (tx, rx) = mpsc::unbounded_channel();
            keep.push(rx);
            hub.conns.insert(
                h + 100,
                Conn {
                    plugin: "P".into(),
                    tx,
                },
            );
        }
        let e = hub
            .connect(
                "P",
                Target::Tcp {
                    host: "127.0.0.1".into(),
                    port: 9,
                },
                None,
                |_| {},
            )
            .unwrap_err();
        assert!(e.starts_with("EMFILE"), "{e}");
        assert!(
            hub.listen("P", 0, |_| {})
                .unwrap_err()
                .starts_with("EMFILE")
        );
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.adopt(100, Accepted { handle: 5000, tx });
        assert!(!hub.conns.contains_key(&5000));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Close)));
        // Another plugin still connects (to a closed port: the error comes
        // later as an event, not as a refusal).
        let h = hub
            .connect(
                "Q",
                Target::Tcp {
                    host: "127.0.0.1".into(),
                    port: 9,
                },
                None,
                |_| {},
            )
            .unwrap();
        hub.close(h);
    }

    #[test]
    fn a_reserved_windows_port_says_why() {
        let hint = reserved_port_hint(&std::io::Error::from_raw_os_error(10013));
        assert_eq!(hint.contains("excludedportrange"), cfg!(windows));
        let other = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        assert_eq!(reserved_port_hint(&other), "");
    }
}
