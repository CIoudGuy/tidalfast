use std::sync::Mutex;

use super::*;

type Events = Arc<Mutex<Vec<(u32, String, String)>>>;

fn collector() -> (Notifier, Events) {
    let events: Events = Arc::default();
    let sink = events.clone();
    let notify: Notifier = Arc::new(move |e: ChildEvent| {
        sink.lock()
            .unwrap()
            .push((e.handle, e.kind.to_string(), e.data.clone()));
    });
    (notify, events)
}

fn wait_for(events: &Events, handle: u32, kind: &str) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if let Some((_, _, data)) = events
            .lock()
            .unwrap()
            .iter()
            .find(|(h, k, _)| *h == handle && k == kind)
        {
            return Some(data.clone());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

fn output(events: &Events, handle: u32, kind: &str) -> String {
    let all: Vec<u8> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|(h, k, _)| *h == handle && k == kind)
        .flat_map(|(_, _, d)| base64::engine::general_purpose::STANDARD.decode(d).unwrap())
        .collect();
    String::from_utf8_lossy(&all).into_owned()
}

fn kinds(events: &Events, handle: u32) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter(|(h, _, _)| *h == handle)
        .map(|(_, k, _)| k.clone())
        .collect()
}

/// A shell one-liner as a program with arguments (not a shell command line).
fn script(win: &str, unix: &str) -> Request {
    let (file, flags, line) = if cfg!(windows) {
        ("cmd", vec!["/d", "/c"], win)
    } else {
        ("sh", vec!["-c"], unix)
    };
    let mut args: Vec<String> = flags.into_iter().map(String::from).collect();
    args.push(line.to_string());
    Request {
        file: file.into(),
        args,
        pipes: [true, true, true],
        ..Request::default()
    }
}

fn data_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("luna-child-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn code_of(exit: &str) -> (Option<i64>, Option<String>) {
    let v: serde_json::Value = serde_json::from_str(exit).unwrap();
    (v["code"].as_i64(), v["signal"].as_str().map(str::to_string))
}

