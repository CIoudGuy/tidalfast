---
title: How It Connects
description: How Tidalfast signs in to TIDAL, which endpoints it calls, how streams are fetched, what is stored, and how traffic is routed.
nav_order: 1
---

Tidalfast is a native client for TIDAL. It has no server of its own: every
request goes from your computer to TIDAL (or to the few services listed in
[Privacy](/privacy/)). Playback is always local; Tidalfast decodes the stream
itself and plays it through its own output and EQ.

The endpoint-level details, with the source each fact was checked against, are
in `docs/_reference/tidal-api.md`. This page is the short version.

## Signing in

TIDAL decides what a token may do (catalog, library, stream ceiling) from the
OAuth client it was minted for. Tidalfast supports two sign-ins, both against
`https://auth.tidal.com/v1/oauth2/` with the scopes `r_usr w_usr w_sub`:

- **Device code.** Tidalfast asks TIDAL for a short code, shows it, and opens
  `link.tidal.com` in your browser. You approve the code there while the app
  polls. These clients are served up to 16-bit FLAC.
- **Browser (PKCE).** Tidalfast opens `login.tidal.com/authorize` in your
  browser with a PKCE challenge. TIDAL finishes on one of its own pages whose
  address carries a one-time code; you paste that address back into the app.
  The redirect belongs to TIDAL, so Tidalfast cannot catch it with a local
  listener. These clients are served hi-res FLAC (Max).

Your TIDAL password never passes through Tidalfast; you sign in on TIDAL's own
pages. Tidalfast signs in with the public TIDAL clients other open-source
players use (python-tidal and Mopidy-Tidal ship the same ones), built in so
that signing in needs nothing but your account. `settings.json` can name
others instead (`tidal_client_id`, `tidal_client_secret`,
`tidal_pkce_client_id`); a client secret entered there stays in that file.

Tokens are refreshed shortly before they expire, and once more after a `401`
that says the token expired. A token must be refreshed with the client that
minted it, so the sign-in flow is stored with the grant. A dead refresh token
means signing in again. After sign-in, `GET /v1/sessions` supplies the user id
and the account's store country, which goes on every catalog request.

## What Tidalfast calls

| Surface | Base URL | Used for |
| --- | --- | --- |
| Legacy v1 | `https://api.tidal.com/v1/` | Catalog, favourites, playlists, search, lyrics, pages, `playbackinfopostpaywall`, logout |
| Internal v2 | `https://api.tidal.com/v2/` | The library (folders, favourite playlists and mixes) and Home feed |
| OpenAPI v2 | `https://openapi.tidal.com/v2/` | `trackManifests`, the stream for Lossless; Save for Later; Upload (your uploaded releases, their tracks and details) |
| Images | `https://resources.tidal.com/images/` and `https://images.tidal.com/` | Cover art and mix artwork |

