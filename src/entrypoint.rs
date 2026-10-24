use tidalfast::{app, backend, paths, settings, single_instance, util};

use clap::{CommandFactory, FromArgMatches, Parser};
use fastframe_shell::window::Geometry;

/// A fast, native TIDAL client.
#[derive(Debug, Parser)]
#[command(name = env!("CARGO_BIN_NAME"), version, about)]
struct Cli {
    /// A command for the running instance; without one, the app starts.
    #[command(subcommand)]
    control: Option<Control>,

    /// A TIDAL link to open: tidal:track:…, or a tidal.com
    /// address. The running instance opens it when there is one, which
    /// is how the desktop hands links over. A tidaluna:// link (a plugin's
    /// sign-in redirect) goes to the plugins instead.
    #[arg(value_name = "LINK")]
    link: Option<String>,

    /// Open the window minimized. The login entry made by "Launch at login"
    /// carries this when "Start minimized" is on.
    #[arg(long)]
    minimized: bool,

    /// The name this player goes by for this session.
    #[arg(long)]
    device_name: Option<String>,

    /// Use a separate profile (settings, state, cache, sign-in), for
    /// testing beside the normal one. Also `TIDALFAST_PROFILE=<name>`.
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    /// Log more from the playback engine and the API client.
    #[arg(short, long)]
    verbose: bool,

    /// Start with sample data and no TIDAL connection (for screenshots).
    #[cfg(feature = "demo")]
    #[arg(long)]
    demo: bool,

    #[cfg(feature = "demo")]
    #[arg(long, requires = "demo")]
    demo_update_feed: Option<String>,

    #[cfg(feature = "demo")]
    #[arg(long, requires = "demo")]
    demo_data: Option<std::path::PathBuf>,

    /// Page to open in demo mode, e.g. `home`, `playlist:pl1`, `artist:art0`.
    #[cfg(feature = "demo")]
    #[arg(long)]
    demo_page: Option<String>,

    /// Extra demo surfaces: a comma-separated list of `queue`, `queue-history`, `playing-next`,
    /// `devices`, `sonos-idle`, `sonos-active`, `cast-idle`, `cast-connecting`, `phone-playing`,
    /// `shortcuts`, `create`, `light`, `focus`, `update`, `personal-app`, `offline-mode`, `did-you-mean`,
    /// `windows-taskbar`, `german`, `lyrics`, `lyrics-fullscreen`, `collection-loading`, `paused`, `no-playback`,
    /// `shuffle-selected`, `shuffle-started`, `undated-mix`, `signed-out`, `connecting`, `library-list`,
    /// `library-list-narrow`, `library-list-wide`, `library-grid`, `library-grid-narrow`,
    /// `library-grid-wide`, `collection-sidebar`, `program-prompt`, `permission-credentials`,
    /// `permission-shortcuts`, `permission-pipe`, `permission-audio`, `activity-past`, `profile-empty` or
    /// `profile-plain`.
    #[cfg(feature = "demo")]
    #[arg(long)]
    demo_show: Option<String>,

    /// Run a performance scenario (`startup`, `scroll` or `browse`) in a demo
    /// window, print its numbers and exit. Implies `--demo`.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "SCENARIO", value_parser = tidalfast::bench::Scenario::parse)]
    demo_bench: Option<tidalfast::bench::Scenario>,

    /// Also append the `--demo-bench` report to this file.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "PATH", requires = "demo_bench")]
    demo_bench_out: Option<std::path::PathBuf>,

    /// Interface language, in place of the saved setting and the system's.
    /// Requires demo mode.
    #[cfg(feature = "demo")]
    #[arg(long, value_enum)]
    demo_language: Option<tidalfast::i18n::Locale>,

    /// Write a PNG of the demo window to this path and exit. Implies
    /// `--demo`. Without `--demo-size`, the shot is the window's own frame
    /// buffer: full screen where that request is honoured, and the size of
    /// the tile under a tiling window manager, which decides for itself.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "PATH")]
    demo_shot: Option<std::path::PathBuf>,

    /// How long to let cover art download before the shot is taken.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "MS", default_value_t = 6000)]
    demo_shot_delay: u64,

    /// Inner window size for `--demo-shot`, as `WIDTHxHEIGHT`.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "WIDTHxHEIGHT", value_parser = parse_demo_size)]
    demo_size: Option<[f32; 2]>,

    /// Hold a drag for `--demo-shot`: press the primary button at the first
    /// point, move to the second and keep it held, as `X,Y:X,Y` in points.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "X,Y:X,Y", value_parser = parse_demo_drag)]
    demo_drag: Option<[egui::Pos2; 2]>,

    /// With `--demo-shot`: when the shot is taken, also write each anchor's
    /// resolved theme look (and the cascade's style for it) to this path as
    /// JSON, for comparing a theme with TIDAL's page in a browser.
    #[cfg(feature = "demo")]
    #[arg(long, value_name = "PATH", requires = "demo_shot")]
    demo_dump_looks: Option<std::path::PathBuf>,
}

