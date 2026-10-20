# Security

## Reporting a problem

Please report a security problem privately, not in a public issue. On
GitHub, open the repository's **Security** tab and choose **Report a
vulnerability**. Say which version you run (Settings, About), on which
system, and how to reproduce the problem. The fix goes into the next
release, and the release notes credit you unless you ask them not to.

Fixes go to the latest release on each update channel (stable, beta and
dev). Older versions are not patched; update to get a fix.

## What Tidalfast protects

- **Your TIDAL sign-in.** You sign in on TIDAL's own pages, so your password
  never passes through Tidalfast. The grant TIDAL gives back (its tokens) is
  kept only in the system credential store: Credential Manager on Windows,
  Keychain on macOS, Secret Service on Linux. It is never written to
  settings, state files or the log. Sign out deletes it, and a marker in the
  state folder keeps a deletion the store refused from signing you back in.
  A proxy password, if you set one, is kept the same way.
- **The network.** Tidalfast has no server of its own. Everything it fetches
  goes over HTTPS with the certificate checked, and it sends the TIDAL token
  only to TIDAL. The one download over plain HTTP is Cisco's OpenH264 library
  (Linux and macOS, for animated covers), which Cisco serves no other way:
  it is loaded only when its SHA-256 matches one of Cisco's published
  builds. [How It Connects](docs/_reference/how-it-connects.md) and
  [Privacy](docs/_reference/privacy.md) list every host.
- **What TIDAL sends.** Only unencrypted streams are played; nothing is
  decrypted. The sizes and counts in manifests, stream indexes and artwork
  are checked against limits before memory is set aside for them, so that
  damaged data fails the track or the picture instead of the app.
- **Updates.** A channel's manifest is checked against Tidalfast's Ed25519
  key before anything in it is read. Each download must match the size and
  SHA-256 the signed manifest gives, and the release's signed checksum file,
  before it is installed, and an older version is never offered. The update
  key goes only to the update host over HTTPS. See
  [Updates](docs/_reference/updates.md).
- **Other programs on the computer.** The running copy takes commands (a
  second launch, `tidalfast next` and the like) only from your own user
  account: over a socket only you can open on Linux and macOS, and on
  Windows over a loopback port that answers only requests carrying a random
  token kept in your profile. A `tidal:` link only opens a TIDAL page.
- **Links.** Links in text from TIDAL, release notes and plugins open in the
  browser only when they are `http` or `https` web addresses; anything else
  (a file, a network share, a system settings page) is not opened.
- **Plugins and themes.** TidaLuna plugins and themes are off until you turn
  them on in Settings. The riskier things a plugin can do, such as starting
  a program, each wait for your permission, asked the first time (see
  "Permissions a plugin asks for" in
  [Luna compatibility](docs/_reference/luna-compat.md)).

## Out of scope

A program already running under your user account can read what you can,
your credential store included; Tidalfast cannot protect against that.
Problems in TIDAL's own services belong to TIDAL.
