//! The translation layer between TIDAL's redux world (what Luna plugins see)
//! and Tidalfast: action name -> [`HostCommand`] for `redux.actions[...]`
//! and dispatches, [`LunaEvent`] -> action for `redux.intercept(...)`, and
//! the `redux.store.getState()` slices built from a [`PlaybackSnapshot`].

use serde_json::{Value, json};

use super::host::{HostCommand, LunaEvent, NotifyLevel, PlaybackSnapshot, RepeatMode};

/// Every action type Tidalfast itself dispatches (from [`LunaEvent`]s and
/// context menus). Plugins intercepting anything else wait for an action
/// that only a host sending [`LunaEvent::Action`] can produce.
pub const EMITTED_ACTIONS: &[&str] = &[
    "playbackControls/MEDIA_PRODUCT_TRANSITION",
    "player/PRELOAD_ITEM",
    "playbackControls/SET_PLAYBACK_STATE",
    "playbackControls/SEEK",
    "playbackControls/TIME_UPDATE",
    "playbackControls/SET_VOLUME",
    "playQueue/SET_REPEAT_MODE",
    "playQueue/ENABLE_SHUFFLE_MODE",
    "playQueue/DISABLE_SHUFFLE_MODE",
    "playQueue/RESET",
    "favorites/SET_FAVORITE_IDS",
    "contextMenu/OPEN",
    "contextMenu/OPEN_MEDIA_ITEM",
    "contextMenu/OPEN_MULTI_MEDIA_ITEM",
    "contextMenu/CLOSE",
    // User actions offered to plugins before they happen (cancelable), see
    // `plugins/gate.rs`.
    "playbackControls/SKIP_NEXT",
    "playbackControls/SKIP_PREVIOUS",
    "playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION",
    "playQueue/ADD_NOW",
    "playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE",
    "playQueue/FETCH_FIRST_PAGE_AND_ADD_TO_QUEUE",
    "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
    "playQueue/MOVE_NEXT",
    "playQueue/MOVE_PREVIOUS",
    "playQueue/MOVE_TO",
    "playQueue/REMOVE_ELEMENT",
    "playQueue/CLEAR_ACTIVE_ITEMS",
    "playQueue/TOGGLE_SHUFFLE",
    "router/PUSH",
    "router/NAVIGATED",
    "page/SET_PAGE_ID",
    "search/SET_SEARCH_PHRASE",
    "search/SEARCH_COMMIT",
    "search/SEARCH_RESULT_SUCCESS",
    "content/ADD_MEDIA_ITEM_IDS_TO_FAVORITES",
    "content/REMOVE_MEDIA_ITEM_IDS_FROM_FAVORITES",
    "content/ADD_MEDIA_ITEMS_TO_PLAYLIST",
    "content/ADD_MEDIA_ITEMS_TO_PLAYLIST_SUCCESS",
    "content/LOAD_ITEM_LYRICS",
    "content/LOAD_ITEM_LYRICS_SUCCESS",
    "content/LOAD_ITEM_LYRICS_FAIL",
    "view/ENTERED_NOWPLAYING",
    "view/EXIT_NOWPLAYING",
    "view/REQUEST_FULLSCREEN",
    "view/EXIT_FULLSCREEN",
    "view/FULLSCREEN_ALLOWED",
    "locale/BUNDLE_SWITCH_SUCCESS",
];

/// Why `player/SET_DEVICE_MODE` did nothing, and the store value went back.
pub const EXCLUSIVE_UNSUPPORTED: &str = "this build of Tidalfast has no exclusive (bit-perfect) output on this platform, so the output stays shared";

/// Why a plugin's `player/SET_DEVICE_MODE("exclusive")` waits: the user is
/// asked first, and the output stays shared until they allow it.
pub const EXCLUSIVE_ASKED: &str = "exclusive (bit-perfect) output is switched on for a plugin only after you allow it; you were asked, and the output stays shared until then";

/// Why a plugin's `player/SET_DEVICE_MODE("exclusive")` did nothing after
/// the user said no.
pub const EXCLUSIVE_DENIED: &str = "you did not allow this plugin to switch the output to exclusive (bit-perfect) mode, so it stays shared";

