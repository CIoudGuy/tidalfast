//! What a plugin reads as the signed-in account, through the real app host
//! and client against a made-up TIDAL on this computer: a stream's format
//! for any plugin, its manifest only behind the save-audio permission.
//! Nothing here touches TIDAL.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{Value, json};

use super::*;
use crate::api::{NetActivity, TidalClient};
use crate::auth::tidal::TidalTokens;
use crate::auth::tidal::tests::{FakeRefresher, grant};
use crate::backend::TidalAccess;
use crate::plugins::luna_host::AppHost;

/// In the made-up manifest's address, the one thing a plugin must not read.
const ADDRESS: &str = "made-up-stream-address.flac";

/// What the made-up TIDAL answers for a song's or video's playback.
fn playback_answer(base: &str) -> String {
    let manifest = json!({
        "mimeType": "audio/flac",
        "codecs": "flac",
        "encryptionType": "NONE",
        "urls": [format!("{base}/{ADDRESS}")],
    });
    json!({
        "trackId": 7,
        "assetPresentation": "FULL",
        "audioMode": "STEREO",
        "audioQuality": "LOSSLESS",
        "manifestMimeType": "application/vnd.tidal.bts",
        "manifestHash": "made-up-hash",
        "manifest": base64::engine::general_purpose::STANDARD.encode(manifest.to_string()),
        "licenseSecurityToken": "made-up-license-token",
        "urls": [format!("{base}/{ADDRESS}")],
        "url": format!("{base}/{ADDRESS}"),
        "codec": "FLAC",
        "bitDepth": 16,
        "sampleRate": 44100,
        "albumReplayGain": -7.5,
    })
    .to_string()
}

/// A made-up TIDAL: its address and the requests it was sent, as
/// `<path and query> <sign-in>` lines.
fn made_up_tidal() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (address, record) = (base.clone(), seen.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (address, record) = (address.clone(), record.clone());
            std::thread::spawn(move || {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                }
                let head = String::from_utf8_lossy(&head);
                let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let sign_in = head
                    .lines()
                    .find_map(|l| {
                        let l = l.to_ascii_lowercase();
                        l.strip_prefix("authorization: ").map(str::to_string)
                    })
                    .unwrap_or_default();
                record.lock().unwrap().push(format!("{target} {sign_in}"));
                let path = target.split('?').next().unwrap_or("").to_ascii_lowercase();
                let decoded = percent_encoding::percent_decode_str(&path)
                    .decode_utf8_lossy()
                    .into_owned();
                let (status, body) = if decoded.contains(ADDRESS) {
                    ("200 OK", "made-up audio".to_string())
                } else if decoded.contains("/tracks/")
                    && (decoded.contains("playback")
                        || decoded.contains("url")
                        || decoded.contains("manifest"))
                    || decoded.contains("/videos/")
                {
                    ("200 OK", playback_answer(&address))
                } else if decoded == "/home/feed/static" {
                    ("200 OK", json!({ "page": { "modules": [] } }).to_string())
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            });
        }
    });
    (base, seen)
}

/// The app's real host, signed in to the made-up TIDAL at `base`.
fn app_host(base: &str) -> (Arc<AppHost>, tokio::runtime::Runtime) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let client = TidalClient::with_bases(http, Arc::new(NetActivity::default()), base, base, base);
    let refresher = Arc::new(FakeRefresher {
        calls: AtomicUsize::new(0),
        fail: false,
    });
    let far = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    client.set_tokens(Some(TidalTokens::new(
        grant("app-session", far),
        refresher,
        None,
    )));
    let gate = crate::runtime_gate::RuntimeGate::new(runtime.handle().clone());
    let access = TidalAccess::for_tests(client, gate);
    (Arc::new(AppHost::new(Some(access), None)), runtime)
}

