# TIDAL API reference for a native Rust client (research notes, 2026-10-13)

Sources read (actual source, not summaries): EbbLabs/python-tidal (`tidalapi/*.py`, the maintained fork of tamland/python-tidal),
oskvr37/tiddl (3.4.4, July 2026), tomkoid/tidlers (Rust, commit of 2026-10-13), glima/mare-player (Rust/COSMIC app on tidlers; the
most informative source on 2026 behavior), strawberrymusicplayer/strawberry (`src/tidal/*`), tidal-music/tidal-sdk-web
(`packages/auth`, `packages/player`), and the official OpenAPI spec `https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json` (v1.10.167).
Not found: tidal-dl-ng (exislow/tidal-dl-ng returns 404; repo appears removed) and Mopidy-Tidal's old URL (now EbbLabs/mopidy-tidal, a thin wrapper over tidalapi, not read in depth).
developer.tidal.com is a JS SPA and returns no text to a fetcher; its content was taken from the OpenAPI spec and the SDK source instead.

Confidence tags: [V] = verified in current source of two or more clients; [S] = single source; [?] = inferred or from memory, verify before relying on it.

Secrets rule: this file contains no client ids or secrets. The app reads them from settings (see section 1.1).

---------------------------------------------------------------------------------------------------

## 0. Three API surfaces (do not confuse them)

| Surface | Base URL | Style | Used for |
|---|---|---|---|
| Legacy "v1" | `https://api.tidal.com/v1/` (alias host `api.tidalhifi.com/v1`, used by Strawberry) | plain JSON, `countryCode` param | almost everything the third-party clients use: catalog, favorites, playlists, search, lyrics, playbackinfo, pages |
| Internal "v2" | `https://api.tidal.com/v2/` and `https://tidal.com/v2/` | plain JSON, cursor paging | home feed, my-collection (folders, playlists, artists), favorites/mixes, search (web), profiles, feed activities |
| Official OpenAPI | `https://openapi.tidal.com/v2/` | JSON:API (`application/vnd.api+json`) | documented public API; `trackManifests`, `userCollection*`, `searchResults`, etc. |

`https://api.tidal.com/v2` and `https://tidal.com/v2` both appear in clients (tidalapi uses api.tidal.com/v2 for my-collection and favorites/mixes;
tidlers uses tidal.com/v2 for home feed and search). [V] that both host the same family, but use the host the reference client uses per endpoint.

Practical conclusion for a desktop client: use v1 for catalog/playback, internal v2 for library (folders, favorite playlists, favorite mixes), and treat the
official OpenAPI as optional (it exposes the same data in JSON:API form and needs a registered developer app for the user-scoped flow).

---------------------------------------------------------------------------------------------------

## 1. Authentication

All token endpoints live on `https://auth.tidal.com/v1/oauth2/`, bodies are `application/x-www-form-urlencoded`. The browser-facing authorize page is
`https://login.tidal.com/authorize`.

### 1.1 Client identity is the key concept

TIDAL decides the stream ceiling and available endpoints from the OAuth CLIENT a token was minted for, not only from the subscription. [V: mare-player docs, measured]

- Third-party clients embed first-party TIDAL client ids (secrets too). They are base64-obfuscated in tidalapi/tiddl/tidlers source. Do not copy them into this repo's docs.
- Observed classes:
  - "Device-code" clients (TV / Android-Automotive style). Work with the device flow below. Measured by mare-player on a HI_RES account: asking `HI_RES_LOSSLESS` returns `LOSSLESS`, 16 bit / 44100 Hz. Stream manifests come back unencrypted (see section 3.4).
    Lapsed for the device client Tidalfast was measured with on 2026-10-14: `HI_RES_LOSSLESS` gave DASH `FLAC_HIRES` 24 bit / 88200 Hz, and `LOSSLESS` gave AAC 320 (see 3.6).
  - "Android mobile" PKCE client. Needs the browser PKCE flow; is granted `HI_RES_LOSSLESS`, issues refresh tokens, and serves DASH for both FLAC tiers.
  - tidalapi note: with a PKCE session, `tracks/{id}/urlpostpaywall` is refused client-side (`URLNotAvailable`); use playbackinfopostpaywall.
- Settings design for our app: store per profile `client_id`, `client_secret`, an `auth_flow` enum (`device` | `pkce`), plus `client_unique_key` (random 64-bit hex,
  persisted, used by PKCE). The app must never ship these values in docs, logs, or telemetry. The access token's JWT claims `cid` (numeric client id), `uid`, `sid`
  are readable locally (base64url payload) and are used for play reporting (section 3.9).
- Official/developer route: a registered app at developer.tidal.com gets its own client id, PKCE only, scopes below; tier `THIRD_PARTY`. The playback scope works there too
  (`trackManifests`, section 3.8). Unknown whether a freshly registered app gets hi-res; the spec only says `playback` + `r_usr` scopes. [?]

### 1.2 Device authorization flow (RFC 8628 style) [V: tidalapi, tiddl, tidlers, official sdk-web]

Step 1: start.

```
POST https://auth.tidal.com/v1/oauth2/device_authorization
Content-Type: application/x-www-form-urlencoded
client_id=<client_id>&scope=r_usr w_usr w_sub          (tiddl sends "r_usr+w_usr+w_sub"; both work)
```
Optional (official SDK only): `client_secret=...`, `app_approval=true` (user approves in a TIDAL app; `verificationUriComplete` then becomes `https://tidal.com/approve/XXXXXXXX`).

Response 200:
```json
{ "deviceCode": "uuid-like", "userCode": "ABCDE", "verificationUri": "link.tidal.com",
  "verificationUriComplete": "link.tidal.com/ABCDE", "expiresIn": 300, "interval": 2 }
```
Note: `verificationUri*` come WITHOUT scheme (tidalapi prints `https://{verification_uri_complete}`). Open `https://` + `verificationUriComplete` in the browser.

Step 2: poll every `interval` seconds until `expiresIn` elapses.
```
POST https://auth.tidal.com/v1/oauth2/token
client_id=<id>&client_secret=<secret>&device_code=<deviceCode>
&grant_type=urn:ietf:params:oauth:grant-type:device_code&scope=r_usr w_usr w_sub
```
Client auth: tidalapi/tidlers put `client_secret` in the form body; tiddl uses HTTP Basic (`Authorization: Basic base64(id:secret)`) AND `client_id` in the body. Both accepted. [V]

Pending response is HTTP 400:
```json
{ "status": 400, "error": "authorization_pending", "sub_status": 1002, "error_description": "Device Authorization code is not authorized yet" }
```
Other errors: `expired_token` (stop, restart flow), `slow_down` / `access_denied` / `invalid_request` should be treated as fatal (tidlers does). Official SDK treats 5xx as retryable with backoff (500 ms start) and keeps looping on 4xx until timeout.
(Exact `sub_status` numbers: 1002 pending is from memory [?]; match on `error` string only.)

Success 200 (field names exact, [V]):
```json
{ "access_token": "eyJ...", "refresh_token": "eyJ...", "token_type": "Bearer", "expires_in": 86400,
  "scope": "r_usr w_usr w_sub", "user_id": 12345678, "clientName": "TIDAL_Android_x.y.z",
  "user": { "userId": 12345678, "email": "...", "countryCode": "US", "fullName": null, "username": "...", "channelId": 0, "parentId": 0,
            "acceptedEULA": true, "created": 1500000000000, "updated": 1700000000000, "emailVerified": true, "newUser": false, ... } }
```
`expires_in` = access-token lifetime in seconds. Observed 86400 in older clients; mare tracks it from the response rather than assuming. Persist `refresh_token`, `expires_at = now + expires_in`.
The embedded `user.countryCode` and `user_id` mean the `sessions` call (1.5) is optional for tokens from this flow.

### 1.3 Authorization code + PKCE flow [V: tidalapi, tidlers, official sdk]

1. Generate `code_verifier` (tidalapi: base64url(os.urandom(32)) without padding, 43 chars) and `code_challenge = base64url_nopad(SHA256(code_verifier))`. Generate/persist `client_unique_key` (16 hex chars).
2. Open in the system browser:
```
GET https://login.tidal.com/authorize
  ?response_type=code
  &redirect_uri=<redirect>
  &client_id=<pkce client id>
  &lang=EN
  &appMode=android
  &client_unique_key=<hex>
  &code_challenge=<challenge>
  &code_challenge_method=S256
  &restrict_signup=true
  [&scope=r_usr w_usr w_sub]      (official SDK sends scope; tidalapi omits it here and sends it at token time)
```
3. Redirect URIs: the first-party Android client uses `https://tidal.com/android/login/auth` (tidalapi/tidlers). The user logs in, lands on a TIDAL "Oops" page, and the app needs the `code` from that URL.
   For a native desktop app there is no way to register that redirect to localhost for a first-party client; options:
   a) Show the URL, ask the user to paste the final redirected URL (what tidalapi/tidlers/mare do; parse `?code=`).
   b) With a self-registered developer app: register `http://127.0.0.1:<port>/callback` (or custom scheme) and run a loopback listener. Strawberry uses `tidal://login/auth` for an older client. [S]
4. Exchange:
```
POST https://auth.tidal.com/v1/oauth2/token
code=<code>&client_id=<pkce client id>&grant_type=authorization_code&redirect_uri=<same redirect>
&scope=r_usr w_usr w_sub&code_verifier=<verifier>&client_unique_key=<hex>
```
tidalapi/tidlers send no secret on this exchange (public client); the official SDK sends `client_secret` only if configured. Response shape is identical to 1.2 (includes `refresh_token`).