/// What a plugin-dispatched action means for Tidalfast.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionOutcome {
    /// Do these, in order.
    Commands(Vec<HostCommand>),
    /// Show a toast.
    Notify(NotifyLevel, String),
    /// The payload named nothing to act on (an empty list, a queue that
    /// stays as it is).
    Ignored,
    /// An action TIDAL has that does nothing in Tidalfast, with the reason
    /// (UI-only in TIDAL, or state the host owns); the plugin's log says so.
    Bookkeeping(&'static str),
    /// Not in the table; logged.
    Unknown,
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Object(o) => {
                        o.get("id")
                            .or_else(|| o.get("itemId"))
                            .and_then(|i| match i {
                                Value::String(s) => Some(s.clone()),
                                Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Play `list` starting at `from`.
fn play_now(list: Vec<String>, from: usize) -> HostCommand {
    if from > 0 {
        HostCommand::PlayNowFrom {
            ids: list,
            index: from,
        }
    } else {
        HostCommand::PlayNow(list)
    }
}

/// `forceShuffle` / `shuffle` on an ADD action turns shuffle on.
fn shuffle_flag(payload: &Value) -> Option<HostCommand> {
    (payload["forceShuffle"].as_bool() == Some(true) || payload["shuffle"].as_bool() == Some(true))
        .then_some(HostCommand::SetShuffle(true))
}

/// Put `list` in the queue where the action's `position` says.
fn place(payload: &Value, list: Vec<String>, from: usize) -> ActionOutcome {
    use HostCommand as C;
    if list.is_empty() {
        return ActionOutcome::Ignored;
    }
    let mut cmds = match payload.get("position").and_then(Value::as_str) {
        Some("next") => vec![C::PlayNext(list)],
        Some("last") => vec![C::AddToQueue(list)],
        _ => {
            let from = from.min(list.len() - 1);
            vec![play_now(list, from)]
        }
    };
    cmds.extend(shuffle_flag(payload));
    ActionOutcome::Commands(cmds)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Translate a plugin's `{type, payload}` into what Tidalfast should do.
pub fn action_outcome(kind: &str, payload: &Value, snap: &PlaybackSnapshot) -> ActionOutcome {
    use ActionOutcome::*;
    use HostCommand as C;
    let one = |c: HostCommand| Commands(vec![c]);
    let index = |k: &str| payload.get(k).and_then(num).map(|n| n.max(0.0) as usize);
    match kind {
        "playbackControls/PLAY" => one(C::Play),
        "playbackControls/PAUSE" | "playbackControls/SET_DESIRED_PAUSE_STATE" => one(C::Pause),
        "playbackControls/TOGGLE_PLAYBACK" => one(C::TogglePlayback),
        "playbackControls/STOP" => one(C::Stop),
        "playbackControls/SEEK" => match num(payload) {
            Some(t) => one(C::Seek(t.max(0.0))),
            None => Unknown,
        },
        "playbackControls/SEEK_FORWARDS" => {
            one(C::Seek(snap.position + num(payload).unwrap_or(10.0)))
        }
        "playbackControls/SEEK_BACKWARDS" => one(C::Seek(
            (snap.position - num(payload).unwrap_or(10.0)).max(0.0),
        )),
        "playbackControls/SKIP_NEXT" | "playQueue/MOVE_NEXT" => one(C::Next),
        "playbackControls/SKIP_PREVIOUS" | "playQueue/MOVE_PREVIOUS" => one(C::Previous),
        "playbackControls/SET_VOLUME" => {
            match payload.get("volume").and_then(num).or_else(|| num(payload)) {
                Some(v) => one(C::SetVolume(v.clamp(0.0, 100.0))),
                None => Unknown,
            }
        }
        "playbackControls/INCREASE_VOLUME" => one(C::SetVolume(
            (snap.volume + num(payload).unwrap_or(10.0)).clamp(0.0, 100.0),
        )),
        "playbackControls/DECREASE_VOLUME" => one(C::SetVolume(
            (snap.volume - num(payload).unwrap_or(10.0)).clamp(0.0, 100.0),
        )),
        "playbackControls/SET_MUTE" => one(C::SetMuted(payload.as_bool().unwrap_or(true))),
        "playbackControls/TOGGLE_MUTE" => one(C::SetMuted(!snap.muted)),
        "playQueue/ENABLE_SHUFFLE_MODE" | "playQueue/ENABLE_SHUFFLE_MODE_AND_SHUFFLE_ITEMS" => {
            one(C::SetShuffle(true))
        }
        "playQueue/DISABLE_SHUFFLE_MODE" | "playQueue/DISABLE_SHUFFLE_MODE_AND_UNSHUFFLE_ITEMS" => {
            one(C::SetShuffle(false))
        }
        "playQueue/TOGGLE_SHUFFLE" => one(C::SetShuffle(!snap.shuffle)),
        "playQueue/SET_REPEAT_MODE" => one(C::SetRepeat(RepeatMode::from_tidal(
            num(payload).unwrap_or(0.0) as i64,
        ))),
        "playQueue/TOGGLE_REPEAT_MODE" => one(C::SetRepeat(match snap.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        })),
        "playQueue/MOVE_TO" | "playQueue/SET_CURRENT_INDEX" => {
            match num(payload).or_else(|| payload.get("index").and_then(num)) {
                Some(i) => one(C::MoveTo(i.max(0.0) as usize)),
                None => Unknown,
            }
        }
        "playQueue/ADD_NOW" => {
            let list = ids(&payload["mediaItemIds"]);
            let from = index("fromIndex")
                .unwrap_or(0)
                .min(list.len().saturating_sub(1));
            let mut cmds = vec![play_now(list, from)];
            cmds.extend(shuffle_flag(payload));
            Commands(cmds)
        }
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE" => place(
            payload,
            ids(&payload["items"]),
            index("fromIndex").unwrap_or(0),
        ),
        "playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE"
        | "playQueue/FETCH_FIRST_PAGE_AND_ADD_TO_QUEUE"
        | "playQueue/FETCH_REST_OF_THE_TRACKS_AND_ADD_TO_QUEUE"
        | "playQueue/ADD_NOW_REST_OF_TRACK_LIST" => {
            let Some(list) = payload
                .get("trackListName")
                .and_then(Value::as_str)
                .and_then(|n| snap.track_lists.get(n))
            else {
                return Unknown;
            };
            let rest = matches!(
                kind,
                "playQueue/FETCH_REST_OF_THE_TRACKS_AND_ADD_TO_QUEUE"
                    | "playQueue/ADD_NOW_REST_OF_TRACK_LIST"
            );
            if rest {
                // What the first page already queued stays where it is.
                let queued: std::collections::HashSet<&str> =
                    snap.queue.iter().map(|e| e.id.as_str()).collect();
                let more: Vec<String> = list
                    .iter()
                    .filter(|id| !queued.contains(id.as_str()))
                    .cloned()
                    .collect();
                return if more.is_empty() {
                    Ignored
                } else {
                    one(C::AddToQueue(more))
                };
            }
            place(payload, list.clone(), index("fromIndex").unwrap_or(0))
        }
        "playQueue/ADD_NEXT" => one(C::PlayNext(ids(&payload["mediaItemIds"]))),
        "playQueue/ADD_LAST" | "playQueue/ADD_MEDIA_ITEMS_TO_QUEUE" => {
            one(C::AddToQueue(ids(&payload["mediaItemIds"])))
        }
        "playQueue/ADD_AT_INDEX" => one(C::AddAt {
            ids: ids(&payload["mediaItemIds"]),
            index: index("index").unwrap_or(0),
        }),
        "playQueue/REMOVE_AT_INDEX" => match index("index") {
            Some(i) => one(C::RemoveAt(i)),
            None => Unknown,
        },
        "playQueue/REMOVE_ELEMENT" => {
            let uid = payload
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or_default();
            // The app finds the row by uid when it applies this, so several
            // removals in one tick cannot shift each other's indexes.
            if uid.is_empty() {
                Ignored
            } else {
                one(C::RemoveUid(uid.to_string()))
            }
        }
        "playQueue/MOVE_TRACK" => match (index("fromIndex"), index("toIndex")) {
            (Some(from), Some(to)) => one(C::MoveTrack { from, to }),
            _ => Unknown,
        },
        "playQueue/CLEAR_QUEUE" | "playQueue/CLEAR_ACTIVE_ITEMS" => one(C::ClearQueue),
        "search/SEARCH_COMMIT" => {
            let phrase = payload
                .as_str()
                .or_else(|| payload.get("searchPhrase").and_then(Value::as_str))
                .or_else(|| payload.get("query").and_then(Value::as_str))
                .map(str::trim)
                .unwrap_or_default();
            if phrase.is_empty() {
                Ignored
            } else {
                one(C::Search(phrase.to_string()))
            }
        }
        // TIDAL's answers a plugin edited the titles of: what the reducer
        // stores is the (mutated) payload, so the songs show those titles.
        "search/SEARCH_RESULT_SUCCESS"
        | "content/LOAD_SINGLE_MEDIA_ITEM_SUCCESS"
        | "content/LOAD_ALL_ALBUM_MEDIA_ITEMS_SUCCESS"
        | "content/LOAD_ALL_ALBUM_MEDIA_ITEMS_WITH_CREDITS_SUCCESS"
        | "content/LOAD_PLAYLIST_SUGGESTED_MEDIA_ITEMS_SUCCESS"
        | "content/LOAD_SUGGESTIONS_SUCCESS" => {
            let titles = payload_titles(payload);
            if titles.is_empty() {
                Ignored
            } else {
                one(C::RetitleTracks(titles))
            }
        }
        "router/PUSH" | "router/REPLACE" => {
            let path = payload
                .as_str()
                .or_else(|| payload.get("pathname").and_then(Value::as_str))
                .or_else(|| payload.get("path").and_then(Value::as_str));
            // `Page.open()` of a plugin's page pushes TIDAL's not-found
            // route with the page's name as the search.
            let search = payload
                .get("search")
                .and_then(Value::as_str)
                .map(|s| s.trim_start_matches('?'))
                .filter(|s| !s.is_empty());
            match (path, search) {
                (Some("/not-found"), Some(name)) => one(C::Navigate(format!("/not-found?{name}"))),
                (Some(p), _) if p.starts_with('/') => one(C::Navigate(p.to_string())),
                _ => Ignored,
            }
        }
        // `player.activeDeviceMode`: what @vmohammad/quickExclusiveMode's
        // button switches. In TIDAL that is the exclusive output; here it is
        // the Bit-perfect setting, where the platform has one.
        "player/SET_DEVICE_MODE" => {
            let mode = payload.as_str().or_else(|| payload["mode"].as_str());
            match (mode, snap.exclusive_mode) {
                (Some("exclusive" | "shared"), None) => Bookkeeping(EXCLUSIVE_UNSUPPORTED),
                (Some("exclusive"), Some(_)) => one(C::SetExclusiveMode(true)),
                (Some("shared"), Some(_)) => one(C::SetExclusiveMode(false)),
                _ => Ignored,
            }
        }
        "view/REQUEST_FULLSCREEN" => one(C::SetFullscreen(true)),
        "view/EXIT_FULLSCREEN" => one(C::SetFullscreen(false)),
        "playQueue/RESET" => {
            let elements: Vec<String> = payload["elements"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| ids(&json!([e["mediaItemId"]])).pop())
                        .collect()
                })
                .unwrap_or_default();
            let current: Vec<String> = snap.queue.iter().map(|e| e.id.clone()).collect();
            let mut cmds = Vec::new();
            let playing = snap
                .current_index
                .and_then(|i| snap.queue.get(i))
                .map(|e| e.id.as_str());
            let at = index("currentIndex");
            if elements != current {
                if let (Some(playing), Some(at)) = (playing, at)
                    && elements.get(at).map(String::as_str) == Some(playing)
                {
                    // The playing track stays: change what comes after it
                    // (a better version swapped in for the next one) without
                    // restarting anything.
                    let old_up = snap.current_index.map_or(0, |i| i + 1);
                    let mut left: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for id in &elements[at + 1..] {
                        *left.entry(id.as_str()).or_default() += 1;
                    }
                    for e in snap.queue.iter().skip(old_up) {
                        match left.get_mut(e.id.as_str()) {
                            Some(n) if *n > 0 => *n -= 1,
                            _ => cmds.push(C::RemoveUid(e.uid.clone())),
                        }
                    }
                    let mut have: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for e in snap.queue.iter().skip(old_up) {
                        *have.entry(e.id.as_str()).or_default() += 1;
                    }
                    let mut added = Vec::new();
                    for id in &elements[at + 1..] {
                        match have.get_mut(id.as_str()) {
                            Some(n) if *n > 0 => *n -= 1,
                            _ => added.push(id.clone()),
                        }
                    }
                    if !added.is_empty() {
                        cmds.push(C::PlayNext(added));
                    }
                    return if cmds.is_empty() {
                        Ignored
                    } else {
                        Commands(cmds)
                    };
                }
                cmds.push(C::PlayNow(elements));
            }
            if let Some(i) = at
                && Some(i) != snap.current_index
            {
                cmds.push(C::MoveTo(i));
            }
            if let Some(r) = payload.get("repeatMode").and_then(num) {
                let r = RepeatMode::from_tidal(r as i64);
                if r != snap.repeat {
                    cmds.push(C::SetRepeat(r));
                }
            }
            if let Some(s) = payload.get("shuffleModeEnabled").and_then(Value::as_bool)
                && s != snap.shuffle
            {
                cmds.push(C::SetShuffle(s));
            }
            if cmds.is_empty() {
                Ignored
            } else {
                Commands(cmds)
            }
        }
        "content/ADD_MEDIA_ITEM_IDS_TO_FAVORITES" => one(C::AddFavorites {
            kind: "track".into(),
            ids: ids(&payload["mediaItemIds"]),
        }),
        "content/REMOVE_MEDIA_ITEM_IDS_FROM_FAVORITES" => one(C::RemoveFavorites {
            kind: "track".into(),
            ids: ids(&payload["mediaItemIds"]),
        }),
        "content/TOGGLE_FAVORITE_ITEMS" => {
            let items = payload["items"].as_array().cloned().unwrap_or_default();
            let mut add = Vec::new();
            let mut remove = Vec::new();
            for it in &items {
                let id = ids(&json!([it]))
                    .pop()
                    .or_else(|| ids(&json!([it["item"]])).pop());
                let Some(id) = id else { continue };
                if snap.favorite_tracks.contains(&id)
                    && payload["forceFavorite"].as_bool() != Some(true)
                {
                    remove.push(id);
                } else {
                    add.push(id);
                }
            }
            let mut cmds = Vec::new();
            if !add.is_empty() {
                cmds.push(C::AddFavorites {
                    kind: "track".into(),
                    ids: add,
                });
            }
            if !remove.is_empty() {
                cmds.push(C::RemoveFavorites {
                    kind: "track".into(),
                    ids: remove,
                });
            }
            if cmds.is_empty() {
                Ignored
            } else {
                Commands(cmds)
            }
        }
        "content/ADD_MEDIA_ITEMS_TO_PLAYLIST" => {
            match payload.get("playlistUUID").and_then(Value::as_str) {
                Some(p) => one(C::AddToPlaylist {
                    playlist: p.to_string(),
                    ids: ids(&payload["mediaItemIdsToAdd"]),
                }),
                None => Unknown,
            }
        }
        "message/MESSAGE_INFO" | "message/MESSAGE_WARN" | "message/MESSAGE_ERROR" => {
            let level = match kind {
                "message/MESSAGE_WARN" => NotifyLevel::Warn,
                "message/MESSAGE_ERROR" => NotifyLevel::Error,
                _ => NotifyLevel::Info,
            };
            let msg = payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| payload.to_string());
            Notify(level, msg)
        }
        // Fullscreen, minimize and the rest of the window's own actions.
        "view/ENTER_NATIVE_FULLSCREEN" => one(C::SetFullscreen(true)),
        "view/LEAVE_NATIVE_FULLSCREEN" => one(C::SetFullscreen(false)),
        "view/MINIMIZE" => one(C::Minimize),
        // Play a song by id.
        "content/FETCH_AND_PLAY_MEDIA_ITEM" => {
            let list = ids(&json!([payload["mediaItemId"]]));
            let list = if list.is_empty() {
                ids(&json!([payload["itemId"]]))
            } else {
                list
            };
            if list.is_empty() {
                Unknown
            } else {
                one(C::PlayNow(list))
            }
        }
        other => match bookkeeping(other) {
            Some(why) => Bookkeeping(why),
            None => Unknown,
        },
    }
}