/// Remote control of the running instance, for Raycast scripts, launchers,
/// and hands on keyboards.
#[derive(Debug, clap::Subcommand)]
enum Control {
    /// Toggle play/pause
    PlayPause,
    /// Start playback if paused
    Play,
    /// Pause playback if playing
    Pause,
    /// Skip to the next track
    Next,
    /// Return to the previous track
    Previous,
    /// Seek by this many seconds; negative seeks backwards
    Seek {
        #[arg(allow_negative_numbers = true)]
        seconds: i64,
    },
    /// Seek to a position, in seconds from the start
    SeekTo { seconds: u32 },
    /// Set the volume to a percentage
    Volume {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: u8,
    },
    /// Raise the volume
    VolumeUp {
        #[arg(default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=100))]
        percent: u8,
    },
    /// Lower the volume
    VolumeDown {
        #[arg(default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=100))]
        percent: u8,
    },
    /// Toggle mute
    Mute,
    /// Toggle shuffle, or set it outright
    Shuffle { state: Option<OnOff> },
    /// Cycle the repeat mode, or set it outright
    Repeat { mode: Option<Repeat> },
    /// Save the playing track to your library, or take it back out
    Like,
    /// Play a TIDAL URI: a track, album, playlist, artist, or mix
    PlayUri { uri: String },
    /// Search TIDAL and play the best match: `play-search danza rota`, or
    /// `play-search --kind artist soda stereo`
    PlaySearch {
        /// What to look for: a song, or an album, artist, or playlist to play from the start
        #[arg(long, value_enum, default_value_t = SearchKind::Track)]
        kind: SearchKind,
        /// The words to search for
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
    },
    /// Print the playing track
    NowPlaying {
        /// Print the fields tab-separated instead: state, title, artists,
        /// album, position_ms, duration_ms, volume, shuffle, repeat,
        /// art_url, saved, device.
        #[arg(long)]
        raw: bool,
    },
    /// Bring the window of the running instance forward
    Show,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum SearchKind {
    Track,
    Album,
    Artist,
    Playlist,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Repeat {
    /// Play through and stop
    Off,
    /// Repeat the album, playlist, or queue
    Context,
    /// Repeat this track
    Track,
}

/// Sends one control verb to the running instance over the
/// single-instance channel.
fn run_control(control: Control) -> i32 {
    let raw = matches!(control, Control::NowPlaying { raw: true });
    let verb = match control {
        Control::PlayPause => "playpause".to_owned(),
        Control::Play => "play".to_owned(),
        Control::Pause => "pause".to_owned(),
        Control::Next => "next".to_owned(),
        Control::Previous => "previous".to_owned(),
        Control::Seek { seconds } => format!("seek-by {}", seconds.saturating_mul(1000)),
        Control::SeekTo { seconds } => format!("seek-to {}", u64::from(seconds) * 1000),
        Control::Volume { percent } => format!("volume-set {percent}"),
        Control::VolumeUp { percent } => format!("volume-by {percent}"),
        Control::VolumeDown { percent } => format!("volume-by -{percent}"),
        Control::Mute => "mute".to_owned(),
        Control::Shuffle { state: None } => "shuffle".to_owned(),
        Control::Shuffle { state: Some(state) } => {
            let state = match state {
                OnOff::On => "on",
                OnOff::Off => "off",
            };
            format!("shuffle-set {state}")
        }
        Control::Repeat { mode: None } => "repeat".to_owned(),
        Control::Repeat { mode: Some(mode) } => {
            let mode = match mode {
                Repeat::Off => "off",
                Repeat::Context => "context",
                Repeat::Track => "track",
            };
            format!("repeat-set {mode}")
        }
        Control::Like => "save-toggle".to_owned(),
        Control::PlayUri { uri } => format!("play-uri {uri}"),
        Control::PlaySearch { kind, query } => {
            let kind = match kind {
                SearchKind::Track => "track",
                SearchKind::Album => "album",
                SearchKind::Artist => "artist",
                SearchKind::Playlist => "playlist",
            };
            format!("play-search {kind} {}", query.join(" "))
        }
        Control::NowPlaying { .. } => "nowplaying".to_owned(),
        Control::Show => "show".to_owned(),
    };
    let printed = match single_instance::send(&verb) {
        Ok(single_instance::Reply::Ok) => return 0,
        Ok(single_instance::Reply::NowPlaying(snapshot)) => {
            if raw {
                format!("{snapshot}\n")
            } else {
                format!("{}\n", format_now_playing(&snapshot))
            }
        }
        Err(error) => {
            eprintln!("Tidalfast is not running or does not support remote control: {error}");
            return 1;
        }
    };
    write_reply(&mut std::io::stdout().lock(), &printed)
}

/// Writes a command's reply to standard output. A reader that stops early,
/// such as `head`, closes the pipe; that ends the command quietly instead of
/// panicking as `println!` does.
fn write_reply(out: &mut impl std::io::Write, text: &str) -> i32 {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("cannot print the reply: {error}");
            1
        }
    }
}

/// The desktop entry's name, which the window's app id has to match for the
/// desktop to find its icon: inside a Flatpak the entry is exported under the
/// sandbox's app id, which Flatpak always sets in `FLATPAK_ID`.
#[cfg(target_os = "linux")]
fn desktop_entry() -> String {
    fastframe_now_playing::desktop_entry("tidalfast")
}

#[cfg(target_os = "linux")]
const PULSEAUDIO_PROPERTIES: [(&str, &str); 2] = [
    ("PULSE_PROP_application.name", "Tidalfast"),
    ("PULSE_PROP_stream.description", "TIDAL playback"),
];

#[cfg(target_os = "linux")]
fn missing_pulseaudio_properties(
    mut is_set: impl FnMut(&str) -> bool,
) -> impl Iterator<Item = (&'static str, &'static str)> {
    PULSEAUDIO_PROPERTIES
        .into_iter()
        .filter(move |(key, _)| !is_set(key))
}

/// Gives the PulseAudio backend useful stream metadata.
///
/// That backend leaves both names empty unless its caller provides them. An
/// unnamed stream cannot be identified reliably by mixers and audio processors.
#[cfg(target_os = "linux")]
fn configure_pulseaudio_properties() {
    for (key, value) in missing_pulseaudio_properties(|key| std::env::var_os(key).is_some()) {
        // SAFETY: run calls this at process startup, before it creates any
        // threads. Keep explicit values supplied by the launcher or user.
        unsafe { std::env::set_var(key, value) };
    }
}

/// The `nowplaying` snapshot as one human-readable line.
fn format_now_playing(snapshot: &str) -> String {
    let mut fields = snapshot.split('\t');
    let state = fields.next().unwrap_or_default();
    let title = fields.next().unwrap_or_default();
    let artists = fields.next().unwrap_or_default();
    let _album = fields.next();
    let position_ms: u32 = fields.next().and_then(|ms| ms.parse().ok()).unwrap_or(0);
    let duration_ms: u32 = fields.next().and_then(|ms| ms.parse().ok()).unwrap_or(0);
    let clock = |ms: u32| format!("{}:{:02}", ms / 60_000, ms % 60_000 / 1000);
    match state {
        "playing" | "paused" => {
            let mark = if state == "playing" { "▶" } else { "⏸" };
            format!(
                "{mark} {title} — {artists}  [{} / {}]",
                clock(position_ms),
                clock(duration_ms)
            )
        }
        _ => "Nothing playing".to_owned(),
    }
}

/// Which build this is: the program's file name and when it was built
/// (its modification time), which tells apart builds of one version.
/// No folder, which would carry the user's name.
fn build_line() -> String {
    let exe = std::env::current_exe().ok();
    let name = exe
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let built = exe
        .and_then(|exe| std::fs::metadata(exe).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map_or_else(|| "at an unknown time".into(), |time| format!("{time:.0}"));
    format!("program {name}, built {built}")
}

/// Keeps addresses out of the log: a stream's address carries a signed
/// token, and a failing request's error names the address it asked.
fn redact_links(
    _record: &log::Record<'_>,
    message: &str,
) -> Option<std::borrow::Cow<'static, str>> {
    fastframe_log::redact::is_link(message).then(|| fastframe_log::redact::links(message).into())
}