### 1.4 Refresh [V]
```
POST https://auth.tidal.com/v1/oauth2/token
grant_type=refresh_token&refresh_token=<rt>&client_id=<id>&client_secret=<secret>     (tidalapi: secret in body)
   or Basic auth id:secret + grant_type=refresh_token&refresh_token=<rt>[&scope=r_usr w_usr w_sub]   (tiddl, tidlers)
```
- The refresh response omits `refresh_token` (tidlers `RefreshTokenGrantResponse` keeps the old one); keep the stored one unless a new one is returned. Response includes `access_token`, `expires_in`, `token_type`, `user_id`, `clientName`, `user` object.
- Must refresh with the SAME client id (and secret, if any) that minted the token. A PKCE token refreshed with the device client fails. tidalapi tracks `is_pkce` to pick credentials. Store the flow type with the token.
- Errors come as `{ "status":400|401, "error":"invalid_grant"|..., "sub_status":..., "error_description":"..." }`. A dead refresh token means full re-login. Don't loop.
- Refresh proactively (e.g. when < 5 min left) and reactively on 401 (below), then retry the request once.
- Other grants seen: `client_credentials` (Basic id:secret; catalog only, no user data, used by developer apps) and `update_client` (official SDK; exchange a refresh token for another client's token). Not needed.
- Logout: `POST https://api.tidal.com/v1/logout` with `Authorization: Bearer <access>` (tiddl, tidlers). [V]

### 1.5 `sessions` call (userId, countryCode) [V]
```
GET https://api.tidal.com/v1/sessions
Authorization: Bearer <access_token>
```
```json
{ "sessionId": "uuid", "userId": 12345678, "countryCode": "US", "channelId": 0, "partnerId": 0,
  "client": { "id": 8888, "name": "TIDAL_...", "authorizedForOffline": false, "authorizedForOfflineDate": null } }
```
- `countryCode` is the account's STORE country, not geo-IP; it must go on every v1 request as `countryCode`. Cache `userId`, `countryCode` next to the token. Re-fetch on login only.
- Legacy: older tidalapi also sent `sessionId` as a query param and `x-tidal-sessionid`; the current stack authenticates by Bearer only.

### 1.6 Official OAuth scopes (OpenAPI spec) [V]
`collection.read`, `collection.write`, `entitlements.read`, `playback`, `playlists.read`, `playlists.write`, `recommendations.read`, `search.read`, `search.write`, `user.read`
(tier THIRD_PARTY), plus `r_usr`, `w_usr` (tier INTERNAL; first-party and the clients above use `r_usr w_usr w_sub` where `w_sub` = write subscription).
Scopes are not enforced as hard 403s by the v1 API for these first-party tokens; they matter for the OpenAPI.

### 1.7 Subscription check
`GET https://api.tidal.com/v1/users/{userId}/subscription?countryCode=XX` (tidlers uses this as `check_login`). Fields [?, from memory]:
`startDate`, `validUntil`, `status` ("ACTIVE"), `subscription: { type: "PREMIUM"|"HIFI"|"HIFI_PLUS"|..., offlineGracePeriod }`, `highestSoundQuality`, `premiumAccess`, `canGetTrial`, `paymentType`.
mare-player warns `highestSoundQuality` is stale (MQA-era) and must not be trusted for the quality ceiling.

---------------------------------------------------------------------------------------------------

## 2. v1 endpoints

### 2.1 Common request rules [V]
- Base `https://api.tidal.com/v1/`. Header `Authorization: Bearer <access_token>`. Query `countryCode=<2-letter>` on essentially every call (users/* favorites too). Many also accept `locale=en_US` and `deviceType=BROWSER`.
- Paging: `limit` + `offset`. Response envelope:
```json
{ "limit": 50, "offset": 0, "totalNumberOfItems": 312, "items": [ ... ] }
```
- Max `limit` depends on endpoint: 100 for albums/items, playlists/items, artists/albums, mixes/items (tiddl tests 100); favorites lists up to 1000 (tidalapi default item_limit 1000, hard cap 10000); search up to 300 total (and offsets beyond 300 return nothing);
  v2 `my-collection/*` max 50 and CURSOR-based (response `cursor`; pass `cursor=` for next page; `offset` ignored).
- Items are sometimes wrapped: `{ "items": [ { "item": {..track..}, "type": "track", "created": "2024-..." } ] }` (favorites, playlist items, mix items, album items). `type` is lowercase `track` | `video` (also `album`/`artist`/`playlist` in mixed lists).
  Favorites wrap with `created` (date added). Playlist items additionally carry `dateAdded`, `index`, `itemUuid` inside `item`, plus `cut: null`.
- Videos appear mixed into album/playlist/mix lists; filter by `type == "track"` for an audio client.
- Header `x-tidal-client-version` (tidalapi sends `2025.7.16`, tidlers home feed `2026.1.5` for web / `2.171.1` for phone). Not required for v1 in practice [?], required-looking for v2 home feed. Send it.
- Default User-Agent used by tidalapi/tidlers: `Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36`.

### 2.2 Entity shapes (fields consumers rely on) [V: tiddl pydantic models, mare tests]
Track: `id, title, duration (s), ai (bool, TIDAL's AI label: true on tracks such as UDiO's, checked live 2026-10-18 in album items and search results), replayGain (dB), peak, allowStreaming, streamReady, adSupportedStreamReady, djReady, stemReady, streamStartDate, premiumStreamingOnly, trackNumber, volumeNumber, version, popularity, copyright, bpm, key, keyScale, url, isrc, editable, explicit, audioQuality ("LOW"|"HIGH"|"LOSSLESS"|"HI_RES_LOSSLESS"), audioModes (["STEREO","DOLBY_ATMOS"]), mediaMetadata.tags (["LOSSLESS","HIRES_LOSSLESS","DOLBY_ATMOS"]), artist {id,name,type,picture}, artists[], album {id,title,cover,vibrantColor,videoCover,releaseDate}, mixes {"TRACK_MIX":"<mixId>"}`.
Album: `id,title,duration,numberOfTracks,numberOfVideos,numberOfVolumes,releaseDate,type ("ALBUM"|"EP"|"SINGLE"),version,cover (uuid),vibrantColor,videoCover,explicit,upc,popularity,audioQuality,audioModes,mediaMetadata.tags,artist,artists[],streamReady,allowStreaming,copyright,url`. `cover` is null on some. The album's `artists[]` carry each artist's `picture` (uuid) and `handle`, `userId`: checked live 2026-10-19 in `pages/album` for Cole World: The Sideline Story (96622144), J. Cole with `picture` `5ac8f8f2-…`; a track's `artists[]` carry it too. TIDAL's album header draws that picture (20 points, round) before the names; Tidalfast keeps it (160 px) for an album's own artists only, not for every song's, and draws it in the album and Credits headers and the pinned bar. Before this it looked the artist up among artists read elsewhere and showed the placeholder for any other (The Weeknd's "House Of Balloons (Original)").
Album items (`albums/{id}/items`) carry each track's `volumeNumber` and `trackNumber`; verified live on the 2-disc album "The Fall-Off" (J. Cole): tracks come in volume order, `volumeNumber` 1 then 2, and Tidalfast lists each volume under its own heading (`numberOfVolumes` on the album is not needed). A track without `volumeNumber` counts for volume 1.
Artist: `id,name,type ("MAIN"|"FEATURED"),artistTypes,picture (uuid or null),popularity,artistRoles[{categoryId,category}],mixes {"ARTIST_MIX":"..","MASTER_ARTIST_MIX":".."},url,handle,selectedAlbumCoverFallback`.
Playlist: `uuid,title,description,numberOfTracks,numberOfVideos,duration,creator {id,(name)} (may be {} for editorial),type ("USER"|"EDITORIAL"|"ARTIST"|"PODCAST"|...),publicPlaylist,created,lastUpdated,lastItemAddedAt,image (uuid, wide),squareImage (uuid),promotedArtists[],popularity,url`. Own playlists: `creator.id == userId`.
Mix (v1, from pages): `id,title,subTitle,shortSubtitle,mixType,contentBehavior,images {SMALL|MEDIUM|LARGE: {url,width,height}},sharingImages,detailImages`. v2 shape: `mixImages[]`, `detailMixImages[]`, `titleTextInfo {text,color}`, `subtitleTextInfo`, `shortSubtitleTextInfo`, `type`, `updated` (ms), `isStableId`, `artifactIdType`, `contentBehavior`.

### 2.3 Catalog details
| What | Request | Notes |
|---|---|---|
| Track | `GET tracks/{id}?countryCode=` | |
| Track radio (tracks) | `GET tracks/{id}/radio?limit=100` | returns plain `items` of tracks (non-wrapped) |
| Track radio (mix id) | `GET tracks/{id}/mix` -> `{ "id": "<mixId>" }` | then `mixes/{mixId}/items`; 404 `subStatus 2001` "TrackMixId ... not found" when none. Also available as `track.mixes.TRACK_MIX` |
| Track credits | `GET tracks/{id}/credits?includeContributors=true` | |
| Lyrics | `GET tracks/{id}/lyrics?countryCode=` | section 2.7 |
| Album | `GET albums/{id}` ; items `GET albums/{id}/items?limit=100&offset=` (mixed track/video) | `albums/{id}/tracks` also exists (tracks only) |
| Album review | `GET albums/{id}/review` -> `{source, lastUpdated, summary, text}` | Live 2026-10-17 (signed-in test profile, J. Cole's 2014 Forest Hills Drive 209237067 and KOD 88460916): 200 with `source: "TiVo"`, `summary: ""` and `text` in the same `[wimpLink artistId=""]`/`albumId=""` markup as the artist bio (about 2,000 characters; a review can hold a stray `[` before a link, `[[wimpLink ...]`, which the bio parser keeps as text). 404 when TIDAL has none. Tidalfast shows it only in full on the Credits page's Info tab (since 1.0.2, not also under the album's songs) with "(Album review from TiVo)" under it, as TIDAL's web player does |
| Album credits | `GET albums/{id}/credits` ; `GET albums/{id}/items/credits?limit=100&offset=` | Live 2026-10-17 (same albums). `albums/{id}/credits` is a bare array of the release's roles, `[{type, contributors: [{name, id?}]}]` (2014 Forest Hills Drive: Administration, A&R, Art Direction, Executive Producer, Primary Artist, Record Label ...), the shape of `tracks/{id}/credits`. `items/credits` is the paged envelope `{items: [{type: "track", item: <track>, credits: [{type, contributors}]}], limit, offset, totalNumberOfItems}`: one entry per song with that song's roles (2014 Forest Hills Drive 13 songs, Intro with 13 roles; KOD 12 songs with only Composer and Lyricist). Role names come as TIDAL stores them, some as code names (`AdditionalProducer`, `Executive Producer`, `Mastering Engineer`); a contributor has an `id` only when TIDAL has an artist for the name. Each `item` is a full track (artists, `explicit`, numbers), which the Credits page's cards use for their artists line, E badge and menu. TIDAL's web player (checked 2026-10-19, Cole World: The Sideline Story 96622144) opens Credits as a page, `/album/{id}/credits`, and reads the release's roles and the review from `pages/album` (`ALBUM_HEADER` module: `album`, `review {text, source}`, `credits {items: [{type, contributors}]}`, the same data as `albums/{id}/credits` and `albums/{id}/review`); it lists the people of a role in alphabetical order (52 of 52 roles on that album), the endpoints in TIDAL's own. Tidalfast's Credits button, the album menu's Credits and the pinned bar's Credits open the same page (`src/ui/album_credits.rs`): the Credits tab's cards from `items/credits` (read 100 songs at a time by `album_credits`), the Info tab's review and "Additional Credits" from `albums/{id}/review` and `albums/{id}/credits`. The page's header has no Report button: no TIDAL report endpoint is known, so Tidalfast offers none |
| Artist | `GET artists/{id}` ; bio `artists/{id}/bio` -> `{source, lastUpdated, text}` (text contains `[wimpLink ...]` markup; 404 if none); `artists/{id}/links` | |
| Artist top tracks | `GET artists/{id}/toptracks?limit=&offset=` | sorted by popularity; the artist page's View all pages through it 100 at a time (`tidal:toptracks:{id}` queue context). **Capped, live-checked 2026-10-15** (signed-in test profile, `limit=100`): Eminem (17275) `totalNumberOfItems` 300, offsets 0/100/200 return 100 each, 300/400/500 return `items: []` with the same total; Rittz (4881899) total 220 (100, 100, 20). The total is the whole list; there is nothing past it. The artist page's Top Tracks module reports the same total (300, 220) |
| Artist albums | `GET artists/{id}/albums?filter=<F>&limit=100&offset=` | `filter` omitted (or `ALBUMS`) = albums; `EPSANDSINGLES`; `COMPILATIONS` = appears-on [V tidalapi + tiddl]. Live 2026-10-15: `filter=EPSINGLES` is **not** a filter (404, `subStatus` 2001); use `EPSANDSINGLES`. Eminem: albums 55 (1,239 tracks by `numberOfTracks`), EPs and singles 56 (71 tracks), compilations total 452 (first page of 100: 1,497 tracks). Rittz: albums 17 (247 tracks), EPs and singles 53 (55 tracks). One page of 100 held each list except compilations; offset past the total returns `items: []` |
| Artist followers / listeners | Live 2026-10-15 (signed-in test profile): the count is `data.attributes.numberOfFollowers` of `GET https://openapi.tidal.com/v2/artists/{id}?countryCode=` (JSON:API; same user token): Eminem 1,951,000, Rittz 19,118 (looks rounded for large counts). v1 `artists/{id}` has only `popularity` (0-100: 93, 27). The web v2 page `GET v2/artist/{id}?deviceType=BROWSER&platform=WEB` has `header.listeners`, `header.followersAmount` and `header.followingsAmount`, but all were `null` for both artists with that request; openapi's `popularity` is a 0-1 float (0.96, 0.62). No `artists/{id}/profile` or `profiles?artistId=` (404). `openapi .../artists/{id}/relationships/followers` lists follower ids (artists and users), not a count | |
| Artist videos | `GET artists/{id}/videos` | |
| Similar artists | `GET artists/{id}/similar?limit=50` | |
| Artist radio | `GET artists/{id}/radio?limit=100` (tracks) ; `GET artists/{id}/mix` -> `{id: mixId}` | |
| Artist page | `GET pages/artist?artistId={id}&deviceType=BROWSER` | page of modules (header w/ bio, top tracks, albums, etc.). Live 2026-10-15 (Eminem), module `type` and `title` in order: `ARTIST_HEADER`, `TRACK_LIST` "Top Tracks", `ALBUM_LIST` "Featured Albums", "EP & Singles", "Compilations", `MIXED_TYPES_LIST` "Playlists" (92), `VIDEO_LIST` "Videos", `ALBUM_LIST` "Appears On", `ITEM_LIST_WITH_ROLES` "Credits" (4 inline of 3,434; `showMore.apiPath` = `pages/contributor?artistId=`), `ARTIST_LIST` "Fans Also Like", "Influencers", `SOCIAL`, `ARTICLE_LIST` "Articles". Every list module carries `pagedList.dataApiPath` (`pages/data/<uuid>?artistId=`) and `showMore.apiPath` (`pages/single-module-page/...`). A smaller artist (Rittz) has no Compilations, Social or Articles modules |
| Mix tracks | `GET mixes/{mixId}/items?limit=100&offset=` | wrapped `{item, type}` |
| Album page shelves | `GET pages/album?albumId={id}&deviceType=BROWSER&locale=en_US` | The shelves under an album's footer come from this page, the same one TIDAL's web player reads. **Live-checked 2026-10-18** (The Fall-Off 496439179, 2014 Forest Hills Drive 209237067, 4 Your Eyez Only, Revenge Of The Dreamers III, Forest Hills Drive: Live): modules in order `ALBUM_HEADER`, `ALBUM_ITEMS` (the songs), then `ALBUM_LIST` "Other versions", `ALBUM_LIST` "More Albums by <artist>", `ALBUM_LIST` "Related Albums", `ARTIST_LIST` "Related Artists". Each list module carries its cards inline (`pagedList.items`, at most 20) and `showMore.apiPath` (`pages/single-module-page/<page>/<n>/<module>/1?albumId=` which answers one module with the full list: up to 50 artists, every version, all albums by the artist). The module uuids are the same on every album (`bbb3ef50…` versions, `46ef43c4…` more by, `a8449d6d…` related albums, `13f97524…` related artists in `pages/data/<uuid>`). Only the "More Albums by" module carries `artistId=` in its paths; an album by two main artists (Revenge Of The Dreamers III: Dreamville, J. Cole) has one such module for each. A module with nothing to show is absent, not empty: Forest Hills Drive: Live has no "Other versions". Titles come back in English whatever the app's language, so Tidalfast titles the shelves itself. The review is not in this page (`albums/{id}/review`). `albums/{id}/similar` answers "no radio" and is not used. Tidalfast loads the page once when the footer is within a screen of the view, keeps it with the album page, and draws a shelf only when it has cards |
| Mix as page | `GET pages/mix?mixId={id}&deviceType=BROWSER` | module 0 = `MIX_HEADER` (key `mix`), module 1 = track list |
| Playlist | `GET playlists/{uuid}` ; items `GET playlists/{uuid}/items?limit=100&offset=&order=INDEX&orderDirection=ASC` ; `playlists/{uuid}/tracks` (tracks only) | **`ETag` response header** on items/tracks (section 2.5) |
| Playlist recs | `GET playlists/{uuid}/recommendations/items?limit=&offset=` | Live 2026-10-17 (signed-in test profile, two of the account's own playlists, of 63 and 20 songs): 200 with `{limit, offset, totalNumberOfItems: 100, items: [{type: "track", item: <track>}]}`, the wrapped shape of a playlist's items without `created`. 100 songs for a 63-song playlist and 95 for a 20-song one, so TIDAL offers about a hundred and Tidalfast pages through them five at a time with Refresh. Songs by the playlist's artists, not necessarily absent from the playlist. The playlist page shows them (`src/ui/album_extras.rs`) only for a playlist the user can edit, and the Add button writes through the ordinary add-to-playlist call |
| Video credits | Live 2026-10-15 (video 441699021): `GET videos/{id}/credits` answers 200 with a bare `[]`, and `videos/{id}/contributors?limit=50` 200 with `{items: [], totalNumberOfItems: 0}`; TIDAL keeps no credits for that video, so neither shape is confirmed with data | |
| Video | `GET videos/{id}`; playback `videos/{id}/playbackinfopostpaywall?videoquality=HIGH&playbackmode=STREAM&assetpresentation=FULL` | HLS (m3u8), mime `application/vnd.tidal.emu` or `dash+xml`; see section 3.1 (Videos) for what Tidalfast plays |
| ISRC lookup | official: `GET https://openapi.tidal.com/v2/tracks?countryCode=&filter[isrc]=` | JSON:API |

Sort params for items lists: `order` in `INDEX|DATE|NAME|ARTIST|ALBUM|LENGTH`, `orderDirection` in `ASC|DESC`.

### 2.4 Favorites (v1: `users/{userId}/favorites/...`) [V]
List (all use `countryCode`, `limit`, `offset`; envelope per 2.1, each item `{created, item:{...}}`):
- `GET users/{id}/favorites/tracks` ; `.../albums` ; `.../artists` ; `.../videos`
- Ordering: `order` + `orderDirection`.
  - tracks: `order` in `DATE|NAME|ARTIST|ALBUM|LENGTH` (tidalapi ItemOrder also lists INDEX)
  - albums: `DATE|NAME|ARTIST|RELEASE_DATE`
  - artists: `DATE|NAME`
  - videos: `DATE|NAME|ARTIST`
  - Default is date added; `orderDirection=DESC` gives newest first. [V]
- IDs only (cheap "is it favorited" cache; call once at start): `GET users/{id}/favorites/ids?countryCode=` ->
  `{ "PLAYLIST":["uuid",..], "ALBUM":["id",..], "VIDEO":[..], "TRACK":["id",..], "ARTIST":["id",..] }` (strings). [V tiddl, tidlers]
- Favorite playlists (v1 combined): `GET users/{id}/playlistsAndFavoritePlaylists?limit=50&offset=` -> items `{created, playlist:{...}, type:"USER_CREATED"|"FAVORITE"...}` [?]; limit 50. Superseded by v2 below.
- Favorite playlists, folders (v2): `GET https://api.tidal.com/v2/my-collection/playlists/folders?folderId=root&limit=50&includeOnly=PLAYLIST&order=DATE&orderDirection=DESC[&cursor=...]` (includeOnly `FOLDER` lists folders; empty includeOnly lists both). Response: `{ "items":[ {name,id/trn,itemType:"PLAYLIST"|"FOLDER", data:{...playlist...}, addedAt, lastModifiedAt,...} ], "cursor":"...", "totalNumberOfItems": n }`. Includes own and followed playlists. `order` `DATE|NAME`.
- Favorite mixes (v2): `GET https://api.tidal.com/v2/favorites/mixes?limit=50&offset=&order=DATE|NAME|MIX_TYPE&orderDirection=` -> items are v2 mixes (with `dateAdded`).
- Favorite artists (v2, folders; used by tidlers): `GET https://api.tidal.com/v2/my-collection/artists/folders?folderId=root&limit=&order=DATE&cursor=&countryCode=&locale=`.

Add (POST, form body, `countryCode` as query):
- `POST users/{id}/favorites/tracks`  body `trackIds=1,2,3` (tidlers) [tidalapi still sends singular `trackId=`; plural is newer, try plural first, fall back] 
- `POST users/{id}/favorites/albums`  body `albumIds=` (tidlers) / `albumId=` (tidalapi)
- `POST users/{id}/favorites/artists` body `artistIds=` (tidlers) / `artistId=` (tidalapi)
- `POST users/{id}/favorites/videos`  form `videoIds=<id>` -> 200, empty body [V live 2026-10-18 on the test account: the video was in `favorites/videos` (total 0 -> 1) and in `favorites/ids` `VIDEO` straight away, and was gone again after the DELETE below; the singular `videoId` was not needed]. Tidalfast saves and removes videos this way, and reads whether one is saved from `favorites/ids` `VIDEO` like the other kinds. TIDAL's web player uses the JSON:API form instead (`openapi.tidal.com/v2/userCollectionVideos/me/relationships/items`, POST and DELETE); not needed.
- Add favorite playlist (v2): `PUT https://api.tidal.com/v2/my-collection/playlists/folders/add-favorites?folderId=root&uuids=<uuid,uuid>`
- Add favorite mixes (v2): `PUT https://api.tidal.com/v2/favorites/mixes/add?mixIds=a,b&onArtifactNotFound=FAIL` ; remove: `.../favorites/mixes/remove?...`; response includes `addedItems` / `deletedItems`.
Remove: 
- `DELETE users/{id}/favorites/tracks/{trackId}` ; `.../albums/{albumId}` ; `.../artists/{artistId}` ; `.../videos/{videoId}` (live 2026-10-18: 200, empty body; tidlers also repeats the id as query param; not required by tidalapi).
- Remove favorite playlist or folder (v2): `PUT https://api.tidal.com/v2/my-collection/playlists/folders/remove?trns=trn:playlist:<uuid>,trn:folder:<id>`. (Note PUT, not DELETE.)
All adds return 200/201 with `{ "lastUpdated": <ms>, "etag"...}`-style bodies [?]; treat any 2xx as success. Adding an already-favorite item is a no-op success.

Blocked songs, TIDAL's "Don't play this" for mixes and radios (v1, live 2026-10-17 on a Max account):
- `GET users/{id}/blocks/tracks?limit=&offset=` -> `{items:[{created, item:{track}, type:"TRACK"}], totalNumberOfItems}`; `.../blocks/artists` has the same shape with artists. `blocks/items`, `blocks` and v2 `blocks` do not exist (404).
- `POST users/{id}/blocks/tracks` form `trackId=<id>` -> 200, empty body. The plural `trackIds` answers 404 (unlike favourites). Blocking an id that does not exist answers 404.
- `DELETE users/{id}/blocks/tracks/{trackId}` -> 204 (404 when the song is not blocked); `GET` on that path is 405.
Blocked artists (live 2026-10-18, same account): `GET users/{id}/blocks/artists?limit=&offset=` lists them (`items[].item.{id,name}`); `POST users/{id}/blocks/artists` form `artistId=<id>` blocks one (200, empty body); `DELETE users/{id}/blocks/artists/{artistId}` lifts it (204; a 404 means it was not blocked and counts as done). Checked on one artist nobody had blocked, blocked and then unblocked: the list was the same before and after.
Tidalfast posts "Don't play this" from the player bar (shown while a mix or radio plays), blocks and unblocks artists from the artist menus and the artist page's More menu, and lists and unblocks artists and songs in Settings > Blocked. It reads both lists once per session (and each time Settings > Blocked opens) and its own queue leaves what they name out (`queue.md`, rule 13).

### 2.5 User playlists (v1 + v2 folder API) [V]
- Own playlists list (v1): `GET users/{id}/playlists?limit=50&offset=` (tidlers `list_playlists`) -> items are playlist objects directly (not wrapped). Public subset: `GET https://api.tidal.com/v2/user-playlists/{id}/public?limit=&offset=` -> items `{playlist, followInfo, profile}`.
- Create (v2): `PUT https://api.tidal.com/v2/my-collection/playlists/folders/create-playlist?name=<title>&description=<desc>&folderId=root&isPublic=false` -> `{ "data": { "uuid": "...", ...playlist }, "trn": ... }` (tidalapi reads `.data.uuid`; tidlers returns a `CollectionPlaylistEntry`).
- Create folder: `PUT .../my-collection/playlists/folders/create-folder?name=&folderId=root`. Move/rename of folders: tidalapi `Folder.rename` = `PUT .../my-collection/playlists/folders/rename?trn=trn:folder:<id>&name=`; move playlists: `.../move?trns=...&folderId=` [?].
- **Playlist folders (v2, `https://api.tidal.com/v2/my-collection/playlists/folders`)**. Sources: python-tidal/tidalapi (`Folder`, `Favorites.playlist_folders`, `create_folder`, `move_items_to_folder`, `rename`, `remove`) and other open clients; every endpoint below is [?] (from those sources, not exercised against a live account by Tidalfast's tests, which run against mocks only).
  - List a level: `GET .../folders?folderId=<root|id>&limit=50&order=DATE&orderDirection=DESC[&cursor=]` (live 2026-10-15: 200, top-level keys `items`, `lastModifiedAt`, `totalNumberOfItems`, no `cursor` key when everything fits; each item `{addedAt, data, itemType, ...}` with `itemType` `PLAYLIST` and `data` the full v2 playlist object; the account checked had no folders, so the `FOLDER` item shape is still unconfirmed). Items carry `itemType` `FOLDER` or `PLAYLIST` and `data` (folder: `id`, `name`, `trn` = `trn:folder:<id>`, `totalNumberOfItems`; playlist: the playlist object, `uuid`). `includeOnly=FOLDER|PLAYLIST` narrows it. Pages by opaque `cursor`. The tree is read one `folderId` at a time (root first, then each folder); there is also a flattened listing of every playlist regardless of folder, which is what `playlistsAndFavoritePlaylists` already gives Tidalfast, so the flat playlist list stays the source of the rows and the folders only group them.
  - Create: `PUT .../folders/create-folder?name=&folderId=root`. Rename: `PUT .../folders/rename?trn=trn:folder:<id>&name=`. Delete: `PUT .../folders/remove?trns=trn:folder:<id>` (the same call that un-follows a playlist). Tidalfast first moves the folder's playlists to the top level, so deleting a folder never removes a playlist.
  - Move playlists in or out: `PUT .../folders/move?folderId=<id|root>&trns=trn:playlist:<uuid>[,...]`.
  - Sidebar behaviour: folders and their playlists are shown at once after an edit; a refused write toasts and the folders are read again, which restores the shown state.
- Rename / edit description (v1): `POST playlists/{uuid}` form `title=...&description=...` (both fields sent; tidalapi fills the unchanged one from the current value). No ETag required [V tidalapi `edit`].
- Make public/private (v2): `PUT https://api.tidal.com/v2/playlists/{uuid}/set-public` / `set-private`. [?] (tidalapi `set_public`/`set_private`, not exercised live; Tidalfast's tests run against mocks). Tidalfast's header Private/Public button (closed lock = private, open lock = public) and the ... menu's Make public/private send only this call, show the new state at once, toast "Playlist is now private/public", and put the old state back with an error toast if TIDAL refuses. The current state is the v1 playlist's `publicPlaylist`.
- Delete playlist: `DELETE playlists/{uuid}` (v1, tidalapi) ; or the collection remove call above with `trn:playlist:<uuid>`.
- **Optimistic concurrency (ETag)**: read items first: `GET playlists/{uuid}/items?...` and take the `ETag` response header. Send it back on every mutating item call as **`If-None-Match: <etag>`** (yes, If-None-Match, not If-Match). Without it, or if stale, mutation is rejected (412 `Precondition Failed`) [V tidalapi + tidlers]. After a mutation, re-fetch to get a new ETag; do not reuse.
  - Add: `POST playlists/{uuid}/items` form `trackIds=1,2` (tidalapi) / `itemIds=1,2` (tidlers; also covers videos) `&toIndex=<pos>&onArtifactNotFound=SKIP&onDupes=SKIP|ADD`; optional query `limit`. `toIndex` 0 = top; use track count for append (tidalapi uses count). Response `{ "lastUpdated": ..., "addedItemIds": [..] }`.
  - Add a video (what the web player does; live 2026-10-18 on the test account): `POST https://openapi.tidal.com/v2/playlists/{uuid}/relationships/items` (JSON:API, no ETag) body `{"data":[{"id":"441699021","type":"videos"}],"meta":{"onDuplicates":"ADD"}}` -> 200 `{"data":[{"id","type":"videos","meta":{"addedAt","itemId"}}],"meta":{"skipped":[]}}`; `type` is `tracks` for a song. The playlist's `numberOfVideos` then counts it. Remove: `DELETE` the same URL with `{"data":[{"id","type":"videos","meta":{"itemId":"<itemId from the add>"}}]}` -> 200. `GET` of the same URL with `include=items` lists `{id, type}` entries. Tidalfast uses it for videos only (the v1 form was not tried with a video id). **Caveat, same day: `GET playlists/{uuid}/items` (v1, on `api.tidal.com` and on `tidal.com`, with or without `countryCode`, `locale`, `deviceType`, `replace`, `order`) answered 500 whenever the page it returned contained a video (two different videos tried), and 200 again once the videos were removed; a read of `limit=1` answered 200 while item 0 was a song; the web player shows the same "Server error" on such a playlist.** The openapi `relationships/items` read (20 per page, `page[cursor]`) still lists songs and videos. Because Tidalfast's v1 edit starts with a `limit=1` read, a song add to a playlist whose first item is a video fails too; for an append, Tidalfast then adds the song through the relationship (`type` `tracks`) instead. **Reading such a playlist (live 2026-10-18, test playlist of song, video, song):** v1 `items` of `limit=100` and of `limit=1&offset=1` (the video alone) answered 500 `{"status":500,"subStatus":999,"userMessage":"Internal Server Error"}`, while `limit=1&offset=0` (a song) answered 200 with the ETag `\"1792309962540\"`, and `GET playlists/{uuid}` answered 200 with the same ETag, `numberOfTracks` 2 and `numberOfVideos` 1 (a video is not counted in `numberOfTracks`). `GET https://openapi.tidal.com/v2/playlists/{uuid}/relationships/items?include=items,items.artists,items.albums,items.albums.coverArt,items.thumbnailArt` answered 200: `data` `[{id, type: "tracks"|"videos", meta: {itemId, addedAt}}]` in playlist order, and `included` with each song and video `attributes` (`title`, `version`, `duration` as ISO 8601 `PT3M49S`, `explicit`; a video also `releaseDate` [?, not looked at in this check]), their `artists` (`attributes.name`), a song's `albums` (`attributes.title`) with `coverArt` and a video's `thumbnailArt` (`artworks`, `attributes.files[].href` ending `.../images/aaaaaaaa/bbbb/cccc/dddd/eeeeeeeeeeee/1280x1280.jpg`, the five groups are the image uuid), `links.self` only (no `next` with 3 items). Tidalfast, when a v1 items read answers 5xx, walks that relationship, builds v1-shaped entries (`{item, type: "track"|"video", created}`) and serves the page from them, so the page, playing the playlist and Luna plugins' `playlists/{uuid}/items` reads (answered 200 in v1's shape) all work; a video is a row with `tidal:video:<id>` (`Video::as_track`), and downloads leave it out. The list is kept 30 s (and v1 is not tried again for that playlist for 10 min) so paging does not repeat the walk; an add, remove or reorder forgets it. Edits of a playlist v1 cannot list take the ETag and length from `GET playlists/{uuid}` (`numberOfTracks` + `numberOfVideos`). [?] Not verified live (the account's allowed writes were add-only): that a v1 `DELETE`/`POST items/{index}` with that ETag succeeds on such a playlist and that its indexes count the videos; that the relationship pages by `links.next` `page[cursor]` (20 per page) when a playlist has more than 20 entries (read with the same code as Save for Later, whose second page was never seen either); a v1 `POST trackIds` (add at a position) on a playlist holding a video. The Luna `v1_call` form of the plugin read (status 200, no ETag) is covered by a unit test only; the `v1_json` form (`host.playlist_items`) was run live through the same code (the page of the test playlist came back with the video as a `video` entry). `DELETE https://api.tidal.com/v1/playlists/{uuid}` answered 204, and the playlist list was back to its earlier playlists.
  - Merge another playlist: same POST with `fromPlaylistUuid=<uuid>&onArtifactNotFound=SKIP&onDupes=SKIP`.
  - Move: `POST playlists/{uuid}/items/{index[,index...]}` form `toIndex=<newPos>`. (Indices are positions within the CURRENT order, not ids.)
  - Remove: `DELETE playlists/{uuid}/items/{index[,index...]}` with `order=INDEX&orderDirection=ASC` query (tidlers) and `If-None-Match`. Remove from the highest index down or in one comma list; indices shift after each call.
  - Items of one playlist can only be fetched 100 at a time; clear/remove in chunks of <= 50 indices (tidalapi `clear`).
- Duplicates: playlist items carry `itemUuid` (stable) and `index`. Use `index` for mutations.

### 2.6 Search [V]
v1: `GET search?query=<q>&types=ARTISTS,ALBUMS,TRACKS,VIDEOS,PLAYLISTS&limit=50&offset=0&countryCode=XX` (tidalapi; `types` omitted searches everything; max `limit` 300).
```json
{ "artists":   {"limit":..,"offset":..,"totalNumberOfItems":..,"items":[Artist..]},
  "albums":    {... items:[Album..]}, "playlists": {...}, "tracks": {...}, "videos": {...},
  "topHit": { "type": "ARTISTS"|"ALBUMS"|"TRACKS"|"PLAYLISTS"|"VIDEOS", "value": { ...entity... } } }   // topHit may be null
```
- Search artist objects often lack `type`; search albums may lack `artist`. Make those optional (tiddl had to).
- Extra v1 params: `includeContributors=true`, `includeDidYouMean=true`, `includeUserPlaylists=true`, `supportsUserData=true`.
- Web v2 variant (tidlers): `GET https://tidal.com/v2/search/?query=&types=albums,artists,playlists,topHits,tracks,videos&limit=&offset=&countryCode=&locale=` returns `{ ..., "topHits":[...], "contentTypeFilters":[...] }` in a new shape (modules of typed items); v1 shape is simpler and still works.
- Autocomplete: `GET https://tidal.com/v2/suggestions/?query=&explicit=true&hybrid=true&countryCode=` (tidlers). [S]
- Recent searches (live 2026-10-16 on a test account, GET only; the account had one saved query, `example query`). TIDAL's Recent Searches card (opened artists, albums and playlists with artwork) has **no endpoint Tidalfast can read or write**, so Tidalfast keeps that list in `settings.json` (`recent_items`, see `src/recent_searches.rs`). What exists on the account is a list of typed queries, without items or artwork [V]:
  - `GET https://tidal.com/v2/suggestions/?query=<text>&explicit=true&hybrid=true&countryCode=XX` answers with `history: [{query, highlights}]`, the saved queries that begin with the text (`query` percent-encoded, `example%20query`). An empty `query` returns `history: []` and `suggestions: []`, so the card is not fed from here.
  - Official OpenAPI (INTERNAL tier by the spec, yet the session token read it, 200): `GET https://openapi.tidal.com/v2/searchSuggestions?filter[query]=a&countryCode=XX&include=history` gives `relationships.history.data: [{id, type: "searchHistoryEntries"}]`; `GET .../searchSuggestions/{id}/relationships/history?countryCode=XX&include=history` lists them with `included[].attributes.query` (opaque entry `id`, percent-encoded query). `DELETE https://openapi.tidal.com/v2/searchHistoryEntries/{id}` deletes one (scopes `search.write`, `w_usr`; not tried, it would change the account). The spec has no create call and no `GET /searchHistoryEntries`.
  - 404/"could not find this" for `v2 search/history`, `search/recent`, `search/recents`, `search-history`, `searchhistory`, `recent-searches`, `recentsearches`, `suggestions/history`, `suggestions/recent`, `search/queries`, `users/{id}/search/history`, `profiles/{id}/search-history`, `user-search-history`, `openapi searchHistoryEntries`; and for `v1 search/history`, `searchhistory`, `users/{id}/searchhistory`, `search-history`, `recentsearches`, `search/history` ("TIDAL has no radio for this").
  - Not found: how TIDAL's own apps add an opened item (the web player sits behind a bot check; no write was tried on a live account). If that call is ever captured, the list can sync. Until then it is local only.

### 2.7 Lyrics [V]
`GET tracks/{id}/lyrics?countryCode=XX` -> 200
```json
{ "trackId": 123, "lyricsProvider": "Musixmatch", "providerCommontrackId": "12345", "providerLyricsId": "678",
  "lyrics": "plain text\nwith newlines", "subtitles": "[00:05.67]First line\n[00:09.10]Second line", "isRightToLeft": false }
```
- `subtitles` is LRC with `[mm:ss.xx]` stamps (two-digit hundredths); it can be absent/empty when only plain `lyrics` exist, and `lyrics` can be empty when only subtitles exist (mare handles both; treat all provider/id fields as optional).
  Lines are not guaranteed sorted, and multi-stamp lines like `[01:02.34][02:18.56]Same hook` occur (expand them, then sort). Tolerate `[mm:ss.xxx]`.
- 404 = no lyrics; do not retry. Rights-restricted lyrics may return 401/403 [?].
- Official API equivalent: `GET https://openapi.tidal.com/v2/tracks/{id}/relationships/lyrics` (JSON:API, `include=lyrics`).

### 2.8 Pages (home / explore / mixes) [V]
Legacy page endpoints: `GET pages/<slug>?countryCode=XX&deviceType=BROWSER[&locale=en_US]`.
Known slugs: `home`, `explore`, `for_you`, `hires`, `videos`, `genre_page`, `genre_page_local`, `moods`, `my_collection_my_mixes`, `artist` (+`artistId`), `mix` (+`mixId`), `album` (+`albumId`), genre/mood sub-pages via `PAGE_LINKS` (`apiPath` like `pages/genre_rnb`; normalize leading `/` and `v1/` prefix before calling).
`pages/home` is the legacy home; the modern one is the v2 feed (2.9). Both documented.

Page JSON:
```json
{ "selfLink": null, "id": "...", "title": "Explore",
  "rows": [ { "modules": [ { "id": "...", "type": "ALBUM_LIST", "title": "New Albums", "description": "", "width": 100,
        "preTitle": null, "layout": null, "listFormat": "HORIZONTAL",
        "showMore": { "title": "View all", "apiPath": "pages/data/<uuid>" },      // or "viewAll": "pages/..."
        "pagedList": { "limit": 50, "offset": 0, "totalNumberOfItems": 20, "items": [ ...entities... ],
                       "dataApiPath": "pages/data/<uuid>" },
        "items": [ ... ]            // only for FEATURED_PROMOTIONS
  } ] } ] }
```
- Module `type` values seen: `FEATURED_PROMOTIONS`, `MULTIPLE_TOP_PROMOTIONS`, `PAGE_LINKS`, `PAGE_LINKS_CLOUD`, `ALBUM_LIST`, `ARTIST_LIST`, `TRACK_LIST`, `PLAYLIST_LIST`, `VIDEO_LIST`, `MIX_LIST`, `MIXED_TYPES_LIST` (items `{type, item}`), `HIGHLIGHT_MODULE` (`highlights[].item`), `ITEM_LIST_WITH_ROLES` (items `{item, roles}`), `ALBUM_ITEMS`, `MIX_HEADER` (`mix`), `ARTIST_HEADER` (`artist`,`bio`), `ALBUM_HEADER` (`album`), `TEXT_BLOCK`, `ARTICLE_LIST`, `SOCIAL` (`socialProfiles`). Unknown types must be skipped, not errors (tidalapi throws NotImplementedError; do better).
- Lazy modules: a list module can arrive with `pagedList.items: []` and its list at `pagedList.dataApiPath` (else `showMore.apiPath`). Fetch that path (same params) for the cards; dropping such modules loses whole shelves. Tidalfast shows them as pending shelves and fills them a few at a time.
- "View all" / next page: `GET <apiPath>` with `countryCode`/`deviceType` (paths like `pages/data/<uuid>?...`); returns another page.
- Promotion cards (`FEATURED_PROMOTIONS`): `{ header, shortHeader, shortSubHeader, imageId, artifactId, type: "PLAYLIST"|"ALBUM"|"ARTIST"|"MIX"|"CATEGORY_PAGES"|"PAGE"|"EXTURL"|"VIDEO", ... }`. Promo `imageId` image is 550x400 only (square sizes 403) - see section 4.
- Playlist items inside pages use `squareImage`/`image` UUIDs; mix items use the v1 mix shape or the v2 `mixImages` shape depending on module (handle both).
- `pages/my_collection_my_mixes` -> a MIX_LIST (the user's My Mix 1..N, Daily/Discovery/New Arrivals, artist mixes) plus favorite mixes.
- Mix id is also carried on albums/tracks/artists as `mixes: {...}`.

### 2.9 v2 home feed and other v2/openapi calls still in use
- Home (web feed): `GET https://tidal.com/v2/home/feed/static?countryCode=XX&locale=en_US&limit=20&deviceType=BROWSER&platform=WEB&timeOffset=%2B01%3A00` + header `x-tidal-client-version`. Paging by `page.cursor`. (tidalapi uses api.tidal.com/v2 with `deviceType=BROWSER&platform=WEB&locale=`.) `timeOffset` is the local UTC offset like `+02:00` (tidlers: chrono `%:z`).
```json
{ "uuid": "...", "page": { "cursor": null }, "header": { "vibes": { "items": [] } },
  "items": [ { "type": "SHORTCUT_LIST"|"HORIZONTAL_LIST"|"HORIZONTAL_LIST_WITH_CONTEXT"|"VERTICAL_LIST_CARD"|"TRACK_LIST",
               "moduleId": "...", "title": "...", "subtitle": null, "icons": [], "showQuickPlay": true, "viewAll": "home/pages/<module>"|null,
               "header": {..only WITH_CONTEXT..},
               "items": [ { "type": "MIX"|"ALBUM"|"PLAYLIST"|"ARTIST"|"TRACK"|"DEEP_LINK", "following": false, "data": { ...entity... } } ] } ] }
```
  python-tidal's `Session.home()` reads this feed by default (`api.tidal.com/v2/home/feed/static`, `deviceType=BROWSER&platform=WEB&locale=`), with `pages/home` only as `use_legacy_endpoint`; Tidalfast does the same and falls back to `pages/home` when the feed fails or is empty. Set `TIDALFAST_DUMP_HOME=1` to save the raw v2/v1 answers and per-read timings to `home-dump.json` in the state directory (no tokens).
  `data` is an entity (playlist: `uuid,title,squareImage,trn,...`; mix: v2 mix shape; track: track). The `phone` variant (`deviceType=PHONE&platform=ANDROID`) has `page.cursor` mandatory. A client should model items as a tagged union with a catch-all Unknown. `viewAll` paths resolve under `https://tidal.com/v2/` or `api.tidal.com/v2/` (e.g. `home/pages/...`) [?].
- Activity feed (the web player's Feed page, observed 2026-10-14): `GET https://tidal.com/v2/feed/activities?userId=<id>&locale=en_US&deviceType=BROWSER&countryCode=XX` (the sidebar first asks with `limit=9`). Answer:
  `{ "activities": [ { "followableActivity": { "activityType": "NEW_ALBUM_RELEASE" | "NEW_HISTORY_MIX", "occurredAt": "2026-10-01T00:00:00.000Z", "album": {..v1 album, type ALBUM|EP|SINGLE, explicit, videoCover..} | "historyMix": {..v1 mix: title "September 2026", subTitle "A, B, C and more", images SMALL/MEDIUM/LARGE (ready-made URLs)..} }, "seen": true } ], "stats": { "totalNotSeenActivities": 0 } }`.
  The page groups by `occurredAt` month: This month / Last month / Older. Tidalfast: `TidalClient::feed`, `api/tidal/feed.rs`. `GET .../v2/my-activity/timeline`, `.../my-activity/top-artists?year=&month=` (listening stats). [S]
- Home's `SHORTCUT_LIST` module (observed live, 2026-10-17, `home/feed/static`, first module): `moduleId "SHORTCUT_LIST"`, `title "Shortcuts"`, `icons []`, no `viewAll`, six `{type, data}` items of mixed kinds: `PLAYLIST` (the account's own playlists: `uuid`, `title`, `squareImage`, `creator`, `numberOfTracks`), `ARTIST` (followed artists: `id`, `name`, `picture`, `popularity`, `vibrantColor`). On the observed account: playlist, artist, artist, playlist, artist, artist; mixes, albums and tracks are not seen there but the tile code accepts them. TIDAL's client draws this module only as the compact tile grid at the top of Home (small cover, title, a line under it, three columns; artists with a round cover and the line "Artist"), never as a shelf and never with a "Shortcuts" heading. Tidalfast keeps the module with `ShelfLayout::Shortcuts`, draws it as the tile grid and skips it in the shelves; without one (feed failed or no such module) the grid shows the library's six most recent playlists, and while the feed is loading it is empty so the tiles never swap.
- Home's tabs (observed): For you = `v2/home/feed/static`, Staff Picks = `v2/home/feed/editorial`, Uploads = `v2/home/feed/uploads`, same query as static; `header.vibes.items` lists them (`{name, type: STATIC|EDITORIAL|UPLOADS}`). Module types beyond 2.9: `COMPACT_GRID_CARD` (songs in a 3-column grid, optional `subtitle`, `icons: ["SPOTLIGHT_INFO"]` for the (i)), `HORIZONTAL_LIST` of `MAGAZINE` items ("Featured": a promotion `{header (kicker, e.g. LISTEN / UPLOAD HEADLINERS), shortHeader (title), shortSubHeader (line), imageId (550x400), type, artifactId}`). Upload entities carry `upload: true`, `spotlighted`, and `payToStream` + `priceCode` (sold, "Uploads for purchase"). `viewAll` paths like `home/pages/UPLOADS_FOR_YOU/view-all`.
- Search as typed (observed): `GET https://tidal.com/v2/suggestions/?query=<q>&explicit=true&hybrid=true&countryCode=XX` -> `{ history: [], suggestions: [{query, highlights:[{start,length}]}], directHits: [{type: ARTISTS|TRACKS|ALBUMS|PLAYLISTS, value: {..v1 entity..}}], suggestionUuid }`.
- Search results page (observed): `GET https://tidal.com/v2/search/?query=<q>&types=ARTISTS,ALBUMS,TRACKS,VIDEOS,PLAYLISTS,UPLOADS&limit=50&includeContributors=true&includeDidYouMean=true&includeUserPlaylists=true&supportsUserData=true&locale=en_US&deviceType=BROWSER&countryCode=XX` -> `{ artists, albums, tracks, videos, playlists, uploads: {items, totalNumberOfItems}, topHits: [{type, value, tabPosition}] (ranked, up to 100), genres: [{title, apiPath: "pages/genre_hip_hop"}] ("Go to genre"), contentTypeFilters, queryId }`.
- Explore's top links (`pages/explore`, a `PAGE_LINKS` module): New = `pages/explore_new_music` (PLAYLIST_LIST New Arrivals, TRACK_LIST New Tracks, ALBUM_LIST New Albums, VIDEO_LIST New Music Videos), Top = `pages/explore_top_music`, Videos = `pages/videos` (MULTIPLE_TOP_PROMOTIONS "Featured" with `header` "WATCH" and `type: VIDEO`, PLAYLIST_LIST video playlists with `numberOfVideos`, VIDEO_LIST (video: `imageId` 16:9, `duration`, `type: "Music Video"`), MIX_LIST `VIDEO_DAILY_MIX` "My Video Mix N").
- Profiles: `GET https://api.tidal.com/v2/profiles/{userId}` -> `name, handle, ...`; `GET api/v1/users/{id}` -> `firstName,lastName,picture`.
- Playlist creator names: a playlist object carries only `creator.id` (numeric user id; `{}` or `0` for editorial lists, shown as "TIDAL"), so "Created by <name>" needs a lookup. Tidalfast asks `GET https://api.tidal.com/v2/profiles/{userId}` and reads `name` (a name equal to the id counts as none). Live 2026-10-15 (own account): 200 with `{userId, name, handle, picture, color, profileType ("MY_PROFILE" for one's own), artistId, numberOfFollowers, numberOfFollows, prompts, socialLinks}`; `name` is the display name, `handle` may be null. Inside a folder listing a playlist's `creator` is `{id, name: null, picture: null, type: "USER"}`, so the lookup is still needed there; up to 6 profiles at once, only for creators drawn on screen. 403 and 404 (private or deleted profile) are treated as "no public profile" and the playlist reads as a plain "Playlist" [unverified: TIDAL's own fallback text and whether 403 or 404 is returned for a private profile]; other failures are retried and never cached. Answers are kept in `user-names.json` (cache dir) for 7 days (names) or 1 day (no profile). tidalapi and tiddl expose no creator-name lookup; openapi.tidal.com `userPublicProfiles/{id}` is an alternative [unverified, not used]. The raw id is never shown.
- Save for Later (My Collection > Save for Later; observed in the web player and checked live 2026-10-18 on the signed-in test profile, same session token as the rest of v2, no extra scope): JSON:API on `https://openapi.tidal.com/v2/userCollectionSaveForLaters/me/relationships/items`.
  - List: `GET ...?include=items` answers `{data: [{id, type}], included: [], links: {self, next?}}` (`countryCode` is added to `links.self`); empty account: `data: []`. Types: `albums`, `tracks`, `artists`, `playlists`, `videos` (mixes cannot be saved). `included` carries no artwork, so Tidalfast reads each row from the v1 catalogue (`albums/{id}` and so on). Paging: Tidalfast follows `links.next`'s `page[cursor]` [unverified: no account has had a second page].
  - Add: `POST` the same path with `Content-Type: application/vnd.api+json` and body `{"data":[{"id":"<id>","type":"tracks"}]}`; 2xx. Verified live with one song (id `type` `tracks`): the list then held it. Remove: `DELETE` the same path with the same body shape (`type: "videos"` was captured from the web player); verified live, the list was empty again. Albums, artists, playlists and videos use the same body [unverified on the wire, same resource shape].
  - Tidalfast: `api/tidal/save_later.rs`; the list is read once at sign-in and again when the page opens, the menus read the in-memory set of saved ids.
- Purchases (My Collection > Purchases): `GET https://tidal.com/v2/marketplace/v1/user-purchases`, also served at `https://api.tidal.com/v2/marketplace/v1/user-purchases` (the host Tidalfast's v2 calls use), session token accepted. Answer `{"albums": [], "tracks": []}` for an account with no purchases (checked 2026-10-18). A listed entry has never been seen; Tidalfast reads one as a v1 album or track (bare, or under `item`/`album`/`track`) [unverified] and says so when an entry cannot be read, rather than showing an empty page.
- New-arrival mixes: `GET https://openapi.tidal.com/v2/userRecommendations/{userId}/relationships/newArrivalMixes` (JSON:API). Also `userDailyMixes`, `userDiscoveryMixes`, `userNewReleaseMixes` in the spec.

### 2.10 TIDAL Upload (the account's own releases)

Sources (2026-10-18): the official OpenAPI spec (v1.10.183, `tidal-api-oas.json`), which documents every call below, and TIDAL's web player bundle
(`tidal.com/assets/index-CaNNwb6Q.js`, `yourUploads-CwE76Xjn.js`, listed by `tidal.com/service-worker.js`; `listen.tidal.com` now redirects its assets to
`tidal.com`, and its HTML is behind a bot check, so the bundle was read from the service worker's list). Every call is JSON:API on
`https://openapi.tidal.com/v2/` with `Content-Type: application/vnd.api+json`; the spec's scope is `w_usr` for writes and `r_usr` for reads, and the
session token Tidalfast already holds is accepted. Tags: [L] checked live on the test account on 2026-10-18 (one short silent WAV uploaded as an
Unlisted single "Tidalfast test", a second one, every write below exercised on those two test releases only, then both deleted; the account's list
was the same as before afterwards), [B] read from the web player's bundle, [S] spec only.

- Sidebar: Music, Explore, Feed, then **Upload** (`/your-uploads`, icon `3.0:upload`) when the account has the `creator-content-uploader` flag and
  the setting `hideUploadTab` ("Hide the Uploads tab from the navigation") is off. TIDAL's Hide Uploads tab therefore hides both Home's Uploads tab and
  this row; Tidalfast follows that. [B]
- The account's artist: `GET artists?filter[owners.id]=me` -> `data[0]` `{id, attributes {name, ownerType: "USER", numberOfFollowers, ...}}` (the web
  player's `useCheckArtistName`; with none it asks "Before uploading, let us know what name to release your music under." [B], an account without
  an artist was not seen). The v2 profile (`v2/profiles/{userId}`) also carries `artistId`. [L]
- Upload page list: `GET albums?filter[owners.id]=<userId>&include=albumStatistics,artists,coverArt,owners,priceConfig,usageRules&countryCode=XX`
  (the web player's include list; Tidalfast asks `include=artists,coverArt&sort=-createdAt`). Album attributes: `title`, `albumType` (`ALBUM|EP|SINGLE`),
  `accessType` (`PUBLIC|UNLISTED|PRIVATE`), `numberOfItems`, `explicit`, `createdAt`; `relationships.artists`, `relationships.coverArt` (artworks with
  `attributes.files[] {href, meta {width, height}, type}`: square 1280, 640, 320 and so on, plus wide crops such as 1280x720, all on
  `resources.tidal.com/images/`). Uploads TIDAL made no cover for still have an artwork: TIDAL's own placeholder image. `barcodeId` reads
  `000000000000` on uploads. Paging: `links.meta.nextCursor` -> `page[cursor]` (every release of the test account came in one page; a second page was not seen [?]). [L]
- Card: cover (or TIDAL's placeholder `upload-cover-*.svg`: #373737 with a darker disc and the TIDAL mark), title, artist name, then a capitals line:
  a lock for Unlisted (`general:lock-locked`) or a globe and "PUBLIC" for Public, "·", "N TRACKS"; "Uploading..." while a file is on its way (its cover an
  up arrow in a ring). A click opens the release's dialog (`EDIT_ALBUM`), not a page. [B]
- Card menu (right-click on a card of TIDAL desktop's Upload page; confirmed 2026-10-19 in the desktop app on a live account): **Edit album**, **Share** (a
  submenu with one link-icon row, **Copy link**) and **Delete album** in red. Copy link gives `https://tidal.com/upload/<id>`, for example
  `https://tidal.com/upload/12345678-90ab-4cde-8f01-234567890abc` (a made-up id; the shape was confirmed on a live account); the id is the release's, a UUID like this example and
  the same id the list and the other calls above use (that the two match is inferred from the format, not checked against a live release). Edit
  album opens the release dialog; Delete album asks "Delete album and tracks?" first. A pasted link of that shape (`tidal.com/upload/<id>`) opens the
  Upload page and, when the release is already listed, its dialog; a release not listed yet (the page still loading) leaves just the page. [L]
- A release's tracks: `GET albums/{id}/relationships/items?include=items,items.sourceFile` (the web player adds `items.albums, items.artists,
  items.priceConfig, items.shares, items.trackStatistics, items.usageRules`). Data entries carry `meta.trackNumber`/`volumeNumber`; included `tracks`
  have `title`, `duration` (ISO 8601, `PT3M5S`), `accessType`; included `trackSourceFiles` have `status {technicalFileStatus: UPLOAD_REQUESTED|PROCESSING|
  OK|ERROR|DELETED, moderationFileStatus: NOT_MODERATED|SCANNING|FLAGGED|TAKEN_DOWN|OK|ERROR, aiScanningFileStatus}` [L]: seen OK/OK, OK/FLAGGED (the
  track's `accessType` then PRIVATE) and OK/ERROR on the account's releases; the 4-second test file was processed (duration `PT4S`) within about a
  minute. Each source file still carries its `uploadLink` (a signed address) in these answers: never log them. The web player polls this every second
  while a file is processing; Tidalfast every 3 s while that release's dialog is open, for 5 minutes at most. [B][L]
- Choosing files ("+ Upload", or a drop): `accept` is `audio/*` (aiff, flac, mp3, mp4, mpeg, m4a, wav, ogg); at most 5 GB a file. One file and no
  releases yet: straight into a new single; else **Organize N new track(s)**: "Add to an existing album" (a radio list of releases, `Add`) or "Upload as
  Single". [B]
- New release: `POST albums` `{"data":{"type":"albums","attributes":{"albumType":"SINGLE","title":"<file name without extension>"},
  "relationships":{"artists":{"data":[{"id":"<artistId>","type":"artists"}]}}}}` -> 201 `data.id`. The type follows the number of files: 1-2
  SINGLE, 3-6 EP, 7+ ALBUM; several files are titled "Untitled". Its access starts `UNLISTED`. The dialog "New single" opens at once. [L] (one
  file; the type rule for several files is [B] only)
- Each file: `POST tracks` `{"data":{"type":"tracks","attributes":{"accessType":"UNLISTED","title":"<name>"},"relationships":{"albums":{"data":[{album}]},
  "artists":{"data":[{artist}]}}}}` (access: the album's, PRIVATE becoming UNLISTED), then `POST trackSourceFiles`
  `{"data":{"type":"trackSourceFiles","attributes":{"md5Hash":"<hex md5>","size":<bytes>},"relationships":{"track":{"data":{"id":"<trackId>","type":"tracks"}}}}}`
  -> 201 `data.attributes.uploadLink {href, meta {method, headers}}`, then the file itself: `<method>` (PUT) to `href` with `Content-Type: <the file's
  type>` and every `meta.headers` entry, the body the raw file (XHR, no multipart): 201, 201, then 200 from the PUT [L]. The MD5 is lowercase hex (hash-wasm's
  `digest()`; TIDAL accepted it and processed the file) [L]. A failure
  after the track exists deletes the track, and a new release none of whose files went is deleted too. Tidalfast streams the file from disk with the
  shared proxy settings and reports progress at each whole percent. [B][S]
- Release dialog ("New single" / "New EP" / "New album", or "Edit album"): cover with "Change image" (JPEG/PNG) and "Create spectrogram"; Title; Artist
  name with Edit ("Edit name": "This name will be used on your profile and all of your tracks.", Name, Save); Album type (Single, EP, Album);
  Visibility (Public "Discoverable and streamable when at least one track is public.", Unlisted "Only visible to you and anyone with the link. Hidden from
  profiles when all tracks are private."; disabled with "This album contains tracks that are still being uploaded or scanned. You can change visibility
  once all tracks have finished scanning." while any track is); Tracks with "Add tracks", each row number, title, lock and duration or "Preparing...",
  "..." (Edit track details, Move to another album, Share, Delete track) and a drag handle; Delete and Done. [B]
- Done sends only what changed: `PATCH albums/{id}` `{"data":{"id","type":"albums","attributes":{"title"?, "albumType"?, "accessType"?, "barcodeId"?}}}`
  -> 200 with title and albumType [L]; `accessType` (visibility) was left Unlisted in the live test and is [B][S] only. Paid access
  (`priceConfigurations`, `usageRules`) is not offered by Tidalfast.
- Cover: `POST artworks` `{"data":{"type":"artworks","attributes":{"mediaType":"IMAGE","sourceFile":{"md5Hash","size"}}}}` -> `data.id` and
  `data.attributes.sourceFile.uploadLink`; the image goes there as above; then `PATCH albums/{id}/relationships/coverArt` `{"data":[{"id":"<artworkId>",
  "type":"artworks"}]}`: 201, 200 (PUT of a 1400 px PNG), 200 [L].
- Spectrogram: `GET albums/{id}/relationships/suggestedCoverArts` -> `data[] {id, type: artworks, meta {backgroundColor, foregroundColor}}`. The web player
  polls every 2 s (5 minutes at most) until there are 9, saying "We're generating your spectrogram artwork. It should be ready soon." meanwhile; picking
  one is the coverArt PATCH above with that artwork id [L]: nine suggestions, with `include=suggestedCoverArts` carrying each artwork's files, were
  there a few minutes after the upload. Tidalfast asks once per click. [B][L]
- Track details: `PATCH tracks/{id}` `{"data":{"id","type":"tracks","attributes":{"title"}}}` (also `bpm`, `key`, `keyScale`, `toneTags`, `isrc`,
  `explicit`, `accessType`). Move: `PATCH tracks/{id}/relationships/albums` `{"data":[{"id":"<albumId>","type":"albums"}]}`. Order:
  `PATCH albums/{id}/relationships/items` `{"data":[{"id":"<trackId>","type":"tracks"}],"meta":{"positionIndex":<1-based>}}`. All three 200 [L].
- Artist name: `PATCH artists/{id}` `{"data":{"id","type":"artists","attributes":{"name"}}}` (`meta.dryRun` exists). [S] Not sent live: the
  account's name is not to be changed by tests.
- Delete: `DELETE tracks/{id}`, `DELETE albums/{id}` ("Delete album and tracks?" "This will permanently delete the album and all tracks it contains.
  You can't undo this action."): 200 each [L]; deleting a release deleted its tracks too (the account's `tracks?filter[owners.id]` had none of the
  test tracks afterwards) [L].
- Errors: an `errors[]` entry with `detail` "User has reached the maximum number of artworks", `QUOTA_EXCEEDED` ("You have reached the maximum number
  of uploads."). [B]
- Tidalfast: `api/tidal/uploads.rs` (calls, parsing, bodies), `upload_jobs.rs` (backend side, the file's PUT), `uploads.rs` (state), `ui/uploads.rs`.
- The account's own profile (checked live 2026-10-19 on the signed-in test profile, GET only): the web player's `/user/{userId}` route sends the
  signed-in account to `/artist/{artistId}` when its v2 profile (`v2/profiles/{userId}`) carries `artistId` (a number, such as `12345678`), and to
  `/` otherwise; there is no user profile page of its own [B]. That artist page is the ordinary artist page with "my page" changes [B]: Edit
  (`EDIT_ARTIST`, also reached with `?edit-profile=true`) instead of Follow, "Add bio" when there is no biography, Play and Shuffle only with playable
  items, Report only with the `content-reporting` flag (own profile: the inaccurate-releases journey, `REPORT_ARTIST_PROFILE`), and, with no modules,
  "Your profile is empty" / "Start creating playlists, adding your social profiles, or uploading a track." (`artist-profile-empty`). [B]
  - The page's modules come from `GET v2/artist/{id}?locale&countryCode&deviceType&platform` (`{item, header {followersAmount, biography,
    playableContent, profilePrivacyType}, items: [modules]}`; for other artists `ARTIST_TOP_TRACKS` "Top Tracks", `ARTIST_ALBUMS` "Albums",
    `ARTIST_TOP_SINGLES` "EP & Singles", ... with `viewAll: artist/<MODULE>/view-all?artistId=`) [L]. For the uploader's own artist it answered
    `items: []`, `playableContent.items: []` and null counts from Tidalfast's session, also with `x-tidal-authenticated-userid` set [L], so the modules
    TIDAL's apps show there ("Albums", "Uploaded tracks") could not be read; `artist/{id}/playable` answered `{items: [], play: true}` [L].
  - v1 answers in full instead [L]: `artists/{id}` (name, `picture` = the account's picture uuid, `userId`), `artists/{id}/albums` (the uploaded ALBUM,
    `upload: true`, `audioQuality: "HIGH"`, empty `mediaMetadata.tags`), `artists/{id}/albums?filter=EPSANDSINGLES` (the uploaded singles),
    `artists/{id}/toptracks` (every upload of the account including PRIVATE and UNLISTED ones: `accessType`, `upload: true`, `bpm`, `key`, `keyScale`),
    `albums/{id}` and `albums/{id}/tracks` for an uploaded release. OpenAPI `artists/{id}` has `numberOfFollowers` (a count, shown as "N fans"), `ownerType: "USER"`.
  - What TIDAL's own profile lists, from screenshots of a live account: "Albums" (the uploaded album) and "Uploaded tracks" (its songs), and the
    empty profile at a time the account had only unlisted singles. Tidalfast therefore shows the artist's ALBUM releases and the top tracks that are on
    them, plus the account's public playlists [?: TIDAL's rule for which releases and songs the profile lists is inferred from two screenshots].
  - Tidalfast: opening the own profile (account menu) opens the artist page (`App::profile_target`); the own artist (`App::own_artist_id`) is known
    from the profile or from TIDAL Upload's `artists?filter[owners.id]=me`. Edit and Add bio open the web player's editor in the browser
    (`https://tidal.com/artist/{id}?edit-profile=true`); Tidalfast sends no profile writes. Report is left out: `POST openapi userReports`
    (`{data: {type: "userReports", attributes: {reason, description}, relationships: {reportedResources: {data: [{id, type}]}}}}`, spec) is the
    generic report, while the own profile's Report is the inaccurate-releases claim, whose calls are not known [?]. An upload's album page shows
    TIDAL's LOW badge (lossy only); upload rows and the player bar show the upload badge (`upload: true` on v1 tracks and albums).

---------------------------------------------------------------------------------------------------

## 3. Playback

### 3.1 Request [V]
```
GET https://api.tidal.com/v1/tracks/{id}/playbackinfopostpaywall
    ?audioquality=LOW|HIGH|LOSSLESS|HI_RES_LOSSLESS
    &playbackmode=STREAM          (alternatives: OFFLINE - needs offline authorization)
    &assetpresentation=FULL       (PREVIEW = 30 s preview)
    [&countryCode=XX]
Authorization: Bearer <access_token>
```
- `playbackinfo` (no `postpaywall`) is the same shape and is what the official web player (tidal-sdk-web) still calls: `GET {legacyApiUrl}/tracks/{id}/playbackinfo?audioquality=&playbackmode=STREAM&assetpresentation=FULL` with optional headers
  `x-tidal-streamingsessionid: <uuid>`, `x-tidal-token: <clientId>`, `x-tidal-prefetch: true`, `x-tidal-playlistuuid`. Use `postpaywall` (what every third-party client uses); on a subscribed account it returns the full stream.
- Legacy `tracks/{id}/urlpostpaywall?urlusagemode=STREAM&audioquality=&assetpresentation=FULL` returns `{ "urls":[...], "codec", "securityType", "securityToken"}`; broken or refused for PKCE/new clients. Don't use.
- Videos: `videos/{id}/playbackinfopostpaywall?videoquality=HIGH|MEDIUM|LOW|AUDIO_ONLY&playbackmode=STREAM&assetpresentation=FULL`. Tidalfast asks `HIGH` and expects `manifestMimeType: application/vnd.tidal.emu`, a base64 JSON manifest shaped like a BTS one (`mimeType`, `urls` naming the HLS master playlist, optional `encryptionType`). The master lists one `EXT-X-STREAM-INF` variant per resolution (`RESOLUTION`, `BANDWIDTH`, `CODECS` avc1 + mp4a); each variant's media playlist lists MPEG-TS segments (H.264 + ADTS AAC). An `encryptionType` other than `NONE`, an `EXT-X-KEY` with a method other than `NONE`, a DASH manifest or fMP4 segments (`EXT-X-MAP`) are refused as "This video can't be played in Tidalfast". [?] Shape from third-party clients (tidal-dl-ng, tiddl); not yet checked against a live answer by Tidalfast (2026-10-14).
- Similar videos: `GET videos/{id}/recommendations?limit=` exists (live 2026-10-15, video 441699021: 200, `{items, totalNumberOfItems, offset, limit}`, 10 items), and each item is wrapped as `{sources: ["SUGGESTED_VIDEOS"], video: {...}}`, not `{item, type}`; Tidalfast read only the latter until this check, so it always fell back to the artist's videos. When it fails or is empty Tidalfast lists `GET artists/{id}/videos`. Neither endpoint appears in the open clients (checked 2026-10-14: tidalapi `media.py` Video has only `videos/{id}` and `videos/{id}/urlpostpaywall`; tiddl's API has `videos/{id}`, `videos/{id}/playbackinfopostpaywall` and `artists/{id}/videos`), so `recommendations` stays a guess from TIDAL's own client and the artist fallback is what is certain. Video credits: no open client reads any; Tidalfast shows the video's artists and `releaseDate` from `videos/{id}` and does not call a `videos/{id}/credits` endpoint. That endpoint exists (200, a bare `[]` for 441699021, as does `videos/{id}/contributors` with an empty page), but no video with credits was found to confirm the item shape.
- Video play reporting: the `playback_session` event for a video (product type and fields) is not known, so Tidalfast does not report video plays to TIDAL; they stay in its local statistics.
- Quality names: `LOW` = HE-AAC ~96 kbps, `HIGH` = AAC-LC 320 kbps, `LOSSLESS` = FLAC 16/44.1, `HI_RES_LOSSLESS` = FLAC up to 24/192 (the old `HI_RES` = MQA is deprecated and ignored/mapped).
- Atmos: tracks with `audioModes` containing `DOLBY_ATMOS` can be requested in Atmos (E-AC-3 JOC, codec `ec-3`/`ac-4`) by a client entitled to it; response `audioMode: "DOLBY_ATMOS"`. Out of scope for stereo player. [S]

### 3.2 Response [V]
```json
{ "trackId": 123456, "assetPresentation": "FULL", "audioMode": "STEREO", "audioQuality": "LOSSLESS",
  "manifestMimeType": "application/vnd.tidal.bts",          // or "application/dash+xml"
  "manifestHash": "base64...", "manifest": "<base64>",
  "albumReplayGain": -7.84, "albumPeakAmplitude": 0.998, "trackReplayGain": -8.1, "trackPeakAmplitude": 0.998,
  "bitDepth": 16, "sampleRate": 44100 }
```
- **`audioQuality` in the RESPONSE is what you actually got**; the API silently downgrades instead of erroring (client ceiling, or track not mastered in that tier). Show it in the UI. `bitDepth`/`sampleRate` present only for lossless tiers (absent for LOW/HIGH; assume 16/44100 or read from codec).
- `trackReplayGain`/`albumReplayGain` are dB (negative typical); `*PeakAmplitude` linear 0..1+. Any field may be missing; default 0.0 dB / 1.0 peak. `manifestHash` is a base64 digest; compare for change detection.
- TIDAL's own player (tidal-sdk-web `normalizeVolume`) plays a normalised track at `min(10^((replayGain + 4) / 20), 1)` times the linear slider level: a 4 dB preamp over the ReplayGain value, never above unity, album gain by default (`loudnessNormalizationMode: 'ALBUM'`, `desiredVolumeLevel: 1`). Tidalfast adds the same preamp and keeps the peak at full scale. [V] source read 2026-10-17
- `assetPresentation: "PREVIEW"` means the account/client was not allowed full playback (no subscription, region, or client not entitled). Treat as an error for a paid-account player.

### 3.3 Manifest variant A: `application/vnd.tidal.bts` (base64 JSON) [V]
Decoded:
```json
{ "mimeType": "audio/mp4", "codecs": "mp4a.40.2", "encryptionType": "NONE", "urls": ["https://sp-pr-fa.audio.tidal.com/mediatracks/....mp4?token=..."] }
```
- `mimeType` `audio/mp4` (AAC, `codecs` `mp4a.40.2`; LOW is `mp4a.40.5`) or `audio/flac` (`codecs: "flac"`, raw FLAC file in one progressive HTTP URL).
- `urls` has normally one URL, a complete file served with HTTP range support; plays directly in any HTTP audio source. `keyId` appears only when `encryptionType != "NONE"`.
- tiddl docs table (current for device-flow clients): LOW/HIGH -> bts, `audio/mp4`, m4a; LOSSLESS -> bts, `audio/flac`, `.flac`; HI_RES_LOSSLESS -> `dash+xml`, `audio/mp4`, FLAC in fMP4 (`.m4a`, tiddl remuxes with ffmpeg `-c copy` to `.flac`; and notes a "HI_RES_LOSSLESS" request may contain AAC-in-MP4 when no lossless master exists).

### 3.4 Manifest variant B: `application/dash+xml` (base64 MPEG-DASH MPD) [V]
Decoded (static MPD, a single Representation, absolute URLs):
```xml
<?xml version='1.0' encoding='UTF-8'?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011" type="static"
     mediaPresentationDuration="PT3M45.2S" minBufferTime="PT1.5S">
 <Period id="0">
  <AdaptationSet id="0" contentType="audio" mimeType="audio/mp4" segmentAlignment="true">
   <Representation id="FLAC_HIRES" codecs="flac" bandwidth="3129472" audioSamplingRate="96000">
    <SegmentTemplate timescale="96000" initialization="https://.../0.mp4?token=..." media="https://.../$Number$.mp4?token=..." startNumber="1">
     <SegmentTimeline><S d="393216" r="43"/><S d="209920"/></SegmentTimeline>
    </SegmentTemplate>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>
```
- `codecs` seen: `flac` (FLAC in fragmented MP4 - both LOSSLESS and HI_RES tiers when the client is served DASH), `mp4a.40.2`/`mp4a.40.5` (AAC), `ec-3` / `ac-4` / `mhm1` (Atmos/360).
- Segment URLs: the `initialization` URL is segment 0; media URL template uses `$Number$`; **xml-unescape `&amp;` in the attribute values** (tidlers unescapes entities). Number of media segments = sum over `<S>` of `(r ?? 0) + 1`; segment numbers run `startNumber`.. (tidalapi/tiddl enumerate `$Number$` from 0 to total inclusive, where index 0 yields the init segment URL via template - verify against your `startNumber`; the safest is to use `initialization` for the init and `media` with `startNumber..startNumber+count-1`).
  Download = concatenate init + all media segments -> valid fMP4; or feed the whole MPD to a DASH-capable player. mare-player passes the XML to GStreamer as `data:application/dash+xml;base64,<mpd>` and relies on `type="static"` and absolute URLs (no re-fetch of manifest).
- No `<ContentProtection>` in the MPD for non-DRM streams; if present or `drmData` exists (OpenAPI `trackManifests`), the client is DRM-entitled (Widevine/FairPlay) and a pure-Rust client cannot play it.
- Rust implementation note: parse with `quick-xml` (tidlers does); also fall back if MPD has multiple Periods/Representations (take the highest `bandwidth`).

### 3.5 Which qualities are encrypted, and for which client types
- Observed for device-flow ("TV/automotive") and PKCE Android clients as used by tidalapi, tiddl, tidlers, mare-player in 2026: `encryptionType` is `"NONE"` for all four tiers and nobody implements decryption. All clients above play/download directly. [V]
- Historically (2019-2021) some formats (esp. old MQA / "OLD_AES" with `keyId`, via `urlpostpaywall` `securityToken`) were AES encrypted for certain client ids. None of the current repos handle it, so with the client classes above it is not encountered. Unconfirmed whether other client ids (e.g. a freshly registered developer app) get DRM (Widevine) manifests - the official `trackManifests` explicitly models `drmData`. [?]
- Practical rule: if `encryptionType != "NONE"`, or MPD has `ContentProtection`, treat as unsupported for that client id and surface "this client id cannot play protected streams"; do not attempt decryption.

### 3.6 LOSSLESS: raw FLAC or FLAC-in-MP4? (answer: depends on client id)
- **Measured 2026-10-14 (Tidalfast, device-code client, Max account): `playbackinfopostpaywall?audioquality=LOSSLESS` answers `audioQuality: "HIGH"`, BTS `mp4a.40.2` (AAC 320), even for tracks TIDAL holds in FLAC.** Same for `playbackinfo` (no postpaywall). Neither `HI_RES` nor a lowercase name helps; `urlpostpaywall` answers 401/4005.
  On that client `HI_RES_LOSSLESS` gives DASH `FLAC_HIRES` for a hi-res track (JID & Lil Durk "Bruddanem", 257665679: 24/88200) but **AAC 320 for a CD-quality-only track** (77646168). So v1 playbackinfo can no longer give 16-bit FLAC at all on that client.
  The PKCE (Max sign-in) client measured the same day is unaffected: `LOSSLESS` gives DASH `FLAC,44100,16` (bitDepth 16), `HI_RES_LOSSLESS` gives `FLAC_HIRES` for the hi-res track and `FLAC,44100,16` for the CD one.
  What works on the device client: OpenAPI `trackManifests` (3.8) with `formats` up to `FLAC` answers DASH `Representation id="FLAC,44100,16"` (unencrypted, no `drmData`, `formats: ["FLAC"]`) for both tracks; with `FLAC_HIRES` added it answers `FLAC_HIRES,88200,24` for the hi-res one and `FLAC,44100,16` for the CD one. Tidalfast asks Lossless there, and falls back to it when a Max request comes back lossy.
- Device-flow clients (tiddl docs table): LOSSLESS = BTS with `audio/flac` raw FLAC. HI_RES request is downgraded to this.
- PKCE Android client (mare-player, measured 2026): **both lossless tiers are DASH**, `codecs="flac"` inside fragmented MP4 (init + `.mp4` segments). The BTS path is only AAC.
- So the player must support both: (1) `vnd.tidal.bts` -> one URL, `audio/mp4` or `audio/flac`; (2) `dash+xml` -> FLAC/AAC fMP4 segments. Detect by `manifestMimeType`, then by `codecs`; never assume from the requested quality.
- FLAC-in-MP4 decoding: symphonia (`isomp4` + `flac`) handles `fLaC`/`dfLa` boxes; ffmpeg does; GStreamer dashdemux + flacparse path used by mare. For gapless playback, concatenate segments in a ring/streaming reader instead of per-segment sources.

### 3.7 Expiry, retries
- Stream URLs carry `?token=...` and expire; the official player treats a fetched playbackinfo as valid for 1 hour (`MANIFEST_EXPIRATION_MS = 3600000`) and re-fetches after that. Treat anything older than ~1 h (prefetch included) as stale; refetch at seek time if a segment returns 403/410.
- Pre-fetch the next track's playbackinfo late (last 20-30 s) rather than minutes ahead; do not cache across sessions.
- The official player sends `x-tidal-prefetch: true` for prefetches and retries playbackinfo 3 times on 429/5xx.

### 3.8 Official replacement: `trackManifests` (OpenAPI) [V spec + sdk-web]
```
GET https://openapi.tidal.com/v2/trackManifests/{trackId}
    ?manifestType=MPEG_DASH|HLS      (HLS for Apple/FairPlay)
    &formats=HEAACV1&formats=AACLC&formats=FLAC&formats=FLAC_HIRES&formats=EAC3_JOC   (repeat or comma list; list the ones you accept)
    &uriScheme=DATA|HTTPS            (DATA: attributes.uri is a data: URL carrying the manifest; HTTPS: a URL to fetch)
    &usage=PLAYBACK|DOWNLOAD
    &adaptive=true|false
    [&shareCode=...]
Authorization: Bearer <token with scopes playback + r_usr>   header x-playback-session-id: <uuid>
```
Response JSON:API: `data.attributes = { formats[], hash, trackPresentation: "FULL"|"PREVIEW", previewReason, uri, trackAudioNormalizationData {replayGain,peakAmplitude}, albumAudioNormalizationData, drmData {drmSystem, licenseUrl, certificateUrl, initData[]} (absent = no DRM) }`.
Quality mapping used by the official SDK: HI_RES_LOSSLESS -> `[HEAACV1, AACLC, FLAC, FLAC_HIRES]`, LOSSLESS -> `[HEAACV1, AACLC, FLAC]`, HIGH -> `[HEAACV1, AACLC]`, LOW -> `[HEAACV1]`.
`adaptive` is the web player's **Adaptive streaming** setting (`adaptiveAudioStreamingEnabled`, default on; the SDK's `audioAdaptiveBitrateStreaming`, default `true`); shaka's ABR then moves between the qualities within a song (8 s switch interval, keep at rate / 0.95, step up at rate / 0.85). The desktop app (`enableDesktopFeatures`) and Safari force it to `false` and hide the switch. Read from the web bundle 2026-10-18; what an `adaptive=true` manifest holds was not fetched. Tidalfast's version changes quality between songs only: [Audio output](audio-output.md#adaptive-streaming). [S]
Errors: 403 codes `CLIENT_NOT_ENTITLED`, `CONCURRENT_PLAYBACK`, `GEO_RESTRICTED`, `PREREQUISITE_MISSING`, `PURCHASE_REQUIRED`; 404 `CONTENT_NOT_FOUND`.
Measured 2026-10-14 with a device-code token from a first-party client (scopes `r_usr w_usr w_sub`, no `playback` scope): works without a registered app and without `x-playback-session-id`, and tolerates the extra `countryCode` query. `adaptive=false&uriScheme=DATA` gives `attributes.uri` = `data:application/dash+xml;base64,<MPD>`, one Representation whose id is `FORMAT,RATE,BITS` (e.g. `FLAC,44100,16`), `SegmentTemplate`/`SegmentTimeline` like 3.4. `formats` in the answer names the one served. Normalisation comes as `trackAudioNormalizationData` / `albumAudioNormalizationData` `{replayGain, peakAmplitude}`. Errors are JSON:API (`{"errors":[{"status","code","detail"}]}`, e.g. 404 `CONTENT_NOT_FOUND`, 401 `UNAUTHORIZED`), not v1 `subStatus`. With the PKCE (Max sign-in) client the same request answers **with `drmData` (Widevine)** for the same tracks, so it is useless there; Tidalfast refuses DRM answers, asks playbackinfo instead and stops asking `trackManifests` for that sign-in. [V]

### 3.9 Play-log / event reporting (optional) [S: mare-player, matches tidal-sdk-web event-producer]
Reporting makes plays count for Recently Played, recommendations, and royalties. mare-player does it; tiddl/tidalapi do not.
```
POST https://ec.tidal.com/api/event-batch
Content-Type: application/x-www-form-urlencoded
Authorization: Bearer <access_token>
```
Body is AWS SQS `SendMessageBatch` form-encoding, one entry:
```
SendMessageBatchRequestEntry.1.Id=<uuid>
SendMessageBatchRequestEntry.1.MessageBody=<JSON envelope, below>
SendMessageBatchRequestEntry.1.MessageAttribute.1.Name=Name
SendMessageBatchRequestEntry.1.MessageAttribute.1.Value.StringValue=playback_session
SendMessageBatchRequestEntry.1.MessageAttribute.1.Value.DataType=String
SendMessageBatchRequestEntry.1.MessageAttribute.2.Name=Headers
SendMessageBatchRequestEntry.1.MessageAttribute.2.Value.StringValue=<JSON: {"app-name","app-version","client-id","consent-category":"NECESSARY","os-name","requested-sent-timestamp","authorization":"<raw token, no Bearer>"}>
SendMessageBatchRequestEntry.1.MessageAttribute.2.Value.DataType=String
```
MessageBody JSON:
```json
{ "group": "play_log", "version": 2, "ts": 1760000000000, "uuid": "<uuid>",
  "user": { "id": <uid>, "clientId": <cid>, "sessionId": "<sid claim or empty>" },
  "client": { "token": "<cid as string>", "deviceType": "phone", "version": "<app version>", "platform": "android" },
  "payload": { "playbackSessionId": "<uuid>", "productType": "TRACK", "actualProductId": "123", "requestedProductId": "123",
     "actualAssetPresentation": "FULL", "actualAudioMode": "STEREO", "actualQuality": "LOSSLESS",
     "sourceType": "ALBUM"|"PLAYLIST"|"MIX"|"ARTIST"|"TRACK"|"", "sourceId": "<id or empty>",
     "startTimestamp": ms, "endTimestamp": ms, "startAssetPosition": sec, "endAssetPosition": sec, "isPostPaywall": true,
     "actions": [ {"actionType":"PLAYBACK_START","assetPosition":s,"timestamp":ms}, {"actionType":"PLAYBACK_STOP","assetPosition":s,"timestamp":ms} ] },
  "extras": null }
```
Notes: `uid`/`cid`/`sid` are read from the access-token JWT payload. `sourceType`/`sourceId` must be strings (empty string, not null, or the event is silently dropped). Only containers (ALBUM/PLAYLIST/MIX/ARTIST) surface in Recently Played.
Count a play after >= 30 s listened or >= 50% of duration. The envelope's `client` block should describe the same client the token belongs to (platform/deviceType/version); a mismatch looks like fraud - report honestly or not at all. Fire-and-forget on a background task.

---------------------------------------------------------------------------------------------------

## 4. Images

Base: `https://resources.tidal.com/images/{uuid-with-slashes}/{W}x{H}.jpg`. Image ids are UUIDs with hyphens (`7e58f111-5b1a-492a-aaf1-88fb55ce8a44`); **replace every `-` with `/`**:
`https://resources.tidal.com/images/7e58f111/5b1a/492a/aaf1/88fb55ce8a44/320x320.jpg`
Original: `.../origin.jpg` (tidalapi `image_url_origin`). Videos: `https://resources.tidal.com/videos/{uuid/with/slashes}/{W}x{H}.mp4` or `/origin.mp4` (animated album covers via `videoCover`). Checked 2026-10-14: 80, 160, 320, 640, 750, 1280 square and origin answer (other sizes 403); H.264 in MP4 (moov first), no audio track, about 15 s; 320 ~ 240 KB, 640 ~ 540 KB, 1280 ~ 1.9 MB. The web player plays `1280x1280.mp4` muted, autoplaying and looping over the album header's cover.
Unsupported size -> HTTP 403/404 (mare: requesting 320x320 for a promo image gave 403). Use only listed sizes.

| Kind | Source field | Valid sizes |
|---|---|---|
| Album cover | `album.cover` (also `track.album.cover`) | square 80, 160, 320, 640, 1280 (+ `origin`; tidalapi also lists 3000 as the origin size) |
| Album video cover | `album.videoCover` | square 80, 160, 320, 640, 750, 1280, origin (mp4) |
| Artist picture | `artist.picture` | square 160, 320, 480, 750 (mare uses 750); wide variant 1080x720 for banners [?] |
| Playlist square | `playlist.squareImage` | square 160, 320, 480, 640, 750, 1080 |
| Playlist wide | `playlist.image` | 160x107, 480x320, 750x500, 1080x720 |
| Promo / featured | `imageId` of FEATURED_PROMOTIONS | 550x400 only |
| Video thumbnail | `video.imageId` / `imagePath` | 160x107, 480x320, 750x500, 1080x720 (same family as playlist wide) [?] |
| Mix | `images.{SMALL,MEDIUM,LARGE}.url` (v1) or `mixImages[].url` / `detailMixImages[].url` (v2) | URLs are given ready-made, do not build; nominal sizes 320 / 640 / 1500 square. Take the URL as-is. |
| User picture | `profile.picture` / `user.picture` | square 100, 210, 600 [?] |

Fallbacks: artist without `picture` -> use `selectedAlbumCoverFallback` (an album cover id); album without `cover` -> placeholder; playlist: `squareImage` else wide `image` else first-tracks mosaic.
Caching: image ids are immutable; cache by `(uuid,size)` on disk indefinitely. Requests are unauthenticated.

---------------------------------------------------------------------------------------------------

## 5. Limits, headers, errors, availability

### 5.1 Headers
- `Authorization: Bearer <access_token>` (every API call, v1/v2/openapi). tidalapi lowercases header name; either works.
- `x-tidal-client-version: <ver>` recommended (see 2.1). `User-Agent`: a realistic mobile/browser UA (clients above use an Android WebView UA). OpenAPI also allows `Accept: application/vnd.api+json`.
- Token endpoint: `Content-Type: application/x-www-form-urlencoded` (no JSON). `Accept-Language`/`locale` param (e.g. `en_US`) affects page and mix text.
- Optional playback headers: `x-tidal-streamingsessionid`, `x-tidal-token` (client id), `x-tidal-prefetch`.
- Mutations of playlist items: `If-None-Match: <etag>` (2.5).

### 5.2 Rate limiting
- No documented numeric limits for v1. Clients map HTTP **429** to `TooManyRequests` and read `Retry-After` (seconds; tidalapi int, default -1). Honor it; otherwise exponential backoff starting 500 ms (official SDK), max ~3-4 tries; 5xx/429 are the retryable class.
- OpenAPI spec defines a `Default429Response` and `503`; same policy.
- Be polite: cap concurrency (e.g. 4 concurrent API requests, 2 image fetches per host per row), coalesce duplicate requests, cache catalog responses (tiddl caches albums/tracks 1 h; favorites/playlists revalidate always).
- Playback info is the most throttled/monitored endpoint (429 with message "Stream unavailable"); never call it for tracks you won't play soon.

### 5.3 Error body formats
v1 / v2 legacy: 
```json
{ "status": 401, "subStatus": 11002, "userMessage": "The token has expired. (Expired on time)" }
```
- Detect expiry via **401 with `userMessage` starting "The token has expired."** (tidalapi) -> refresh once, retry. Other 401s: `subStatus 1001` "Invalid token" (bad/revoked token -> re-login); `subStatus 4005` "Asset is not ready for playback" (**401 status but it is an availability error, not auth**, mare maps it to "unavailable, skip track"). Never treat every 401 as token expiry (mare explicitly warns).
- `403` with `subStatus` playback family (playbackinfo): `4010` monthly stream quota exceeded, `4032` / `4035` content not available in your location, `4033` not available for your subscription, `4006`/`4002` etc. variants [?]. The official web player maps exactly 4010, 4032, 4035, 4033 and treats other 4xx as "not allowed", 429/5xx as retryable. Tidalfast's engine does the same for a song's stream: a 4xx answer skips the song as unavailable, while 429, 5xx and no answer at all (DNS, connection refused or reset, timeout, TLS error, a body cut off) are an outage (`tidal_engine::catalog::UNREACHABLE`, set in `api/tidal_catalog.rs`): the song stays the playing one and waits for the network (see How it connects).
- `404` object not found (`subStatus 2001` for missing mix/track radio, `subStatus 1002` general) -> treat as absent, not fatal. tidalapi: 404 -> `ObjectNotFound`.
- `412` Precondition Failed on stale playlist ETag; `400` for bad params (e.g. limit > 100: `"limit cannot be greater than..."`); sometimes HTTP 200 with truncated/invalid JSON or empty body (tiddl retries up to 5 with 2 s delay "JSON decode error"). Be defensive: parse errors -> retry idempotent GETs.
- OpenAPI/JSON:API errors: `{ "errors":[{ "status":"403","code":"CLIENT_NOT_ENTITLED","detail":"..." }] }`.
- Auth errors: `{ "status":400, "error":"invalid_grant", "sub_status":..., "error_description":"..." }` (note `snake_case` here vs `subStatus` camelCase in API errors).

### 5.4 Country / availability gotchas
- `countryCode` must be the account's store country from `sessions`. A wrong value returns 404/empty or different catalog; not geo-IP checked in most calls, but playback ("GEO_RESTRICTED", 4032/4035) is geo-checked by IP: a VPN or travel can break playback while browsing still works.
- Track/album availability flags: `allowStreaming` (rights say streaming allowed), `streamReady` (asset prepared), `adSupportedStreamReady` (free tier), `premiumStreamingOnly`, `streamStartDate` (future date = not yet released: show greyed). Gray-out when `!(allowStreaming && streamReady)`. Greyed tracks in playlists/albums still appear in lists (and keep their index!). Video items in lists can have `allowStreaming:false`.
- A track can be replaced by another id (`replacement` relation in OpenAPI; the same recording on a different album). Don't dedupe by title only; dedupe by `isrc` for "same song" logic.
- Album `audioQuality`/`mediaMetadata.tags` advertise capability (`HIRES_LOSSLESS` tag exists on tracks that are hi-res) but NOT what a given client will receive; always read the playbackinfo response (3.2).
- Downgrade is silent (HI_RES_LOSSLESS -> LOSSLESS) for device-flow clients; show actual quality + `bitDepth`/`sampleRate` in the now-playing UI.
- `concurrent playback`: TIDAL allows one active stream per account; a second device starting playback can end yours with 403 `CONCURRENT_PLAYBACK` (OpenAPI) or a token-level playback error [?].
- ISO country in `countryCode` can differ from account language; `locale=en_US` format is `ll_CC`.
- Monthly stream quota (4010) exists for some plan types.

---------------------------------------------------------------------------------------------------

## 6. Minimal Rust implementation checklist

1. Settings: `client_id`, `client_secret` (optional for PKCE), `auth_flow`, `redirect_uri` (PKCE), `quality`, user agent override. Token store in the OS keychain: `access_token`, `refresh_token`, `expires_at`, `user_id`, `country_code`, `flow`, `client_unique_key`.
2. HTTP client (reqwest): default headers (`x-tidal-client-version`, UA), automatic `Authorization`, `countryCode` injection, 401 handler (expired -> refresh once -> retry), 429 `Retry-After`, JSON error classifier (`status/subStatus/userMessage`).
3. Typed models with `#[serde(default)]` everywhere; treat all optional fields as optional (tiddl and mare both had to relax models repeatedly: album.artist, artist.type, search shapes, mix variants, unknown page module types).
4. Playback pipeline: playbackinfopostpaywall -> inspect `manifestMimeType` -> BTS (progressive URL) or MPD (init + numbered fMP4 segments, xml-unescape) -> decode (FLAC/AAC) -> apply ReplayGain from response (track or album mode, clamp to peak) -> show actual quality.
5. Prefetch next track's manifest near the end of the current; re-fetch when older than ~1 h.
6. Library: favorites via `/favorites/ids` once + paged lists; playlists via `my-collection/playlists/folders` (cursor) and `users/{id}/playlists`; ETag-guarded item edits.
7. Optional: `ec.tidal.com/api/event-batch` play reporting after 30 s or 50%.

---------------------------------------------------------------------------------------------------

## 7. Surprises and uncertainties (summary)

1. The client id decides quality and manifest style; subscription tier alone does not. Device-flow clients are silently capped at LOSSLESS 16/44.1; hi-res needs the Android PKCE client (measured by mare-player). A user's own paid account does not remove this.
2. "LOSSLESS" is now DASH FLAC-in-fMP4 for the PKCE client, but raw `audio/flac` BTS for device-flow clients (tiddl docs). Player must handle both. As of 2026-10-14 v1 `LOSSLESS` gives AAC 320 on the device client Tidalfast uses; ask `trackManifests` with formats up to `FLAC` for 16-bit FLAC (3.6, 3.8).
3. 401 does not mean auth failure: `subStatus 4005` is an availability error. Expired-token detection is by `userMessage` text.
4. Playlist item mutations need `If-None-Match` carrying the ETag from the last GET of the items (counter-intuitive header name), and remove/move use positional indices, not ids.
5. Favorites add param names differ between clients (`trackId` vs `trackIds`, `albumId` vs `albumIds`); I could not verify which the server prefers today. Test both against a live account.
6. PKCE redirect for first-party clients is `https://tidal.com/android/login/auth` (cannot be a localhost loopback); manual paste of the final URL is how every third-party client does it. A self-registered developer app could use its own redirect.
7. Playlist deletion has two paths (v1 `DELETE playlists/{uuid}` and v2 `my-collection/.../remove?trns=trn:playlist:..`); unverified which one removes followed (not owned) playlists correctly. Use the v2 collection remove for un-follow.
8. Encryption: all current clients report `encryptionType: NONE` and nobody decrypts. Whether a brand-new developer-registered app is served DRM'd manifests (the OpenAPI models `drmData`) is unknown - test early if the official route is considered.
9. tidal-dl-ng repo is gone (404); `Mopidy-Tidal` is now `EbbLabs/mopidy-tidal` and defers entirely to tidalapi, so no extra endpoints came from it.
10. Subscription endpoint fields and a few v2 params (`rename`/`move` folder endpoints, `videoIds`, user picture sizes, the 1002/pending sub_status number) are from memory and marked [?]; verify with a live account before relying on them.

## 8. Sharing and embeds

- Share links (confirmed on a live account in the official desktop app): `https://tidal.com/<type>/<id>` for `track`, `album`, `artist`, `mix`, `video` and `playlist` (uuid); `tidal.com/browse/...` is still accepted when parsing. "Share with anyone" page (checked 2026-10-18 in the web player): `https://tidal.com/<type>/<id>/u` for `track`, `album` and `artist` only (links to TIDAL and other music services, by Feature.fm); `playlist/<uuid>/u` and `video/<id>/u` are "Page not found". Copy link gives tracks, albums and artists the `/u` page and the others the plain link.
- Collaborator invite link (confirmed on a live account): `https://tidal.com/playlist/<uuid>/invite?code=<code-uuid>`, one use per link. [?] The API call that creates an invite is NOT established (no open client documents it; candidates such as v2 `playlists/{uuid}/invites` are unverified). Tidalfast shows "Invite as collaborator" in an own playlist's Share menu but only tells the user it is not available yet; it makes no write against the account until the endpoint is measured.
- Embed snippet (video `441699021`, confirmed on a live account in the official app): `<iframe src="https://embed.tidal.com/videos/<id>" width="500" height="328" allow="encrypted-media; fullscreen; clipboard-write https://embed.tidal.com; web-share" sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox" style="color-scheme: light dark" title="TIDAL Embed Player" />`. Paths: `tracks`, `albums`, `playlists`, `mixes`, `videos`. [?] Only the video height (328) is confirmed; Tidalfast uses 96 for tracks and 400 for albums, playlists and mixes. developer.tidal.com renders client-side, so the embed docs could not be read here.