/// Every action type TIDAL's desktop app has (`action-types.txt`).
pub fn is_tidal_action(kind: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    static TYPES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TYPES
        .get_or_init(|| include_str!("action-types.txt").lines().collect())
        .contains(kind)
}

/// Why a TIDAL action that has no effect in Tidalfast has none, for the
/// plugin's log: `None` for a name TIDAL does not have. Every type in
/// `action-types.txt` either acts (see [`action_outcome`]) or has a reason
/// here, so a dispatch is never silently dropped.
pub fn bookkeeping(kind: &str) -> Option<&'static str> {
    let (ns, name) = kind.split_once('/').unwrap_or(("", kind));
    let known = is_tidal_action(kind)
        // Names of the former catch-all that TIDAL's list does not carry.
        || kind == "NOOP"
        || matches!(
            ns,
            "contextMenu"
                | "message"
                | "page"
                | "router"
                | "lastFm"
                | "featureFlags"
                | "activePlayer"
                | "modal"
                | "search"
                | "sortOrders"
                | "settings"
                | "player"
                | "view"
                | "locale"
        );
    if !known {
        return None;
    }
    // A reply to a request, or TIDAL's own loading of content.
    if name.ends_with("_SUCCESS")
        || name.ends_with("_FAIL")
        || name.ends_with("_MODIFIED")
        || name.starts_with("LOAD_")
        || name.starts_with("LAZY_LOAD_")
        || name.starts_with("RECEIVED_")
    {
        return Some(
            "TIDAL's own loading of content, or its reply; Tidalfast loads content itself",
        );
    }
    Some(match ns {
        "route" => {
            "a step of TIDAL's page loading; Tidalfast draws its own pages (router/PUSH opens one)"
        }
        "tidalApi" | "tidalPublicApi" | "tidalOpenPlatformApi" | "__rtkq" | "etag" => {
            "TIDAL's web app request cache; there is none in Tidalfast"
        }
        "eventTracking" | "experimentationPlatform" | "accumulatedPlaybackTime" => {
            "analytics; Tidalfast sends none"
        }
        "remotePlayback" | "cloudQueue" | "chromeCast" | "network" => {
            "TIDAL Connect, casting and TIDAL's own sync are not part of Tidalfast"
        }
        "user" | "userProfiles" | "session" | "auth" | "launchHandler" => {
            "the signed-in account and session belong to the app; a plugin cannot change them"
        }
        "settings" => "Tidalfast's settings belong to the user; a plugin cannot change them",
        "folders" => {
            "changing playlist folders is not translated; the playlists API through fetch asks the user first"
        }
        "content" => {
            "changing playlists or TIDAL's list state from an action is not translated; the playlists API through fetch asks the user first"
        }
        "playQueue"
        | "playbackControls"
        | "playbackControlsActions"
        | "player"
        | "activePlayer"
        | "favorites" => {
            "state the host owns (queue, player, favourites); it is read from the playback state, not set by an action"
        }
        "search" => "TIDAL's search page state; search/SEARCH_COMMIT runs a search",
        "contextMenu" | "message" | "page" | "router" => {
            "the host drives menus, messages and pages itself"
        }
        _ => {
            "TIDAL web UI state (dialogs, lists, prompts, flags); Tidalfast draws its own interface"
        }
    })
}

