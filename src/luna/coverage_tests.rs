//! Behaviour the plugin API shares with TidaLuna's own source, one test per
//! area of `docs/_reference/luna-api-coverage.md`: the lib's content classes
//! with a host that serves tracks, albums and MusicBrainz answers, and the
//! core helpers (tracing, intercepts, search, the settings transfer).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::compat_tests::{HEADER, results};
use super::*;

/// A small TIDAL catalogue: songs 1 (LOSSLESS) and 2 (HI_RES_LOSSLESS) share
/// an ISRC, 3 is Atmos-only, and MusicBrainz answers from canned JSON.
#[derive(Default)]
pub(super) struct CatalogueHost {
    playback_asks: Mutex<Vec<Option<String>>>,
    pub(super) notes: Mutex<Vec<String>>,
    /// The host commands plugins issued, as `Debug` text.
    pub(super) commands: Mutex<Vec<String>>,
}

fn song(id: u64, isrc: &str, album: u64, tags: &[&str], quality: &str) -> Value {
    json!({
        "id": id, "title": "Song", "version": null, "duration": 200,
        "trackNumber": 3, "volumeNumber": 1, "isrc": isrc, "copyright": "(c) X",
        "replayGain": -6.5, "peak": 0.9, "url": format!("http://www.tidal.com/track/{id}"),
        "audioQuality": quality,
        "audioModes": if tags == ["DOLBY_ATMOS"] { json!(["DOLBY_ATMOS"]) } else { json!(["STEREO"]) },
        "mediaMetadata": { "tags": tags },
        "album": { "id": album, "title": "Album", "cover": "aaaa-bbbb" },
        "artists": [{ "id": 100, "name": "Artist A" }, { "id": 101, "name": "Artist B" }],
        "artist": { "id": 100, "name": "Artist A" },
    })
}

fn only_girl(id: u64, isrc: &str) -> Value {
    let mut t = song(id, isrc, 10, &["LOSSLESS"], "LOSSLESS");
    t["title"] = json!("Only Girl (In The World)");
    t["duration"] = json!(235);
    t
}

/// MusicBrainz's ISRC answer for USUM71023200 (live 2026-10-22, in one of
/// the orders it gives): Pride and Prejudice's chapter 57 and two Only Girl
/// recordings share it.
fn brainz_only_girl(url: &str) -> Option<Value> {
    let chapter = json!({ "id": "chapter57", "title": "Chapter 57", "length": 614000 });
    if url.contains("/isrc/USUM71023200") {
        return Some(json!({ "recordings": [
            chapter,
            { "id": "rnb2012", "title": "Only Girl (In the World)", "length": 230507 },
            { "id": "loud", "title": "Only Girl (In the World)", "length": 235493 },
        ] }));
    }
    if url.contains("/isrc/ISRC5") {
        return Some(json!({ "recordings": [chapter] }));
    }
    let release = |title: &str, track: &str, rec: &str, isrcs: Value| {
        json!({ "releases": [{ "title": title, "text-representation": { "language": "eng" }, "media": [{ "tracks": [{
            "title": track, "artist-credit": [{ "joinphrase": "" }],
            "recording": { "id": rec, "isrcs": isrcs },
        }] }] }] })
    };
    if url.contains("/recording/chapter57") {
        return Some(release(
            "Pride and Prejudice",
            "Chapter 57",
            "chapter57",
            json!(["USUM71023200", "ISRC57"]),
        ));
    }
    if url.contains("/recording/loud") {
        return Some(release(
            "Loud",
            "Only Girl (in the World)",
            "loud",
            json!(["USUM71023200", "USUM71112341"]),
        ));
    }
    if url.contains("/recording/rnb2012") {
        return Some(release(
            "R&B Collection 2012",
            "Only Girl (In the World)",
            "rnb2012",
            json!(["USUM71023200"]),
        ));
    }
    None
}

