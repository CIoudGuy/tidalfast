//! Single-instance guard and remote-control channel.
//!
//! A second instance would duplicate the MPRIS player
//! and tray icon. A second launch hands its request to the running instance
//! and exits.
//!
//! The guard is fastframe-instance's slot: an exclusive lock on a file in a
//! private per-user directory, which the system releases when the process
//! ends, even after a crash. Requests travel over a socket in that directory
//! that only the user can open (on Linux and macOS), or on Windows a loopback
//! port that answers only requests carrying the random token the running
//! instance writes beside the lock.
//!
//! Clients send one `tidalfast:<verb>` line and receive one reply. Commands
//! enter the same action queue as tray and media-key events. Read commands use
//! snapshots, so the listener thread never accesses app state. The
//! `tidalfast` command-line subcommands are clients of this channel; MPRIS
//! remains for media keys and desktop players on Linux.
//!
//! Clients poll the current snapshot; the app does not push updates.
//! `play-uri`, `open-link`, and `transfer` validate their free-text arguments
//! here before anything reaches the app.
//!
//! A TIDAL link the desktop hands to a second launch reaches the running
//! instance the same way, as `open-link`.

/// The name every request and reply starts with, so a copy of another app
/// never obeys Tidalfast's requests.
const NAME: &str = "tidalfast";

/// The reply to an accepted command.
const OK_REPLY: &str = "ok";
/// The reply to `nowplaying`, before the snapshot.
const NOW_REPLY: &str = "now ";

pub enum Outcome {
    /// This process is the only instance. Hold the guard until it exits.
    Only(Guard),
    /// Another instance is running and took the request to show its window
    /// or open a link.
    Surfaced,
}

/// What a control client asked the running instance to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlCommand {
    /// Bring the window forward, creating it if needed.
    Show,
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Milliseconds; negative seeks backwards.
    SeekBy(i64),
    /// Percentage points; negative lowers the volume.
    VolumeBy(i8),
    /// Absolute percentage.
    SetVolume(u8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    /// Set shuffle explicitly, avoiding missed toggle updates.
    SetShuffle(bool),
    /// Repeat set outright, for the same reason.
    SetRepeat(crate::player::RepeatMode),
    /// Absolute position, in milliseconds.
    SeekTo(u32),
    /// Save the playing track to the library, or take it back out.
    ToggleSaved,
    /// Play a `tidal:` URI: a track, album, playlist, artist, or mix.
    PlayUri(String),
    /// Search TIDAL for a track, album, artist, or playlist, and play the
    /// best match.
    PlaySearch {
        kind: String,
        query: String,
    },
    /// Open the page for a TIDAL link the desktop or another launch
    /// handed over, and bring the window forward.
    OpenLink(String),
    /// A `tidaluna://` link (TidaLuna's scheme, a plugin's sign-in
    /// redirect): delivered to plugins only, never opened as a TIDAL page.
    PluginLink(String),
}