/// (id, title) of every track a TIDAL answer carries: `tracks.items` of a
/// search, a `mediaItem` / `mediaItems` entry (`{ type, item }`), or plain
/// track objects in `items`.
fn payload_titles(payload: &Value) -> Vec<(String, String)> {
    fn track(v: &Value, out: &mut Vec<(String, String)>) {
        let v = match v.get("item") {
            Some(inner)
                if v.get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|t| t == "track") =>
            {
                inner
            }
            _ => v,
        };
        let id = match v.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return,
        };
        if let Some(title) = v.get("title").and_then(Value::as_str) {
            out.push((id, title.to_string()));
        }
    }
    let mut out = Vec::new();
    for list in [
        payload.pointer("/tracks/items"),
        payload.get("mediaItems"),
        payload.get("items"),
    ]
    .into_iter()
    .flatten()
    {
        match list {
            Value::Array(a) => a.iter().for_each(|v| track(v, &mut out)),
            Value::Object(o) => o.values().for_each(|v| track(v, &mut out)),
            _ => {}
        }
    }
    if let Some(item) = payload.get("mediaItem") {
        track(item, &mut out);
    }
    if let Some(hit) = payload.get("topHits")
        && hit.get("type").and_then(Value::as_str) == Some("TRACKS")
        && let Some(value) = hit.get("value")
    {
        track(value, &mut out);
    }
    out
}