impl LunaHost for CatalogueHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, c: HostCommand) {
        self.commands.lock().unwrap().push(format!("{c:?}"));
    }
    fn track(&self, id: &str) -> Option<Value> {
        match id {
            "1" => Some(song(1, "ISRC1", 10, &["LOSSLESS"], "LOSSLESS")),
            "2" => Some(song(
                2,
                "ISRC1",
                11,
                &["LOSSLESS", "HIRES_LOSSLESS"],
                "HI_RES_LOSSLESS",
            )),
            "3" => Some(song(3, "ISRC3", 10, &["DOLBY_ATMOS"], "HI_RES_LOSSLESS")),
            // Rihanna's "Only Girl (In The World)" from Loud, whose ISRC
            // MusicBrainz also gives an audiobook chapter, and a song whose
            // ISRC MusicBrainz knows only for that chapter.
            "4" => Some(only_girl(4, "USUM71023200")),
            "5" => Some(only_girl(5, "ISRC5")),
            _ => None,
        }
    }
    fn album(&self, id: &str) -> Option<Value> {
        match id {
            "10" => Some(json!({
                "id": 10, "title": "Album Ten", "cover": "aaaa-bbbb", "numberOfTracks": 10,
                "upc": "111", "releaseDate": "2020-05-01", "genre": "Rock",
                "recordLabel": "Label X", "copyright": "(p) Y",
                "artists": [{ "id": 100, "name": "Artist A" }],
                "artist": { "id": 100, "name": "Artist A" },
            })),
            // Another UPC, and a track count MusicBrainz's release disagrees with.
            "11" => Some(json!({
                "id": 11, "title": "Album Eleven", "cover": "cccc-dddd",
                "numberOfTracks": 9, "upc": "222", "copyright": "(p) Y",
                "artists": [{ "id": 100, "name": "Artist A" }],
            })),
            _ => None,
        }
    }
    fn artist(&self, id: &str) -> Option<Value> {
        (id == "100" || id == "101")
            .then(|| json!({ "id": id, "name": format!("Artist {id}"), "picture": "p-q" }))
    }
    fn playlist_items(&self, _uuid: &str) -> Vec<Value> {
        (1..=3)
            .map(|i| json!({ "item": { "id": i }, "type": "track" }))
            .collect()
    }
    fn lyrics(&self, _id: &str) -> Option<Value> {
        Some(json!({ "lyrics": "la la", "subtitles": null }))
    }
    fn tracks_by_isrc(&self, isrc: &str) -> Vec<Value> {
        if isrc == "ISRC1" {
            vec![self.track("1").unwrap(), self.track("2").unwrap()]
        } else {
            Vec::new()
        }
    }
    fn playback_info(&self, _id: &str, quality: Option<&str>) -> Option<Value> {
        self.playback_asks
            .lock()
            .unwrap()
            .push(quality.map(str::to_string));
        // A tier TIDAL answers with figures that differ from what the tier
        // fixes, and the codec and bit rate its manifest named.
        if quality == Some("HIGH") {
            return Some(json!({
                "audioQuality": "HIGH", "bitDepth": 16, "sampleRate": 48000,
                "stream": { "mimeType": "audio/mp4", "codec": "aac", "bitrate": 222222, "encrypted": false },
            }));
        }
        // A single FLAC file (BTS): the host asked its size, 951 kb/s over
        // the song's 200 seconds.
        Some(json!({
            "audioQuality": "LOSSLESS", "bitDepth": 16, "sampleRate": 44100,
            "stream": { "mimeType": "audio/flac", "codec": "flac", "encrypted": false, "bytes": 23_775_000 },
        }))
    }
    fn notify(&self, _plugin: Option<&str>, _level: NotifyLevel, message: &str) {
        self.notes.lock().unwrap().push(message.to_string());
    }
    fn outbound(&self, _plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        let NetRequest::Fetch { url, .. } = request else {
            return NetVerdict::Allow;
        };
        let body = if let Some(b) = brainz_only_girl(url) {
            b
        } else if url.contains("musicbrainz.org/ws/2/isrc/ISRC1") {
            json!({ "recordings": [{ "id": "rec1", "title": "Song", "length": 201000 }] })
        } else if url.contains("musicbrainz.org/ws/2/recording/rec1") {
            json!({ "releases": [{ "text-representation": { "language": "eng" }, "media": [{ "tracks": [{
                "title": "MB Title", "artist-credit": [{ "joinphrase": "" }],
                "recording": { "id": "rec1", "isrcs": ["ISRC1", "ISRC9"], "first-release-date": "2019-01-02" },
            }] }] }] })
        } else if url.contains("barcode:111") || url.contains("barcode:222") {
            json!({ "releases": [{ "id": "rel1", "title": "MB Album", "track-count": 10, "barcode": "B1" }] })
        } else {
            return NetVerdict::Respond {
                status: 404,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: b"{}".to_vec(),
            };
        };
        NetVerdict::Respond {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
        }
    }
}

