# TIDAL "My Activity" stats and play reporting (research notes)

Researched 2026-10-13 from primary source: tidal-music/tidal-sdk-web (`packages/event-producer`, `packages/player`),
tidal-music/tidal-sdk-android (`eventproducer`, `player/events`), the official OpenAPI spec (v1.10.167,
`tidal-music.github.io/tidal-api-reference/tidal-api-oas.json`), tomkoid/tidlers, glima/mare-player, EbbLabs/python-tidal,
oskvr37/tiddl. No live calls were made (no token used), so everything below is source-derived.

Secrets rule: no client ids, secrets or tokens in this file.

Legend: [V] = read in source, [?] = unverified / inferred, [X] = checked and absent.

---------------------------------------------------------------------------------------------------

## 1. "My Activity" listening statistics

### 1.1 Summary

- Not in the official OpenAPI (`openapi.tidal.com/v2`, 289 paths). [X] No activity, history, recap, listening-time or top-N
  resource exists there. `trackStatistics`/`albumStatistics` are artist-facing global counters (scope `r_usr`, tier INTERNAL).
- It lives on the private first-party API, host `https://api.tidal.com/v2` (the same host as `feed/activities`, `home/feed/STATIC`).
  [V] Only one open-source client has touched it: tidlers (`src/client/api/activity.rs`, added 2026-10, tomkoid/tidlers).
- Only TWO endpoints are known: `GET /my-activity/timeline` and `GET /my-activity/top-artists`. [V]
  Top tracks, albums, genres, listening hours and share cards have no known endpoint. [?] (no GitHub code at all; searches for
  `my-activity/top-tracks`, `top-albums`, `listening-time`, `nrOfStreams`, `totalNrOfDaysLeft` returned nothing.)
- Auth: the normal user Bearer token; tidlers sends the same token as every other v2 call (token minted with scope
  `r_usr w_usr w_sub` by a first-party client). No special scope. [V] tidlers uses its Android-mobile PKCE client for it.
  Whether a device-code (TV) client token or a registered developer-app token is accepted is untested. [?]

### 1.2 Endpoints [V, from tidlers]

Common to both:

```
GET https://api.tidal.com/v2/my-activity/<name>
Authorization: Bearer <access token>
?countryCode=<user country, e.g. NO>     (tidlers always adds it)
&locale=<e.g. en_US>                     (tidlers always adds it)
```

Other headers: tidlers adds nothing special for these two (the `x-tidal-client-version` / `deviceType=BROWSER` helper in
request_builder is opt-in and not used by activity). [V] Unknown whether the server needs them. [?]

#### GET /my-activity/timeline

No other params. Lists the months for which a monthly report exists (drives the month picker).

```json
{ "timeline": [ { "year": 2026, "month": 9, "title": "September" } ] }
```
tidlers model: `timeline: [{year:i32, month:u32, title:string}]`. Order (newest first or oldest first) not stated. [?]
`title` is presumably localized by `locale`. [?]

#### GET /my-activity/top-artists

Params: `year=<YYYY>`, `month=<1-12>` (+ countryCode, locale). Response (field names from tidlers' serde model; optionality as
tidlers declares it):

```jsonc
{
  "activity": {                       // optional; the "card" shown on the home screen / share card data
    "@class": "...",                  // Jackson type tag, value not recorded anywhere [?]
    "activityType": "...",            // string, enum values unknown [?] (likely something like TOP_ARTISTS_MONTHLY)
    "artists": [ { "id": 123, "name": "...", "nrOfStreams": 42 } ],   // summary list, top few
    "images":  [ { "id": "...", "url": "https://..." } ],             // collage images for the card
    "month": 9, "year": 2026,
    "totalNrOfDaysLeft": 3,           // optional: days until month closes (report still "in progress")
    "title": "...", "subtitle": "..." // optional, localized
  },
  "artists": [                        // optional; the full ranked list
    { "artist": {
        "id": 123, "name": "...", "picture": "uuid-or-null", "popularity": 77, "doublePopularity": 77.3,
        "artistRoles": [ { "categoryId": -1, "category": "Artist" } ],  // tidlers' ArtistRole type
        "artistTypes": ["ARTIST","CONTRIBUTOR"],
        "mixes": { "ARTIST_MIX": "mixid" },                              // optional
        "selectedAlbumCoverFallback": "uuid-or-null", "vibrantColor": "#aabbcc"
      },
      "nrOfStreams": 42 } ],
  "disclaimers": [ { "text": "...", "url": "https://..." } ],           // optional
  "metadata": { "timeline": [ { "year": 2026, "month": 9, "title": "September" } ] }  // same list as /timeline
}
```
Notes:
- `nrOfStreams` = stream count per artist for that month. Number of entries / paging / `limit` param: unknown. [?]
- `metadata.timeline` embeds the month picker, so `/timeline` may be redundant. [V]
- Images use the usual resource-uuid scheme (`https://resources.tidal.com/images/<uuid with / for ->/<w>x<h>.jpg`). [?] for the
  `images[].url` field, which looks like a full URL already.