#[test]
fn command_lines_split_like_a_shell_when_a_shell_adds_nothing() {
    let unix = |s| parse_command_line(s, false);
    let win = |s| parse_command_line(s, true);
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(
        unix("mpv --no-video  'a b' \"c d\" e\\ f"),
        v(&["mpv", "--no-video", "a b", "c d", "e f"])
    );
    assert_eq!(
        unix("x \"\" y"),
        v(&["x", "", "y"]),
        "an empty word is a word"
    );
    assert_eq!(
        win(r#""C:\Program Files\mpv\mpv.exe" --pause "a b""#),
        v(&[r"C:\Program Files\mpv\mpv.exe", "--pause", "a b"])
    );
    assert_eq!(
        win(r"mpv C:\music\a.flac"),
        v(&["mpv", r"C:\music\a.flac"]),
        "backslashes are paths on Windows"
    );
    assert_eq!(
        win("mpv 'a'"),
        v(&["mpv", "'a'"]),
        "single quotes are not quotes on Windows"
    );
    assert_eq!(
        win(r#"tool "a & b""#),
        v(&["tool", "a & b"]),
        "inside quotes cmd does not read it"
    );
    // Anything a shell would act on needs the shell.
    for line in [
        "a && b",
        "a | b",
        "a > out",
        "a; b",
        "a $HOME",
        "a `b`",
        "a $(b)",
        "a *.mp3",
        "a\nb",
        "a 'unclosed",
        "~/x",
        "a (b)",
    ] {
        assert_eq!(unix(line), None, "{line}");
    }
    for line in [
        "a & b",
        "a | b",
        "a > out",
        "a %PATH%",
        "a ^b",
        "a \"%X%\"",
        "a (b)",
        "a \"unclosed",
    ] {
        assert_eq!(win(line), None, "{line}");
    }
    assert_eq!(unix("   "), Some(vec![]));
}

#[test]
fn a_request_is_resolved_to_one_program_path() {
    let dir = data_dir("prepare");
    let p = prepare(&script("echo hi", "echo hi"), &dir).unwrap();
    let exe = p.spec.exe.to_string_lossy().to_string();
    assert!(Path::new(&exe).is_absolute(), "{exe}");
    assert!(!exe.starts_with(r"\\?\"), "{exe}");
    assert_eq!(p.program, exe);
    assert!(!p.shell && p.remember);
    // (/bin/sh may be a link to dash: the resolved file is what counts.)
    assert!(!p.name.is_empty());
    assert!(p.command.contains("echo hi"), "{}", p.command);
    // The folder is the plugin's own, unless it said otherwise.
    assert_eq!(p.spec.cwd.as_deref(), Some(dir.as_path()));

    // A missing program is ENOENT; a bad folder too.
    let missing = Request {
        file: "definitely-not-a-program-xyz".into(),
        ..Request::default()
    };
    let err = prepare(&missing, &dir).unwrap_err();
    assert!(err.starts_with("ENOENT"), "{err}");
    let bad_cwd = Request {
        cwd: Some(dir.join("nope").to_string_lossy().into_owned()),
        ..script("echo", "echo")
    };
    assert!(prepare(&bad_cwd, &dir).unwrap_err().starts_with("ENOENT"));
    // The current directory is never searched for a bare name.
    assert!(resolve_program("Cargo.toml", None).is_none());

    // A command line with `&&` runs the shell: not rememberable.
    let line = Request {
        command: Some("echo a && echo b".into()),
        ..Request::default()
    };
    let p = prepare(&line, &dir).unwrap();
    assert!(p.shell && !p.remember, "{p:?}");
    assert_eq!(p.command, "echo a && echo b");
    // So is `{ shell: true }`.
    let forced = Request {
        shell: true,
        ..script("echo a", "echo a")
    };
    assert!(prepare(&forced, &dir).unwrap().shell);
    // A plain command line is the program, not the shell.
    let plain = Request {
        command: Some(
            if cfg!(windows) {
                "cmd /d /c echo hi"
            } else {
                "sh -c 'echo hi'"
            }
            .into(),
        ),
        ..Request::default()
    };
    let p = prepare(&plain, &dir).unwrap();
    assert!(!p.shell && p.remember);
    assert_eq!(
        p.spec.args.last().map(String::as_str),
        Some(if cfg!(windows) { "hi" } else { "echo hi" })
    );
    // Null bytes and huge argument lists are refused.
    let nul = Request {
        file: "a\0b".into(),
        ..Request::default()
    };
    assert!(prepare(&nul, &dir).is_err());
    let huge = Request {
        args: vec!["x".repeat(MAX_COMMAND + 1)],
        ..script("echo", "echo")
    };
    assert!(prepare(&huge, &dir).unwrap_err().starts_with("E2BIG"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_in_the_plugins_own_folder_cannot_be_remembered() {
    let dir = data_dir("inside");
    let inside = dir.join("tools").join("run.exe");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(&inside, b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The folder can be reached by another name for it: an 8.3 short name
    // on Windows (CI's C:\Users\RUNNER~1), a link on macOS (/var is
    // /private/var). prepare resolves the program before asking, as
    // is_inside resolves the folder, so it compares the program as
    // resolve_program finds it.
    let asked = Request {
        file: inside.to_string_lossy().into_owned(),
        ..Request::default()
    };
    assert!(!prepare(&asked, &dir).unwrap().remember);
    let found = resolve_program(&inside.to_string_lossy(), None).unwrap();
    assert!(is_inside(&found, &dir));
    assert!(!is_inside(Path::new("/usr/bin/sh"), &dir));
    let resolved = clean(std::fs::canonicalize(&dir).unwrap());
    let sibling = PathBuf::from(format!("{}-other/run.exe", resolved.display()));
    assert!(
        !is_inside(&sibling, &dir),
        "a folder that only shares the prefix"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_runs_until_the_user_answers() {
    let dir = data_dir("ask");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let p = prepare(&script("echo hello", "echo hello"), &dir).unwrap();
    let (h, outcome) = hub.submit("Mpv", p.clone(), false, &notify);
    assert_eq!(outcome, Outcome::Asked);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events.lock().unwrap().is_empty(), "no process, no event");
    let asked = hub.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].plugin, "Mpv");
    assert_eq!(asked[0].program, p.program);
    assert!(asked[0].remember && !asked[0].shell);

    // Allow once: it runs, and the answer is not kept.
    assert_eq!(hub.answer(asked[0].id, ProgramChoice::Once, &notify), None);
    assert!(hub.requests().is_empty());
    let exit = wait_for(&events, h, "close").expect("closed");
    assert_eq!(code_of(&exit), (Some(0), None));
    assert_eq!(output(&events, h, "stdout").trim(), "hello");
    let order = kinds(&events, h);
    assert_eq!(order.first().map(String::as_str), Some("spawn"));
    assert_eq!(order.last().map(String::as_str), Some("close"));
    assert!(order.contains(&"exit".to_string()));
    assert!(
        order.iter().position(|k| k == "stdout").unwrap() < order.len() - 1,
        "output before close"
    );
    hub.forget(h);

    // The next one asks again.
    let (_, outcome) = hub.submit("Mpv", p, false, &notify);
    assert_eq!(outcome, Outcome::Asked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn always_is_kept_for_the_program_and_deny_stops_the_asking() {
    let dir = data_dir("always");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let p = prepare(&script("echo a", "echo a"), &dir).unwrap();

    // Deny: the plugin gets EACCES, and asking again at once is refused
    // without bothering the user.
    let (h1, _) = hub.submit("Mpv", p.clone(), false, &notify);
    let id = hub.requests()[0].id;
    assert_eq!(hub.answer(id, ProgramChoice::Deny, &notify), None);
    let err = wait_for(&events, h1, "error").unwrap();
    assert!(err.starts_with("EACCES"), "{err}");
    assert!(wait_for(&events, h1, "close").is_some());
    let (h2, outcome) = hub.submit("Mpv", p.clone(), false, &notify);
    assert_eq!(outcome, Outcome::Refused);
    assert!(
        wait_for(&events, h2, "error")
            .unwrap()
            .starts_with("EACCES")
    );
    assert!(hub.requests().is_empty());
    // Another plugin is a different question.
    let (_, outcome) = hub.submit("Other", p.clone(), false, &notify);
    assert_eq!(outcome, Outcome::Asked);
    let other = hub.requests()[0].id;
    hub.answer(other, ProgramChoice::Deny, &notify);

    // Always: asked once more (the denial is forgotten), then the plugin and
    // the program come back to be saved, and the next call starts at once.
    hub.denied.clear();
    let (h3, _) = hub.submit("Mpv", p.clone(), false, &notify);
    let id = hub.requests()[0].id;
    let kept = hub.answer(id, ProgramChoice::Always, &notify);
    assert_eq!(kept, Some(("Mpv".to_string(), p.program.clone())));
    assert!(wait_for(&events, h3, "close").is_some());
    let (h4, outcome) = hub.submit("Mpv", p, true, &notify);
    assert_eq!(outcome, Outcome::Started);
    assert!(hub.requests().is_empty());
    assert_eq!(code_of(&wait_for(&events, h4, "close").unwrap()).0, Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shell_command_lines_are_never_remembered_even_when_allowed() {
    let dir = data_dir("shell");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let line = Request {
        command: Some("echo one && echo two".into()),
        pipes: [true, true, true],
        ..Request::default()
    };
    let p = prepare(&line, &dir).unwrap();
    assert!(p.shell);
    // A saved "allowed" does not apply: it asks, and has no Always.
    let (h, outcome) = hub.submit("Mpv", p, true, &notify);
    assert_eq!(outcome, Outcome::Asked);
    let asked = hub.requests();
    assert!(asked[0].shell && !asked[0].remember);
    assert_eq!(
        hub.answer(asked[0].id, ProgramChoice::Always, &notify),
        None,
        "nothing to save"
    );
    assert!(wait_for(&events, h, "close").is_some());
    let out = output(&events, h, "stdout");
    assert!(out.contains("one") && out.contains("two"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_question_is_asked_once_and_answers_every_waiting_call() {
    let dir = data_dir("same");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let p = prepare(&script("echo x", "echo x"), &dir).unwrap();
    let (a, _) = hub.submit("Mpv", p.clone(), false, &notify);
    let (b, _) = hub.submit("Mpv", p, false, &notify);
    assert_ne!(a, b);
    let asked = hub.requests();
    assert_eq!(asked.len(), 1, "one dialog for both");
    hub.answer(asked[0].id, ProgramChoice::Once, &notify);
    for h in [a, b] {
        assert_eq!(code_of(&wait_for(&events, h, "close").unwrap()).0, Some(0));
        assert_eq!(output(&events, h, "stdout").trim(), "x");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exit_codes_stderr_and_stdin_reach_the_plugin() {
    let dir = data_dir("io");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let failing = prepare(
        &script("echo oops 1>&2 & exit 3", "echo oops >&2; exit 3"),
        &dir,
    )
    .unwrap();
    let (h, _) = hub.submit("P", failing, true, &notify);
    // (A shell-free request cannot use `&`: the script runs inside cmd/sh.)
    let exit = wait_for(&events, h, "close").unwrap();
    assert_eq!(code_of(&exit), (Some(3), None));
    assert!(output(&events, h, "stderr").contains("oops"));

    // Input written before the answer is delivered once it is allowed.
    let echo = Request {
        file: if cfg!(windows) { "findstr" } else { "cat" }.into(),
        args: if cfg!(windows) {
            vec!["^".into()]
        } else {
            vec![]
        },
        pipes: [true, true, true],
        ..Request::default()
    };
    let p = prepare(&echo, &dir).unwrap();
    let (h, outcome) = hub.submit("P", p, false, &notify);
    assert_eq!(outcome, Outcome::Asked);
    hub.write(h, b"ping\n".to_vec()).unwrap();
    hub.close_stdin(h).unwrap();
    let id = hub.requests()[0].id;
    hub.answer(id, ProgramChoice::Once, &notify);
    assert_eq!(code_of(&wait_for(&events, h, "close").unwrap()).0, Some(0));
    assert_eq!(output(&events, h, "stdout").trim(), "ping");
    // After it ended there is nobody to write to.
    hub.forget(h);
    assert!(hub.write(h, b"x".to_vec()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn sleeper(dir: &Path) -> Prepared {
    let req = if cfg!(windows) {
        Request {
            file: "ping".into(),
            args: vec!["-n".into(), "60".into(), "127.0.0.1".into()],
            pipes: [false, true, true],
            ..Request::default()
        }
    } else {
        Request {
            file: "sleep".into(),
            args: vec!["60".into()],
            pipes: [false, true, true],
            ..Request::default()
        }
    };
    prepare(&req, dir).unwrap()
}

#[test]
fn programs_are_killed_when_the_plugin_goes_or_the_permission_is_revoked() {
    let dir = data_dir("kill");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let p = sleeper(&dir);

    // Unloading the plugin stops its programs, and only its own.
    let (mine, _) = hub.submit("Mpv", p.clone(), true, &notify);
    let (theirs, _) = hub.submit("Other", p.clone(), true, &notify);
    assert!(wait_for(&events, mine, "spawn").is_some());
    assert!(wait_for(&events, theirs, "spawn").is_some());
    assert_eq!(hub.running_for("Mpv"), 1);
    hub.close_plugin("Mpv");
    let exit = wait_for(&events, mine, "exit").expect("killed");
    assert_eq!(code_of(&exit), (None, Some("SIGTERM".into())));
    assert!(wait_for(&events, mine, "close").is_some());
    assert!(
        !kinds(&events, theirs).contains(&"exit".to_string()),
        "the other plugin's runs on"
    );

    // Revoking the permission stops what runs under it.
    hub.revoke("Other", &p.program);
    assert!(wait_for(&events, theirs, "exit").is_some());

    // A plugin asked about a program and was unloaded: the question is gone.
    let (w, outcome) = hub.submit("Mpv", p.clone(), false, &notify);
    assert_eq!(outcome, Outcome::Asked);
    hub.close_plugin("Mpv");
    assert!(hub.requests().is_empty());
    assert!(!kinds(&events, w).contains(&"spawn".to_string()));

    // kill() on one program, with the signal it asked for.
    let (h, _) = hub.submit("Mpv", p, true, &notify);
    wait_for(&events, h, "spawn").unwrap();
    assert!(hub.kill(h, "SIGKILL"));
    assert_eq!(
        code_of(&wait_for(&events, h, "exit").unwrap()),
        (None, Some("SIGKILL".into()))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plugin_cannot_start_a_crowd_of_programs() {
    let dir = data_dir("crowd");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    let p = sleeper(&dir);
    for _ in 0..MAX_PER_PLUGIN {
        let (_, outcome) = hub.submit("P", p.clone(), false, &notify);
        assert_ne!(outcome, Outcome::Refused);
    }
    let (extra, outcome) = hub.submit("P", p, false, &notify);
    assert_eq!(outcome, Outcome::Refused);
    assert!(
        wait_for(&events, extra, "error")
            .unwrap()
            .starts_with("EAGAIN")
    );
    hub.close_plugin("P");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_that_cannot_start_reports_an_error_not_a_hang() {
    let dir = data_dir("nostart");
    let (notify, events) = collector();
    let mut hub = ChildHub::default();
    // A file that exists and counts as a program but is not one. macOS's
    // posix_spawnp, which Rust starts programs with there, runs a file the
    // system cannot execute with /bin/sh, as execvp does; so there it names
    // an interpreter that does not exist, which no fallback runs.
    let junk = dir.join(if cfg!(windows) { "junk.exe" } else { "junk" });
    let body: &[u8] = if cfg!(target_os = "macos") {
        b"#!/nonexistent/tidalfast-test-interpreter\n"
    } else {
        b"not a program"
    };
    std::fs::write(&junk, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&junk, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let req = Request {
        file: junk.to_string_lossy().into_owned(),
        ..Request::default()
    };
    let p = prepare(&req, &dir).unwrap();
    assert!(!p.remember, "inside the plugin's folder");
    let (h, _) = hub.submit("P", p, false, &notify);
    let id = hub.requests()[0].id;
    hub.answer(id, ProgramChoice::Once, &notify);
    let err = wait_for(&events, h, "error").unwrap();
    assert!(err.contains(':'), "{err}");
    assert!(wait_for(&events, h, "close").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn synchronous_calls_cannot_wait_for_an_answer() {
    let dir = data_dir("sync");
    let (notify, _events) = collector();
    let mut hub = ChildHub::default();
    let p = prepare(&script("echo s", "echo s"), &dir).unwrap();
    // Not allowed yet: the question is filed (once), and the call fails.
    hub.ask_for_later("P", &p);
    hub.ask_for_later("P", &p);
    let asked = hub.requests();
    assert_eq!(asked.len(), 1);
    // Allow once lets the next call through, and only one.
    hub.answer(asked[0].id, ProgramChoice::Once, &notify);
    assert!(hub.take_once("P", &p.program));
    assert!(!hub.take_once("P", &p.program));
    // Always is kept as for any call.
    hub.ask_for_later("P", &p);
    let id = hub.requests()[0].id;
    assert_eq!(
        hub.answer(id, ProgramChoice::Always, &notify),
        Some(("P".into(), p.program))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_sync_collects_output_and_stops_a_runaway() {
    let dir = data_dir("runsync");
    let p = prepare(
        &script(
            "echo out & echo err 1>&2 & exit 2",
            "echo out; echo err >&2; exit 2",
        ),
        &dir,
    );
    // (`&` needs the shell, which a script argument does not: it is inside.)
    let p = p.unwrap();
    let r = run_sync(&p.spec, None, Duration::from_secs(20), 1 << 20);
    assert_eq!(r.error, None, "{r:?}");
    assert_eq!(r.status, Some(2));
    assert!(String::from_utf8_lossy(&r.stdout).contains("out"));
    assert!(String::from_utf8_lossy(&r.stderr).contains("err"));

    let slow = sleeper(&dir);
    let started = Instant::now();
    let r = run_sync(&slow.spec, None, Duration::from_millis(300), 1 << 20);
    assert!(
        r.error
            .as_deref()
            .is_some_and(|e| e.starts_with("ETIMEDOUT")),
        "{r:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(r.signal.as_deref(), Some("SIGTERM"));

    let bad = Prepared {
        spec: Spec {
            exe: dir.join("missing.exe"),
            ..p.spec.clone()
        },
        ..p
    };
    let r = run_sync(&bad.spec, None, Duration::from_secs(5), 10);
    assert!(r.error.unwrap().starts_with("ENOENT"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_waiting_for_the_plugin_is_counted_until_it_is_handled() {
    let backlog = Arc::new(AtomicUsize::new(0));
    backlog.fetch_add(100, Ordering::Relaxed);
    let event = ChildEvent {
        handle: 1,
        kind: "stdout",
        data: String::new(),
        credit: Some((backlog.clone(), 100)),
    };
    assert_eq!(backlog.load(Ordering::Relaxed), 100);
    drop(event);
    assert_eq!(backlog.load(Ordering::Relaxed), 0);
}
