---
redirect_from: /using-fastpotify/
title: Everyday Use
description: Play music, arrange playlists, find lyrics, and make Tidalfast your own.
nav_order: 3
---

## Middle-click autoscroll

On Windows, and on Linux once turned on, middle-click anywhere
in a scrolling list, then move the pointer away from the starting point. That
list follows the pointer, faster as the distance grows, and the pointer shows
the direction it is scrolling. Speed and dead zone match Chromium's
autoscroll. Moving across another pane keeps the original list in control.
A small dead zone prevents an ordinary middle-click from moving the view.
You can also hold the middle button while moving and release it to stop.
Otherwise click again, press Esc, turn the wheel, or switch to another window
to stop.
Text fields keep middle-click paste, and sliders and scroll bars keep their
own dragging.

This works automatically on Windows, with no setting to enable. On Linux, turn
on **Middle-click autoscroll** under **Settings > Appearance**. It is off by
default, because Linux desktops usually paste the primary selection on middle
click. macOS retains its existing middle-click behavior.

## Scrolling shelves

Point at a horizontal shelf, such as a row of mixes or Recently played on
Home, and hold `Shift` while turning the mouse wheel. The shelf moves while
the surrounding page stays put. Release `Shift` to scroll the page normally.

## Dragging beyond the visible list

Hold a dragged song near the top or
bottom of an editable playlist's visible area to scroll. Scrolling gets faster
closer to the edge and stops when you move away or release the mouse. This
lets you move a song from the end to the beginning without dropping it along
the way. Clear filters and sorting before reordering playlist songs.

Drag a song from the player bar, the queue, or another
list into an open editable playlist. The line between rows marks its insertion
position. Dropping below the last row appends; the blank area of an empty
playlist accepts its first song. The source song stays in its list or queue,
and playback continues unchanged. Dragging a row within the same playlist
still moves that row.

Select several songs with `Ctrl`-click (`Cmd`-click
on macOS) or `Shift`-click, then drag any selected row. The whole selection
travels together in its displayed order, even if you selected the rows in
a different order. The preview names the first song and counts the rest.
Drop it on a sidebar playlist to append, between rows of another editable
playlist to insert, or on Favourites to save every selected song.
Selected rows have a translucent neutral highlight. Keyboard focus uses
the row highlight without an extra outline.

Dragging an unselected row copies just that song. Reordering within a
playlist still moves one song at a time. Several selected songs dropped back
between the rows of their own open playlist stay where they are. Dropped on
that playlist in the sidebar, they ask before being added again.

## Copying and pasting songs

Press `Ctrl+A` (`Cmd+A` on macOS) in a playlist, album or favourites list to
select every song the list shows. A filter narrows the selection to the
matching songs.

`Ctrl+C` (`Cmd+C`) copies the selected songs' TIDAL links, one per line, ready
to paste into another playlist or a message. `Ctrl+V` (`Cmd+V`) in a playlist
you can edit adds every song link on the clipboard to its end. Songs already
in the playlist ask before being added twice, and links that are not songs,
such as albums, are skipped.

`Ctrl+X` (`Cmd+X`) in a playlist you can edit copies the selected songs'
links the same way and removes the songs from the playlist, so `Ctrl+V` in
another playlist moves them there.

These keys edit the text instead while a search, filter or other text field
has focus.

## Album credits and reviews, recommended songs

An album page has a **Credits** button (also **Credits** in the album's
menu). It opens a window with the album's own credits (executive producers,
labels and so on) followed by every song with its roles, such as producers,
composers and engineers; names that are TIDAL artists open their page.

When TIDAL has a review of the album, it is shown under the songs with **Read
more**, which opens the whole text in a window like an artist's biography.
Names in it are links.

Under a playlist you can edit, **Recommended songs** lists ten songs TIDAL
suggests for it. **Add** puts a song into the playlist at once and takes it off
the list; **Refresh** shows the next ten. Right-click a suggestion for its
menu, or double-click to play it.

## Starting a playlist and resuming

With Shuffle off and the playlist in its original
order, its **Play** button explicitly starts at the first available song.
The selected song appears in the player immediately, including while local
playback reconnects. The full playlist remains the playback source, even
when only its first page is loaded.

Double-click a song to start at that row. Sorting the table plays its chosen
order when Shuffle is off. Shuffle chooses a random starting song unless you
choose a specific row. To resume the current song at its paused position,
use **Play** in the bottom player bar or press `Space`.