- Missing data months probably return `activity: null` / empty `artists`. [?]
- Monthly only. Nothing found for year/all-time ("TIDAL Rising"/recap). The history mixes (below) are the only all-time/yearly
  artifact visible through public code.

#### Related, already visible in public code

- `GET https://api.tidal.com/v2/feed/activities?countryCode&locale` (tidlers `get_activity_feed`): the *new releases from
  followed artists* feed, NOT listening stats. It can contain `activityType: "NEW_HISTORY_MIX"` items with a `historyMix`
  `{id, titleTextInfo.text, subTitleTextInfo.text, images{SMALL,MEDIUM,LARGE:{width,height,url}}}`. [V]
- Mix types `HISTORY_MONTHLY_MIX`, `HISTORY_YEARLY_MIX`, `HISTORY_ALLTIME_MIX` exist (python-tidal `MixType`). [V] These are
  generated from listening history ("My most listened" playlists). They are fetched like any mix (`pages/mix?mixId=...` /
  `mixes/{id}/items`, see tidal-api.md). The mix id comes from the home feed / `pages/my_collection_my_mixes`. [?] on
  exact module name.
- Search history: official `GET /searchSuggestions/{id}/relationships/history`, `DELETE /searchHistoryEntries/{id}` (scope
  `search.read/search.write`). Not listening history. [V]

### 1.3 Can desktop/web call these with the same OAuth token?

- Strong indication yes: tidlers calls them with a normal desktop-style session token (PKCE/device login, scope
  `r_usr w_usr w_sub`), no mobile-only signing, from a non-mobile library. [V] But tidlers' default client is a TIDAL first-party
  client; the response for other client ids is unknown. [?]
- Official/developer-app tokens: the OpenAPI scope list has no activity scope. Expect 401/403 on `api.tidal.com/v2/my-activity`
  with a developer-app token. [?]
- Not seen in the `listen.tidal.com` web player's public code (that code is closed); the mobile apps' "My Activity" screens are
  assumed to use these. [?]

### 1.4 What we could not find (open questions)

1. Top tracks / albums / genres / listening time / total streams endpoints. Guess pattern `/my-activity/top-tracks`,
   `/top-albums`, `/top-genres`, `/listening-time`, `/summary` [?]. Pure guesses; probe with the user's own token and treat 404
   as "does not exist".
2. Share-card ("My activity" image) generation: probably server-side; the `activity.images` + `title/subtitle` suggests the app
   renders the card locally from these. [?]
3. Yearly recap ("TIDAL Rising" is an emerging-artist program, not a user recap; no user-recap API found). [?]
4. Exact `activityType` enum, `@class` value, ordering and paging of `artists`.
5. Whether the stats count only reported plays (section 2). Almost certainly derived from the same `play_log` stream, so plays
   from a client that does not report are invisible in stats, Recently Played and history mixes. [?]

Probe plan (Tidalfast): implement `my-activity/timeline` + `top-artists` behind an "experimental" flag, log raw JSON to
the debug log (no token), then probe the guessed siblings one time and record the HTTP codes here.

---------------------------------------------------------------------------------------------------

## 2. Play reporting (event producer)

### 2.1 Summary

- Official clients (Android, iOS, web, and the web/Android player SDKs) report playback through the **Event Producer**: events
  are queued locally and POSTed in batches of up to 10 to `https://ec.tidal.com/api/event-batch`
  (`/api/public/event-batch` when logged out). [V android `EventProducer.kt` `TIDAL_PRODUCTION_TL_CONSUMER_URI`,
  `SqsService.kt`, `MAX_BATCH_SIZE = 10`; web `submit.ts`]