/// Adds a line to the running copy's log that this launch was handed to it.
fn note_handoff(log_file: &std::path::Path) {
    use std::io::Write as _;
    let line = format!(
        "[{:.0} INFO tidalfast] A launch of tidalfast {} ({}) found this copy running and handed over to it; the running copy keeps playing with its own build\n",
        jiff::Timestamp::now(),
        env!("CARGO_PKG_VERSION"),
        build_line(),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log_file) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub(crate) fn run() -> eframe::Result<()> {
    #[cfg(feature = "demo")]
    tidalfast::bench::mark_start();
    #[cfg(target_os = "linux")]
    configure_pulseaudio_properties();
    // First of all: `--apply-update <job>` makes this process the update
    // helper, which installs and exits; otherwise the receipt and error an
    // update relaunch carries are taken out of the arguments.
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let launch = fastframe_update::intercept(&tidalfast::updates::launch_config(&arguments));

    let cli = Cli::from_arg_matches(&Cli::command().get_matches_from(&launch.arguments))
        .unwrap_or_else(|error| error.exit());
    if let Some(name) = cli.profile.as_deref()
        && !paths::set_profile(name)
    {
        eprintln!(
            "--profile {name}: use 1 to 32 letters, digits, - or _ (and the same name as {})",
            paths::PROFILE_ENV
        );
        std::process::exit(2);
    }
    // Demo mode invents plays, settings, and a signed-in account. Without a
    // folder of its own it would write them into the real profile, where
    // they would pass for the user's history.
    #[cfg(feature = "demo")]
    let cli = match cli.demo_bench {
        Some(scenario) => {
            let (page, show) = scenario.demo_flags();
            Cli {
                demo: true,
                demo_page: cli.demo_page.or(page.map(str::to_string)),
                demo_show: cli.demo_show.or(show.map(str::to_string)),
                ..cli
            }
        }
        None => cli,
    };
    #[cfg(feature = "demo")]
    let cli = if (cli.demo || cli.demo_shot.is_some()) && cli.demo_data.is_none() {
        Cli {
            demo_data: Some(
                std::env::temp_dir().join(format!("tidalfast-demo-{}", std::process::id())),
            ),
            ..cli
        }
    } else {
        cli
    };
    // A control launch is a client, not a second app: talk to the running
    // instance and exit before touching the log file it is writing to.
    if let Some(control) = cli.control {
        std::process::exit(run_control(control));
    }
    // A link is read before anything starts: one that is not a TIDAL
    // link ends the launch here rather than reaching the running instance.
    let link = cli.link.as_deref().map(|text| {
        // A tidaluna:// link is TidaLuna's, for plugins; its query can
        // carry a sign-in code, so it is never printed.
        if tidalfast::link::is_plugin_link(text) {
            return tidalfast::link::plugin_link(text).unwrap_or_else(|| {
                eprintln!("not a link plugins can take");
                std::process::exit(2);
            });
        }
        match tidalfast::link::parse(text) {
            Some(uri) => uri,
            None => {
                eprintln!("not a TIDAL link: {text}");
                std::process::exit(2);
            }
        }
    });
    // The application (audio engine, Web API, MPRIS, tray) outlives any
    // window. Closing to the tray destroys the window and the shell below
    // creates a new one when the tray or MPRIS asks for it. Plain window
    // lifecycle, portable across desktops.
    let waker = backend::Waker::default();
    MINIMIZED_START.store(cli.minimized, std::sync::atomic::Ordering::SeqCst);

    // A second launch surfaces the instance already running instead of
    // starting a rival one. Held for the lifetime of the process.
    #[cfg(feature = "demo")]
    let demo = cli.demo || cli.demo_shot.is_some();
    #[cfg(feature = "demo")]
    let guarded = !demo;
    #[cfg(not(feature = "demo"))]
    let guarded = true;
    let dirs = paths::AppDirs::discover();
    #[cfg(feature = "demo")]
    let dirs = cli
        .demo_data
        .as_ref()
        .map(|base| paths::AppDirs {
            config: base.join("config"),
            state: base.join("state"),
            cache: base.join("cache"),
        })
        .unwrap_or(dirs);
    let instance = if guarded {
        match single_instance::acquire(&waker, link.as_deref()) {
            single_instance::Outcome::Only(guard) => Some(guard),
            single_instance::Outcome::Surfaced => {
                // This launch never starts its own log: the running copy
                // owns the file. A line there says a launch was handed
                // over, so a report from "the new build" that was really
                // the old one still running can be told apart.
                note_handoff(&dirs.log_file());
                return Ok(());
            }
        }
    } else {
        None
    };
    let default_filter = if cli.verbose {
        "info,tidalfast=debug"
    } else {
        // Which system face draws each script the interface font lacks,
        // one line per script at startup, for reports of odd-looking text.
        "warn,tidalfast=info,fastframe_fonts=info"
    };
    let dirs_ready = dirs.ensure();
    // Launched from a desktop, stderr goes nowhere; keep the run's log where
    // a bug report can find it, and a line per panic in the panic log (with
    // any link in its message removed: a URL can carry a token).
    let mut logging = fastframe_log::Logging::new("tidalfast", env!("CARGO_PKG_VERSION"))
        .filter(default_filter)
        .panic_log(dirs.panic_log())
        .panic_message(fastframe_log::PanicMessage::Redacted(
            fastframe_log::redact::links,
        ))
        .redact(redact_links);
    // The usual log file, or one of this run's own when that cannot be
    // replaced; the reason is kept in `logging-failed.txt` either way.
    if let Some(file) = tidalfast::logfile::choose(&dirs) {
        logging = logging.file(file);
    }
    if let Err(error) = logging.init() {
        eprintln!("not logging: {error}");
        tidalfast::logfile::record_failure(&dirs, &format!("not logging: {error}"));
    }
    log::info!("{}", build_line());
    if let Err(error) = dirs_ready {
        log::warn!("unable to create the application directories: {error}");
    }
    // Folders left by features this version no longer has, once, off the
    // UI thread: MilkDrop presets alone can be well over 100 MB.
    // Update downloads and staging folders no install needs any more go too.
    if guarded {
        let dirs = dirs.clone();
        let update_job = launch
            .receipt
            .as_ref()
            .map(|receipt| receipt.path().to_path_buf());
        let _ = std::thread::Builder::new()
            .name("cleanup".into())
            .spawn(move || {
                // The update folders first: they take a moment, and the
                // first update check comes seconds later.
                tidalfast::updates::tidy(&dirs, update_job.as_deref());
                drop(tidalfast::cleanup::sweep(&dirs));
            });
    }
    #[cfg(feature = "demo")]
    tidalfast::bench::phase("logging");
    let mut settings = settings::Settings::load(&dirs.settings_file());
    #[cfg(feature = "demo")]
    tidalfast::bench::phase("settings");
    if let Some(name) = cli.device_name {
        settings.device_name = name;
    }
    // The login entry names the program that is running now, so an update
    // or a move of the program does not leave it pointing at nothing.
    if guarded
        && settings.launch_at_login
        && let Err(error) = tidalfast::autostart::sync(true, settings.start_minimized)
    {
        log::warn!("launch at login: {error}");
    }
    // An AppImage adds itself to the desktop's launcher, off this thread.
    #[cfg(target_os = "linux")]
    if settings.appimage_integration {
        let _ = std::thread::Builder::new()
            .name("appimage-register".into())
            .spawn(tidalfast::appimage::integrate);
    }
    #[cfg(target_os = "linux")]
    tidalfast::window::init_linux_window_controls();
    tidalfast::window::set_custom_titlebar(settings.custom_titlebar);
    // Colour emoji: the fonts are found off this thread. A demo capture
    // draws every picture in the frame that shows it.
    #[cfg(feature = "demo")]
    tidalfast::emoji::install(demo);
    #[cfg(not(feature = "demo"))]
    tidalfast::emoji::install(false);
    // The walk of the installed fonts for faces of other scripts happens
    // once per process; start it now so it runs while the window and its
    // GL context are made rather than after them, when `attach` needs it.
    let _ = std::thread::Builder::new()
        .name("font-scan".into())
        .spawn(|| {
            drop(fastframe_fonts::FontSetup::default().definitions());
            tidalfast::fonts::warm();
        });
    #[cfg(feature = "demo")]
    tidalfast::bench::phase("emoji");
    // macOS delivers links as Apple Events; install before the event loop.
    #[cfg(target_os = "macos")]
    if let Some(guard) = &instance {
        tidalfast::mac_links::install(guard.commands(), waker.clone());
    }

    // A capture run is a throwaway process next to the real one: no tray
    // icon of its own, and no second MPRIS service to fight over media keys.
    #[allow(unused_mut)]
    let mut options = app::AppOptions::default();
    #[cfg(feature = "demo")]
    if demo {
        options.restore_sign_in = false;
    }
    #[cfg(feature = "demo")]
    if cli.demo_shot.is_some() || cli.demo_bench.is_some() {
        options = app::AppOptions {
            media_controls: false,
            restore_sign_in: false,
            tray: false,
        };
    }
    // An off-screen development window (`TIDALFAST_WINDOW_POS`) registers
    // nothing the desktop shows: no media overlay, tray icon or thumbnail
    // buttons, so a measurement or live test never appears on the screen.
    // A live test profile still restores its sign-in.
    #[cfg(feature = "demo")]
    if dev_window_placed() {
        options.media_controls = false;
        options.tray = false;
    }
    #[cfg(windows)]
    let desktop_surfaces = options.media_controls;
    #[allow(unused_mut)]
    let mut app = app::App::new(&waker, dirs, settings, options);
    #[cfg(feature = "demo")]
    tidalfast::bench::phase("app");
    app.update_receipt = launch.receipt;
    if let Some(error) = launch.error {
        app.report_update_failure(error);
    }
    if let Some(guard) = &instance {
        app.set_remote_control(guard);
    }
    if let Some(uri) = link {
        if tidalfast::link::is_plugin_link(&uri) {
            app.deliver_plugin_link(uri);
        } else {
            app.open_link(uri);
        }
    }
    #[cfg(feature = "demo")]
    if demo {
        tidalfast::demo::populate(&mut app);
        tidalfast::demo::apply_flags(&mut app, cli.demo_page.as_deref(), cli.demo_show.as_deref());
        if let Some(feed) = &cli.demo_update_feed {
            match fastframe_update::Source::local(feed) {
                Ok(source) => app.update_source = source,
                Err(error) => {
                    eprintln!("{error:#}");
                    std::process::exit(2);
                }
            }
            app.update_restart_arguments =
                vec!["--demo".into(), "--demo-page".into(), "settings".into()];
            if let Some(base) = &cli.demo_data {
                app.update_restart_arguments
                    .extend(["--demo-data".into(), base.to_string_lossy().into_owned()]);
            }
            app.actions.push(tidalfast::model::Action::CheckForUpdates);
        }
        if let Some(locale) = cli.demo_language {
            app.settings.language = tidalfast::settings::LanguageChoice::Locale(locale);
            app.locale = locale;
        }
    }
    #[cfg(feature = "demo")]
    let shot = cli.demo_shot.clone().map(|path| Shot {
        path,
        due: std::time::Instant::now() + std::time::Duration::from_millis(cli.demo_shot_delay),
        asked: false,
        looks: cli.demo_dump_looks.clone(),
    });
    #[cfg(feature = "demo")]
    let demo_drag = cli
        .demo_drag
        .map(|[from, to]| DemoDrag { from, to, frame: 0 });
    #[cfg(feature = "demo")]
    let demo_inner = cli.demo_size;
    #[cfg(feature = "demo")]
    let bench =
        std::rc::Rc::new(std::cell::RefCell::new(cli.demo_bench.map(|scenario| {
            tidalfast::bench::Bench::new(scenario, cli.demo_bench_out.clone())
        })));
    #[cfg(feature = "demo")]
    tidalfast::window::set_fixed_size(demo_inner.is_some());
    #[cfg(feature = "demo")]
    let demo_storage = app.dirs.cache.join("demo-window.ron");
    #[cfg(feature = "demo")]
    tidalfast::bench::phase("demo data");
    let shell = fastframe_shell::Shell::new(app, &waker);
    // The menu-bar item answers only while AppKit's loop runs. Elsewhere
    // nothing needs the main thread between headless ticks, so the shell
    // sleeps until the waker wakes it.
    #[cfg(target_os = "macos")]
    let shell = shell.idle(fastframe_tray::idle);
    shell.run(|lease| {
        #[cfg(windows)]
        let creator_waker = waker.clone();
        #[cfg(feature = "demo")]
        let creator_shot = shot.clone();
        #[cfg(feature = "demo")]
        let creator_bench = std::rc::Rc::clone(&bench);
        let geometry = lease.peek(main_geometry);
        #[cfg(all(windows, target_arch = "aarch64"))]
        let locale = lease.peek(|app| app.locale);
        #[cfg(feature = "demo")]
        let options = {
            let options =
                native_options(shot.is_some() && demo_inner.is_none(), demo_inner, geometry);
            if demo {
                demo_native_options(options, demo_storage.clone())
            } else {
                options
            }
        };
        #[cfg(not(feature = "demo"))]
        let options = native_options(false, None, geometry);
        let options = match lease.peek(|app| {
            app.compact_mode
                .then(|| (app.settings.clone(), app.window_geometry))
        }) {
            Some((settings, main)) => compact_options(options, &settings, &main),
            None => options,
        };
        let options = profile_options(options);
        #[cfg(feature = "demo")]
        let options = match dev_window_position() {
            Some(position) => dev_window_options(options, position),
            None => options,
        };
        #[cfg(windows)]
        let thumbbar_enabled = desktop_surfaces;
        #[cfg(feature = "demo")]
        tidalfast::bench::phase("window options");
        eframe::run_native(
            "Tidalfast",
            options,
            Box::new(move |cc| {
                #[cfg(feature = "demo")]
                tidalfast::bench::phase("window and GL");
                if let Some(gl) = &cc.gl {
                    use eframe::glow::HasContext;
                    // eframe has made this window's GL context current
                    // before calling the app creator. These identify the
                    // renderer actually selected, which may differ from
                    // the listed GPU.
                    unsafe {
                        log::info!(
                            "OpenGL renderer: {}; vendor: {}; version: {}",
                            gl.get_parameter_string(eframe::glow::RENDERER),
                            gl.get_parameter_string(eframe::glow::VENDOR),
                            gl.get_parameter_string(eframe::glow::VERSION)
                        );
                    }
                }
                let mut app = lease.take(&cc.egui_ctx);
                // Built once per window, before the first frame; the
                // handler wakes the loop so a menu pick is not held until
                // the next repaint.
                #[cfg(target_os = "macos")]
                {
                    tidalfast::mac_touchbar_crash_guard::install();
                    tidalfast::mac_menu::init();
                    let ctx = cc.egui_ctx.clone();
                    tidalfast::mac_menu::set_waker(move || ctx.request_repaint());

                    tidalfast::notch::init();
                    let ctx_notch = cc.egui_ctx.clone();
                    tidalfast::notch::set_waker(move || ctx_notch.request_repaint());
                }
                app.attach(&cc.egui_ctx);
                // Only the first window of a start at login: a window the
                // tray opens later is one the person asked for.
                if MINIMIZED_START.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    cc.egui_ctx
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                #[cfg(feature = "demo")]
                tidalfast::bench::phase("fonts and loaders");
                // The main window's own handle, for the checks of whether
                // other windows cover it.
                #[cfg(windows)]
                let occlusion = {
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    match cc.window_handle().map(|handle| handle.as_raw()) {
                        Ok(RawWindowHandle::Win32(window)) => {
                            Some(tidalfast::occlusion::Watch::new(window.hwnd.get()))
                        }
                        _ => None,
                    }
                };
                #[cfg(windows)]
                let thumbbar = {
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    let mut toolbar = tidalfast::thumbbar::ThumbBar::new();
                    if thumbbar_enabled
                        && let Ok(handle) = cc.window_handle()
                        && let RawWindowHandle::Win32(window) = handle.as_raw()
                    {
                        let wake = creator_waker.clone();
                        // The shell and toolbar share this window's thread
                        // and lifetime; the toolbar is detached on shell
                        // drop.
                        unsafe { toolbar.attach(window.hwnd.get(), move || wake.wake()) };
                    }
                    toolbar
                };
                Ok(Box::new(Shell {
                    app,
                    recovery_checked: false,
                    #[cfg(windows)]
                    thumbbar,
                    #[cfg(windows)]
                    occlusion,
                    #[cfg(feature = "demo")]
                    shot: creator_shot.clone(),
                    #[cfg(feature = "demo")]
                    drag: demo_drag,
                    #[cfg(feature = "demo")]
                    bench: creator_bench.borrow_mut().take(),
                    #[cfg(feature = "demo")]
                    script: tidalfast::dev_script::DevScript::from_env(),
                }))
            }),
        )
        .inspect_err(|error| {
            log::error!("Native window failed: {error}");
            #[cfg(all(windows, target_arch = "aarch64"))]
            if matches!(
                error,
                eframe::Error::Glutin(_)
                    | eframe::Error::NoGlutinConfigs(..)
                    | eframe::Error::OpenGL(_)
            ) {
                tidalfast::window::report_missing_opengl(locale);
            }
        })
    })
}

/// The first window opens minimized (`--minimized`).
static MINIMIZED_START: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The main window's remembered geometry. A demo run opens at its default
/// size, wherever the window manager puts it.
fn main_geometry(app: &app::App) -> Geometry {
    if app.offline {
        Geometry::default()
    } else {
        app.window_geometry
    }
}

const fn main_window_decorated(custom_titlebar: bool) -> bool {
    !custom_titlebar
}

#[cfg(any(test, feature = "demo"))]
fn parse_demo_size(spec: &str) -> Result<[f32; 2], String> {
    let (width, height) = spec
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {spec}"))?;
    let width: f32 = width
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in {spec}"))?;
    let height: f32 = height
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in {spec}"))?;
    if width < 1.0 || height < 1.0 {
        return Err(format!("size must be at least 1x1, got {spec}"));
    }
    Ok([width, height])
}

#[cfg(feature = "demo")]
fn parse_demo_drag(spec: &str) -> Result<[egui::Pos2; 2], String> {
    let point = |point: &str| -> Result<egui::Pos2, String> {
        let (x, y) = point
            .split_once(',')
            .ok_or_else(|| format!("expected X,Y, got {point}"))?;
        let x: f32 = x
            .trim()
            .parse()
            .map_err(|_| format!("invalid x in {point}"))?;
        let y: f32 = y
            .trim()
            .parse()
            .map_err(|_| format!("invalid y in {point}"))?;
        Ok(egui::pos2(x, y))
    };
    let (from, to) = spec
        .split_once(':')
        .ok_or_else(|| format!("expected X,Y:X,Y, got {spec}"))?;
    Ok([point(from)?, point(to)?])
}

/// The main window, at the demo's fixed `inner_size` or where `geometry`
/// remembers.
fn native_options(
    fullscreen: bool,
    inner_size: Option<[f32; 2]>,
    geometry: Geometry,
) -> eframe::NativeOptions {
    // Keep the native profile path even when Flatpak supplies its app ID.
    #[cfg(target_os = "linux")]
    let persistence_path = eframe::storage_dir("tidalfast").map(|dir| dir.join("app.ron"));
    #[cfg(not(target_os = "linux"))]
    let persistence_path = None;
    #[cfg(target_os = "linux")]
    let app_id = desktop_entry();
    #[cfg(not(target_os = "linux"))]
    let app_id = "tidalfast";
    let icon = if cfg!(target_os = "macos") {
        // macOS takes the dock icon from the bundle's .icns, which is the
        // 1024px drawing with the platform's rounding. Setting a window
        // icon there replaces it with this flat 128px square.
        egui::IconData::default()
    } else {
        app_icon()
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title("Tidalfast")
        .with_app_id(app_id)
        .with_taskbar(true)
        .with_icon(icon);
    let size = inner_size.unwrap_or([1240.0, 800.0]);
    let viewport = viewport
        // macOS: no title bar strip above the app. The content runs to
        // the top edge and the traffic lights float over it, the way
        // every other music player on the platform looks; the interface
        // leaves room for them with `theme::titlebar_inset`.
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        // Windows has no equivalent to macOS's floating traffic lights.
        // Removing its decorations lets the app surface fill the window.
        .with_decorations(main_window_decorated(tidalfast::window::custom_titlebar()))
        .with_inner_size(size)
        .with_min_inner_size(inner_size.unwrap_or(tidalfast::window::MAIN_MIN_SIZE))
        .with_fullscreen(fullscreen);
    let viewport = if inner_size.is_some() {
        viewport.with_max_inner_size(size)
    } else {
        // The size, position and maximized state it was left in,
        // before the window exists. eframe's own saved window, where
        // there is one, still wins over this.
        geometry.apply(viewport)
    };
    eframe::NativeOptions {
        viewport,
        persist_window: true,
        persistence_path,
        ..Default::default()
    }
}

/// Compact mode's window in place of the main one: the same app id and icon,
/// the small frameless always-on-top shape, and no saved geometry, so the
/// next main window does not open at the compact player's size.
fn compact_options(
    mut options: eframe::NativeOptions,
    settings: &tidalfast::settings::Settings,
    main: &Geometry,
) -> eframe::NativeOptions {
    let mut viewport = tidalfast::ui::compact::viewport(settings, main);
    viewport.app_id = options.viewport.app_id.take();
    viewport.icon = options.viewport.icon.take();
    options.viewport = viewport;
    options.persist_window = false;
    // eframe restores the saved window whether or not it saves one, and the
    // saved one is the main window, maximized or full size. The compact
    // window reads a file of its own, which holds no window.
    options.persistence_path = Some(
        options
            .persistence_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
            .or_else(|| eframe::storage_dir("tidalfast"))
            .unwrap_or_default()
            .join("compact-window.ron"),
    );
    options
}

fn profile_options(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if options.persist_window {
        options.persistence_path = eframe::storage_dir("tidalfast").map(|dir| dir.join("app.ron"));
    }
    options
}

#[cfg(any(test, feature = "demo"))]
fn demo_native_options(
    mut options: eframe::NativeOptions,
    storage: std::path::PathBuf,
) -> eframe::NativeOptions {
    // Saving and loading are separate in eframe. An unused, unsaved path
    // prevents the normal profile's geometry and zoom from entering a demo.
    options.persistence_path = Some(storage);
    options.persist_window = false;
    options
}

#[cfg(test)]
mod native_window_tests {
    use super::*;

    /// A development window opens where it is told, as a plain window,
    /// whatever the profile or the user's own saved window says (a live
    /// test once opened maximized over the user's screen).
    #[cfg(feature = "demo")]
    #[test]
    fn the_development_window_ignores_saved_window_state() {
        let saved = Geometry {
            maximized: true,
            ..Geometry::default()
        };
        let options = profile_options(native_options(false, None, saved));
        let options = dev_window_options(options, egui::pos2(-4000.0, -4000.0));
        assert!(!options.persist_window);
        assert!(
            options
                .persistence_path
                .as_ref()
                .is_some_and(|p| p.ends_with("dev-window.ron")),
            "{:?}",
            options.persistence_path
        );
        assert_eq!(options.viewport.maximized, Some(false));
        assert_eq!(
            options.viewport.position,
            Some(egui::pos2(-4000.0, -4000.0))
        );
    }
    /// A reader that closes the pipe early ends the command quietly.
    #[test]
    fn a_closed_pipe_ends_a_reply_quietly() {
        struct Closed;
        impl std::io::Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(write_reply(&mut Closed, "playing\n"), 0);
        let mut buffer = Vec::new();
        assert_eq!(write_reply(&mut buffer, "playing\n"), 0);
        assert_eq!(buffer, b"playing\n");
    }

    #[test]
    fn window_geometry_is_kept_without_touching_demo_storage() {
        let main = profile_options(native_options(false, None, Geometry::default()));
        assert_eq!(
            main.persistence_path,
            eframe::storage_dir("tidalfast").map(|dir| dir.join("app.ron"))
        );
        let demo_path = std::path::PathBuf::from("temporary/demo.ron");
        let demo = profile_options(demo_native_options(
            native_options(false, None, Geometry::default()),
            demo_path.clone(),
        ));
        assert_eq!(demo.persistence_path, Some(demo_path));
    }

    #[test]
    fn launcher_identity_and_storage_are_the_native_profile() {
        let main = native_options(false, None, Geometry::default());
        #[cfg(target_os = "linux")]
        {
            let id = desktop_entry();
            assert_eq!(main.viewport.app_id.as_deref(), Some(id.as_str()));
            assert_eq!(
                main.persistence_path,
                eframe::storage_dir("tidalfast").map(|dir| dir.join("app.ron"))
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(main.viewport.app_id.as_deref(), Some("tidalfast"));
            assert_eq!(main.persistence_path, None);
        }
        assert!(main.persist_window);
        assert_eq!(main.viewport.taskbar, Some(true));
    }

    /// The main window opens with the size, position and maximized state it
    /// was left in, set before it exists; a demo's fixed size wins.
    #[test]
    fn the_main_window_opens_where_it_was_left() {
        let geometry = Geometry {
            size: Some([1100.0, 700.0]),
            position: Some([100.0, 150.0]),
            maximized: true,
        };
        let options = native_options(false, None, geometry);
        assert_eq!(options.viewport.inner_size, Some(egui::vec2(1100.0, 700.0)));
        assert_eq!(options.viewport.position, Some(egui::pos2(100.0, 150.0)));
        assert_eq!(options.viewport.maximized, Some(true));

        let fresh = native_options(false, None, Geometry::default());
        assert_eq!(fresh.viewport.inner_size, Some(egui::vec2(1240.0, 800.0)));
        assert_eq!(fresh.viewport.position, None);
        assert_eq!(fresh.viewport.maximized, None);

        let demo = native_options(false, Some([760.0, 520.0]), geometry);
        assert_eq!(demo.viewport.inner_size, Some(egui::vec2(760.0, 520.0)));
        assert_eq!(demo.viewport.position, None);
        assert_eq!(demo.viewport.maximized, None);
    }

    #[test]
    fn log_lines_lose_their_addresses() {
        let record = log::Record::builder().build();
        assert_eq!(
            redact_links(
                &record,
                "GET https://sp-ad-cf.audio.tidal.com/x.flac?token=abc failed"
            )
            .as_deref(),
            Some("GET <link> failed")
        );
        assert_eq!(
            redact_links(&record, "tidal:track:1 ended at 1000 ms"),
            None
        );
    }

    #[test]
    fn a_launch_handed_over_adds_a_line_to_the_running_copys_log() {
        let dir = std::env::temp_dir().join(format!("tidalfast-handoff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("tidalfast.log");
        std::fs::write(&log, "[t INFO tidalfast] Starting\n").unwrap();
        note_handoff(&log);
        let text = std::fs::read_to_string(&log).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[1].contains("handed over"), "{text}");
        assert!(
            !text.contains(std::path::MAIN_SEPARATOR_STR),
            "no folder: {text}"
        );
    }

    #[test]
    fn main_window_uses_the_platform_decoration_policy() {
        let options = native_options(false, None, Geometry::default());
        assert_eq!(
            options.viewport.decorations,
            Some(!tidalfast::window::custom_titlebar())
        );
        assert_eq!(options.viewport.fullsize_content_view, Some(true));
        assert_eq!(options.viewport.titlebar_shown, Some(false));
        assert_eq!(options.viewport.title_shown, Some(false));
    }

    /// The window asks for vsync: AppKit resize animations need it, and on
    /// Wayland eframe paces frames by the compositor's frame callbacks
    /// instead of waiting in the swap, so a hidden window cannot block (#266).
    #[test]
    fn windows_wait_for_vsync() {
        assert!(
            native_options(false, None, Geometry::default())
                .glow_options
                .vsync
        );
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_drag_presses_at_the_first_point_and_holds_at_the_second() {
        let [from, to] = parse_demo_drag("10,20:110, 220").unwrap();
        assert_eq!(
            (from, to),
            (egui::pos2(10.0, 20.0), egui::pos2(110.0, 220.0))
        );
        assert!(parse_demo_drag("10,20").is_err());
        assert!(parse_demo_drag("10:20").is_err());
        let mut drag = DemoDrag { from, to, frame: 0 };
        let frames: Vec<_> = (0..=DemoDrag::REST + DemoDrag::GLIDE + 5)
            .map(|_| drag.events())
            .collect();
        let presses = frames
            .iter()
            .flatten()
            .filter(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }))
            .count();
        assert_eq!(presses, 1, "one press, never a release");
        assert!(
            !frames
                .iter()
                .flatten()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: false, .. }))
        );
        assert_eq!(frames[0][0], egui::Event::PointerMoved(from));
        assert_eq!(frames.last().unwrap()[0], egui::Event::PointerMoved(to));
    }

    #[test]
    fn demo_size_parses_width_by_height() {
        assert_eq!(parse_demo_size("760x800").unwrap(), [760.0, 800.0]);
        assert!(parse_demo_size("wide").is_err());
        let options = native_options(false, Some([760.0, 800.0]), Geometry::default());
        assert_eq!(options.viewport.inner_size, Some(egui::vec2(760.0, 800.0)));
        assert_eq!(
            options.viewport.min_inner_size,
            Some(egui::vec2(760.0, 800.0))
        );
        assert_eq!(
            options.viewport.max_inner_size,
            Some(egui::vec2(760.0, 800.0))
        );
    }

    #[test]
    fn demo_window_storage_is_separate_and_never_saved() {
        let cache = std::path::PathBuf::from("isolated-demo/cache");
        let options = demo_native_options(
            native_options(false, Some([760.0, 520.0]), Geometry::default()),
            cache.join("demo-window.ron"),
        );
        assert_eq!(
            options.persistence_path,
            Some(cache.join("demo-window.ron"))
        );
        assert!(!options.persist_window);
        // Shell keeps this policy in its own field, so it holds even
        // after on_exit has handed the App back.
    }

    #[test]
    fn only_windows_removes_the_native_frame() {
        assert!(!main_window_decorated(true));
        assert!(main_window_decorated(false));
    }
}