The **Shuffle** button beside **Play** starts a fresh shuffled play of that
page's songs at once and turns shuffle on for that playback, like TIDAL. This
works on albums, playlists, artists, mixes, and Favourites. The shuffle icon
in the player bar is still a plain on/off toggle.

Scroll an album, playlist, mix, radio or artist page past its header and a bar
stays pinned at the top with the cover, the title, who made it and the page's
buttons (Play, Shuffle, Add or Added, Edit and Private on your own playlist,
Share, More), so they are always in reach. A multi-disc album lists each
volume under its own **Volume** heading, with the songs numbered from 1 again;
sorting the table lists the songs flat, as on a single disc.

When you start a song from a sorted playlist or Favourites, it appears in
the player bar straight away while playback connects. Sorting changes the
order you hear without rearranging the playlist saved on TIDAL.

Sorted and filtered views omit unavailable songs and local files from playback.
The rows stay visible, and selecting a repeated song starts that occurrence. Filtering a playlist or Favourites plays only the matching songs,
including repeated entries. Play is disabled when the view has no playable
songs; it never falls back to the unfiltered playlist in that case. Clearing
the filter restores the original view. Existing Shuffle behavior is unchanged.

Right-clicking a song in an editable playlist offers **Remove from this
playlist** even when the list is sorted or filtered, since removal does not
depend on position. **Move up**, **Move down**, and drag-reorder stay on the
default order, where the rows on screen match the order saved on TIDAL.
Right-clicking the playing song in the player bar offers the same removal
while it plays from a playlist you can edit.

## Card menus

Right-clicking a card on Home, Explore, search, the artist pages or My
Collection opens its menu, anywhere on the card, including over the Play
button that appears on hover. An artist's menu offers **Bio**, **Go to artist
radio**, **Block** (or **Unblock**), **Credits**, **Follow** (or **Unfollow**) and
**Share**. Blocking an artist makes TIDAL leave them out of your mixes and
radios, and the artist page's **…** menu has the same item. A playlist's
menu opens with its cover, title and size ("19 Items (46:59)"), then **Play
now**, **Shuffle**, **Play next**, **Add to playlist**, **Follow** and
**Share**. TIDAL's **Report** is not offered: Tidalfast has no working way
to do it yet.

Album, song, mix and video menus follow TIDAL's items in TIDAL's order. An
album's: **Play now**, **Play next**, **Add to queue**, **Add to playlist**, **Add to** (or
**Remove from**) **My Collection**, **Save for Later**, **Go to artist**,
**Share**. A song's: **Play next**, **Add to queue**, **Add to playlist**, **Remove from
playlist** (on a playlist you can edit), **My Collection**, **Save for
Later**, **Go to track radio**, **Go to album**, **Credits** (for the song
that is playing), **Go to artist**, **Share**. A mix's: **Play next**, **Add
to playlist**, **My Collection**, **Share**. A video's: **Play next**,
**Add to queue**, **Save for Later**, **Go to artist**, **Share**; TIDAL's **Add to playlist**
and **Add to My Collection** for videos are not offered yet. Tidalfast's
own items come after TIDAL's: **Download**, an album's
**Credits**, a video's **Play now** and **Open on TIDAL**, then anything a
plugin adds.

## Refreshing a playlist

Choose **Refresh** in a playlist's **…** menu to reload
its details and songs, including changes made elsewhere. The
menu item reads **Refreshing…** and is disabled while loading. Current songs,
filtering and sorting stay visible. Tidalfast finishes saving your edits before
loading changes from TIDAL. If loading fails, your songs stay visible and
you can choose **Retry**.

## Playing from the sidebar

Double-click a playlist, Favourites, album, or artist row in the
Library sidebar to start playing it. A single click still opens the row's page.
Pointing at a row's cover art also shows a play button.

## Finding a setting

The Settings page has its own search field under the title. Type to narrow
the page to matching rows; sections without matches disappear. Clear the
field to see everything again.

## Keyboard and screen readers

Right-click a search, filter, settings or playlist-editing text field for
**Cut**, **Copy**, **Paste** and **Select all**. Cut and Copy require a text
selection. The usual keyboard shortcuts, including Undo, still work.
Ctrl, Cmd and Alt arrow keys move the caret while
a text field has focus. Playback and navigation shortcuts on those keys
remain available from song rows and other controls.

The main window provides screen-reader names for playback controls, library
and song rows, menus, sliders, and settings switches. `Tab` and `Shift+Tab`
move keyboard focus, shown by an outline. `Enter` activates the
focused control; on a song row, it plays that song. The row's **More** button
opens its menu from the keyboard too.