fn id_value(s: &str) -> Value {
    s.parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::from(s))
}

fn quality_or_default(snap: &PlaybackSnapshot) -> &str {
    snap.audio_quality.as_deref().unwrap_or("LOSSLESS")
}

/// `playbackControls.playbackContext` for the current track.
pub fn playback_context(snap: &PlaybackSnapshot, id: Option<&str>) -> Value {
    let id = id.map(str::to_string).or_else(|| {
        snap.current_index
            .and_then(|i| snap.queue.get(i))
            .map(|e| e.id.clone())
    });
    json!({
        "actualProductId": id,
        "actualAudioMode": "STEREO",
        "actualAudioQuality": quality_or_default(snap),
        "actualAssetPresentation": "FULL",
        "actualDuration": snap.duration,
        "actualStreamType": null,
        "actualVideoQuality": null,
        "assetPosition": snap.position,
        "bitDepth": snap.bit_depth,
        "sampleRate": snap.sample_rate,
        "codec": snap.codec,
        "bitrate": snap.bitrate,
        "playbackSessionId": id.as_deref().map(|i| format!("tidalfast-{i}")),
    })
}

fn source_type(snap: &PlaybackSnapshot) -> String {
    snap.source
        .as_ref()
        .map(|s| s.kind.clone())
        .unwrap_or_default()
}