/// The eframe adapter around the long-lived [`app::App`]: delegates frames
/// and, when the window goes away, hands the state back for the next window.
struct Shell {
    app: fastframe_shell::Held<app::App>,
    /// Whether this window's first frame checked that a monitor shows it.
    recovery_checked: bool,
    #[cfg(windows)]
    thumbbar: tidalfast::thumbbar::ThumbBar,
    /// Whether other windows fully cover this one (winit does not say on
    /// Windows); `None` without a Win32 handle.
    #[cfg(windows)]
    occlusion: Option<tidalfast::occlusion::Watch>,
    /// A pending `--demo-shot` capture, if this is a screenshot run.
    #[cfg(feature = "demo")]
    shot: Option<Shot>,
    /// A drag held for `--demo-drag`, if this run shows one.
    #[cfg(feature = "demo")]
    drag: Option<DemoDrag>,
    /// The `--demo-bench` scenario this run measures, if any.
    #[cfg(feature = "demo")]
    bench: Option<tidalfast::bench::Bench>,
    /// `TIDALFAST_DEV_SCRIPT`: scripted input for off-screen tests.
    #[cfg(feature = "demo")]
    script: tidalfast::dev_script::DevScript,
}

/// A scripted pointer for `--demo-drag`: rest on `from`, press there, glide
/// to `to` and hold, so the shot shows a drag in progress through the same
/// code a real pointer runs.
#[cfg(feature = "demo")]
#[derive(Clone, Copy)]
struct DemoDrag {
    from: egui::Pos2,
    to: egui::Pos2,
    frame: u32,
}

