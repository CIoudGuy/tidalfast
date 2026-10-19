//! Programs plugins run (`child_process`): the one place a Luna plugin leaves
//! the sandbox, so it is off until the user says yes.
//!
//! * Every program a plugin starts needs the user's answer first: "\<plugin>
//!   wants to run \<program> on your computer" with Allow once, Always or
//!   Deny. The program is the resolved executable path, never the arguments.
//!   Always is kept per plugin in `plugins.json` and can be revoked from the
//!   plugin's Permissions. Nothing is ever allowed by default.
//! * A command line that needs a shell (`a && b`, redirections, `%VAR%`) runs
//!   the shell, so it is asked about every time and cannot be remembered, and
//!   neither can a program inside the plugins' own data folder (the plugin
//!   could swap the file after the user agreed).
//! * Processes run hidden (no console window on Windows), never on the UI or
//!   the plugin thread: each has a few small threads that report back as
//!   [`ChildEvent`]s, which the plugin gets asynchronously. They are killed
//!   when the plugin is unloaded, disabled, uninstalled or revoked.
//! * Output is relayed with backpressure: a program printing faster than the
//!   plugin reads waits instead of filling memory.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use base64::Engine;

/// Running programs one plugin may have at a time.
pub const MAX_PER_PLUGIN: usize = 16;
/// Output bytes (base64) in flight to the plugin before a reader waits.
const BACKLOG: usize = 4 << 20;
/// After a denial the same plugin and program are refused without asking
/// again for this long (a plugin retrying in a loop must not flood the user).
const DENY_COOLDOWN: Duration = Duration::from_secs(60);
/// The longest command line or argument list taken from a plugin.
const MAX_COMMAND: usize = 64 * 1024;
const MAX_ENV: usize = 256;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What happened to a program, for `__luna_child(handle, kind, data)`.
#[derive(Debug)]
pub struct ChildEvent {
    pub handle: u32,
    /// "spawn" (`data` = pid), "stdout" / "stderr" (base64 `data`), "exit" and
    /// "close" (`data` = `{"code": n|null, "signal": "SIGTERM"|null}`) or
    /// "error" (`data` = "CODE: message").
    pub kind: &'static str,
    pub data: String,
    /// Output bytes this event holds in the reader's backlog; given back when
    /// the event is dropped, handled or not.
    credit: Option<(Arc<AtomicUsize>, usize)>,
}

impl Drop for ChildEvent {
    fn drop(&mut self) {
        if let Some((backlog, n)) = self.credit.take() {
            backlog.fetch_sub(n, Ordering::Relaxed);
        }
    }
}

impl ChildEvent {
    fn new(handle: u32, kind: &'static str, data: String) -> Self {
        ChildEvent {
            handle,
            kind,
            data,
            credit: None,
        }
    }
}

/// Where a program's events go (a message to the plugin thread).
pub type Notifier = Arc<dyn Fn(ChildEvent) + Send + Sync>;

/// The user's answer to a [`ProgramRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramChoice {
    Once,
    Always,
    Deny,
}

/// A question for the user: may this plugin run this program?
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRequest {
    pub id: u64,
    /// The plugin's package name.
    pub plugin: String,
    /// The executable's full path (what an Always answer is kept for).
    pub program: String,
    /// The executable's file name.
    pub name: String,
    /// The command line, shortened, for the user to judge.
    pub command: String,
    /// The command line needs a shell: the shell is what runs.
    pub shell: bool,
    /// Whether Always is offered.
    pub remember: bool,
}

// ------------------------------------------------------------------ prepare

/// What a plugin asked to run, as it passed it.
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// A program name or path (`spawn`, `execFile`); empty for a command line.
    pub file: String,
    pub args: Vec<String>,
    /// A command line (`exec`).
    pub command: Option<String>,
    /// `{ shell: true }`: always through the shell.
    pub shell: bool,
    pub cwd: Option<String>,
    /// Variables the plugin added or changed.
    pub env: Option<Vec<(String, String)>>,
    /// Which of stdin, stdout, stderr are pipes (the rest are closed).
    pub pipes: [bool; 3],
}

