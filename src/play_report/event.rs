//! The `playback_session` event (group `play_log`, version 2), as TIDAL's
//! own SDKs build it (docs/_reference/tidal-activity.md, 2.3 and 2.4).
//!
//! Identity is honest: the `client` block says what Tidalfast is, a desktop
//! app on this computer's operating system at this build's version. It
//! never claims a phone or TIDAL's own apps. The user block carries the ids
//! read from the access token's claims, as the official SDKs do.

use base64::Engine as _;
use serde_json::{Value, json};

use super::listen::Listen;

pub const GROUP: &str = "play_log";
pub const NAME: &str = "playback_session";
pub const VERSION: u32 = 2;
pub const APP_NAME: &str = "Tidalfast";
pub const DEVICE_TYPE: &str = "desktop";

/// The claims of a TIDAL access token the events need.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Claims {
    /// `uid`: the user.
    pub uid: Option<u64>,
    /// `cid`: the OAuth client's numeric id.
    pub cid: Option<u64>,
    /// `sid`: the session.
    pub sid: Option<String>,
}

fn claim_number(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// Reads the claims from the token's payload; a token that is not a JWT
/// gives none.
pub fn claims(token: &str) -> Claims {
    let payload = token
        .split('.')
        .nth(1)
        .and_then(|part| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(part.trim_end_matches('='))
                .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    Claims {
        uid: claim_number(payload.get("uid")),
        cid: claim_number(payload.get("cid")),
        sid: match payload.get("sid") {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Number(number)) => Some(number.to_string()),
            _ => None,
        },
    }
}

/// `sourceType` and `sourceId` for what a track was played from. Both are
/// strings, empty when unknown: `null` makes TIDAL drop the event.
pub fn source(context_uri: Option<&str>) -> (&'static str, String) {
    let Some(uri) = context_uri else {
        return ("", String::new());
    };
    if let Some(seed) = crate::util::station_seed(uri) {
        // A radio station plays the seed's mix.
        let id = crate::util::uri_id(&seed).unwrap_or_default().to_string();
        return match crate::util::uri_kind(&seed) {
            Some("artist") => ("ARTIST", id),
            Some("album") => ("ALBUM", id),
            Some("playlist") => ("PLAYLIST", id),
            Some("track") => ("TRACK", id),
            _ => ("", String::new()),
        };
    }
    let id = crate::util::uri_id(uri).unwrap_or_default().to_string();
    match crate::util::uri_kind(uri) {
        Some("album") => ("ALBUM", id),
        Some("playlist") => ("PLAYLIST", id),
        Some("mix") => ("MIX", id),
        Some("artist") => ("ARTIST", id),
        Some("track") => ("TRACK", id),
        _ => ("", String::new()),
    }
}

fn seconds(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

/// The event's `payload`.
pub fn payload(listen: &Listen) -> Value {
    let track_id = crate::util::uri_id(&listen.track.uri)
        .unwrap_or_default()
        .to_string();
    let (source_type, source_id) = source(listen.context_uri.as_deref());
    let actions: Vec<Value> = listen
        .actions
        .iter()
        .map(|action| {
            json!({
                "actionType": action.kind,
                "assetPosition": seconds(action.position_ms),
                "timestamp": action.timestamp_ms,
            })
        })
        .collect();
    json!({
        "playbackSessionId": listen.session_id,
        "productType": "TRACK",
        "requestedProductId": track_id,
        "actualProductId": track_id,
        "actualAssetPresentation": "FULL",
        "actualAudioMode": "STEREO",
        "actualQuality": listen.quality.unwrap_or_default().api_name(),
        "sourceType": source_type,
        "sourceId": source_id,
        "isPostPaywall": true,
        "startTimestamp": listen.start_ms(),
        "startAssetPosition": seconds(listen.start_position_ms()),
        "endTimestamp": listen.end_ms(),
        "endAssetPosition": seconds(listen.end_position_ms()),
        "actions": actions,
    })
}

/// The event as queued: everything but the identity, which is read from
/// the token when it is sent.
pub fn queued_body(listen: &Listen, uuid: &str, ts_ms: i64) -> Value {
    json!({
        "group": GROUP,
        "version": VERSION,
        "ts": ts_ms,
        "uuid": uuid,
        "payload": payload(listen),
        "extras": Value::Null,
    })
}

/// The operating system as the `client.platform` field and the `os-name`
/// header name it.
pub fn platform() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("windows", "Windows")
    } else if cfg!(target_os = "macos") {
        ("macos", "macOS")
    } else {
        ("linux", "Linux")
    }
}