#[cfg(feature = "demo")]
impl DemoDrag {
    /// Frames to rest before pressing, so the rows under `from` are laid out.
    const REST: u32 = 20;
    /// Frames the glide from `from` to `to` takes.
    const GLIDE: u32 = 20;

    fn events(&mut self) -> Vec<egui::Event> {
        let frame = self.frame;
        self.frame = self.frame.saturating_add(1);
        let pos = if frame <= Self::REST {
            self.from
        } else {
            let t = ((frame - Self::REST) as f32 / Self::GLIDE as f32).min(1.0);
            self.from.lerp(self.to, t)
        };
        let mut events = vec![egui::Event::PointerMoved(pos)];
        if frame == Self::REST {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
        }
        events
    }
}

/// A screenshot the window still owes us.
///
/// Cover art arrives over the network, so the capture waits for `due` before
/// asking egui for the frame buffer. The image comes back as an input event
/// on a later frame, which is where it gets written and the window closed.
#[cfg(feature = "demo")]
#[derive(Clone)]
struct Shot {
    path: std::path::PathBuf,
    due: std::time::Instant,
    asked: bool,
    /// Where to write the anchors' looks when the shot is asked for.
    looks: Option<std::path::PathBuf>,
}

/// Writes the anchors' looks for `--demo-dump-looks`.
#[cfg(feature = "demo")]
fn write_looks(path: &std::path::Path) {
    let Some(report) = tidalfast::ui::plugin_dom::dump_looks() else {
        log::error!("no anchor tree yet, {} not written", path.display());
        return;
    };
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    match std::fs::write(path, text) {
        Ok(()) => log::info!("wrote the anchors' looks to {}", path.display()),
        Err(error) => log::error!("could not write {}: {error}", path.display()),
    }
}