In a playlist, album or Favourites, focus a song row and use the up and down
arrows to move between whole rows in the displayed order. Rows scroll into
view as you move; Enter plays the focused song. Tab still reaches artist
links and each row's Like and More controls.
Arrow navigation selects only the destination song. Hold `Shift` with up or
down to extend or shrink the selection from its anchor. Press `Delete` to
remove selected songs from a playlist you can edit, just like **Remove from
this playlist**. On macOS, both `Delete` and `Fn+Delete` work. Text fields,
dialogs and open menus keep Delete from acting on the list.

Press and hold the player bar's volume slider to update volume without
moving the pointer. The seek slider previews
on press and seeks on release.

The default local output smooths volume and mute changes over 30 ms. Explicit
track changes and play/pause/stop use nominal 50 ms fades. Stopping can shorten
the fade to the queued audio. Natural track transitions remain gapless.

Left and right arrows adjust a focused volume slider by five percentage
points, or the seek slider by one percent of the song. Screen readers can
also read and set these sliders' values. `Ctrl+F` (`Cmd+F` on macOS) focuses
search. `Space` plays or pauses the current song even
when a song row or control has focus. Text fields keep Space for typing.
Unmodified letter shortcuts still yield to the focused control.

This is the first part of screen-reader support; testing with NVDA on
Windows is still to do.

## Keyboard shortcuts

| Shortcut | What it does |
| --- | --- |
| `Space` | Play or pause |
| `Ctrl+←` / `Ctrl+→` | Previous or next |
| `Shift+←` / `Shift+→` | Seek 10 seconds |
| `Ctrl+↑` / `Ctrl+↓` | Volume |
| `M` | Mute |
| `B` | Like or unlike the playing song |
| `S` / `R` | Shuffle / cycle repeat |
| `Q` | Queue panel |
| `Ctrl+F` or `/` | Search |
| `Ctrl+B` | Show or hide the sidebar |
| `Alt+←` / `Alt+→` | Back or forward |
| `Ctrl+H` / `Ctrl+L` | Home / Favourites |
| `Ctrl+Shift+A` / `Ctrl+Shift+B` | Playing artist / album |
| `Ctrl+A` | Select every song in a playlist, album or Favourites |
| `Ctrl+C` / `Ctrl+V` | Copy the selected songs' links / add copied song links to your playlist |
| `Ctrl+X` | Copy the selected songs' links and remove the songs from your playlist |
| `Esc` | Close the now-playing view |
| `Ctrl+,` | Settings |
| `Ctrl+/` or `?` | All shortcuts |
| `Ctrl+Q` | Quit |

On macOS, `Cmd` replaces `Ctrl`. A focused text field keeps its usual keys
for its own text.

## Controlling it from the command line

The `tidalfast` command controls the copy already running, on every
platform. Each verb prints nothing on success:

```
tidalfast play-pause          tidalfast volume 40
tidalfast play                tidalfast volume-up [percent]
tidalfast pause               tidalfast volume-down [percent]
tidalfast next                tidalfast mute
tidalfast previous            tidalfast shuffle [on|off]
tidalfast seek 15             tidalfast repeat [off|context|track]
tidalfast seek -- -15         tidalfast like
tidalfast seek-to 90          tidalfast play-uri tidal:playlist:<id>
tidalfast show                tidalfast now-playing [--raw]
tidalfast play-search <words> tidalfast play-search --kind artist <name>
```

`shuffle` and `repeat` toggle when used without an argument. Pass a state to
set it directly. `like` adds or removes the playing track from your favourites.

`play-search` searches TIDAL and plays the best match: a song, or with
`--kind album`, `artist`, or `playlist`, one of those from the start.

`now-playing` prints one readable line. `--raw` prints tab-separated fields.

A verb exits non-zero when Tidalfast is not running.

`tidalfast <link>` opens a TIDAL link, a `tidal:` URI, in the running app, or
starts the app on it. This is what the desktop runs when a link is clicked.
On Linux, Tidalfast is also an MPRIS player, so
`playerctl --player=tidalfast play-pause` and media keys work too.

Launchers such as Raycast or Alfred can use these commands. They reach
Tidalfast through a private channel only your user can open (on Windows, a
loopback port that answers only requests carrying a random token). Use the
command rather than the channel itself.

## Updates

The Windows installer and the portable Windows and Linux downloads
update from inside Tidalfast. Click the green update button to download a
release, then choose when to restart and install it. Settings can enable
automatic background downloads; restarting always waits for your click.
Closing the update window keeps a download running. You can also check for a
new release from Settings, or on macOS from the application menu.

