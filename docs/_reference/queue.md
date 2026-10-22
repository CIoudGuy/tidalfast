---
title: The Queue's Rules
description: What the queue shows, what Add to queue does, and what the app promises about both.
nav_order: 2
---

The queue is the list of what plays next. It has two parts. On top,
under **Playing next**, are the songs you queued yourself. Below them,
under **Next up**, are the songs that come next in whatever playlist or
album is playing. Your songs always play first.

Above both, **Playing from** names where the playing song came from: the
playlist, album, artist, or mix, which opens when clicked, Liked
Songs, or a radio named after the song, playlist, album, or artist it is
based on, which opens the radio's page. The line shows only while a song is
playing from somewhere TIDAL reports.

These are the rules the app follows. The queue tests in `src/app.rs`
check every one of them.

Starting and resuming are separate actions. With Shuffle off, a playlist's
**Play** button starts at its first available song in the selected order.
A page's or card's **Play** always starts the first item in the page's
order and turns Shuffle off; **Shuffle** turns it on and starts a random
item. Double-clicking a row starts there and keeps the current Shuffle mode.
Shuffle switched while the song is still loading (a song far down a long
playlist waits for the pages before it) is the mode it plays in: with
Shuffle off, the songs after it follow in the playlist's order.
A song in an artist's Popular tracks plays the artist from that song, all of
their songs as the artist's **Play** does, under the artist's name; not only
the songs shown.
A new context starts with an empty History in the play queue panel, which
lists only songs played since (up to 100, newest last, saved with the
session). The song just left joins History the moment the song changes;
clicking a row goes back to it in the same context; the listening history
is untouched. In the panel, one click on a Next up row skips to it (rule 5
below); the second click of a double-click is ignored, so it never skips
twice. History, the playing song and Next up are one list in the
panel's single scroll area (no separate History scroller); it opens with
"Playing from" at the top, History above it, and keeps that place on song
changes until the user scrolls. The
panel is fully opaque. **Play** in
the player bar resumes the current song at its paused position.

Since 0.10.0, the Shuffle button on a collection page changes the
global playback mode without starting that collection. It can be selected
before anything is playing; the next **Play** uses the selection.
While another collection plays, toggling Shuffle changes that playback's mode
without switching to the page's collection. The queue then reflects the new
play order.

A shuffle plays each recording once: a song listed again under another track
id or album (same ISRC, or without an ISRC the same title and first artist
within two seconds, an explicit and a clean version counting as one) is
left out, also when later pages join. Tidalfast keeps the last 200 songs
played per context, and the last 200 overall, in `recent_plays.json` in the
state folder (track ids and ISRCs only, written a few seconds after a
change). A new shuffle starts on a song not among the recent ones and plays
those last; a shuffled Repeat all round does not open with the last 50 songs
(at most half the context) of the one before; autoplay never brings back
one of the last 100 songs played.

Since 0.8.0, starting a playlist in its original order explicitly
names its first available song from the loaded prefix. If that prefix is not
loaded, it requests playlist position zero. A page loaded from the middle
never becomes the beginning. This keeps the full TIDAL playlist context;
the app does not replace it with a shortened list of loaded songs. A request
waiting for local playback to reconnect keeps the song chosen at the click.

Sorted and filtered views omit unavailable songs and local files from their
playback requests. The displayed rows keep their positions, and selecting a
repeated song starts that occurrence. A filtered playlist or My Collection tracks view
plays its matching songs in displayed order, including duplicates. An empty or entirely
unplayable view disables Play instead of starting the unfiltered context.

Since 0.9.0, starting another song on this computer keeps the
selected Repeat mode, with Shuffle on or off. Loading a playlist no longer
silently disables repeat in the playback engine.

Since 0.9.0, silencing the old song during a local track change
keeps the decoder paced while the replacement loads. Cached audio no longer
races to the end and causes an unwanted extra skip during that handoff.

Since 0.9.0, an unexpected playback engine restart retains the engine's
in-memory playback state. Recovery restores the same song and position, paused
or playing, with its playlist context, exact shuffle order, manually queued
songs (including duplicates), repeat settings, and pending context pages.
Playback started on the replacement engine takes precedence over recovery.

1. **The list shows the play order.** The top row plays next, followed by the
   rows below it.

2. **Add to queue adds a song to your part of the queue.** It goes after
   the songs you queued earlier and before the playlist's songs. Queue
   the same song twice and it plays twice. A double-click only counts
   once.

   Since 0.9.0, an album's **Add to queue** adds its playable songs
   in album order, including repeated songs. This also works for singles and
   EPs. A fully loaded album appears immediately. Otherwise, a loading notice
   appears while all its track pages are fetched, then its songs are appended
   together after the songs already queued. A failed fetch adds no partial
   album. Clear queue and sign-out cancel an album still being fetched.