#[cfg(feature = "demo")]
impl Shell {
    fn drive_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else {
            return;
        };

        // Nothing here is driven by user input, so the frames have to be
        // asked for: art still has to load and the request has to be issued.
        ctx.request_repaint();

        if !shot.asked && std::time::Instant::now() >= shot.due {
            if let Some(path) = &shot.looks {
                write_looks(path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.asked = true;
        }
        // The theme crossfade takes pictures of its own (a plugin theme
        // changing the palette); only the one asked for here is the shot.
        if !shot.asked {
            return;
        }

        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            return;
        };

        let [width, height] = [image.size[0] as u32, image.size[1] as u32];
        let pixels: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect();
        match image::RgbaImage::from_raw(width, height, pixels) {
            Some(buffer) => match buffer.save(&shot.path) {
                Ok(()) => log::info!("wrote {}x{} to {}", width, height, shot.path.display()),
                Err(error) => log::error!("could not write {}: {error}", shot.path.display()),
            },
            None => log::error!("the frame buffer did not match {width}x{height}"),
        }
        self.shot = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for Shell {
    /// egui's memory stays out of every profile: its options carry a zoom,
    /// which would be restored over the app's own (settings.zoom) and would
    /// scale the remembered window geometry the new window opens with.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    #[cfg(feature = "demo")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.script.input(ctx, raw_input);
        if let Some(bench) = self.bench.as_mut() {
            bench.input(raw_input);
        }
        if let Some(drag) = self.drag.as_mut() {
            // The script owns the pointer: drop whatever the real one did.
            raw_input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                )
            });
            raw_input.events.extend(drag.events());
        }
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // A window restored where no monitor shows it any more (a display
        // unplugged since) comes back onto one.
        if !std::mem::replace(&mut self.recovery_checked, true) && !dev_window_placed() {
            fastframe_shell::window::recover_offscreen(ctx, frame);
        }
        let app = &mut *self.app;
        #[cfg(target_os = "macos")]
        for command in tidalfast::mac_menu::drain_commands() {
            use tidalfast::mac_menu::MenuCommand;
            use tidalfast::model::{Action, Dialog, Page};
            let action = match command {
                MenuCommand::PlayPause => Action::TogglePlay,
                MenuCommand::Next => Action::Next,
                MenuCommand::Previous => Action::Previous,
                MenuCommand::SeekForward => Action::SeekBy(10_000),
                MenuCommand::SeekBackward => Action::SeekBy(-10_000),
                MenuCommand::ToggleShuffle => Action::ToggleShuffle,
                MenuCommand::CycleRepeat => Action::CycleRepeat,
                MenuCommand::VolumeUp => Action::VolumeBy(5),
                MenuCommand::VolumeDown => Action::VolumeBy(-5),
                MenuCommand::ToggleMute => Action::ToggleMute,
                MenuCommand::Home => Action::Open(Page::Home),
                MenuCommand::Search => Action::FocusSearch,
                MenuCommand::LikedSongs => Action::Open(Page::LikedSongs),
                MenuCommand::Sidebar => Action::ToggleSidebar,
                MenuCommand::Queue => Action::ToggleQueuePanel,
                MenuCommand::Settings => Action::Open(Page::Settings),
                MenuCommand::CheckForUpdates => Action::CheckForUpdates,
                MenuCommand::Shortcuts => Action::ShowDialog(Dialog::Shortcuts),
                MenuCommand::Back => Action::Back,
                MenuCommand::Forward => Action::Forward,
                MenuCommand::OpenRepo => {
                    ctx.open_url(egui::OpenUrl::new_tab(
                        "https://github.com/CIoudGuy/tidalfast",
                    ));
                    continue;
                }
                // Editing goes through egui, which owns the text field
                // and the clipboard.
                MenuCommand::Cut => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut);
                    continue;
                }
                MenuCommand::Copy => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                    continue;
                }
                MenuCommand::Paste => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    continue;
                }
                MenuCommand::SelectAll => {
                    ctx.input_mut(|input| {
                        input.events.push(egui::Event::Key {
                            key: egui::Key::A,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::COMMAND,
                        });
                    });
                    continue;
                }
            };
            app.actions.push(action);
        }
        #[cfg(windows)]
        for command in self.thumbbar.drain_commands() {
            if let Some(action) = command.action(&app.thumb_state(false)) {
                app.actions.push(action);
            }
        }
        #[cfg(feature = "demo")]
        if let Some(bench) = self.bench.as_mut() {
            bench.logic(ctx, frame, app);
        }
        // Before the logic, which sets the drawing budget from it.
        #[cfg(windows)]
        if let Some(occlusion) = self.occlusion.as_mut() {
            occlusion.sync(ctx);
        }
        app.background_frame(ctx);
        #[cfg(windows)]
        self.thumbbar
            .sync(app.thumb_state(ctx.system_theme() != Some(egui::Theme::Light)));
        #[cfg(feature = "demo")]
        self.drive_shot(ctx);
        #[cfg(feature = "demo")]
        self.script.frame(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let app = &mut *self.app;
        app.frame_ui(ui);
        #[cfg(feature = "demo")]
        if let Some(bench) = self.bench.as_mut() {
            bench.after_ui();
        }
        if let Some(receipt) = app.update_receipt.take() {
            std::thread::spawn(move || {
                if let Err(error) = receipt.acknowledge() {
                    log::error!("Could not confirm the update: {error:#}");
                }
            });
        }
        #[cfg(windows)]
        self.thumbbar
            .sync(app.thumb_state(ui.ctx().system_theme() != Some(egui::Theme::Light)));
    }

    /// The window paints itself over eframe's own ground.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let app = &mut *self.app;
        app.save_state();
    }
}

