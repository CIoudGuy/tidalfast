//! Regression tests for the Luna security review (2026-10-21): one test per
//! confirmed finding, each with the trigger that showed it, through the real
//! plugin manager where the finding is about plugin behaviour.

use std::io::Read;
use std::net::TcpListener;

use super::*;

/// A local TCP server that keeps everything it receives.
fn recording_server() -> (u16, Arc<Mutex<Vec<u8>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let got: Arc<Mutex<Vec<u8>>> = Arc::default();
    let keep = got.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let keep = keep.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    keep.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            });
        }
    });
    (port, got)
}

/// Serves plugins named in `plugins` (name, code) from one local server;
/// returns each install URL.
fn serve_plugins(plugins: &[(&str, String)]) -> Vec<String> {
    let files: Files = Arc::default();
    for (name, code) in plugins {
        let mut f = files.lock().unwrap();
        f.insert(
            format!("/{name}.json"),
            json!({ "name": name, "version": "1.0.0", "hash": "s1" })
                .to_string()
                .into_bytes(),
        );
        f.insert(format!("/{name}.mjs"), code.as_bytes().to_vec());
    }
    let base = serve(files);
    plugins
        .iter()
        .map(|(name, _)| format!("{base}/{name}.mjs"))
        .collect()
}

/// S1: a plugin cannot act as another by writing another plugin's name on
/// a socket object the runtime gave it and calling the socket's methods
/// from a promise job (where no plugin code is on the stack). Before the
/// fix the runtime trusted the object's `plugin` field there, so a plugin
/// could write into, or close, another plugin's connections (an mpv pipe,
/// a Discord IPC socket) and ask permissions in its name.
#[test]
fn a_plugin_cannot_use_another_plugins_sockets_through_a_forged_owner() {
    let (port, got) = recording_server();
    let victim = format!(
        r#"export const unloads = new Set();
const ch = await __ipcRenderer.invoke("__Luna.registerNative", "Victim/conn.native.ts", `exports.go = () => new Promise((res) => {{
	const s = require("net").createConnection({{ port: {port}, host: "127.0.0.1" }}, () => {{ s.write("victim;"); res("connected"); }});
	s.on("error", (e) => res("error " + e.message));
}});`);
await __ipcRenderer.invoke(ch, "go");
"#
    );
    let attacker = format!(
        r#"{}
const n = await native("Compat/atk.native.ts", `exports.go = () => new Promise((res) => {{
	const net = require("net");
	const forged = [];
	for (let h = 1; h <= 64; h++) {{
		const s = new net.Socket();
		try {{ s.plugin = "Victim"; }} catch {{}}
		s.handle = h;
		s.readyState = "open";
		forged.push(s);
		// No plugin frame on the stack when the job runs.
		Promise.resolve().then(s._write.bind(s, "PWNED" + h + ";", "utf8", Function.prototype));
	}}
	setTimeout(() => res(forged.map((s) => String(s.plugin)).filter((p) => p === "Victim").length), 500);
}});`);
await check("forged", () => n("go"));
R.done = true;
"#,
        super::HEADER
    );
    let urls = serve_plugins(&[("Victim", victim), ("Compat", attacker)]);
    let dir = temp_dir("s1");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&urls[0]);
    wait_until(&luna, 20, |_| {
        String::from_utf8_lossy(&got.lock().unwrap()).contains("victim;")
    });
    luna.install(&urls[1]);
    wait_until(&luna, 30, |_| super::results(&dir)["done"] == json!(true));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let seen = String::from_utf8_lossy(&got.lock().unwrap()).to_string();
    let r = super::results(&dir);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(seen.contains("victim;"), "{seen} {r}");
    assert!(
        !seen.contains("PWNED"),
        "wrote into Victim's socket: {seen} {r}"
    );
}
