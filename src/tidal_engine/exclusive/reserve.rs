//! Asks the sound server to let go of a sound card.
//!
//! PipeWire (through WirePlumber) and PulseAudio hold every card they use
//! open, so a direct ALSA open of `hw:` fails as busy. Both follow the
//! desktop's device reservation protocol (`org.freedesktop.ReserveDevice1`,
//! the one JACK uses): whoever owns the bus name `…ReserveDevice1.AudioN`
//! holds card N, and is asked to give it up with `RequestRelease`. The
//! sound servers reserve their cards at low priority and release them to
//! anyone asking with a higher one, then take them back once the name is
//! free again. This holds the name while Tidalfast plays bit-perfect.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::fdo::{RequestNameFlags, RequestNameReply};

/// Above the sound servers' own (PulseAudio 0, WirePlumber -20) and below
/// JACK's, so a pro audio server can still take the card from us.
const PRIORITY: i32 = 10;
/// How long the whole exchange may take.
const TIMEOUT: Duration = Duration::from_secs(3);

/// What the reservation came to.
#[derive(Debug)]
pub(super) enum Outcome {
    /// Tidalfast holds the card; the sound server has let it go.
    Held(Reservation),
    /// Another program holds the card and would not let it go.
    Refused(String),
    /// No session bus, or nothing there speaks the protocol: the card is
    /// opened without asking.
    Unavailable,
}

/// The card's reservation, kept until dropped.
#[derive(Debug)]
pub(super) struct Reservation {
    release: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        drop(self.release.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn bus_name(card: i32) -> String {
    format!("org.freedesktop.ReserveDevice1.Audio{card}")
}

fn object_path(card: i32) -> String {
    format!("/org/freedesktop/ReserveDevice1/Audio{card}")
}

/// What other programs see of the reservation.
struct Reserved {
    device: String,
}

#[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
impl Reserved {
    /// Tidalfast lets the card go when playback stops, not on request: a
    /// program that wants it waits for the name to come free.
    fn request_release(&self, _priority: i32) -> bool {
        false
    }

    #[zbus(property)]
    fn priority(&self) -> i32 {
        PRIORITY
    }

    #[zbus(property)]
    fn application_name(&self) -> String {
        "Tidalfast".into()
    }

    #[zbus(property)]
    fn application_device_name(&self) -> String {
        self.device.clone()
    }
}

/// Reserves card `card` (its ALSA index), asking whoever holds it to let
/// go. `device` names the card for other programs.
pub(super) fn acquire(card: i32, device: &str) -> Outcome {
    let (answer, outcome) = mpsc::channel();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let device = device.to_string();
    let spawned = thread::Builder::new()
        .name("alsa-reserve".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    log::debug!("device reservation: no runtime: {error}");
                    let _ = answer.send(Err(None));
                    return;
                }
            };
            runtime.block_on(async move {
                match reserve(card, device).await {
                    Ok(connection) => {
                        let _ = answer.send(Ok(()));
                        // Held until the reservation is dropped.
                        let _ = released.await;
                        let _ = connection.release_name(bus_name(card)).await;
                    }
                    Err(refusal) => {
                        let _ = answer.send(Err(refusal));
                    }
                }
            });
        });
    let thread = match spawned {
        Ok(thread) => thread,
        Err(_) => return Outcome::Unavailable,
    };
    match outcome.recv_timeout(TIMEOUT) {
        Ok(Ok(())) => Outcome::Held(Reservation {
            release: Some(release),
            thread: Some(thread),
        }),
        Ok(Err(Some(owner))) => {
            let _ = thread.join();
            Outcome::Refused(owner)
        }
        Ok(Err(None)) => {
            let _ = thread.join();
            Outcome::Unavailable
        }
        Err(_) => {
            // The bus did not answer; the thread ends when its calls fail
            // or the release channel closes.
            drop(release);
            log::debug!("device reservation for card {card} timed out");
            Outcome::Unavailable
        }
    }
}