/// The toolbar goes before the window; the app then returns to the shell
/// with the `Held` field.
#[cfg(windows)]
impl Drop for Shell {
    fn drop(&mut self) {
        self.thumbbar.detach();
    }
}

/// Development only: `TIDALFAST_WINDOW_POS="X,Y"` opens a demo window at
/// that screen position without taking focus, and leaves it there rather
/// than moving it back onto a display, so performance measurements can run
/// off-screen while someone else uses the machine.
#[cfg(feature = "demo")]
fn dev_window_position() -> Option<egui::Pos2> {
    let value = std::env::var("TIDALFAST_WINDOW_POS").ok()?;
    let (x, y) = value.split_once(',')?;
    Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The off-screen development window (`TIDALFAST_WINDOW_POS`): a plain
/// window at `position` that never takes focus. A profile left maximized
/// or fullscreen would open on a display whatever the position, and eframe
/// loads its saved window (size and place, which win over the position)
/// from any storage it is given, whatever `persist_window` says, while
/// every profile shares the main one: a storage of its own keeps the
/// user's window out, so a test never opens over the user's work.
#[cfg(feature = "demo")]
fn dev_window_options(
    mut options: eframe::NativeOptions,
    position: egui::Pos2,
) -> eframe::NativeOptions {
    options.persist_window = false;
    options.persistence_path =
        eframe::storage_dir("tidalfast").map(|dir| dir.join("dev-window.ron"));
    options.viewport = options
        .viewport
        .with_maximized(false)
        .with_fullscreen(false)
        .with_position(position)
        .with_active(false)
        .with_taskbar(false);
    options
}
fn dev_window_placed() -> bool {
    #[cfg(feature = "demo")]
    return dev_window_position().is_some();
    #[cfg(not(feature = "demo"))]
    false
}

/// The window icon, from the shared runtime drawing.
fn app_icon() -> egui::IconData {
    const SIZE: usize = 128;
    egui::IconData {
        rgba: util::app_icon_rgba(SIZE),
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn pulse_metadata_defaults_preserve_existing_properties() {
        let defaults = missing_pulseaudio_properties(|_| false).collect::<Vec<_>>();
        assert_eq!(defaults, PULSEAUDIO_PROPERTIES);

        let defaults = missing_pulseaudio_properties(|key| key == "PULSE_PROP_application.name")
            .collect::<Vec<_>>();
        assert_eq!(
            defaults,
            [("PULSE_PROP_stream.description", "TIDAL playback")]
        );

        assert_eq!(missing_pulseaudio_properties(|_| true).count(), 0);
    }

    /// A link on the command line is a link, and a control verb is still a
    /// verb: the two do not get in each other's way.
    #[test]
    fn a_link_and_a_verb_are_told_apart() {
        // #given / #when / #then
        let launch =
            Cli::try_parse_from(["tidalfast", "tidal:track:77646170"]).expect("a link parses");
        assert_eq!(launch.link.as_deref(), Some("tidal:track:77646170"));
        assert!(launch.control.is_none());

        let launch = Cli::try_parse_from([
            "tidalfast",
            "https://tidal.com/browse/album/77646168?u",
            "--verbose",
        ])
        .expect("a web address parses");
        assert!(launch.link.is_some());
        assert!(launch.verbose);

        let verb = Cli::try_parse_from(["tidalfast", "next"]).expect("a verb parses");
        assert!(matches!(verb.control, Some(Control::Next)));
        assert!(verb.link.is_none());

        let bare = Cli::try_parse_from(["tidalfast"]).expect("a plain launch parses");
        assert!(bare.link.is_none() && bare.control.is_none());
    }

    #[test]
    fn the_login_entry_flag_is_a_known_option() {
        let cli = Cli::try_parse_from(["tidalfast", "--minimized"]).expect("the flag parses");
        assert!(cli.minimized);
        assert!(!Cli::try_parse_from(["tidalfast"]).unwrap().minimized);
    }
}