/// Installs plugin `name`, `body` after the shared header.
fn start(name: &str, body: &str, host: Arc<AppHost>) -> (Luna, PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Compat.json".into(),
        json!({ "name": name, "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Compat.mjs".into(), format!("{HEADER}{body}").into_bytes());
    let base = serve(files);
    let dir = temp_dir("account-read");
    let luna = Luna::start(config(&dir), host).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    (luna, dir)
}

fn question(luna: &Luna) -> PermissionRequest {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = luna.pending_permission() {
            return r;
        }
        assert!(Instant::now() < end, "no question was asked");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Every read of a playback answer a plugin might spell: songs and videos,
/// the legacy addresses, any host of the API, and spellings a server reads
/// as the same path.
const STREAM_READS: &str = r#"
const reads = [
	"https://desktop.tidal.com/v1/tracks/7/playbackinfo?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=FULL",
	"https://api.tidal.com/v1/tracks/7/playbackinfopostpaywall?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=FULL",
	"https://listen.tidal.com/v1/tracks/7/urlpostpaywall?urlusagemode=STREAM&audioquality=LOSSLESS&assetpresentation=FULL",
	"https://api.tidal.com/v1/tracks/7/streamurl?soundQuality=LOSSLESS",
	"https://api.tidal.com/v1/tracks/7/offlineurl?soundQuality=LOSSLESS",
	"https://api.tidal.com/v1/videos/7/playbackinfo?videoquality=HIGH&playbackmode=STREAM&assetpresentation=FULL",
	"https://api.tidal.com/v1/videos/7/playbackinfopostpaywall?videoquality=HIGH&playbackmode=STREAM&assetpresentation=FULL",
	"https://api.tidal.com/v1/videos/7/urlpostpaywall?urlusagemode=STREAM&videoquality=HIGH&assetpresentation=FULL",
	"https://api.tidal.com/v1/tracks/7/PlaybackInfo?audioquality=LOSSLESS",
	"https://api.tidal.com/v1/tracks/7/playback%69nfo?audioquality=LOSSLESS",
	"https://api.tidal.com/v1/tracks//7/playbackinfo/?audioquality=LOSSLESS",
];
const auth = await lib.TidalApi.getAuthHeaders();
const readAll = () => Promise.all(reads.map((url) => fetch(url, { headers: auth }).then(async (r) => ({ url, status: r.status, text: await r.text() }), (e) => ({ url, status: 0, text: String(e?.message ?? e) }))));
"#;

/// The hole found in plugin round 28: the placeholder sign-in reached a
/// song's manifest through the app, with no permission. Any plugin keeps
/// the format.
#[test]
fn a_plugin_without_the_save_audio_permission_reads_the_format_not_the_stream() {
    let (base, seen) = made_up_tidal();
    let (host, _runtime) = app_host(&base);
    let body = format!(
        "{STREAM_READS}\nR.reads = await readAll();\nR.info = await lib.TidalApi.playbackInfo(7, \"LOSSLESS\");\nR.done = true;\n"
    );
    let (luna, dir) = start("AnyPlugin", &body, host);
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(true));
    assert!(luna.pending_permission().is_none(), "nothing is asked");
    let results = results(&dir);
    let reads = results["reads"].as_array().unwrap();
    assert_eq!(reads.len(), 11);
    let seen = seen.lock().unwrap().clone();
    let calls: Vec<&String> = seen.iter().filter(|l| !l.contains(ADDRESS)).collect();
    assert!(calls.len() >= 11, "each went to TIDAL: {seen:?}");
    assert!(
        calls.iter().all(|l| l.ends_with("bearer app-session")),
        "with the app's session: {seen:?}"
    );
    // TidalApi.playbackInfo keeps to the same fields.
    let info = &results["info"];
    assert_eq!(info["audioQuality"], "LOSSLESS", "{info}");
    assert_eq!(info["stream"]["codec"], "flac", "{info}");
    for key in ["manifest", "manifestHash", "licenseSecurityToken", "urls"] {
        assert!(info.get(key).is_none(), "{key} in {info}");
    }
    for read in reads {
        let url = &read["url"];
        assert_eq!(read["status"], 200, "{read}");
        let text = read["text"].as_str().unwrap();
        assert!(
            !text.contains(ADDRESS) && !text.contains("made-up-license-token"),
            "{url}: {text}"
        );
        let answer: Value = serde_json::from_str(text).unwrap();
        for key in ["manifest", "manifestHash", "urls", "url"] {
            assert!(answer.get(key).is_none(), "{url}: {key} in {text}");
        }
        // What a plugin reads for a song's quality.
        assert_eq!(answer["audioQuality"], "LOSSLESS", "{url}");
        assert_eq!(answer["bitDepth"], 16, "{url}");
        assert_eq!(answer["sampleRate"], 44100, "{url}");
        assert_eq!(answer["codec"], "FLAC", "{url}");
        assert_eq!(answer["stream"]["codec"], "flac", "{url}");
        assert_eq!(
            answer["manifestMimeType"], "application/vnd.tidal.bts",
            "{url}"
        );
    }
}

/// With the permission the read is what it always was.
#[test]
fn a_plugin_allowed_to_save_audio_reads_the_manifest_as_before() {
    let (base, _seen) = made_up_tidal();
    let (host, _runtime) = app_host(&base);
    let body = format!(
        r#"{STREAM_READS}
const n = await native("Compat/audio.native.ts", `"use strict";
const {{ fetchMediaItemStream }} = require("@luna/lib.native");
exports.read = async () => {{
	const s = await fetchMediaItemStream({{ trackId: 7, audioQuality: "LOSSLESS" }});
	return await new Promise((res, rej) => {{ let n = 0; s.on("data", (c) => (n += c.length)); s.on("end", () => res(n)); s.on("error", rej); }});
}};`);
R.before = (await readAll())[0].text;
R.stream = await n("read").catch((e) => "ERR " + (e?.message ?? e));
R.after = (await readAll())[0].text;
R.done = true;
"#
    );
    let (luna, dir) = start("Saver", &body, host);
    let q = question(&luna);
    assert_eq!(q.access, Access::SaveAudio);
    luna.answer_permission(q.id, ProgramChoice::Once);
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(true));
    let results = results(&dir);
    let before: Value = serde_json::from_str(results["before"].as_str().unwrap()).unwrap();
    assert!(before.get("manifest").is_none(), "{results}");
    let after: Value = serde_json::from_str(results["after"].as_str().unwrap()).unwrap();
    assert!(after["manifest"].is_string(), "{results}");
    assert_eq!(after["manifestHash"], "made-up-hash", "{results}");
    assert_eq!(after["audioQuality"], "LOSSLESS", "{results}");
}