- The body is **AWS SQS `SendMessageBatch` in form-encoded form** (not a JSON envelope). Response is SQS XML. [V]
- The `play_log` group is what feeds history/Recently Played, activity stats and (per mare) royalty accounting.
  `streaming_metrics` group is QoS telemetry (consent `PERFORMANCE`, not needed for credit). [V group names; payout/history
  linkage is TIDAL's documented purpose but not provable from client code, mare-player asserts it [?]]
- A third-party client that implements only `play_log/playback_session` (what mare-player does) is, per mare's author, enough to
  appear in Recently Played and aggregate counters. [?] (author claim, not independently verified)
- Official spec doc referenced by READMEs: `github.com/tidal-music/tidal-sdk/blob/main/EventProducer.md` (not fetched).

### 2.2 Transport [V]

```
POST https://ec.tidal.com/api/event-batch
Content-Type: application/x-www-form-urlencoded
Authorization: Bearer <access token>          (web: only if a token exists; otherwise use /api/public/event-batch)
```
Form fields, for event index i = 1..10 (1-based):

```
SendMessageBatchRequestEntry.i.Id                                   = <event uuid>   (must be distinct within a batch)
SendMessageBatchRequestEntry.i.MessageBody                          = <JSON string, see 2.3>
SendMessageBatchRequestEntry.i.MessageAttribute.1.Name              = Name
SendMessageBatchRequestEntry.i.MessageAttribute.1.Value.StringValue = <event name, e.g. playback_session>
SendMessageBatchRequestEntry.i.MessageAttribute.1.Value.DataType    = String
SendMessageBatchRequestEntry.i.MessageAttribute.2.Name              = Headers
SendMessageBatchRequestEntry.i.MessageAttribute.2.Value.StringValue = <JSON object string of headers, see 2.4>
SendMessageBatchRequestEntry.i.MessageAttribute.2.Value.DataType    = String
```
Response (200): SQS XML `SendMessageBatchResponse/SendMessageBatchResult` with `SendMessageBatchResultEntry/Id` for each accepted
event and `BatchResultErrorEntry` (`Id`, `SenderFault`) for failures. `SenderFault=true` means malformed: drop the event;
otherwise keep and retry. [V web submit.ts]. Non-2xx: XML `ErrorResponse/Error/{Type,Code}`; code
`AWS.SimpleQueueService.BatchEntryIdsNotDistinct` = duplicate ids in batch (dedupe). [V]

Scheduling (official): web flushes the queue every 30 s (`eventBatchInterval`), monitoring event every 60 s, 10 s fetch timeout,
queue persisted (IndexedDB / Room DB) with an outage flag when sends fail. Android chunk size 10. [V] Android interval
constant not read. [?]

### 2.3 MessageBody (the event JSON)

Android serializes the whole event with Gson (`DefaultEventReporter`: `gson.toJson(event)`) so the body is:

```jsonc
{
  "group": "play_log",            // or "streaming_metrics"
  "version": 2,
  "ts": 1760000000000,            // ms epoch, "true time" (NTP-corrected on official clients)
  "uuid": "<same as Id>",
  "user":   { "id": 12345678, "clientId": 1234, "sessionId": "<jwt sid claim>" },   // id from jwt uid claim; clientId may be null
  "client": { "token": "<jwt cid claim as string>", "deviceType": "mobile", "version": "2.87.0", "platform": "android" },
  "payload": { ... per event, see below ... },
  "extras": { ... }               // optional free-form map from the MediaProduct; web hoists it out of payload
}
```
- `user.id` / `clientId` / `sessionId` / `client.token` are read from the JWT claims `uid`, `cid`, `sid` of the access token.
  [V android UserSupplier/ClientSupplier]
- Android `deviceType` enum serialized values: `androidAuto`, `tv`, `tablet`, `mobile`. [V] mare-player sends `phone`, which is
  not in the official enum; use `mobile`. [V/?] (server may be lenient.)
- The **web SDK omits `user` and `client`** (event = `{group,name?,version,payload,extras?,ts,uuid}`; server derives identity from
  headers/token). Web `createPayload` = whole raw event minus `consentCategory`, plus `ts` and `uuid`. [V] mare reports web-style
  events are not surfaced in Recently Played and therefore mimics mobile. [?]
- `ts` should be accurate; both SDKs use a TrueTime NTP library. A client with a bad clock risks rejection. [?]
- Event size is validated client-side (Android `EventSizeValidator`); limit value not read. [?]

#### playback_session (group `play_log`, version 2)  [V android AudioPlaybackSession + web playback-session.ts]

One per track listen, sent when the playback session ends (track change/stop).

```jsonc
"payload": {
  "playbackSessionId": "<uuid>",
  "productType": "TRACK",                  // TRACK | VIDEO | UC (web); android also BROADCAST
  "requestedProductId": "12345",           // string track id requested
  "actualProductId":    "12345",           // may differ (e.g. substitution)
  "actualAssetPresentation": "FULL",       // FULL | PREVIEW
  "actualAudioMode": "STEREO",             // STEREO | DOLBY_ATMOS (| SONY_360RA web)
  "actualQuality": "LOSSLESS",             // LOW | HIGH | LOSSLESS | HI_RES_LOSSLESS (video: other enum)
  "sourceType": "ALBUM",                   // string; "" when none (web/mare). Android: nullable
  "sourceId": "999",                       // string; "" when none
  "isPostPaywall": true,                   // android hard-codes true for audio; web via get-is-post-paywall (false for PRIVATE_LINK_SHARING / previews)
  "startTimestamp": 1760000000000,         // ms epoch
  "startAssetPosition": 0.0,               // seconds in the track
  "endTimestamp": 1760000180000,
  "endAssetPosition": 180.0,
  "actions": [
    { "actionType": "PLAYBACK_START", "assetPosition": 0.0,   "timestamp": 1760000000000 },
    { "actionType": "PLAYBACK_STOP",  "assetPosition": 180.0, "timestamp": 1760000180000 }
    // pause/resume/seek produce additional START/STOP pairs; only these two action types exist [V]
  ]
}
```
Notes:
- Web defaults for a missing field are `-1` (positions/timestamps) and `""` (ids). `sourceType`/`sourceId` are non-nullable
  strings in the web schema; mare reports that `null` fails validation and the event is silently dropped. [V web / ? mare]
- Known `sourceType` values seen in comments/tests: `ALBUM`, `PLAYLIST`, `MIX`, `ARTIST`, `TRACK`, `PRIVATE_LINK_SHARING`.
  Full list not found. [?] mare: container types appear in Recently Played, `TRACK` counts toward "most listened" only. [?]
- No explicit "counts as a stream after 30 s" rule is visible in client code; the client simply reports positions/actions and the
  backend decides. mare-player uses its own gate: report only if listened >= 30 s or >= 50 % of duration. [V mare]. TIDAL's
  documented royalty threshold is 30 s [?] (public knowledge, not from code).
- Gapless/crossfade: one session per track; each has its own `playbackSessionId`. (web cypress tests `case-gapless`,
  `case-crossfade` exist; not read.) [?]

#### streaming_metrics group (version 2, consent `PERFORMANCE`; sent only when logged in)  [V web player]

Linked by `streamingSessionId` (one per play attempt, not equal to `playbackSessionId`). Likely not required for history/credit. [?]

- `streaming_session_start`: `{streamingSessionId, timestamp, startReason: EXPLICIT|IMPLICIT, isOfflineModeStart, sessionProductType:
  TRACK|VIDEO|BROADCAST, sessionProductId, sessionType: PLAYBACK|DOWNLOAD, sessionTags[], networkType: ETHERNET|WIFI|MOBILE|OTHER|NONE,
  mobileNetworkType, hardwarePlatform, operatingSystem, operatingSystemVersion, screenWidth, screenHeight, browser, browserVersion,
  abTestName, abTestGroup}`
- `playback_info_fetch`: `{streamingSessionId, startTimestamp, endTimestamp, endReason: COMPLETE|ERROR|OTHER, errorCode, errorMessage}`
- `drm_license_fetch`: same shape as above (web `drm-license-fetch.ts`, not read in detail). [?]
- `playback_statistics`: `{streamingSessionId, idealStartTimestamp, actualStartTimestamp|null, actualProductId, productType,
  actualQuality, actualAudioMode, actualAssetPresentation, actualStreamType: ON_DEMAND|LIVE, cdm: WIDEVINE|FAIR_PLAY|NONE, cdmVersion,
  stalls[{reason: SEEK|UNEXPECTED, assetPosition, startTimestamp, endTimestamp}], adaptations[{assetPosition, timestamp, mimeType,
  codecs, bandwidth, videoWidth, videoHeight}], hasAds, outputDevice, endTimestamp, endReason, errorCode, errorMessage}`
  (Android additionally: `mediaStorage`, `tags[]` such as ADAPTIVE_PLAYBACK.)
- `streaming_session_end`: `{streamingSessionId, timestamp}`
- `progress`: the web code only strips `streamingSessionId` from a `progress` event if present; no emitter exists in the public
  web/Android/iOS SDK source. Treat as obsolete/internal. [X]

### 2.4 Headers attribute (MessageAttribute "Headers", a JSON object serialized to a string)  [V]

Android (`HeadersUtils`): `app-version`, `os-version` (SDK int), `os-name` ("Android"), `requested-sent-timestamp` (ms, string),
`device-vendor`, `device-model`, `consent-category` (`NECESSARY` | `PERFORMANCE` | `TARGETING`), `authorization` (RAW token, no
"Bearer ", omitted on monitoring events or when logged out), `client-id` (credentials clientId). Plus `app-name` (EventProducer
config). Web (`headerUtils.ts`): `app-name`, `app-version`, `browser-name`, `browser-version`, `client-id`, `consent-category`,
`os-name`, `requested-sent-timestamp`, `authorization` (raw token).
- `play_log` events use consent `NECESSARY` (always allowed); `streaming_metrics` use `PERFORMANCE` (user can opt out). [V]
- Events logged out go to `/api/public/event-batch` without Authorization/`authorization`; group name becomes `play_log_open` on web. [V]

### 2.5 What mare-player does (only open-source third-party implementation found)  [V]

`src/tidal/play_reporter.rs` (GPL-3.0; do not copy code, reimplement from this description):
- Sends ONLY `playback_session` (group `play_log`, version 2), one event per SQS batch, background task, never blocks playback.
- Builds the Android-style envelope (`user`/`client` objects, `extras: null`), `isPostPaywall: true`, `actualAssetPresentation: FULL`,
  `actualAudioMode: STEREO`, exactly two actions (START at start position, STOP at the high-water-mark end position).
- Reads `uid`, `cid`, `sid` from the access token's JWT payload (base64url middle segment); claims client/platform identity of the
  same first-party client the token belongs to (Android mobile; `app_version` taken from the token response `clientName`
  e.g. `TIDAL_Android_<ver>`).
- Both an `Authorization: Bearer` HTTP header and a raw `authorization` entry inside the Headers attribute, plus `client-id`,
  `app-name`, `app-version`, `os-name`, `consent-category: NECESSARY`, `requested-sent-timestamp`.
- Gate: report when listened >= 30 s OR >= 50 % of the track; the position is a high-water mark; finalize on track end, skip,
  or app teardown. No retry queue, no persistence, no batching (fire and forget; failures only logged). That is weaker than the
  official clients (offline plays are lost).
- No `streaming_metrics`, no `playback_statistics`, no monitoring events.
- Outcome claims (all [?], author comments only): appears in Recently Played when `sourceType` is a container (ALBUM, PLAYLIST,
  MIX, ARTIST); `TRACK`/track id counts toward "Most Listened" only; "credits royalties".
- tidlers, python-tidal and tiddl do NOT implement any play reporting (grep for `event-batch`, `ec.tidal.com`, `playback_session`
  found nothing). [X]

### 2.6 Recently played / history endpoints

- No dedicated "history" REST resource in the official OpenAPI. [X] (only `userPlaybackStates`, `playQueues` — now-playing
  / queue sync for TIDAL Connect, scopes `r_usr`/`w_usr`; not history.)
- Recently Played appears as a module on private-API pages (home / `pages/...`), populated from `play_log` events. Module
  type name is not recorded in the sources read. [?]
- v1 favorites/`users/{id}/...` contain no play history. [?]
- History mixes (`HISTORY_*_MIX`) are the only history-derived objects reachable via public code (section 1.2).

### 2.7 Auth and eligibility notes for Tidalfast

- Same bearer token as the rest of the app; the token JWT must be decoded for `uid`, `cid`, `sid` (all integers except `sid`).
- Identity consistency matters: report with the platform/deviceType of the OAuth client the token belongs to (mare's reasoning).
  Tidalfast's own client identity decides what to put in `client.*` and `app-name`/`app-version`/`os-name`. [?] whether
  claiming "windows"/"desktop" (Electron-style desktop client) is accepted and surfaces in Recently Played is untested.
- No evidence found that developer-registered (THIRD_PARTY) apps may POST to `ec.tidal.com`. Endpoint accepted bearer tokens
  from first-party clients only in known reports. [?]
- Ethics/ToS: faking mobile identity to get plays credited is an impersonation risk; prefer truthful client fields and treat
  Recently Played surfacing as best-effort. A maintainer decision.

### 2.8 Minimal implementation (design notes; built, see section 4)

1. On track end/skip/stop with listened >= 30 s (or >= 50 %), build `playback_session` v2 (2.3) with true start/end wall-clock
   and positions; `sourceType/sourceId` from the container being played (`""` if none).
2. Persist to a small on-disk queue; flush every ~30 s and on exit in batches of <= 10 using the form encoding in 2.2; parse the
   SQS XML: remove acked ids and `SenderFault=true`; keep others; back off on network failure.
3. Optionally add `streaming_session_start/_end` and `playback_statistics` later (QoS only).
4. Add a settings toggle "Report plays to TIDAL" (privacy.md), default per maintainer decision; log request status, never the token.
5. Verify end to end: play 3 tracks from an album, wait ~1 min, check Recently Played on another TIDAL client and
   `my-activity/top-artists` for the current month.

---------------------------------------------------------------------------------------------------

## 3. Confirmed vs uncertain

Confirmed [V]: event-batch URL, SQS form encoding, batch size 10, 30 s web flush, header attribute keys, play_log
`playback_session` v2 schema (Android + web), JWT claims used for identity, `GET api.tidal.com/v2/my-activity/{timeline,top-artists}`
with `year`/`month`/`countryCode`/`locale` and their field names (as modeled by tidlers), no activity/history in the official OpenAPI.
Uncertain [?]: any other my-activity endpoint (tracks/albums/genres/hours), real JSON samples/enums, acceptance of non-first-party
tokens, whether `playback_session` alone credits streams/payouts, Recently Played surfacing rules, `deviceType` leniency
(`phone` vs `mobile`), minimum-length rule used by the backend, event size limits.

---------------------------------------------------------------------------------------------------

## 4. Tidalfast implementation (2026-10-13)

- Play reporting: `src/play_report/` (listen tracker fed by every engine state report, `playback_session` v2 builder, SQS
  form/XML, on-disk queue `<state>/play-reports.json`, reporter task). Every listen that actually played is reported, as the
  official SDKs do (the backend decides what counts). Batches of <= 10 every 30 s and on quit; `SenderFault=true` and
  malformed-batch errors drop; other failures back off per event (30 s doubling to 6 h, 16 tries, 14 days max); network
  failures hold the whole queue (30 s doubling to 30 min) without spending tries. Events are tied to the account they were
  played on and only sent with that account's token.
- Identity: Android-style envelope with `user` {uid, cid, sid from the JWT} and `client` {token: cid, deviceType `desktop`,
  version: Tidalfast's version, platform `windows`/`macos`/`linux`}; headers `app-name: Tidalfast`, `app-version`, `os-name`,
  `client-id` (the grant's OAuth client), `consent-category: NECESSARY`, `requested-sent-timestamp`, `authorization`.
  `sourceType` ALBUM/PLAYLIST/MIX/ARTIST/TRACK from the context URI (stations map to their seed), `""` otherwise.
  Untested live: whether TIDAL surfaces `desktop` events in Recently Played / My Activity.
- Setting `report_plays` ("Report plays to TIDAL"), default on; `Command::SetPlayReporting`. Off clears the queue.
- My Activity: `TidalClient::my_activity_timeline[_raw]`, `my_activity_top_artists[_raw](year, month)`
  (`src/api/tidal/activity.rs`, tolerant models). Backend: `Command::MyActivity` -> `Event::MyActivity`.
  Live check: start with `TIDALFAST_DUMP_ACTIVITY=1`; after sign-in the raw JSON (timeline, top artists for this month and
  the newest reported month, and the HTTP result of each guessed sibling path) goes to the log and
  `<state>/my-activity-dump.json`, for investigating further endpoints.
- Local statistics: `src/play_stats.rs`, `<state>/listens.jsonl` (versioned JSON lines, <= 200k records). Stream = >= 30 s or
  >= 50 % heard. Backend: `Command::Stats` -> `Event::Stats`.
- My Activity pages (`src/activity.rs` state, `src/ui/activity.rs` views; `Page::MyActivity`, `Page::TopArtists`), opened from
  the profile page's "My Activity" button and the avatar menu. The overview reads this month's `top-artists` and the timeline,
  then the newest past month's report for the Previous Month card; Top Artists reads one report per month tab. Numbers are
  TIDAL's, shown as sent (top 25, TIDAL's order). Reports are kept per account and day ("updated once a day") and read again
  on a new day, another account or Retry. Days left: `activity.totalNrOfDaysLeft`, else the calendar. Share copies the hero
  as a picture (a window capture cropped to it). Below, set apart, "Listening in Tidalfast": this month's listening time,
  streams and top 20 songs from the local log (`Command::Stats`, `StatsRange::ThisMonth`), labelled as Tidalfast-only.