Everything is plain HTTPS with `Authorization: Bearer <token>`. The responses
are mapped into Tidalfast's own models, so the interface does not depend on
TIDAL's JSON shapes. Home, Mixes and Radio come from TIDAL's page endpoints.
Lyrics come from TIDAL when it has them, then from [LRCLIB](https://lrclib.net).

## Playback

For each track Tidalfast asks `tracks/{id}/playbackinfopostpaywall` for the
quality set in Settings (`LOW`, `HIGH`, or `HI_RES_LOSSLESS` for Max). For
Lossless it asks `trackManifests/{id}` with the formats up to 16-bit FLAC
instead, because `playbackinfopostpaywall` answers `LOSSLESS` with AAC 320;
the old request is the fallback if `trackManifests` refuses or answers with
DRM (as it does for the Max sign-in, whose `LOSSLESS` playbackinfo is FLAC). A Max request
that comes back lossy (a track with no hi-res master) is asked again from
`trackManifests` with every format up to hi-res. The reply carries a
manifest:

- `application/vnd.tidal.bts`: one URL to a complete AAC or FLAC file.
- `application/dash+xml`: a static MPEG-DASH description of FLAC or AAC in
  fragmented MP4 (an init segment plus numbered media segments).

TIDAL silently downgrades when the client or the track cannot be served at the
requested tier, so Tidalfast reads the quality, bit depth and sample rate from
the reply and shows what you are actually getting. ReplayGain values from the
reply feed volume normalisation. Stream URLs are short-lived and are fetched
shortly before they are needed.

A download that is cut off carries on from the first missing byte with a
range request; an expired URL is swapped for a fresh one. Failed attempts
with nothing arriving between them are retried, with pauses growing to 3 s,
for 20 s, so a network that drops for a few seconds resumes the song
straight away. A song that has waited a second for its next sound shows as
loading, its position held, until the sound comes back.

Only TIDAL's own answer that a song cannot be played (403 for the country
or the plan, 404, 401 with `subStatus` 4005, a protected stream) skips it,
with a notice naming it. No answer is not that: no network, a name that
does not resolve, a connection refused, reset or timed out, a certificate
error (a captive portal), an answer cut off, or TIDAL's servers failing
(5xx) or rate limiting (429). Then nothing is skipped and the queue stays as
it is. The song plays what it has already downloaded, then waits at its
position, showing as loading, with one notice: "No internet connection.
Tidalfast will carry on when you're back online". It asks TIDAL again after
2 s, then after pauses doubling to 15 s, and at once when the system reports
the network back or you press Play or seek; then it carries on from where it
stopped. A song loaded ahead for the next one, and autoplay's station at the
end of the songs, wait the same way. Downloaded songs need no network and
play as usual; Next and Previous move to them. Offline mode works as before:
its refusal of a request is the setting, not an outage, so nothing waits on
it.

Only unencrypted streams (`encryptionType: NONE`) are played. Tidalfast does
not decrypt protected streams, so Dolby Atmos and other DRM-protected formats
are not supported, and it does not substitute audio from any other source.
Streaming needs an active TIDAL subscription; the quality you can reach
depends on your plan and on which sign-in you used.

## What is stored

- The TIDAL grant (tokens and the sign-in flow) is kept in the system
  credential store: Credential Manager on Windows, Keychain on macOS, and
  Secret Service on Linux. A proxy password, if you set one, is a separate
  entry in the same store.
- Settings, session and history are readable JSON in the config and state
  directories; metadata caches are in the cache directory and can be deleted
  at any time. Sign out removes the grant and every account's cached library.

[Settings & Files](/settings-and-files/) lists every location.

## Uploading

The Upload page lists the releases you uploaded to TIDAL and sends new ones.
A file you choose goes from your disk straight to the upload address TIDAL
gives for it (a signed link on TIDAL's storage), through the same proxy
settings, after Tidalfast has told TIDAL its size and MD5. Nothing is
uploaded unless you pick or drop a file on that page. Covers you choose go
the same way.

## Proxies and the network

All of Tidalfast's own web requests (TIDAL, artwork, lyrics, update check) go
through one shared client, so a proxy change takes effect without a restart.
Settings offers **Off**, **System**, a manual **HTTP** proxy, or a manual
**SOCKS5** proxy with an optional login. TIDAL checks stream access by IP, so
a VPN or proxy in another region can break playback while browsing still works.

While the TidaLuna Plugins page is open (and only then), the cards in view
show their authors' GitHub profile pictures. They are fetched from
`github.com/<login>.png` (which redirects to `avatars.githubusercontent.com`)
or from the `avatarUrl` a plugin package gives when that is on `github.com` or
`avatars.githubusercontent.com`; any other host is ignored. They go through
the same shared client and artwork cache as covers, off the UI thread, at 80
pixels, and no cookie or account detail is sent. Without the picture (loading,
offline, no GitHub account known) the card keeps its letter avatar.

## Limits Tidalfast works within

- TIDAL's API for these clients is not a documented public contract; endpoints
  can change. When a feature breaks, check `tidal-api.md` and the log first.
- Rate limits answer with `429` and `Retry-After`; Tidalfast waits and retries
  idempotent reads, and avoids asking for stream info for tracks it will not
  play soon.
- One active stream per account: starting playback on another device can end
  this one.
- Tracks can be greyed out for your region or plan. Tidalfast keeps them in
  lists at their positions and skips them during playback.