/// What will run, resolved.
#[derive(Clone, Debug)]
pub struct Spec {
    pub exe: PathBuf,
    pub args: Vec<String>,
    /// Windows: the rest of a `cmd /c` line, passed as it is.
    pub raw_tail: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Option<Vec<(String, String)>>,
    pub pipes: [bool; 3],
}

/// A [`Request`] resolved, ready to ask about.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub spec: Spec,
    pub program: String,
    pub name: String,
    pub command: String,
    pub shell: bool,
    pub remember: bool,
}

/// `Err` carries "CODE: message" for the plugin's error event.
pub fn prepare(req: &Request, data_dir: &Path) -> Result<Prepared, String> {
    let total: usize = req.file.len()
        + req.args.iter().map(|a| a.len() + 1).sum::<usize>()
        + req.command.as_deref().map_or(0, str::len);
    if total > MAX_COMMAND {
        return Err("E2BIG: argument list too long".into());
    }
    if req.env.as_ref().is_some_and(|e| e.len() > MAX_ENV) {
        return Err("E2BIG: environment too large".into());
    }
    if req.file.contains('\0')
        || req.args.iter().any(|a| a.contains('\0'))
        || req.command.as_deref().is_some_and(|c| c.contains('\0'))
    {
        return Err("ERR_INVALID_ARG_VALUE: arguments must not contain null bytes".into());
    }
    let cwd = match &req.cwd {
        Some(c) => {
            let p = PathBuf::from(c);
            if !p.is_dir() {
                return Err(format!("ENOENT: the working directory {c} does not exist"));
            }
            Some(p)
        }
        None => {
            // A plugin's own folder, not wherever Tidalfast was started.
            data_dir.is_dir().then(|| data_dir.to_path_buf())
        }
    };
    // Work out file + args, or the shell line.
    let (file, args, line): (String, Vec<String>, Option<String>) = match (&req.command, req.shell)
    {
        (Some(c), _) if c.trim().is_empty() => {
            return Err("EINVAL: the command is empty".into());
        }
        (Some(c), false) => match parse_command_line(c, cfg!(windows)) {
            Some(mut t) if !t.is_empty() => {
                let file = t.remove(0);
                (file, t, None)
            }
            _ => (String::new(), Vec::new(), Some(c.clone())),
        },
        (Some(c), true) => (String::new(), Vec::new(), Some(c.clone())),
        (None, true) => {
            let mut line = quote_for_line(&req.file);
            for a in &req.args {
                line.push(' ');
                line.push_str(&quote_for_line(a));
            }
            (String::new(), Vec::new(), Some(line))
        }
        (None, false) => {
            if req.file.is_empty() {
                return Err("ERR_INVALID_ARG_VALUE: the program is empty".into());
            }
            (req.file.clone(), req.args.clone(), None)
        }
    };
    let (exe, spec_args, raw_tail, shell, command) = match line {
        Some(line) => {
            let (exe, args, raw) = shell_line(&line)?;
            (exe, args, raw, true, line)
        }
        None => {
            let exe = resolve_program(&file, cwd.as_deref())
                .ok_or_else(|| format!("ENOENT: spawn {file} ENOENT"))?;
            let command = preview(&file, &args);
            (exe, args, None, false, command)
        }
    };
    let program = exe.to_string_lossy().into_owned();
    let name = exe
        .file_name()
        .map_or_else(|| program.clone(), |n| n.to_string_lossy().into_owned());
    let remember = !shell && !is_inside(&exe, data_dir);
    Ok(Prepared {
        spec: Spec {
            exe,
            args: spec_args,
            raw_tail,
            cwd,
            env: req.env.clone(),
            pipes: req.pipes,
        },
        program,
        name,
        command: shorten(&command, 300),
        shell,
        remember,
    })
}