/// Marks this process as the running instance until dropped.
pub struct Guard {
    /// The slot's lock; `None` when running unguarded.
    _slot: Option<fastframe_instance::Guard>,
    /// Filled by control clients, drained by the app every frame.
    commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    /// Current-track snapshot for `nowplaying` requests.
    now_playing: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Guard {
    fn unguarded() -> Self {
        Self {
            _slot: None,
            commands: Default::default(),
            now_playing: std::sync::Arc::new(std::sync::Mutex::new(NOTHING_PLAYING.to_owned())),
        }
    }

    /// The queue a control client's commands land in. The app drains it.
    pub fn commands(&self) -> std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>> {
        std::sync::Arc::clone(&self.commands)
    }

    /// The slot the app writes the now-playing snapshot into.
    pub fn now_playing_slot(&self) -> std::sync::Arc<std::sync::Mutex<String>> {
        std::sync::Arc::clone(&self.now_playing)
    }
}

/// Snapshot value reported when nothing is playing.
pub const NOTHING_PLAYING: &str = "stopped";

/// Where the running instance's lock and channel live: the per-user
/// runtime directory on Linux (the app's own inside Flatpak), the user's
/// private temporary directory on macOS, where a socket path under
/// Application Support can outgrow the 104 bytes macOS allows with a long
/// user name, and beside Tidalfast's state on Windows.
fn slot() -> fastframe_instance::Slot {
    #[cfg(not(windows))]
    {
        // A named test profile has a slot of its own, so it runs beside
        // the default profile instead of handing its launch over.
        match crate::paths::profile() {
            Some(profile) => {
                fastframe_instance::Slot::new(&crate::paths::profile_app_name(Some(profile)))
            }
            None => fastframe_instance::Slot::new(NAME),
        }
    }
    #[cfg(windows)]
    {
        fastframe_instance::Slot::at(
            crate::paths::AppDirs::discover().state.join("instance"),
            NAME,
        )
    }
}

/// What the running instance said back.
pub enum Reply {
    /// The command was accepted.
    Ok,
    /// The `nowplaying` snapshot: [`NOTHING_PLAYING`], or tab-separated
    /// `state, title, artists, album, position_ms, duration_ms, volume,
    /// shuffle, repeat, art_url, saved, device`.
    NowPlaying(String),
}

/// Sends one verb to the running instance and reads its reply.
pub fn send(verb: &str) -> std::io::Result<Reply> {
    reply(&slot().send(verb)?)
}

/// Reads the running instance's reply, without the `tidalfast:` prefix the
/// channel already checked.
fn reply(line: &str) -> std::io::Result<Reply> {
    if line == OK_REPLY {
        Ok(Reply::Ok)
    } else if let Some(snapshot) = line.strip_prefix(NOW_REPLY) {
        Ok(Reply::NowPlaying(snapshot.to_owned()))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the running Tidalfast answered something unexpected",
        ))
    }
}

/// Claims the running-instance role, or hands `link` (a canonical
/// `tidal:` URI, see [`crate::link::parse`], or a `tidaluna://` link for
/// plugins, see [`crate::link::plugin_link`]) to the instance that has it
/// and asks that one to come forward.
pub fn acquire(waker: &crate::backend::Waker, link: Option<&str>) -> Outcome {
    claim(&slot(), waker, link)
}

fn claim(
    slot: &fastframe_instance::Slot,
    waker: &crate::backend::Waker,
    link: Option<&str>,
) -> Outcome {
    let mut guard = Guard::unguarded();
    let request = match link {
        Some(uri) if crate::link::is_plugin_link(uri) => format!("plugin-link {uri}"),
        Some(uri) => format!("open-link {uri}"),
        None => "show".to_owned(),
    };
    let handler = handler(
        std::sync::Arc::clone(&guard.commands),
        std::sync::Arc::clone(&guard.now_playing),
        waker.clone(),
    );
    match slot.claim(&request, handler) {
        fastframe_instance::Claim::First(slot) => {
            guard._slot = Some(slot);
            Outcome::Only(guard)
        }
        fastframe_instance::Claim::Running(_) => Outcome::Surfaced,
        fastframe_instance::Claim::Declined => {
            log::warn!("Tidalfast is already running and declined this launch's request");
            Outcome::Surfaced
        }
        fastframe_instance::Claim::Unanswered => {
            log::warn!(
                "Tidalfast is already running but did not answer; not starting a second copy"
            );
            Outcome::Surfaced
        }
    }
}

/// Answers one control request on the channel's thread: queues commands for
/// the app and wakes it, and answers reads from the published snapshots.
/// `None` refuses a request that is not ours, so the client gets no reply.
fn handler(
    commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    now_playing: std::sync::Arc<std::sync::Mutex<String>>,
    waker: crate::backend::Waker,
) -> impl FnMut(&str) -> Option<String> + Send + 'static {
    move |request| {
        let queue = |command| {
            commands
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(command);
            waker.wake();
        };
        match parse(request)? {
            Request::Command(command) => {
                // The verb only: a link's address stays out of the log.
                let verb = request.split_whitespace().next().unwrap_or_default();
                log::info!("another launch or a control command asked for {verb}");
                queue(command);
                Some(OK_REPLY.to_owned())
            }
            Request::NowPlaying => {
                let snapshot = now_playing
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                Some(format!("{NOW_REPLY}{snapshot}"))
            }
        }
    }
}

/// A parsed request line: a command for the app, or a read the listener
/// answers itself.
enum Request {
    Command(ControlCommand),
    NowPlaying,
}

