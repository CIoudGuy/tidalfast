//! Demo builds only: `TIDALFAST_API_PROBE=<file>` sends the read-only TIDAL
//! requests listed in `<file>` with the signed-in session and writes TIDAL's
//! answers to `<file>.out.json`, for checking response shapes live.
//!
//! One request per line: `v1 <path>[?query]` or `v2 <path>[?query]`, e.g.
//! `v1 artists/17275/toptracks?limit=100&offset=0`; `{userId}` in a path or a
//! query value is the signed-in user's id. A v2 query pair `h:<name>=<value>`
//! is sent as a header instead (`h:x-tidal-authenticated-userid={userId}`).
//! Lines starting with `#`
//! are skipped. Only GET is ever sent; the session's tokens never leave the
//! client and are not written anywhere.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::backend::TidalAccess;

pub const ENV: &str = "TIDALFAST_API_PROBE";

pub fn start(tidal: TidalAccess) {
    let Some(file) = std::env::var_os(ENV).map(PathBuf::from) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("api-probe".into())
        .spawn(move || run(&tidal, &file));
}

fn run(tidal: &TidalAccess, file: &std::path::Path) {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut out = serde_json::Map::new();
    // Wait for the sign-in to be restored.
    for _ in 0..30 {
        if tidal
            .block_on(|c| async move { c.v1_json("genres", &[]).await })
            .is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (base, target) = line.split_once(' ').unwrap_or(("v1", line));
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let query: Vec<(String, String)> = query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let path = path.to_string();
        let v2 = base == "v2";
        let answer: Option<Value> = tidal.block_on(move |c| async move {
            let user = c.user_id().map(|id| id.to_string()).unwrap_or_default();
            let path = path.replace("{userId}", &user);
            let q: Vec<(&str, String)> = query
                .iter()
                .map(|(k, v)| (k.as_str(), v.replace("{userId}", &user)))
                .collect();
            let r = if v2 {
                c.v2_json(&path, &q).await
            } else {
                c.v1_json(&path, &q).await
            };
            Ok::<Value, crate::api::ApiError>(match r {
                Ok(v) => v,
                Err(e) => json!({ "probe_error": e.to_string() }),
            })
        });
        out.insert(
            line.to_string(),
            answer.unwrap_or_else(|| json!({ "probe_error": "no session or timed out" })),
        );
    }
    let dest = file.with_extension("out.json");
    let _ = std::fs::write(
        &dest,
        serde_json::to_vec_pretty(&Value::Object(out)).unwrap_or_default(),
    );
}