fn is_inside(exe: &Path, dir: &Path) -> bool {
    let dir = clean(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()));
    let exe = clean(exe.to_path_buf());
    let (exe, dir) = (exe.to_string_lossy(), dir.to_string_lossy());
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = format!("{}{sep}", dir.trim_end_matches(sep));
    if cfg!(windows) {
        exe.to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
    } else {
        exe.starts_with(&prefix)
    }
}

/// The shell and its arguments for a command line.
fn shell_line(line: &str) -> Result<(PathBuf, Vec<String>, Option<String>), String> {
    #[cfg(windows)]
    {
        let comspec = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| resolve_program("cmd.exe", None))
            .ok_or("ENOENT: spawn cmd.exe ENOENT")?;
        Ok((
            clean(comspec),
            vec!["/d".into(), "/s".into(), "/c".into()],
            Some(format!("\"{line}\"")),
        ))
    }
    #[cfg(not(windows))]
    {
        let sh = ["/bin/sh", "/usr/bin/sh"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
            .ok_or("ENOENT: spawn /bin/sh ENOENT")?;
        Ok((sh, vec!["-c".into(), line.to_string()], None))
    }
}

fn quote_for_line(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '"', '\'']) {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// `program arg "arg two"`, for the dialog.
fn preview(file: &str, args: &[String]) -> String {
    let mut out = quote_for_line(file);
    for a in args {
        out.push(' ');
        out.push_str(&quote_for_line(a));
    }
    out
}

fn shorten(s: &str, max: usize) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if s.chars().count() <= max {
        return s;
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

/// Splits a command line into words the way a shell would, when that is all
/// the shell would do. `None` means it needs a shell: pipes, redirections,
/// command separators, substitutions, globs, variables.
pub fn parse_command_line(cmd: &str, windows: bool) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' || c == '\r' {
            return None;
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if windows => {
                if c == '%' {
                    return None;
                }
                cur.push(c);
            }
            Some('"') => match c {
                '$' | '`' => return None,
                '\\' => match chars.peek() {
                    Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                        cur.push(n);
                        chars.next();
                    }
                    _ => cur.push(c),
                },
                _ => cur.push(c),
            },
            Some(_) => cur.push(c),
            None => match c {
                '"' => {
                    quote = Some('"');
                    started = true;
                }
                '\'' if !windows => {
                    quote = Some('\'');
                    started = true;
                }
                ' ' | '\t' => {
                    if started || !cur.is_empty() {
                        words.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                '&' | '|' | '<' | '>' | '(' | ')' => return None,
                '%' | '^' if windows => return None,
                ';' | '`' | '$' | '*' | '?' | '[' | ']' | '{' | '}' | '~' | '#' | '!'
                    if !windows =>
                {
                    return None;
                }
                '\\' if !windows => {
                    cur.push(chars.next()?);
                    started = true;
                }
                _ => cur.push(c),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !cur.is_empty() {
        words.push(cur);
    }
    Some(words)
}

// ---------------------------------------------------------------- resolve

fn exts() -> &'static [&'static str] {
    if cfg!(windows) {
        &[".com", ".exe", ".bat", ".cmd"]
    } else {
        &[""]
    }
}

fn is_program(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else {
        return false;
    };
    if !m.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// `base` as it is or with an executable extension.
fn with_exts(base: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        let name = base.file_name()?.to_string_lossy().to_ascii_lowercase();
        if exts().iter().any(|e| name.ends_with(e)) && is_program(base) {
            return Some(base.to_path_buf());
        }
        let s = base.as_os_str().to_string_lossy().into_owned();
        exts()
            .iter()
            .map(|e| PathBuf::from(format!("{s}{e}")))
            .find(|p| is_program(p))
    } else {
        is_program(base).then(|| base.to_path_buf())
    }
}

/// The program a name means: a path as given (relative to `cwd`), or the
/// first match on `PATH` (never the current directory). Returned as an
/// absolute path without Windows' `\\?\` prefix.
pub fn resolve_program(name: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let has_dir = name.contains('/') || (cfg!(windows) && name.contains('\\'));
    let found = if has_dir {
        let p = PathBuf::from(name);
        let p = if p.is_absolute() {
            p
        } else {
            cwd.map_or_else(|| p.clone(), |c| c.join(&p))
        };
        with_exts(&p)
    } else {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .filter(|d| !d.as_os_str().is_empty())
            .find_map(|d| with_exts(&d.join(name)))
    }?;
    let found = std::fs::canonicalize(&found).unwrap_or(found);
    Some(clean(found))
}

/// `\\?\C:\x` -> `C:\x`.
fn clean(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\")
        && !rest.starts_with("UNC\\")
    {
        return PathBuf::from(rest);
    }
    p
}

/// Whether two program paths are the same file name (case does not matter on
/// Windows).
pub fn same_program(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

// ------------------------------------------------------------------- start

/// Messages to a running program.
#[derive(Debug)]
pub enum Ctl {
    Stdin(Vec<u8>),
    CloseStdin,
    Kill(String),
}

fn command(spec: &Spec) -> Command {
    let mut cmd = Command::new(&spec.exe);
    cmd.args(&spec.args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
        if let Some(raw) = &spec.raw_tail {
            cmd.raw_arg(raw);
        }
    }
    // Only Windows runs a command line as one raw string.
    #[cfg(not(windows))]
    debug_assert!(spec.raw_tail.is_none());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    if let Some(env) = &spec.env {
        // Added to Tidalfast's own environment, not instead of it: a plugin
        // only sees a few variables of its own, and a program without PATH
        // or SystemRoot does not start.
        for (k, v) in env {
            if !k.is_empty() && !k.contains(['=', '\0']) && !v.contains('\0') {
                cmd.env(k, v);
            }
        }
    }
    let io = |pipe: bool| if pipe { Stdio::piped() } else { Stdio::null() };
    cmd.stdin(io(spec.pipes[0]))
        .stdout(io(spec.pipes[1]))
        .stderr(io(spec.pipes[2]));
    cmd
}

fn spawn_error(e: &std::io::Error, spec: &Spec) -> String {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        _ => "EIO",
    };
    format!("{code}: spawn {} {code}: {e}", spec.exe.display())
}

fn signal_name(status: &std::process::ExitStatus) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|n| {
            match n {
                1 => "SIGHUP",
                2 => "SIGINT",
                6 => "SIGABRT",
                9 => "SIGKILL",
                11 => "SIGSEGV",
                13 => "SIGPIPE",
                15 => "SIGTERM",
                _ => return format!("SIG{n}"),
            }
            .to_string()
        })
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

fn exit_json(code: Option<i32>, signal: Option<&str>) -> String {
    serde_json::json!({ "code": code, "signal": signal }).to_string()
}

/// Starts the program and its threads; events go to `notify`.
fn start(spec: &Spec, handle: u32, notify: Notifier) -> Result<Sender<Ctl>, String> {
    let mut child = command(spec).spawn().map_err(|e| spawn_error(&e, spec))?;
    let pid = child.id();
    notify(ChildEvent::new(handle, "spawn", pid.to_string()));
    let backlog = Arc::new(AtomicUsize::new(0));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut readers = 0;
    if let Some(out) = child.stdout.take() {
        readers += 1;
        pump(out, handle, "stdout", &notify, &backlog, done_tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        readers += 1;
        pump(err, handle, "stderr", &notify, &backlog, done_tx.clone());
    }
    drop(done_tx);
    let stdin_tx = child.stdin.take().map(|stdin| {
        let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
        let _ = std::thread::Builder::new()
            .name(format!("luna-child-in-{handle}"))
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(Some(data)) = rx.recv() {
                    if stdin.write_all(&data).and_then(|()| stdin.flush()).is_err() {
                        break;
                    }
                }
            });
        tx
    });
    let (ctl_tx, ctl_rx) = mpsc::channel::<Ctl>();
    let spawned = std::thread::Builder::new()
        .name(format!("luna-child-{handle}"))
        .spawn(move || {
            let status = watch(&mut child, &ctl_rx, stdin_tx.as_ref());
            drop(stdin_tx);
            let (code, signal) = status;
            notify(ChildEvent::new(
                handle,
                "exit",
                exit_json(code, signal.as_deref()),
            ));
            // The output ends with the program, unless a program it started
            // still holds the pipe: do not wait for that for long.
            let end = Instant::now() + Duration::from_millis(1500);
            for _ in 0..readers {
                let left = end.saturating_duration_since(Instant::now());
                if done_rx.recv_timeout(left).is_err() {
                    break;
                }
            }
            notify(ChildEvent::new(
                handle,
                "close",
                exit_json(code, signal.as_deref()),
            ));
        });
    match spawned {
        Ok(_) => Ok(ctl_tx),
        Err(e) => Err(format!("EAGAIN: could not start a thread: {e}")),
    }
}