/// Installs `body` as plugin `Compat` on the catalogue host, lets `during`
/// act on the running Luna, and returns the plugin's results once it sets
/// `R.done`.
pub(super) fn run_catalogue(
    body: &str,
    during: impl FnOnce(&Luna, &Path),
) -> (Value, Arc<CatalogueHost>) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Compat.json".into(),
        json!({ "name": "Compat", "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Compat.mjs".into(), format!("{HEADER}{body}").into_bytes());
    let base = serve(files);
    let dir = temp_dir("coverage");
    let host = Arc::new(CatalogueHost::default());
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    during(&luna, &dir);
    wait_until(&luna, 40, |_| results(&dir)["done"] == json!(true));
    let r = results(&dir);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    (r, host)
}

/// Waits until the plugin has set `R.<key>`.
pub(super) fn wait_for(luna: &Luna, dir: &Path, key: &str) {
    wait_until(luna, 40, |_| results(dir)[key] == json!(true));
}

pub(super) fn tidal_action(luna: &Luna, kind: &str, payload: Value) {
    luna.event(LunaEvent::Action {
        kind: kind.into(),
        payload,
    });
}

#[test]
fn content_classes_follow_lunas_source() {
    let (r, host) = run_catalogue(
        r##"
const { MediaItem, Album, TidalApi } = lib;
const m1 = await MediaItem.fromId(1);
// TidaLuna's list of FLAC tags.
R.tags = MediaItem.availableTags.join(",");
// MusicBrainz: the recording for the ISRC, its title, id and other ISRCs.
R.mbTitle = await m1.title();
R.mbId = await m1.brainzId();
const isrcs = []; for await (const i of m1.isrcs()) isrcs.push(i);
R.isrcs = isrcs.join(",");
const t = await m1.flacTags();
R.flac = JSON.stringify({ ...t.tags, lyrics: undefined });
R.flacLyrics = t.tags.lyrics;
R.flacCover = t.coverUrl;
// The best version of a recording: the highest quality with the ISRC.
R.fromIsrc = String((await MediaItem.fromIsrc("ISRC1"))?.id);
R.max = String((await m1.max())?.id);
R.maxOfMax = (await (await MediaItem.fromId(2)).max()) === undefined;
const viaApi = []; for await (const tr of TidalApi.isrc("ISRC1")) viaApi.push([tr.id, tr.attributes.mediaTags.join("+")].join(":"));
R.api = viaApi.join(",");
// Spatial-only: HiRes by TIDAL's own claim, what its playback info says after.
R.spatial = (await MediaItem.fromId(3)).bestQuality.name;
// Albums: the MusicBrainz release counts only with the right track count.
const a10 = await Album.fromId(10), a11 = await Album.fromId(11);
R.albumTitles = [await a10.title(), await a11.title()].join("|");
R.label = [a10.recordLabel ?? "none", a11.recordLabel ?? "none"].join("|");
// A playlist's items are Luna's page object.
const page = await TidalApi.playlistItems("pl");
R.page = [page.items.length, page.totalNumberOfItems, page.offset, page.limit].join(",");
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(
        r["tags"],
        "title,trackNumber,discNumber,bpm,year,date,copyright,REPLAYGAIN_TRACK_GAIN,REPLAYGAIN_TRACK_PEAK,comment,isrc,upc,musicbrainz_trackid,musicbrainz_albumid,artist,album,albumArtist,genres,organization,totalTracks,lyrics",
        "{r}"
    );
    assert_eq!(r["mbTitle"], "MB Title", "{r}");
    assert_eq!(r["mbId"], "rec1", "{r}");
    assert_eq!(r["isrcs"], "ISRC1,ISRC9", "{r}");
    let flac: Value = serde_json::from_str(r["flac"].as_str().unwrap_or("{}")).unwrap();
    assert_eq!(flac["title"], "MB Title", "{r}");
    assert_eq!(flac["artist"], json!(["Artist 100", "Artist 101"]), "{r}");
    assert_eq!(flac["albumArtist"], json!(["Artist 100"]), "{r}");
    assert_eq!(flac["trackNumber"], "3", "{r}");
    assert_eq!(flac["discNumber"], "1", "{r}");
    assert_eq!(flac["REPLAYGAIN_TRACK_GAIN"], "-6.5", "{r}");
    assert_eq!(flac["REPLAYGAIN_TRACK_PEAK"], "0.9", "{r}");
    assert_eq!(flac["comment"], "http://www.tidal.com/track/1", "{r}");
    assert_eq!(flac["copyright"], "(c) X", "{r}");
    assert_eq!(flac["isrc"], "ISRC1", "{r}");
    assert_eq!(flac["musicbrainz_trackid"], "rec1", "{r}");
    assert_eq!(flac["musicbrainz_albumid"], "rel1", "{r}");
    assert_eq!(flac["album"], "MB Album", "{r}");
    assert_eq!(flac["upc"], "111", "{r}");
    assert_eq!(flac["genres"], "Rock", "{r}");
    assert_eq!(flac["organization"], "Label X", "{r}");
    assert_eq!(flac["totalTracks"], "10", "{r}");
    assert_eq!(flac["date"], "2020-05-01", "{r}");
    assert_eq!(flac["year"], "2020", "{r}");
    assert_eq!(r["flacLyrics"], "la la", "{r}");
    assert_eq!(
        r["flacCover"], "https://resources.tidal.com/images/aaaa/bbbb/1280x1280.jpg",
        "{r}"
    );
    assert_eq!(r["fromIsrc"], "2", "{r}");
    assert_eq!(r["max"], "2", "{r}");
    assert_eq!(r["maxOfMax"], true, "{r}");
    assert_eq!(r["api"], "1:LOSSLESS,2:LOSSLESS+HIRES_LOSSLESS", "{r}");
    // Atmos-only track 3 asked TIDAL, at the best quality, what it plays as.
    assert_eq!(r["spatial"], "High", "{r}");
    assert!(
        host.playback_asks
            .lock()
            .unwrap()
            .contains(&Some("HI_RES_LOSSLESS".into())),
        "{r}"
    );
    assert_eq!(r["albumTitles"], "MB Album|Album Eleven", "{r}");
    assert_eq!(r["label"], "Label X|none", "{r}");
    assert_eq!(r["page"], "3,3,0,-1", "{r}");
}

/// A user's DiscordRPC status read "Chapter 57 by Rihanna" for Rihanna's
/// "Only Girl (In The World)": MusicBrainz gives its ISRC to an audiobook
/// chapter too, and the title came from whichever recording it listed
/// first. The title, MusicBrainz id and ISRCs are the playing song's.
#[test]
fn a_shared_isrc_keeps_the_songs_own_recording() {
    let (r, _host) = run_catalogue(
        r##"
const { MediaItem } = lib;
const m = await MediaItem.fromId(4);
R.title = await m.title();
R.mbId = await m.brainzId();
const isrcs = []; for await (const i of m.isrcs()) isrcs.push(i);
R.isrcs = isrcs.join(",");
R.flacTitle = (await m.flacTags()).tags.title;
// Only the chapter has the ISRC: TIDAL's own title stays.
const lone = await MediaItem.fromId(5);
R.loneTitle = await lone.title();
R.loneMbId = String(await lone.brainzId());
const loneIsrcs = []; for await (const i of lone.isrcs()) loneIsrcs.push(i);
R.loneIsrcs = loneIsrcs.join(",");
R.done = true;
"##,
        |_, _| {},
    );
    assert_eq!(r["title"], "Only Girl (in the World)", "{r}");
    assert_eq!(r["mbId"], "loud", "{r}");
    assert_eq!(r["isrcs"], "USUM71023200,USUM71112341", "{r}");
    assert_eq!(r["flacTitle"], "Only Girl (in the World)", "{r}");
    assert_eq!(r["loneTitle"], "Only Girl (In The World)", "{r}");
    assert_eq!(r["loneMbId"], "undefined", "{r}");
    assert_eq!(r["loneIsrcs"], "ISRC5", "{r}");
}

#[test]
fn core_helpers_follow_lunas_source() {
    let (r, host) = run_catalogue(
        r##"
const { LunaPlugin, SettingsTransfer, ReactiveStore, Tracer } = core;
// A title with "feat." keeps TIDAL's spelling only when MusicBrainz credits a featured artist.
R.feat = [
	lib.ContentBase.formatTitle("Song (feat. X)", undefined, "Song", [{ joinphrase: " feat. " }]),
	lib.ContentBase.formatTitle("Song (feat. X)", undefined, "Song", [{ joinphrase: " & " }]),
	lib.ContentBase.formatTitle("Song", "Live", "Song", undefined),
].join("|");
// Tracing: a thrown Error carries its context as the cause; data is JSON in messages.
const { trace } = Tracer("[T]");
try { trace.err.withContext("ctx").throw(new Error("boom")); } catch (e) { R.cause = e.cause; }
try { trace.err.throw("bad", { a: 1 }); } catch (e) { R.thrown = e.message; }
trace.msg.warn("note", { b: [2] });
// interceptActionResp: calls for one response type take turns; a failure
// type resolves with the failure's payload (as Luna's code does). TIDAL
// answers (the test plays TIDAL).
const { interceptActionResp } = lib.redux;
const SUCCESS = "content/LOAD_SINGLE_MEDIA_ITEM_SUCCESS", FAIL = "content/LOAD_SINGLE_MEDIA_ITEM_FAIL";
const both = Promise.all([interceptActionResp(() => {}, null, [SUCCESS], []), interceptActionResp(() => {}, null, [SUCCESS], [])]);
R.ready = true;
R.concurrent = JSON.stringify((await both).map((p) => p.n));
const failing = interceptActionResp(() => {}, null, [SUCCESS], [FAIL]);
R.ready2 = true;
R.failure = JSON.stringify(await failing);
// ftch rejects as Luna's helper does; the global require has Luna's cache.
await check("ftch", () => core.ftch.json("https://x.test/nothing"));
R.requireCache = globalThis.require.cache === core.modules;
// Node: stream/consumers reads whole streams; inspector (debugging) is refused with a reason.
const n = await native("Compat/consumers.native.ts", `"use strict";
const c = require("stream/consumers");
const chunks = async function* () { yield "ab"; yield "cd"; };
exports.go = async () => [await c.text(chunks()), JSON.stringify(await c.json((async function* () { yield '{"a":1}'; })())), (await c.buffer((async function* () { yield Buffer.from("xyz"); })())).length].join("|");
exports.insp = () => { try { require("inspector").open(); return "opened"; } catch (e) { return e.message; } };`);
R.consumers = await n("go");
R.inspector = await n("insp");
// recursiveSearch reaches a class instance's methods through its prototype.
class Thing { foo() {} }
R.proto = core.recursiveSearch({ inst: new Thing() }, (k) => k === "foo").next().value?.path.join(".");
// getByName prefers the installed copy of a name; dependants must be plugins.
const dupA = await LunaPlugin.fromStorage({ url: "https://fake.test/DupA", package: { name: "Dup" } });
const dupB = await LunaPlugin.fromStorage({ url: "https://fake.test/DupB", package: { name: "Dup" } });
dupB.store.installed = true;
R.byName = LunaPlugin.getByName("Dup") === dupB && LunaPlugin.getByName("Dup") !== dupA;
try { dupB.addDependant({}); R.dep = "accepted"; } catch (e) { R.dep = e.message; }
// The settings export: Luna's ExportData.
const data = await SettingsTransfer.dump();
R.dump = [data.version, SettingsTransfer.validate(data), SettingsTransfer.validate({ version: 2, stores: {} }), Object.keys(data.stores).sort().join(","), "Compat.results" in data.stores["@luna/pluginStorage"]].join("|");
await SettingsTransfer.restore({ version: 1, stores: { "@luna/storage": { restored: 1 } } });
R.restored = await ReactiveStore.getStore("@luna/storage").get("restored");
R.done = true;
"##,
        |luna, dir| {
            wait_for(luna, dir, "ready");
            let success = "content/LOAD_SINGLE_MEDIA_ITEM_SUCCESS";
            tidal_action(luna, success, json!({ "n": 1 }));
            std::thread::sleep(Duration::from_millis(600));
            tidal_action(luna, success, json!({ "n": 2 }));
            wait_for(luna, dir, "ready2");
            tidal_action(
                luna,
                "content/LOAD_SINGLE_MEDIA_ITEM_FAIL",
                json!({ "n": 3 }),
            );
        },
    );
    assert_eq!(r["feat"], "Song (feat. X)|Song|Song (Live)", "{r}");
    assert_eq!(r["cause"], "[T] ctx", "{r}");
    assert_eq!(r["thrown"], "[T] bad {\"a\":1}", "{r}");
    assert!(
        host.notes
            .lock()
            .unwrap()
            .contains(&"[T] note {\"b\":[2]}".to_string()),
        "{:?}",
        host.notes.lock().unwrap()
    );
    assert_eq!(r["ftch"], "ERR Status code is 404", "{r}");
    assert_eq!(r["requireCache"], true, "{r}");
    assert_eq!(r["consumers"], "abcd|{\"a\":1}|3", "{r}");
    assert!(
        r["inspector"]
            .as_str()
            .unwrap_or("")
            .contains("not available"),
        "{r}"
    );
    assert_eq!(r["concurrent"], "[1,2]", "{r}");
    assert_eq!(r["failure"], "{\"n\":3}", "{r}");
    assert_eq!(r["proto"], "inst.foo", "{r}");
    assert_eq!(r["byName"], true, "{r}");
    assert_eq!(r["dep"], "Cannot add dependancy to non plugin!", "{r}");
    assert_eq!(
        r["dump"], "1|true|false|@luna/pluginStorage,@luna/plugins,@luna/storage|true",
        "{r}"
    );
    assert_eq!(r["restored"], 1, "{r}");
}

#[test]
fn message_boxes_ask_the_user_and_return_the_button() {
    let (r, _host) = run_catalogue(
        r##"
const ui = luna.core.modules["@luna/ui"];
const lnative = luna.core.modules["@luna/lib.native"];
// confirm: { confirmed, reason } from the second button; Esc is a cancel.
const first = ui.confirm({ title: "Delete it?", description: "Really", confirmationText: "Yes", cancellationText: "No" });
R.ready = true;
R.confirm = JSON.stringify(await first);
const second = ui.confirm({ title: "Again?" });
R.ready2 = true;
R.escaped = JSON.stringify(await second);
// Electron's showMessageBox: the response is the index of the button.
const third = lnative.showMessageBox({ title: "Pick", message: "One", detail: "of three", buttons: ["A", "B", "Cancel"], defaultId: 1 });
R.ready3 = true;
R.box = (await third).response;
// A dialog with Esc and no cancel button asks for the first one.
const fourth = lnative.showMessageBox({ message: "Plain", buttons: ["Go", "Stop"] });
R.ready4 = true;
R.plainEsc = (await fourth).response;
// File dialogs are not available: cancelled, and said once in the log.
R.open = JSON.stringify(await lnative.showOpenDialog({}));
R.save = JSON.stringify(await lnative.showSaveDialog({}));
R.done = true;
"##,
        |luna, dir| {
            let next = |luna: &Luna| {
                wait_until(luna, 30, |s| !s.message_boxes.is_empty());
                luna.pending_message_box().unwrap()
            };
            wait_for(luna, dir, "ready");
            let b = next(luna);
            assert_eq!(b.plugin, "Compat");
            assert_eq!(b.title, "Delete it?");
            assert_eq!(b.message, "Really");
            assert_eq!(b.buttons, vec!["No".to_string(), "Yes".to_string()]);
            assert_eq!(b.default_id, 1);
            luna.answer_message_box(b.id, Some(1));
            wait_for(luna, dir, "ready2");
            let b = next(luna);
            assert_eq!(b.buttons, vec!["Cancel".to_string(), "Ok".to_string()]);
            luna.answer_message_box(b.id, None);
            wait_for(luna, dir, "ready3");
            let b = next(luna);
            assert_eq!((b.title.as_str(), b.detail.as_str()), ("Pick", "of three"));
            assert_eq!(b.default_id, 1);
            luna.answer_message_box(b.id, Some(1));
            wait_for(luna, dir, "ready4");
            let b = next(luna);
            // An answer for a box that is gone changes nothing.
            luna.answer_message_box(b.id + 100, Some(0));
            luna.answer_message_box(b.id, None);
        },
    );
    assert_eq!(
        r["confirm"], "{\"confirmed\":true,\"reason\":\"confirm\"}",
        "{r}"
    );
    assert_eq!(
        r["escaped"], "{\"confirmed\":false,\"reason\":\"cancel\"}",
        "{r}"
    );
    assert_eq!(r["box"], 1, "{r}");
    assert_eq!(r["plainEsc"], 0, "{r}");
    assert_eq!(r["open"], "{\"canceled\":true,\"filePaths\":[]}", "{r}");
    assert_eq!(r["save"], "{\"canceled\":true,\"filePath\":\"\"}", "{r}");
}