/// The redux store slices plugins read (`playbackControls`, `playQueue`,
/// `favorites`, `session`, `user`, and the current item in `content`).
pub fn redux_state(snap: &PlaybackSnapshot) -> Value {
    let current_id = snap
        .current_index
        .and_then(|i| snap.queue.get(i))
        .map(|e| e.id.clone());
    let elements: Vec<Value> = snap
        .queue
        .iter()
        .map(|e| json!({ "mediaItemId": e.id, "uid": e.uid, "priority": "priority_none", "context": { "type": source_type(snap), "id": snap.source.as_ref().map(|s| s.id.clone()) } }))
        .collect();
    let mut media_items = serde_json::Map::new();
    if let (Some(id), Some(item)) = (&current_id, &snap.current_item) {
        let kind = snap
            .current_index
            .and_then(|i| snap.queue.get(i))
            .map(|e| e.kind.clone())
            .unwrap_or_else(|| "track".into());
        media_items.insert(id.clone(), json!({ "item": item, "type": kind }));
    }
    for (id, item) in &snap.known_items {
        media_items
            .entry(id.clone())
            .or_insert_with(|| json!({ "item": item, "type": "track" }));
    }
    let track_lists: serde_json::Map<String, Value> = snap
        .track_lists
        .iter()
        .map(|(name, list)| {
            let items: Vec<Value> = list.iter().map(|s| id_value(s)).collect();
            (
                name.clone(),
                json!({ "items": items, "sorted": { "defaultSort": { "items": items } } }),
            )
        })
        .collect();
    let src = snap.source.clone().unwrap_or_default();
    let user_id = snap.user_id.as_deref().map(|id| {
        id.parse::<u64>()
            .map_or_else(|_| Value::from(id), Value::from)
    });
    json!({
        "playbackControls": {
            "playbackState": snap.state.as_tidal(),
            "desiredPlaybackState": snap.desired_state.as_tidal(),
            "latestCurrentTime": snap.position,
            "latestCurrentTimeSyncTimestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0),
            "volume": snap.volume,
            "volumeUnmute": 100,
            "muted": snap.muted,
            "prefilled": false,
            "startAt": 0,
            "mediaProduct": {
                "productId": current_id,
                "productType": "track",
                "referenceId": src.id,
                "sourceId": src.id,
                "sourceType": src.kind,
            },
            "playbackContext": playback_context(snap, current_id.as_deref()),
            // Not TIDAL's: whether the playing song's audio comes from a
            // plugin's `<audio>` (true) or that failed (false); media.js.
            "tidalfastReplacedAudio": snap.replaced_audio,
            "tidalfastReplaceEnded": snap.replace_ended,
            "tidalfastReplacedDuration": snap.replaced_duration,
        },
        "playQueue": {
            "elements": elements,
            "currentIndex": snap.current_index.map(|i| i as i64).unwrap_or(-1),
            // Not TIDAL's: the rows after the playing one a plugin can remove
            // (null: all), so core.js shows only removals the app will do.
            "tidalfastRemovableRows": snap.removable_rows,
            "tidalfastClearableRows": snap.clearable_rows,
            "shuffleModeEnabled": snap.shuffle,
            "repeatMode": snap.repeat.as_tidal(),
            "sourceEntityId": src.id,
            "sourceEntityType": src.kind,
            "sourceName": src.name,
            "sourceTrackListName": src.name,
            "sourceUrl": src.url,
            "backupElements": [],
            "backupSource": [],
            "originalSource": [],
            "lastShuffleSeed": 0,
            "type": "",
            "uniqueIdCounter": snap.queue.len(),
        },
        "favorites": {
            "tracks": snap.favorite_tracks.iter().map(|s| s.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(s.clone()))).collect::<Vec<_>>(),
            "albums": snap.favorite_albums.iter().map(|s| s.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(s.clone()))).collect::<Vec<_>>(),
            "artists": snap.favorite_artists.iter().map(|s| s.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(s.clone()))).collect::<Vec<_>>(),
            "playlists": snap.favorite_playlists,
            "mixes": [],
            "users": [],
            "videos": [],
        },
        // TIDAL keeps the account id as a number: plugins compare it with the
        // API's `creator.id` (multipleplaylists lists the user's own playlists).
        "session": { "countryCode": snap.country_code, "userId": user_id },
        // The account's email is not known to the app: an empty string, as for a
        // TIDAL account without one (Avatar hashes it for Gravatar).
        // `profileName` is what the plugin template greets the user with
        // (`user.meta.profileName`); an account without a public name has an
        // empty one.
        "user": { "meta": { "id": user_id, "countryCode": snap.country_code, "email": "", "profileName": snap.profile_name.clone().unwrap_or_default() } },
        "content": { "mediaItems": media_items, "trackLists": track_lists },
        "remotePlayback": { "isConnected": false },
    })
}