/// Waits for the program, handling messages meanwhile; its exit code and
/// signal (a program that was killed has a signal and no code).
fn watch(
    child: &mut Child,
    ctl: &Receiver<Ctl>,
    stdin: Option<&Sender<Option<Vec<u8>>>>,
) -> (Option<i32>, Option<String>) {
    let mut killed: Option<String> = None;
    let mut stdin = stdin;
    loop {
        match ctl.recv_timeout(Duration::from_millis(15)) {
            Ok(Ctl::Stdin(data)) => {
                if let Some(tx) = stdin {
                    let _ = tx.send(Some(data));
                }
            }
            Ok(Ctl::CloseStdin) => {
                if let Some(tx) = stdin.take() {
                    let _ = tx.send(None);
                }
            }
            Ok(Ctl::Kill(signal)) => {
                if killed.is_none() && child.try_wait().ok().flatten().is_none() {
                    killed = Some(signal);
                    let _ = child.kill();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Nobody listens any more: do not leave it running.
                if killed.is_none() {
                    killed = Some("SIGKILL".into());
                    let _ = child.kill();
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return match killed {
                    Some(sig) if !status.success() || status.code().is_none() => (None, Some(sig)),
                    _ => (status.code(), signal_name(&status)),
                };
            }
            Ok(None) => {}
            Err(_) => return (None, killed),
        }
    }
}

fn pump(
    mut r: impl Read + Send + 'static,
    handle: u32,
    kind: &'static str,
    notify: &Notifier,
    backlog: &Arc<AtomicUsize>,
    done: Sender<()>,
) {
    let notify = notify.clone();
    let backlog = backlog.clone();
    let _ = std::thread::Builder::new()
        .name(format!("luna-child-{kind}-{handle}"))
        .spawn(move || {
            let mut buf = vec![0u8; 32 * 1024];
            loop {
                while backlog.load(Ordering::Relaxed) > BACKLOG {
                    std::thread::sleep(Duration::from_millis(5));
                }
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        let size = data.len();
                        backlog.fetch_add(size, Ordering::Relaxed);
                        notify(ChildEvent {
                            handle,
                            kind,
                            data,
                            credit: Some((backlog.clone(), size)),
                        });
                    }
                }
            }
            let _ = done.send(());
        });
}