3. **When a song starts, its row leaves the queue.** It doesn't matter
   how it started: the song before it ended, you pressed Next, you
   clicked its queue row, or a media key skipped to it. Only that
   occurrence leaves: another copy you queued remains until its own turn.

4. **Next removes the top row right away.**

5. **Playing a row from the queue skips to it.** The rows above it are
   skipped and removed, as if you had pressed Next down to it. The rows
   below it stay, and the playlist keeps going afterwards.

6. **Starting a playlist or album keeps your songs.** The rows underneath
   change to the new collection; your songs stay under **Playing next**
   and play before the collection continues. If you queue an album and then
   start it from the Library, its first song plays now and the separately
   queued album still follows. **Clear queue** removes those manual copies
   while keeping the playing album's remaining songs.

7. **Clear only removes your songs.** The trash button sits beside the
   *Playing next* heading and empties that section; the playlist's rows
   below stay.

8. **Changes appear immediately.** Tidalfast owns the queue and updates its
   own player directly. Toggling shuffle rechecks the queue so the new
   playback order appears promptly without waiting for the song to finish.

9. **Closing the app keeps the queue.** Tidalfast saves it locally. When you
   resume the last song, it restores your queued songs and playlist position.

   The saved session also holds the playback position, Shuffle with the
   order of the songs due next, Repeat, the mute level, and whether the now
   playing view was open. Starting the app plays nothing: the song waits
   paused in the player bar, and **Play** resumes it at the saved position.
   A saved song TIDAL no longer has is skipped for the next one in its
   playlist or album. A song or music video TIDAL cannot be reached for
   (the network is down) is not skipped and nothing leaves the queue: it
   stays the playing item, at its position, and plays once the network is
   back (see [How It Connects](/how-it-connects/)).

Since 0.8.0, selecting several playlist rows and choosing
**Add to queue** preserves repeated occurrences in their selected order.
For example, selecting B, C, B adds all three rows. A repeated click still
counts once, and the notification reports only the rows actually added.

10. **Dragging within *Playing next* reorders it.** Dropping a song, dragged
    from elsewhere, at a position in *Playing next* inserts it there, and
    dragging a row already in *Playing next* elsewhere in the same section
    moves it. *Next up* is never a drop target: it plays from the current
    context, not from a list Tidalfast rewrites. While *Playing next* is
    empty, drop the song on the player bar's Queue button instead.

11. **Any row after the playing song can be removed on its own, as in
    TIDAL.** Every row of *Playing next* and *Next up* in the play queue panel
    has a cross at its right (the **...** menu of a row, and a right-click,
    offer **Remove from queue** too). A queued song leaves *Playing next*;
    other copies of it stay. A *Next up* row leaves the list at once and the
    song is left out of the playlist or album for as long as it plays (also in
    Repeat all rounds); it is back when the playlist or album is started
    again. A lagging answer from the engine that still lists the row does not
    bring it back. The playing song and the History rows have a **...** menu
    instead. This works on this computer's queue only; another device's queue
    is not rewritten.

12. **A plugin can take any row out of the queue, as in TIDAL.** A Luna plugin
    (No Remix removes the remixes of a playlist) that removes a row with
    `playQueue/REMOVE_ELEMENT` takes a row out exactly as rule 11 does for the cross: a
    song you queued, or a *Next up* row. A plugin's clear, like **Clear**
    (rule 7), empties *Playing next* only. This works on this computer's
    queue; another device's queue is not rewritten.

13. **Blocked songs and artists are never played from the queue.** TIDAL's
    block lists (Block on an artist's menu, "Don't play this" in the player
    bar, Settings > Blocked) are read once per session, and TIDAL's own apps
    leave what they name out of what they play; so does Tidalfast. A song
    is passed over when its turn comes if its track id is blocked or any
    artist on it, main or featured, is: in the playing context (every
    Repeat all round too), in *Playing next*, and in autoplay and radio.
    The rows stay in album and playlist pages and in Liked Songs; only the
    queue panel's *Playing next* and *Next up* leave them out, since they
    will not play. A song started by name (a double-click, or the song a
    page names) plays even if blocked, and a context that starts without a
    named song begins on the first song that is not blocked. A context
    with nothing left to play asks for autoplay instead of looping. Block
    and Unblock apply at once, before TIDAL answers; a refusal restores the
    old state. Nothing is removed from the queue: lifting a block makes the
    songs playable again. Music videos are never blocked.

14. **Settings can keep a song out of the queue.** With **Allow explicit
    content** off, songs marked E are greyed out in lists and left out of
    what plays: a playlist, album, artist or mix skips them, Add to queue
    adds only the others, and a shuffle never starts on one (videos are not
    included, as in TIDAL). **Allow AI content** does the same for songs TIDAL
    labels AI. This is a rule of its own, applied with a song's availability
    and with blocked content, and it takes effect for what the engine loads
    next: songs already in the queue stay until they play or are removed.