/// `Err(Some(owner))`: the holder would not let go; `Err(None)`: the
/// protocol is not available.
async fn reserve(card: i32, device: String) -> Result<zbus::Connection, Option<String>> {
    let unavailable = |error: zbus::Error| {
        log::debug!("device reservation for card {card}: {error}");
        None
    };
    let name = bus_name(card);
    let path = object_path(card);
    let connection = zbus::connection::Builder::session()
        .and_then(|builder| builder.serve_at(path.as_str(), Reserved { device }))
        .map_err(unavailable)?
        .build()
        .await
        .map_err(unavailable)?;
    let flags = RequestNameFlags::DoNotQueue.into();
    if owned(
        connection
            .request_name_with_flags(name.as_str(), flags)
            .await,
    )
    .map_err(unavailable)?
    {
        return Ok(connection);
    }
    // Someone holds the card: ask them to let it go.
    let owner = zbus::Proxy::new(
        &connection,
        name.as_str(),
        path.as_str(),
        "org.freedesktop.ReserveDevice1",
    )
    .await
    .map_err(unavailable)?;
    let holder: String = owner
        .get_property("ApplicationName")
        .await
        .unwrap_or_else(|_| "another program".into());
    let released: bool = match owner.call("RequestRelease", &(PRIORITY,)).await {
        Ok(released) => released,
        Err(error) => {
            log::debug!("device reservation: {holder} did not answer: {error}");
            false
        }
    };
    if !released {
        return Err(Some(holder));
    }
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting;
    if owned(
        connection
            .request_name_with_flags(name.as_str(), flags)
            .await,
    )
    .map_err(unavailable)?
    {
        log::info!("{holder} released sound card {card} for bit-perfect playback");
        Ok(connection)
    } else {
        Err(Some(holder))
    }
}

/// Whether a name request made the connection the name's owner. zbus
/// reports a name someone else keeps as `NameTaken`.
fn owned(reply: zbus::Result<RequestNameReply>) -> zbus::Result<bool> {
    match reply {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(true),
        Ok(RequestNameReply::Exists | RequestNameReply::InQueue) | Err(zbus::Error::NameTaken) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sound server holding a card, letting it go when asked or not.
    struct Holder {
        releases: bool,
    }

    #[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
    impl Holder {
        fn request_release(&self, priority: i32) -> bool {
            self.releases && priority > 0
        }

        #[zbus(property)]
        fn application_name(&self) -> String {
            "Fake sound server".into()
        }
    }

    async fn hold(card: i32, releases: bool) -> zbus::Connection {
        let connection = zbus::connection::Builder::session()
            .unwrap()
            .serve_at(object_path(card), Holder { releases })
            .unwrap()
            .build()
            .await
            .unwrap();
        let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
        connection
            .request_name_with_flags(bus_name(card), flags)
            .await
            .unwrap();
        connection
    }

    /// Runs on a private session bus, so no real sound server is asked.
    #[test]
    fn a_sound_server_lets_the_card_go_or_says_who_keeps_it() {
        const CHILD: &str = "TIDALFAST_RESERVE_PRIVATE_BUS";
        if std::env::var_os(CHILD).is_none() {
            let root = std::env::temp_dir().join(format!(
                "tidalfast-reserve-bus-{:016x}",
                rand::random::<u64>()
            ));
            std::fs::create_dir(&root).unwrap();
            let config = root.join("session.conf");
            std::fs::write(&config, r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>"#).unwrap();
            let result = std::process::Command::new("dbus-run-session")
                .arg("--config-file")
                .arg(&config)
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tidal_engine::exclusive::reserve::tests::a_sound_server_lets_the_card_go_or_says_who_keeps_it",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output();
            let _ = std::fs::remove_dir_all(root);
            let Ok(result) = result else {
                eprintln!("dbus-run-session is not installed; skipped");
                return;
            };
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        // Nobody holds card 97: it is simply ours.
        assert!(matches!(acquire(97, "Free card"), Outcome::Held(_)));
        // Card 98's holder lets it go; the name is ours until dropped.
        let _server = runtime.block_on(hold(98, true));
        let held = acquire(98, "Test card");
        assert!(matches!(held, Outcome::Held(_)), "{held:?}");
        drop(held);
        // Card 99's holder keeps it and is named.
        let _stubborn = runtime.block_on(hold(99, false));
        match acquire(99, "Test card") {
            Outcome::Refused(holder) => assert_eq!(holder, "Fake sound server"),
            other => panic!("{other:?}"),
        }
    }
}