// -------------------------------------------------------------------- sync

/// What a synchronous run produced (`spawnSync`, `execSync`).
#[derive(Debug, Default)]
pub struct SyncResult {
    pub pid: u32,
    pub status: Option<i32>,
    pub signal: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// "CODE: message": it could not start, or ran out of time or buffer.
    pub error: Option<String>,
}

/// Runs a program to its end here, within `timeout` (the plugin thread waits,
/// so the caller keeps this short). Only for programs already allowed.
pub fn run_sync(
    spec: &Spec,
    input: Option<Vec<u8>>,
    timeout: Duration,
    max_buffer: usize,
) -> SyncResult {
    let mut out = SyncResult::default();
    let mut child = match command(spec).spawn() {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(spawn_error(&e, spec));
            return out;
        }
    };
    out.pid = child.id();
    let read = |r: Option<Box<dyn Read + Send>>| {
        r.map(|mut r| {
            std::thread::spawn(move || {
                let mut all = Vec::new();
                let mut buf = [0u8; 16 * 1024];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    all.extend_from_slice(&buf[..n]);
                    if all.len() > max_buffer.saturating_add(1) {
                        break;
                    }
                }
                all
            })
        })
    };
    let stdout = read(child.stdout.take().map(|r| Box::new(r) as _));
    let stderr = read(child.stderr.take().map(|r| Box::new(r) as _));
    if let Some(mut stdin) = child.stdin.take() {
        let data = input.unwrap_or_default();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let end = Instant::now() + timeout;
    let mut killed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= end {
            killed = true;
            let _ = child.kill();
            out.error = Some("ETIMEDOUT: spawnSync timed out".into());
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(4));
    };
    let join = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
        h.map(|h| h.join().unwrap_or_default()).unwrap_or_default()
    };
    out.stdout = join(stdout);
    out.stderr = join(stderr);
    if out.stdout.len() > max_buffer || out.stderr.len() > max_buffer {
        out.stdout.truncate(max_buffer);
        out.stderr.truncate(max_buffer);
        out.error
            .get_or_insert("ENOBUFS: spawnSync exceeded maxBuffer".into());
    }
    if let Some(s) = status {
        if killed {
            out.signal = Some("SIGTERM".into());
        } else {
            out.status = s.code();
            out.signal = signal_name(&s);
        }
    }
    out
}