/// Reads one request line, the channel having already checked and removed
/// its `tidalfast:` prefix.
fn parse(line: &str) -> Option<Request> {
    let verb = line.trim_end();
    let (verb, argument) = match verb.split_once(' ') {
        Some((verb, argument)) => (verb, Some(argument.trim())),
        None => (verb, None),
    };
    let command = match (verb, argument) {
        ("show", None) => ControlCommand::Show,
        ("playpause", None) => ControlCommand::PlayPause,
        ("play", None) => ControlCommand::Play,
        ("pause", None) => ControlCommand::Pause,
        ("next", None) => ControlCommand::Next,
        ("previous", None) => ControlCommand::Previous,
        ("seek-by", Some(ms)) => ControlCommand::SeekBy(ms.parse().ok()?),
        ("seek-to", Some(ms)) => ControlCommand::SeekTo(ms.parse().ok()?),
        ("volume-by", Some(delta)) => ControlCommand::VolumeBy(delta.parse().ok()?),
        ("volume-set", Some(volume)) => ControlCommand::SetVolume(volume.parse().ok()?),
        ("mute", None) => ControlCommand::ToggleMute,
        ("shuffle", None) => ControlCommand::ToggleShuffle,
        ("shuffle-set", Some("on")) => ControlCommand::SetShuffle(true),
        ("shuffle-set", Some("off")) => ControlCommand::SetShuffle(false),
        ("repeat", None) => ControlCommand::CycleRepeat,
        // Match explicitly because `RepeatMode::from_api` maps unknown values
        // to `off`; control clients should reject them.
        ("repeat-set", Some("off")) => ControlCommand::SetRepeat(crate::player::RepeatMode::Off),
        ("repeat-set", Some("context")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Context)
        }
        ("repeat-set", Some("track")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Track)
        }
        ("save-toggle", None) => ControlCommand::ToggleSaved,
        ("play-uri", Some(uri)) => ControlCommand::PlayUri(tidal_uri(uri)?),
        ("play-search", Some(argument)) => {
            let (kind, query) = argument.split_once(' ')?;
            if !matches!(kind, "track" | "album" | "artist" | "playlist") {
                return None;
            }
            ControlCommand::PlaySearch {
                kind: kind.to_owned(),
                query: search_text(query)?,
            }
        }
        ("open-link", Some(link)) => ControlCommand::OpenLink(crate::link::parse(link)?),
        ("plugin-link", Some(link)) => ControlCommand::PluginLink(crate::link::plugin_link(link)?),
        ("nowplaying", None) => return Some(Request::NowPlaying),
        _ => return None,
    };
    Some(Request::Command(command))
}

/// Validates the scheme, length, and characters of a TIDAL URI received over
/// the control channel.
fn tidal_uri(text: &str) -> Option<String> {
    let shaped = text.starts_with("tidal:")
        && text.len() <= 128
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.' | '%' | '+'));
    shaped.then(|| text.to_owned())
}