/// Home's feed opens with the permission for TIDAL's official API, for any
/// plugin: the same question, nothing sent before the answer, and the app's
/// session goes out while the plugin keeps its placeholder.
#[test]
fn a_plugin_reads_homes_feed_only_with_the_official_api_permission() {
    let (base, seen) = made_up_tidal();
    let (host, _runtime) = app_host(&base);
    let body = r#"
R.n = (R.n ?? 0) + 1;
const run = R.n;
const auth = await lib.TidalApi.getAuthHeaders();
R["feed" + run] = await fetch("https://api.tidal.com/v2/home/feed/static?deviceType=BROWSER&platform=WEB&locale=en_US", { headers: auth })
	.then(async (r) => r.status + " " + (await r.text()), (e) => "ERR " + (e?.message ?? e));
R.done = run;
"#;
    let (luna, dir) = start("AnyPlugin", body, host);
    let q = question(&luna);
    assert_eq!(
        (q.plugin.as_str(), &q.access),
        ("AnyPlugin", &Access::OpenApi)
    );
    luna.answer_permission(q.id, ProgramChoice::Deny);
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(1));
    let first = results(&dir);
    assert!(
        first["feed1"].as_str().unwrap().contains("did not allow"),
        "{first}"
    );
    assert!(seen.lock().unwrap().is_empty(), "denied: nothing was sent");
    luna.reload("AnyPlugin");
    let q = question(&luna);
    assert_eq!(q.access, Access::OpenApi);
    luna.answer_permission(q.id, ProgramChoice::Once);
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(2));
    let second = results(&dir);
    assert_eq!(
        second["feed2"], r#"200 {"page":{"modules":[]}}"#,
        "{second}"
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].starts_with("/home/feed/static?"), "{seen:?}");
    assert!(seen[0].contains("deviceType=BROWSER"), "{seen:?}");
    assert!(seen[0].ends_with("bearer app-session"), "{seen:?}");
}