/// The `MessageBody`: the queued event plus the user and client blocks.
pub fn message_body(queued: &Value, claims: &Claims) -> String {
    let mut body = queued.clone();
    if let Some(fields) = body.as_object_mut() {
        fields.insert(
            "user".into(),
            json!({
                "id": claims.uid,
                "clientId": claims.cid,
                "sessionId": claims.sid.clone().unwrap_or_default(),
            }),
        );
        fields.insert(
            "client".into(),
            json!({
                "token": claims.cid.map(|cid| cid.to_string()).unwrap_or_default(),
                "deviceType": DEVICE_TYPE,
                "version": env!("CARGO_PKG_VERSION"),
                "platform": platform().0,
            }),
        );
    }
    body.to_string()
}

/// The `Headers` message attribute. Holds the raw token, as TIDAL's SDKs
/// send it: build it only for the request and never log it.
pub fn headers_attribute(token: &str, client_id: &str, now_ms: i64) -> String {
    json!({
        "app-name": APP_NAME,
        "app-version": env!("CARGO_PKG_VERSION"),
        "os-name": platform().1,
        "client-id": client_id,
        "consent-category": "NECESSARY",
        "requested-sent-timestamp": now_ms.to_string(),
        "authorization": token,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play_report::listen::{Action, ActionType};
    use crate::tidal_engine::{LocalTrack, Quality};

    fn jwt(payload: &str) -> String {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.sig",
            engine.encode(r#"{"alg":"none"}"#),
            engine.encode(payload)
        )
    }

    #[test]
    fn claims_come_from_the_token_payload() {
        let token = jwt(r#"{"uid":12345678,"cid":"4321","sid":"abc-def","type":"o2_access"}"#);
        assert_eq!(
            claims(&token),
            Claims {
                uid: Some(12345678),
                cid: Some(4321),
                sid: Some("abc-def".into()),
            }
        );
        assert_eq!(claims("not a token"), Claims::default());
    }

    #[test]
    fn sources_are_strings_never_null() {
        assert_eq!(source(Some("tidal:album:9")), ("ALBUM", "9".into()));
        assert_eq!(
            source(Some("tidal:playlist:ab-cd")),
            ("PLAYLIST", "ab-cd".into())
        );
        assert_eq!(source(Some("tidal:mix:0abc")), ("MIX", "0abc".into()));
        assert_eq!(
            source(Some("tidal:station:artist:7")),
            ("ARTIST", "7".into())
        );
        assert_eq!(source(Some("tidal:collection")), ("", String::new()));
        assert_eq!(source(None), ("", String::new()));
    }

    #[test]
    fn the_event_matches_the_v2_schema_and_says_desktop() {
        let listen = Listen {
            session_id: "s-1".into(),
            track: LocalTrack {
                uri: "tidal:track:77".into(),
                duration_ms: 200_000,
                ..LocalTrack::default()
            },
            context_uri: Some("tidal:album:9".into()),
            quality: Some(Quality::HiResLossless),
            actions: vec![
                Action {
                    kind: ActionType::Start,
                    position_ms: 1_500,
                    timestamp_ms: 1_000,
                },
                Action {
                    kind: ActionType::Stop,
                    position_ms: 61_500,
                    timestamp_ms: 61_000,
                },
            ],
            listened_ms: 60_000,
            completed: false,
        };
        let queued = queued_body(&listen, "u-1", 62_000);
        let claims = Claims {
            uid: Some(5),
            cid: Some(6),
            sid: None,
        };
        let body: Value = serde_json::from_str(&message_body(&queued, &claims)).unwrap();
        assert_eq!(body["group"], "play_log");
        assert_eq!(body["version"], 2);
        assert_eq!(body["uuid"], "u-1");
        assert_eq!(body["user"]["id"], 5);
        assert_eq!(body["user"]["sessionId"], "");
        assert_eq!(body["client"]["token"], "6");
        assert_eq!(body["client"]["deviceType"], "desktop");
        let payload = &body["payload"];
        assert_eq!(payload["actualProductId"], "77");
        assert_eq!(payload["actualQuality"], "HI_RES_LOSSLESS");
        assert_eq!(payload["sourceType"], "ALBUM");
        assert_eq!(payload["sourceId"], "9");
        assert_eq!(payload["startAssetPosition"], 1.5);
        assert_eq!(payload["endAssetPosition"], 61.5);
        assert_eq!(payload["startTimestamp"], 1_000);
        assert_eq!(payload["endTimestamp"], 61_000);
        assert_eq!(payload["actions"][0]["actionType"], "PLAYBACK_START");
        assert_eq!(payload["actions"][1]["actionType"], "PLAYBACK_STOP");
        let headers: Value = serde_json::from_str(&headers_attribute("tok", "client", 9)).unwrap();
        assert_eq!(headers["consent-category"], "NECESSARY");
        assert_eq!(headers["authorization"], "tok");
        assert_eq!(headers["requested-sent-timestamp"], "9");
    }
}