/// Words to search for, checked like the other free-text arguments: not
/// empty, not long, and without control characters.
fn search_text(text: &str) -> Option<String> {
    let text = text.trim();
    let shaped =
        !text.is_empty() && text.chars().count() <= 200 && !text.chars().any(char::is_control);
    shaped.then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::RepeatMode;

    fn command(line: &str) -> Option<ControlCommand> {
        match parse(line) {
            Some(Request::Command(command)) => Some(command),
            _ => None,
        }
    }

    #[test]
    fn parses_every_control_verb() {
        // #given / #when / #then
        assert_eq!(command("show\n"), Some(ControlCommand::Show));
        assert_eq!(command("playpause"), Some(ControlCommand::PlayPause));
        assert_eq!(command("play"), Some(ControlCommand::Play));
        assert_eq!(command("pause"), Some(ControlCommand::Pause));
        assert_eq!(command("next"), Some(ControlCommand::Next));
        assert_eq!(command("previous"), Some(ControlCommand::Previous));
        assert_eq!(
            command("seek-by -10000"),
            Some(ControlCommand::SeekBy(-10_000))
        );
        assert_eq!(command("volume-by +5"), Some(ControlCommand::VolumeBy(5)));
        assert_eq!(
            command("volume-set 40"),
            Some(ControlCommand::SetVolume(40))
        );
        assert_eq!(command("mute"), Some(ControlCommand::ToggleMute));
        assert_eq!(command("shuffle"), Some(ControlCommand::ToggleShuffle));
        assert_eq!(command("repeat"), Some(ControlCommand::CycleRepeat));
        assert_eq!(
            command("shuffle-set on"),
            Some(ControlCommand::SetShuffle(true))
        );
        assert_eq!(
            command("shuffle-set off"),
            Some(ControlCommand::SetShuffle(false))
        );
        assert_eq!(
            command("repeat-set track"),
            Some(ControlCommand::SetRepeat(RepeatMode::Track))
        );
        assert_eq!(
            command("repeat-set context"),
            Some(ControlCommand::SetRepeat(RepeatMode::Context))
        );
        assert_eq!(
            command("repeat-set off"),
            Some(ControlCommand::SetRepeat(RepeatMode::Off))
        );
        assert_eq!(
            command("seek-to 90000"),
            Some(ControlCommand::SeekTo(90_000))
        );
        assert_eq!(command("save-toggle"), Some(ControlCommand::ToggleSaved));
        assert_eq!(
            command("play-uri tidal:playlist:0e5f6c2a-1b2c-4d3e-9f40-123456789abc"),
            Some(ControlCommand::PlayUri(
                "tidal:playlist:0e5f6c2a-1b2c-4d3e-9f40-123456789abc".to_owned()
            ))
        );
        // A link arrives in whatever shape the desktop had it and leaves
        // as the one URI the app navigates by.
        assert_eq!(
            command("open-link https://tidal.com/browse/album/77646168?u"),
            Some(ControlCommand::OpenLink("tidal:album:77646168".to_owned()))
        );
        assert!(matches!(parse("nowplaying"), Some(Request::NowPlaying)));
        assert!(parse("devices").is_none());
    }

    #[test]
    fn play_search_carries_its_kind_and_words() {
        assert_eq!(
            command("play-search track danza rota soda stereo"),
            Some(ControlCommand::PlaySearch {
                kind: "track".into(),
                query: "danza rota soda stereo".into(),
            })
        );
        assert_eq!(
            command("play-search artist Soda Stereo"),
            Some(ControlCommand::PlaySearch {
                kind: "artist".into(),
                query: "Soda Stereo".into(),
            })
        );
        assert_eq!(command("play-search track"), None);
        assert_eq!(command("play-search song danza rota"), None);
        assert_eq!(command("play-search track \u{7}bell"), None);
        assert_eq!(
            command(&format!("play-search track {}", "a".repeat(201))),
            None
        );
    }

    #[test]
    fn rejects_lines_that_are_not_ours() {
        assert!(parse("GET / HTTP/1.1").is_none());
        assert!(parse("frobnicate").is_none());
        assert!(parse("seek-by soon").is_none());
        assert!(parse("volume-set 999").is_none());
        assert!(parse("next please").is_none());
        assert!(parse("").is_none());
    }

    /// Free-text control arguments are validated before reaching the app.
    #[test]
    fn refuses_arguments_that_are_not_shaped_like_tidals_own() {
        // #given / #when / #then
        assert!(command("play-uri http://example.com/pwn").is_none());
        assert!(command("play-uri tidal:track:a b").is_none());
        assert!(command("play-uri other:track:abc").is_none());
        assert!(command("play-uri ../../etc/passwd").is_none());
        assert!(command("play-uri").is_none());
        assert!(command(&format!("play-uri tidal:{}", "x".repeat(200))).is_none());
        assert!(command("transfer abc").is_none());
        assert!(command("open-link https://example.com/track/x").is_none());
        assert!(command("open-link tidal:user:someone").is_none());
        assert!(command("open-link").is_none());
        // A word that is not one of the three is refused rather than read
        // as `off`, which is what `RepeatMode::from_api` would have done.
        assert!(command("repeat-set sometimes").is_none());
        assert!(command("shuffle-set maybe").is_none());
        assert!(command("seek-to -1").is_none());
    }

    /// A slot of its own in a throwaway directory, so a Tidalfast already
    /// running on this machine is left alone.
    fn test_slot(name: &str) -> (fastframe_instance::Slot, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("tidalfast-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (fastframe_instance::Slot::at(&dir, NAME), dir)
    }

    /// The first launch holds the slot; a second one hands its link over
    /// and does not start.
    #[test]
    fn a_plugin_link_is_its_own_verb_and_never_a_tidal_link() {
        let callback = "tidaLuna://login-callback?code=abc&state=xyz";
        assert_eq!(
            command(&format!("plugin-link {callback}")),
            Some(ControlCommand::PluginLink(callback.to_owned()))
        );
        // A TIDAL link is not one for plugins, nor the other way round.
        assert!(command("plugin-link tidal:album:77646168").is_none());
        assert!(command(&format!("open-link {callback}")).is_none());
        assert!(command("plugin-link https://example.com/x").is_none());
        let long = format!("plugin-link tidaluna://x?code={}", "a".repeat(4096));
        assert!(command(&long).is_none());
    }

    #[test]
    fn a_second_launch_hands_a_plugin_link_to_the_first() {
        // #given
        let (slot, dir) = test_slot("second-launch-plugin");
        let waker = crate::backend::Waker::default();
        let Outcome::Only(first) = claim(&slot, &waker, None) else {
            panic!("the first launch runs");
        };

        // #when
        let callback = "tidaLuna://login-callback?code=abc&state=xyz";
        let second = claim(&slot, &waker, Some(callback));

        // #then
        assert!(matches!(second, Outcome::Surfaced));
        assert_eq!(
            *first.commands.lock().expect("the queue"),
            vec![ControlCommand::PluginLink(callback.to_owned())]
        );
        drop(first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_launch_hands_its_link_to_the_first() {
        // #given
        let (slot, dir) = test_slot("second-launch");
        let waker = crate::backend::Waker::default();
        let Outcome::Only(first) = claim(&slot, &waker, None) else {
            panic!("the first launch runs");
        };

        // #when
        let second = claim(&slot, &waker, Some("tidal:album:77646168"));

        // #then
        assert!(matches!(second, Outcome::Surfaced));
        assert_eq!(
            *first.commands.lock().expect("the queue"),
            vec![ControlCommand::OpenLink("tidal:album:77646168".to_owned())]
        );
        drop(first);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Commands reach the queue and reads return the published snapshots.
    #[test]
    fn a_client_reaches_the_command_queue_and_the_snapshot() {
        // #given
        let (slot, dir) = test_slot("round-trip");
        let Outcome::Only(guard) = claim(&slot, &crate::backend::Waker::default(), None) else {
            panic!("the slot is free");
        };
        *guard.now_playing.lock().expect("the snapshot") = "playing\tGo\tThe Band".to_owned();
        let send = |verb: &str| slot.send(verb).and_then(|line| reply(&line));

        // #when
        let accepted = send("next").expect("a reply");
        let volume = send("volume-by -5").expect("a reply");
        let liked = send("save-toggle").expect("a reply");
        let snapshot = send("nowplaying").expect("a reply");
        let search =
            crate::link::parse(&format!("tidal:search:{}", "東京の音楽 ".repeat(20))).unwrap();
        assert!(search.len() > 256, "exercise a long search link");
        let searched = send(&format!("open-link {search}")).expect("a search reply");
        let oversized = send(&format!("open-link tidal:search:{}", "x".repeat(16 * 1024)));
        let refused = send("frobnicate");

        // #then
        assert!(matches!(accepted, Reply::Ok));
        assert!(matches!(volume, Reply::Ok));
        assert!(matches!(liked, Reply::Ok));
        assert!(matches!(searched, Reply::Ok));
        assert!(
            oversized.is_err(),
            "requests beyond the size bound are refused"
        );
        match snapshot {
            Reply::NowPlaying(line) => assert_eq!(line, "playing\tGo\tThe Band"),
            _ => panic!("nowplaying answered with something else"),
        }
        // An unknown verb gets no reply at all, so the client sees a closed
        // connection rather than a command it never sent being obeyed.
        assert!(refused.is_err());
        assert_eq!(
            *guard.commands.lock().expect("the queue"),
            vec![
                ControlCommand::Next,
                ControlCommand::VolumeBy(-5),
                ControlCommand::ToggleSaved,
                ControlCommand::OpenLink(search),
            ]
        );
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }
}