Tidalfast checks each download before installing it. An interrupted or damaged
download leaves the running app alone, and a failed startup restores the
previous installation. Updates keep your settings and sign-ins. On macOS,
move Tidalfast to Applications before updating it.

Package-managed installations update through their package manager,
including Flatpak, apt, dnf, pacman, Nix, and Cargo. Other
installations, the AppImage among them, use the [Download page](/download/).
Portable archives identify themselves with `tidalfast-portable.txt`.

## Library order

The menu below the Library filters selects an order for each section.
**Name** and **Recently played** are available throughout; albums also offer
**Recently added**. Items you pin in Tidalfast remain at the top. Sorting or
dragging Library items changes their order only in Tidalfast; it does not
rearrange your TIDAL library.

Drag playlists to choose **Local custom order**. Selecting another sort keeps
the saved arrangement, so selecting **Local custom order** restores it.

Favourites start pinned at the top. Drag the row between pins to choose its
position, or below the pin block to unpin it. Its right-click menu also offers
**Unpin** and **Pin to top**. Dragging a song onto it still saves that song.

Tidalfast follows the system when it asks for less motion: **Animation
effects** turned off in Windows, **Reduce motion** on macOS, or GNOME's
**Animations** turned off. Its animations then show where they end at once:
the player bar's colour changes without fading, lyrics jump to the sung line,
and a new theme replaces the old one without the reveal. Spinners still turn.

## Windows taskbar controls

Hovering Tidalfast's taskbar button offers **Previous**,
**Play/Pause**, and **Next** beneath its window preview. They control the same
playing device as the player bar, update immediately, and are disabled when
there is no song or the device refuses controls. The icons follow the system
appearance and display scaling.

Clicking or double-clicking the Windows tray icon shows
and raises Tidalfast. Use **Show or hide Tidalfast** in the tray menu to hide
it again.

Closing to the tray removes the window and its preview. Reopening the main
window creates its controls again. Media
keys and the system's now-playing controls continue working while the window
is closed.

A window saved on a display that is no longer connected opens on the
primary display instead: on Windows and X11 it moves to the middle of the
primary monitor as it opens, and on macOS a position whose title bar would be
on no screen is not used. Positions on connected secondary displays still
restore. Reinstalling preserves settings; it is not needed to recover a
position left on an unplugged display.

## macOS Dock menu

Right-click or Control-click Tidalfast's Dock icon for **Play** (or **Pause**
while music plays), **Next**, and **Previous**, above the standard Dock items.
They control the same playing device as the player bar and keep working while
the window is closed to the menu bar.

## The now-playing view

Click the player bar's empty area or the track info (not its buttons or
sliders) and the now-playing view slides up, like TIDAL. It shows the big
cover with **Similar tracks**, **Credits**, and **Lyrics** tabs. Click the
player bar again, use the collapse chevron, or press `Esc` to close it.

## The top bar

The top bar reserves room for the update badge beside Search.
The bar stays above the page. Library, Queue and Lyrics keep their full height.
When the window narrows, Library, Queue and Lyrics give up width before the
top bar runs out of room, and return to the widths you chose once it widens.
With Queue or Lyrics open, the window cannot be made narrower than the room
they need beside the page.
Hover to read the available version; click to open the update window.

## MacBook notch widget

On macOS machines equipped with a display notch, Tidalfast displays an
interactive Now Playing floating island under the notch when the main
window is minimized, in the background, or closed to the tray.

Hovering over the notch expands the widget with album artwork, title,
artist, elapsed and remaining time, a seek bar matching the player bar,
and playback controls (shuffle, previous, play/pause disc, next, repeat,
and save to library). Scrubbing the seek bar seeks
within the playing track.

When music is paused, hovering still reveals the controls so you can
resume playback directly from the notch.

To turn on the notch overlay, check **MacBook notch widget** in
Settings.

## Recent

The queue panel's second tab lists tracks you played through Tidalfast.
A song is added after about 30 seconds, or halfway through a shorter song.
Paused time and seeking do not count. Choosing a row starts that song.

The list is stored in `history.json` and is never uploaded. Settings >
Storage shows its location and has a **Clear history** button.

On Windows, the main window's minimize, maximize, and close buttons share the
top bar with Tidalfast's controls. Drag an empty part of that bar to move or
snap the window, and drag a window edge or corner to resize it.

## Radio