/// The redux action an event becomes, `(type, payload)`. `None` for events
/// that are not actions ([`LunaEvent::OpenUrl`]).
pub fn event_action(event: &LunaEvent, snap: &PlaybackSnapshot) -> Option<(String, Value)> {
    let s = |t: &str| t.to_string();
    Some(match event {
        LunaEvent::TrackChanged { id } => {
            let src = snap.source.clone().unwrap_or_default();
            (
                s("playbackControls/MEDIA_PRODUCT_TRANSITION"),
                json!({
                    "mediaProduct": { "productId": id, "productType": "track", "referenceId": src.id, "sourceId": src.id, "sourceType": src.kind },
                    "playbackContext": playback_context(snap, Some(id)),
                }),
            )
        }
        LunaEvent::Preload { id } => (
            s("player/PRELOAD_ITEM"),
            json!({ "productId": id, "productType": "track" }),
        ),
        LunaEvent::PlaybackState(st) => (
            s("playbackControls/SET_PLAYBACK_STATE"),
            json!(st.as_tidal()),
        ),
        LunaEvent::Seek(t) => (s("playbackControls/SEEK"), json!(t)),
        LunaEvent::Position(t) => (s("playbackControls/TIME_UPDATE"), json!(t)),
        LunaEvent::Volume(v) => (s("playbackControls/SET_VOLUME"), json!({ "volume": v })),
        LunaEvent::Repeat(r) => (s("playQueue/SET_REPEAT_MODE"), json!(r.as_tidal())),
        LunaEvent::Shuffle(on) => (
            s(if *on {
                "playQueue/ENABLE_SHUFFLE_MODE"
            } else {
                "playQueue/DISABLE_SHUFFLE_MODE"
            }),
            Value::Null,
        ),
        LunaEvent::QueueChanged => (s("playQueue/RESET"), redux_state(snap)["playQueue"].clone()),
        LunaEvent::FavoritesChanged => (
            s("favorites/SET_FAVORITE_IDS"),
            redux_state(snap)["favorites"].clone(),
        ),
        LunaEvent::OpenUrl(_) => return None,
        LunaEvent::Action { kind, payload } => (kind.clone(), payload.clone()),
    })
}