// --------------------------------------------------------------------- hub

struct Running {
    plugin: String,
    program: String,
    ctl: Sender<Ctl>,
}

struct Waiting {
    request: ProgramRequest,
    /// Filed by a synchronous call, which could not wait: there is nothing
    /// to start, the answer is for the next call.
    later: bool,
    items: Vec<(u32, Spec)>,
}

/// The programs of every plugin: running ones, and those waiting for an
/// answer. Lives on the plugin thread.
#[derive(Default)]
pub struct ChildHub {
    next: u32,
    next_prompt: u64,
    running: HashMap<u32, Running>,
    waiting: Vec<Waiting>,
    /// Messages for programs that wait for an answer (a plugin may write to
    /// its stdin right after `spawn`).
    queued: HashMap<u32, Vec<Ctl>>,
    denied: HashMap<(String, String), Instant>,
    /// Allow once answers to a synchronous call's question, used up by the
    /// next call.
    once: Vec<(String, String)>,
}

/// What became of a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Running (or failed to start: the error event says).
    Started,
    /// Waiting for the user.
    Asked,
    /// Refused without asking.
    Refused,
}

impl ChildHub {
    fn alloc(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    fn count(&self, plugin: &str) -> usize {
        self.running.values().filter(|r| r.plugin == plugin).count()
            + self
                .waiting
                .iter()
                .filter(|w| w.request.plugin == plugin)
                .map(|w| w.items.len())
                .sum::<usize>()
    }

    /// A plugin wants to run `p`. `allowed`: the user said Always before.
    /// Either way the plugin gets a handle at once; what happens next comes
    /// as events.
    pub fn submit(
        &mut self,
        plugin: &str,
        p: Prepared,
        allowed: bool,
        notify: &Notifier,
    ) -> (u32, Outcome) {
        let handle = self.alloc();
        if self.count(plugin) >= MAX_PER_PLUGIN {
            fail(
                notify,
                handle,
                "EAGAIN: too many programs are running for this plugin",
            );
            return (handle, Outcome::Refused);
        }
        let key = (plugin.to_string(), p.program.to_ascii_lowercase());
        if allowed && p.remember {
            self.run(plugin, &p.program, p.spec, handle, notify);
            return (handle, Outcome::Started);
        }
        if let Some(at) = self.denied.get(&key)
            && at.elapsed() < DENY_COOLDOWN
        {
            fail(notify, handle, &denied_message(&p.name));
            return (handle, Outcome::Refused);
        }
        // The same question is asked once.
        if let Some(w) = self.waiting.iter_mut().find(|w| {
            w.request.plugin == plugin
                && same_program(&w.request.program, &p.program)
                && w.request.command == p.command
        }) {
            w.items.push((handle, p.spec));
            return (handle, Outcome::Asked);
        }
        self.next_prompt += 1;
        self.waiting.push(Waiting {
            request: ProgramRequest {
                id: self.next_prompt,
                plugin: plugin.to_string(),
                program: p.program,
                name: p.name,
                command: p.command,
                shell: p.shell,
                remember: p.remember,
            },
            later: false,
            items: vec![(handle, p.spec)],
        });
        (handle, Outcome::Asked)
    }

    /// Files a question for a synchronous call, which cannot wait for the
    /// answer: the user may allow the next call.
    pub fn ask_for_later(&mut self, plugin: &str, p: &Prepared) {
        if let Some(at) = self
            .denied
            .get(&(plugin.to_string(), p.program.to_ascii_lowercase()))
            && at.elapsed() < DENY_COOLDOWN
        {
            return;
        }
        if self.waiting.iter().any(|w| {
            w.request.plugin == plugin
                && same_program(&w.request.program, &p.program)
                && w.request.command == p.command
        }) {
            return;
        }
        self.next_prompt += 1;
        self.waiting.push(Waiting {
            request: ProgramRequest {
                id: self.next_prompt,
                plugin: plugin.to_string(),
                program: p.program.clone(),
                name: p.name.clone(),
                command: p.command.clone(),
                shell: p.shell,
                remember: p.remember,
            },
            later: true,
            items: Vec::new(),
        });
    }

    /// Uses up an Allow once given to a synchronous call's question.
    pub fn take_once(&mut self, plugin: &str, program: &str) -> bool {
        let key = (plugin.to_string(), program.to_ascii_lowercase());
        match self.once.iter().position(|k| *k == key) {
            Some(at) => {
                self.once.remove(at);
                true
            }
            None => false,
        }
    }

    fn run(&mut self, plugin: &str, program: &str, spec: Spec, handle: u32, notify: &Notifier) {
        match start(&spec, handle, notify.clone()) {
            Ok(ctl) => {
                for msg in self.queued.remove(&handle).unwrap_or_default() {
                    let _ = ctl.send(msg);
                }
                self.running.insert(
                    handle,
                    Running {
                        plugin: plugin.to_string(),
                        program: program.to_string(),
                        ctl,
                    },
                );
            }
            Err(e) => {
                self.queued.remove(&handle);
                fail(notify, handle, &e);
            }
        }
    }

    /// The questions waiting for the user, oldest first.
    pub fn requests(&self) -> Vec<ProgramRequest> {
        self.waiting.iter().map(|w| w.request.clone()).collect()
    }

    /// The user answered. Returns the plugin and program to remember, if
    /// the answer was Always.
    pub fn answer(
        &mut self,
        id: u64,
        choice: ProgramChoice,
        notify: &Notifier,
    ) -> Option<(String, String)> {
        let at = self.waiting.iter().position(|w| w.request.id == id)?;
        let w = self.waiting.remove(at);
        let plugin = w.request.plugin.clone();
        let program = w.request.program.clone();
        match choice {
            ProgramChoice::Deny => {
                self.denied.insert(
                    (plugin.clone(), program.to_ascii_lowercase()),
                    Instant::now(),
                );
                for (handle, _) in w.items {
                    self.queued.remove(&handle);
                    fail(notify, handle, &denied_message(&w.request.name));
                }
                None
            }
            ProgramChoice::Once | ProgramChoice::Always => {
                self.denied
                    .remove(&(plugin.clone(), program.to_ascii_lowercase()));
                for (handle, spec) in w.items {
                    self.run(&plugin, &program, spec, handle, notify);
                }
                if w.later && choice == ProgramChoice::Once {
                    self.once
                        .push((plugin.clone(), program.to_ascii_lowercase()));
                }
                (choice == ProgramChoice::Always && w.request.remember && !w.request.shell)
                    .then_some((plugin, program))
            }
        }
    }

    pub fn write(&mut self, handle: u32, data: Vec<u8>) -> Result<(), String> {
        self.send(handle, Ctl::Stdin(data))
    }

    pub fn close_stdin(&mut self, handle: u32) -> Result<(), String> {
        self.send(handle, Ctl::CloseStdin)
    }

    pub fn kill(&mut self, handle: u32, signal: &str) -> bool {
        if let Some(r) = self.running.get(&handle) {
            return r.ctl.send(Ctl::Kill(signal.to_string())).is_ok();
        }
        // Killed before the answer: it never starts.
        for w in &mut self.waiting {
            w.items.retain(|(h, _)| *h != handle);
        }
        self.waiting.retain(|w| w.later || !w.items.is_empty());
        self.queued.remove(&handle).is_some()
    }

    fn send(&mut self, handle: u32, msg: Ctl) -> Result<(), String> {
        if let Some(r) = self.running.get(&handle) {
            return r
                .ctl
                .send(msg)
                .map_err(|_| "EPIPE: write after the process ended".to_string());
        }
        if self
            .waiting
            .iter()
            .any(|w| w.items.iter().any(|(h, _)| *h == handle))
        {
            self.queued.entry(handle).or_default().push(msg);
            return Ok(());
        }
        Err("EPIPE: the process has ended".into())
    }

    /// The program ended (its "close" event arrived).
    pub fn forget(&mut self, handle: u32) {
        self.running.remove(&handle);
    }

    /// A plugin was unloaded, disabled or removed: its programs stop and its
    /// questions go away.
    pub fn close_plugin(&mut self, plugin: &str) {
        for r in self.running.values().filter(|r| r.plugin == plugin) {
            let _ = r.ctl.send(Ctl::Kill("SIGTERM".into()));
        }
        let gone: Vec<u32> = self
            .waiting
            .iter()
            .filter(|w| w.request.plugin == plugin)
            .flat_map(|w| w.items.iter().map(|(h, _)| *h))
            .collect();
        for h in gone {
            self.queued.remove(&h);
        }
        self.waiting.retain(|w| w.request.plugin != plugin);
    }

    /// The user took the permission back: running programs of that plugin
    /// stop too.
    pub fn revoke(&mut self, plugin: &str, program: &str) {
        for r in self
            .running
            .values()
            .filter(|r| r.plugin == plugin && same_program(&r.program, program))
        {
            let _ = r.ctl.send(Ctl::Kill("SIGTERM".into()));
        }
    }

    /// Whether `handle` is a program of `plugin`, running or waiting.
    pub fn owns(&self, plugin: &str, handle: u32) -> bool {
        self.running
            .get(&handle)
            .is_some_and(|r| r.plugin == plugin)
            || self
                .waiting
                .iter()
                .any(|w| w.request.plugin == plugin && w.items.iter().any(|(h, _)| *h == handle))
    }

    /// A call that fails before anything starts: the plugin gets a handle
    /// and the error as events.
    pub fn reject(&mut self, notify: &Notifier, message: &str) -> u32 {
        let handle = self.alloc();
        fail(notify, handle, message);
        handle
    }

    /// The app is closing: stop everything and give the threads a moment to
    /// do it.
    pub fn stop_all(&mut self) {
        let any = !self.running.is_empty();
        for r in self.running.values() {
            let _ = r.ctl.send(Ctl::Kill("SIGTERM".into()));
        }
        self.waiting.clear();
        self.queued.clear();
        if any {
            std::thread::sleep(Duration::from_millis(120));
        }
    }

    #[cfg(test)]
    pub fn running_for(&self, plugin: &str) -> usize {
        self.running.values().filter(|r| r.plugin == plugin).count()
    }
}

fn denied_message(name: &str) -> String {
    format!("EACCES: you did not allow this plugin to run {name}")
}

fn fail(notify: &Notifier, handle: u32, message: &str) {
    notify(ChildEvent::new(handle, "error", message.to_string()));
    notify(ChildEvent::new(handle, "close", exit_json(None, None)));
}

#[cfg(test)]
#[path = "child_tests.rs"]
mod tests;