**Go to song radio** in a song's menu opens a page of similar songs picked by
TIDAL, without starting playback. Playlist, album, and artist menus have
matching radio entries. **Play**, **Shuffle**, and a double-clicked row play
the songs the page shows. The mixes and radios in your collection are under
**Mixes & Radio** in the Library.

## Bit-perfect playback

**Settings → Audio quality → Bit-perfect (exclusive mode)** sends each track
to the audio device in its own sample rate and bit depth, untouched. The
equalizer, normalisation and crossfade are off while it is on, and the
quality badge says **bit-perfect** once the device plays the music as it is.
When the device cannot be held, Tidalfast plays through the usual output and
the badge says why.

Under the switch a status line says what reaches the device, such as
*Bit-perfect · 24-bit / 96 kHz → USB DAC (exclusive)*, or why it is not
bit-perfect (shared mode, resampled to another rate, the equalizer, the
volume). On Windows it also lists the rates and bit depths the chosen device
takes in exclusive mode. When the device has a volume control in hardware,
the volume slider moves that control and leaves the samples alone, turning
the device down from the level the system had it at (never louder);
otherwise the volume stays at 100 %. **Release the device when paused** (on by
default) lets other apps use the device during a pause; off, it stays held
for an instant resume. Each output device remembers whether bit-perfect was
on for it. Tracks in the same format play gaplessly; a track in another
format reopens the device at that format.

- **Windows** holds the device in WASAPI exclusive mode. Exclusive mode must
  be allowed in the device's Sound properties.
- **Linux** opens the sound card's own ALSA hardware device (`hw:`), past
  PipeWire or PulseAudio. It asks the sound server to release the card first,
  the way JACK does, and the server takes it back when playback stops or
  bit-perfect is turned off. Other apps cannot play on that card meanwhile.
  Pick the card under **Output device**, such as a USB DAC; with the default
  output, ALSA's default card is used. If the card stays busy, another
  program is holding it directly: close it, or stop it from using the card.
  If it is refused for permission, add your user to the `audio` group. The
  Flatpak includes the device access this needs.
  The volume slider moves the card's own hardware volume (its Master, PCM
  or Digital control) when it has one, down from the level the sound server
  left it at, and puts the level back when the card is let go; a card
  without one stays at 100 %.
- **macOS** plays through the shared output for now.

## Equalizer

**Settings → Playback → Equalizer** shapes playback on this computer. The
**Graphic** view has ten sliders; the **Parametric** view has up to twenty
bands, each a peak, a low or high shelf, a low, high or band pass, an all
pass, or a notch, with
its own frequency, gain and Q. Drag a band's handle on the graph to move it,
use the mouse wheel over it to change its width, and double-click the graph
to add a band. Changes glide in and switching the equalizer on or off
crossfades, so nothing clicks.

The **Automatic** preamp turns everything down by the highest boost, so the
equalizer does not clip; a limiter still catches what gets past. Save your
own presets by name, rename or delete them, paste an Equalizer APO or AutoEQ
profile (`ParametricEQ.txt`, or a `GraphicEQ:` line) to import it, and copy
the equalizer back out as an Equalizer APO profile. **Find your headphones**
downloads the AutoEQ project's list on request and loads the correction for
the headphone you pick. **Headphone crossfeed** (off by default, three
strengths after Bauer's BS2B) mixes a little of each side's low pitches into
the other, as speakers would, and is bypassed with the equalizer switch.
**Remember for each output device** switches the
equalizer, crossfeed included, with the output device. The equalizer is bypassed while
bit-perfect is on.

## Lyrics

Choose the microphone button in the player bar, or press **L**, to open lyrics.
Synced lyrics follow the playing line automatically. Scroll to pause following,
choose **Follow** to resume it, or choose a line to jump to that part of the song.

The expand button opens lyrics in full screen. Press **Esc** or choose the
shrink button to return to your previous window size. Full-screen lyrics
scroll smoothly and highlight the playing line automatically. Scrolling by
hand pauses following; choose **Follow** to resume. A wide
window shows the song's cover beside the lyrics, the two centred together,
and a song without words, such as an instrumental, shows just its cover in
the middle of the screen.
Quitting while lyrics are full screen no longer leaves
the next launch stuck in full screen: the window returns to its previous size.

| Dark theme | Light theme |
| --- | --- |
| ![Full-screen lyrics with the dark player bar](/assets/images/lyrics-fullscreen-dark.png) | ![Full-screen lyrics with the light player bar](/assets/images/lyrics-fullscreen-light.png) |
