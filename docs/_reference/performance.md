---
title: Performance
description: How Tidalfast's startup, memory and scrolling are measured, and the latest numbers.
nav_order: 12
---

Tidalfast aims to open its window in well under a second, to stay within
100 to 250 MB of memory in ordinary use, and to scroll a playlist of
thousands of songs at a steady frame time. This page says how those are
measured and what the last measurement found.

## Measuring

A build with the `demo` feature has a benchmark mode. It opens the demo
window, drives one scenario through ordinary frames and prints one line:

```sh
cargo build --release --features demo
target/release/tidalfast --demo --demo-bench startup   # or scroll, browse
```

- `startup`: time from process creation to the first frame laid out and
  to the first frame presented, a breakdown of the in-process steps, and
  memory after the first frame and after five seconds idle on Home.
- `scroll`: a 2,000-song playlist (250 different covers) scrolled down for
  five seconds and back up for five at 90 points a frame. Reports CPU time
  per frame (eframe's measure: layout, tessellation and GL submission,
  without waiting for vsync) at p50, p95, p99 and max, and memory after.
- `session`: the long run described under "Memory over a long session".
- `browse`: twenty pages opened one after another, 1.5 seconds each, then
  memory. `TIDALFAST_BENCH_PAGES=home,album:alb0,...` (encoded pages,
  separated by commas) opens those instead.

Every report ends with the textures egui holds, the Rust heap and the five
largest textures; `scroll` also reports the allocations made a frame, and
`browse` what each part of a pass costs on each page once it has settled
(the player bar, the sidebar, the page and the whole pass: allocations
made on the interface thread and microseconds, averaged over the passes).

`--demo-bench-out FILE` also appends the line to a file, and
`--demo-data DIR` keeps the demo's artwork cache between runs. For runs on
a machine someone is using, `TIDALFAST_WINDOW_POS=-2600,-1800` opens the
window off-screen without taking focus, and `TIDALFAST_REPAINT_TRACE=1`
logs what asks for each repaint. Run each binary several times,
interleaved, and compare medians: the window and GL context step varies by
hundreds of milliseconds with what else the machine is doing.

Memory is the working set (resident pages) and private committed bytes on
Windows, and RSS on Linux. The working set of an off-screen window can be
trimmed by Windows; private bytes are steadier.

## Latest numbers

Windows 11, NVIDIA GPU with OpenGL, 144 Hz display, release build
(`lto = "thin"`, one codegen unit), October 2026.

| Measure | Before | After |
| --- | --- | --- |
| First frame presented, median of 5 interleaved runs on a busy machine | 873 ms | 818 ms |
| First frame presented, quiet machine | 530 to 560 ms | not measured quiet |
| Fonts and loaders after the GL context | 33 to 62 ms | 0 ms |
| Private memory, idle on Home | 118 to 121 MB | 118 to 123 MB |
| Private memory, after scrolling 2,000 songs | 145 MB | 140 to 147 MB |
| Private memory, after 20 pages | 165 MB | unchanged |
| Scroll CPU per frame, p50 / p99 | 1.5 / 2.4 ms (cold art cache), 2.7 / 4.5 ms (warm) | same |
| Memory held by a finished DASH track of N bytes | up to 2N while growing, plus a full copy while it is written to the audio cache | N plus at most one segment, no copy |
| Executable, thin LTO / fat LTO | 28.6 MB / 27.5 MB | thin kept |

Where startup goes: about 40 ms in-process before the window (logging,
settings, the application state and its threads), then 435 ms (quiet) to
over a second (busy machine) creating the window and its OpenGL context in
eframe and the driver, then about 20 ms for the first frame. The walk of
the installed fonts for other scripts now runs on its own thread from the
start, so it overlaps the GL context instead of following it.

## Re-audit before v7

Same machine and build, after the feature batch (Luna, EQ, compact mode,
animated covers, skeletons). The demo window was opened off-screen
(`TIDALFAST_WINDOW_POS`), so a visible-window frame rate was not measured.

| Measure | Baseline | v7 |
| --- | --- | --- |
| First frame presented, 3 runs | 818 ms | 272 to 289 ms (531 ms on the first, cold run) |
| Private memory, idle on Home | 118 to 123 MB | 116 to 123 MB |
| Private memory, after scrolling 2,000 songs | 140 to 147 MB | 138 MB |
| Scroll CPU per frame, p50 / p99 | 2.7 / 4.5 ms (warm) | 0.95 / 1.50 ms, 0 frames over 20 ms |
| Private memory, after 20 pages | 165 MB | 146 MB |
| Repaints per second while playing (demo) | 1 to 3 | 2 to 3, all from the player bar's progress |
| CPU while playing, demo, 20 s | not recorded | 1.8% of one core |

Not measured: the idle (nothing playing) repaint count and a flat EQ
versus an adjusted one, which need a signed-in or scripted session rather
than the demo. Luna is off by default and is started only when its master
switch is on; Luna on versus off is measured below. The flat-EQ cost was
not checked.

### Luna plugins

Same machine, release demo build (`--features demo`), window off-screen,
the demo's song playing, 45 s after start then 60 s sampled every 2 s,
three runs each, interleaved (October 2026). Luna on: DiscordRPC,
VolumeScroll, total-play-time, scroll-to-playing and oneko installed
(`TIDALFAST_DEMO_LUNA`; the demo refuses plugin IPC, so DiscordRPC ran
without a Discord connection). Luna off: no `TIDALFAST_DEMO_LUNA`, so the
runtime is never started.

| Measure | Luna off | Luna on, five plugins |
| --- | --- | --- |
| CPU, one core, median (runs) | 0.3% (0.0, 0.3, 2.4) | 4.8% (3.7, 4.8, 5.3) |
| Private memory | 119 to 121 MB | 146 to 148 MB |
| Working set, average | 123 to 149 MB | 157 to 397 MB (Windows trims an off-screen window's set unevenly) |

With Luna off nothing of the runtime runs, as the code says: the numbers
match the v7 idle figures above.

#### Event-driven pass (2026-10-15)

Same method, at Idle priority, five popular plugins: DiscordRPC, LastFM
(which no longer loads, see `luna-compat.md`), total-play-time (player
bar), CoverTheme (theme) and oneko. `TIDALFAST_LUNA_STATS=1` logs every
10 s how often the Luna thread woke, why, how long it was busy and what ran;
it showed where the idle cost was:

- oneko's `requestAnimationFrame` loop woke the runtime ~33 times a second
  although its sleeping cat changes a sprite frame a couple of times a
  second, and every change rebuilt the overlay;
- each overlay build recomputed the CSS cascade of the whole document
  (~10 ms in QuickJS), because every change, even one inline style or the
  player bar's time text, reset every cached cascade;
- the player bar's `--progress` and `aria-valuenow` changed at every repaint,
  so each repaint of the window sent a new tree to reconcile.

Changes: inline-style changes redo only that element's cascade (all of them
when a selector reads `[style]`); text and `aria-`/`data-` attribute changes
no selector names keep every cascade; unchanged subtrees reuse their
resolved styles; class changes no longer rebuild the rule index; animation
frames fall to ~10 Hz after ten frames that changed nothing and return to
~60 Hz at the next change or user input; the progress anchors move in whole
seconds; timers and frames set from a plugin's own callbacks no longer read
a stack trace to find their owner. While the window is minimized or closed
to the tray, plugins see `document.hidden`, frames pause and plugin timers
run at most once a second (a browser's background tab).

| Measure | Luna off | Before | After |
| --- | --- | --- | --- |
| CPU, one core, median (runs) | 0.21% (0.21, 0.70, 0.03) | 6.05% (7.14, 4.69, 6.05), a second interleaved set 3.49% (0.49, 5.19, 3.49) | 2.12% (1.71, 3.34, 2.12) |
| Luna thread busy (stats) | | 2.7 to 7.5% | 0.45 to 0.9% |
| Luna wake-ups per second | | ~33 | 10 to 23 |
| One overlay build | | ~10 ms | ~2 ms (4 ms after a page change) |
| Private memory | 116 MB | 154 to 166 MB | 154 to 161 MB |

The machine was shared with other builds, so single runs vary a lot; the
stats lines are steadier than the totals. Not yet at the Luna-off level:
what remains is the window repainting for each visible change (oneko's
sprite, the playing song's second ticks) and oneko's 10 Hz idle frames.

#### Unfocused frames and host-only syncs (2026-10-15, later)

The live signed-in profile showed the rest of the cost while a song plays:
every second the player bar's time moved, the anchor tree went to the
runtime and the whole overlay was rebuilt (about 4.5 ms of QuickJS each,
0.6% of a core) although nothing a plugin did had changed. Changes the UI
makes to text and `aria-`/`data-` attributes no selector names now leave the
overlay alone (plugins reacting to them mark it themselves); only structure,
class, matched attribute, viewport and colour-scheme changes rebuild it.
While the window is not focused, animation frames run at ~30 Hz and idle
loops (oneko's sleeping cat) at ~4 Hz instead of ~10 Hz.

Five plugins (DiscordRPC, scroll-to-playing, CoverTheme, total-play-time,
oneko; LastFM is no longer supported), same method, interleaved, three runs
each, the off-screen window unfocused:

| Measure | Before (e8b0a20) | After |
| --- | --- | --- |
| CPU, one core (runs) | 2.64% median (2.64, 2.35, 3.12) | 1.57% median (1.57, 0.34, 1.60) |
| Private memory | 155 to 163 MB | 153 to 158 MB |

Still above the 1% goal in two runs of three; what remains is the window's
own repaint for each visible change (oneko's sprite, the player bar's
seconds) and the overlay sends that follow plugin changes.

#### Background passes shared with the clock (2026-10-15, later)

Each overlay change woke the window twice: the runtime's wake asked for a
pass, and handling its `Changed` request in that pass asked for another.
The second is gone (a pass reads the latest overlay as it starts). While the
window is in the background, a plugin's change no longer wakes it at once:
it waits for the window's own next pass when one is due within a second
(the playing song's next second) and otherwise for at most 250 ms
(`AppHost::change_delay`), so oneko's sprite, total-play-time's text and the
seconds tick share passes; with focus, changes show at once as before. In
the background, animation frames that change something every frame
(spinning covers, visualisers) keep ~30 Hz, while loops that change now and
then (oneko's sleeping cat flips its sprite every few steps) stay at ~4 Hz
instead of returning to 30 Hz after each flip.

Same five plugins and method, interleaved, three runs each, off-screen and
unfocused, Idle priority:

| Measure | Before (4d002ff) | After |
| --- | --- | --- |
| CPU, one core (runs) | 1.63% median (0.98, 1.63, 2.20) | 0.31% median (0.31, 0.16, 0.62) |
| Private memory | 153 to 156 MB | 155 to 163 MB (one run 206 to 209 MB) |

#### Plugin `<video>` and the audiovisualiser (2026-10-15)

(Historical: the audiovisualiser was answered from the output tap by its
module's name until 2026-10-22; that per-plugin code is gone and it is not
supported now, see `luna-compat.md`.)

Same method (release demo build, off-screen and unfocused, Idle priority,
45 s after start then 60 s), two interleaved runs each. CatJam alone: its
clip is a 790x604 VP9 WebM with alpha (325 frames), drawn 52 points tall.
The audiovisualiser with a helper plugin that skips to the next song once
(the visualiser mounts on a song change).

| Measure | Luna off | CatJam | audiovisualiser drawing |
| --- | --- | --- | --- |
| CPU in the first 45 s (start, plugin load, the clip's one decode) | 1.2, 1.3 CPU-s | 3.1, 7.1 CPU-s | 6.0 CPU-s |
| CPU after, one core | 0.10%, 0.05% | 1.46%, 2.66% | 4.17% |
| Private memory | 113 to 116 MB | 131 to 133 MB | 129 MB |

The clip decodes once in 1.3 s on one low-priority thread (325 frames of
video and alpha, kept as 69x53 images, 4.6 MB). What CatJam costs after
that is the window's own pass for each frame of the cat (15 a second
unfocused, 30 focused); nothing is decoded again. The audiovisualiser costs
what its DOM animation does: it restyles 38 bars every 20 ms, so the overlay
is rebuilt and the window repaints about 30 times a second; reading the tap
is a 128-point FFT per read. In the run where it did not mount (no song
change reached it) it cost 0.10%, as nothing reads the tap.

A first build used another pure-Rust decoder, `vp9dec`: it spawned up to
ten threads per frame for its loop filter, and on Windows each thread
start left the OpenGL driver's per-thread memory behind (private memory
grew to 270 to 660 MB while CatJam decoded, against 37 MB for the same
decode in a test process without GL). `rusty_vp9` decodes on the calling
thread only.

## Latest numbers (2026-10-16, performance pass)

Windows 11, NVIDIA GPU, 144 Hz display, release demo build, window off-screen
and unfocused, Idle priority, machine shared with other work. Compared with
the v7 and Luna figures above, after everything since (store stars, avatars
and Rate, the single queue scroll, the plugin click layer, recent playlists,
AltPlay, plugin video and the audiovisualiser, the plugin developer log,
kept updater downloads).

| Measure | Recorded before | Now |
| --- | --- | --- |
| First frame presented, 4 runs | 272 to 289 ms | 292 to 327 ms (window and GL step 234 to 247 ms) |
| Private memory, idle on Home | 116 to 123 MB | 118 to 120 MB |
| Scroll CPU per frame p50 / p99, 2 runs | 0.95 / 1.50 ms | 0.76 to 0.84 / 1.48 to 1.58 ms, 0 frames over 20 ms |
| Private memory, after scrolling | 138 MB | 128 to 132 MB |
| Private memory, after 20 pages | 146 MB | 132 MB |
| Session benchmark, end, private | 202 to 207 MB | 202 MB (flat over five laps) |
| Idle on Home, passes | about 0.25 a second | 1 pass in 4 s |
| Luna off, idle, CPU | 0.0 to 0.3% | 0.03 to 0.47% |
| Luna on (DiscordRPC, VolumeScroll, total-play-time, scroll-to-playing), idle, CPU / private | 4.8% (v7, five plugins, playing) | 0.08 to 0.21%, 132 MB |
| Now playing, covers off, CPU, Luna off / on | not recorded | 11.4% / 13.0% (about 58 passes a second either way) |
| Now playing, animated cover, no pointer, passes a second | 72 | 24 |

Nothing regressed. First-frame times are the busy-machine range of the
earlier runs (the quiet-machine 272 ms was not reproduced; the step that
moved is the window and OpenGL context, which is the driver's). Two causes
of waste were found and fixed, each with a test that fails without the fix:

- **Store pictures.** An author's picture the server refused (a deleted
  GitHub account, or every picture once GitHub's hourly limit of 60
  unauthenticated requests is spent) was forgotten at every two-second
  eviction and asked for again by the next frame that drew its card, so a
  page left open asked GitHub again every two seconds for each such picture.
  A refused picture is now left alone for five minutes (offline and timeout
  failures still retry at once). A GitHub link ending in `?v=4`, which is how
  GitHub writes its own, counted as having a size and was fetched at 460
  pixels (282,530 bytes for one account; 10,710 asked at 80) to be drawn 40
  points wide; it is now asked for at 80 pixels. Everything else was already
  right and is unchanged: a picture is fetched once into the shared image
  cache (memory and the disk file named by the URL's hash), drawn only when
  its card is on screen (`is_rect_visible`), scaled off the interface thread
  to the pixels it covers and uploaded without mipmaps like every cover.
  The Themes tab asked for a pass every 500 ms for as long as it was open when
  it had no catalogue and none loading (offline, or GitHub's limit spent); it
  now waits for a fetch only while one runs. A loaded store page asks for
  no passes.
- **Now playing with an animated cover.** The turning background asked for
  its own timer on top of the clip's frames. egui takes one frame time
  (16.7 ms) off every timed request, so the 33 ms request ran at about 60
  passes a second, not the 30 the code said, and together with the clip's 24
  frames and the player bar the view ran 72 passes a second. With a clip
  playing, the clip's frames now turn the background too (it turns by the
  clock, so only the step is 24 a second instead of 60): 24 passes a second,
  CPU 13.7% down to 6.5% of a core on this machine.

The tilt of the now-playing cover (`cover_tilt.rs`) was measured and left as
it is. While the pointer moves over the window, every pointer event repaints
(egui-winit asks for one per event, a pass per display refresh at most), so
the view runs at the display's rate with or without the tilt asking, and a
pointer moved over empty parts of the view gave the same pass rate (145 a
second) as one moved over the cover; the tilt's own request only continues
for the fraction of a second the spring takes to settle after the pointer
stops. The cost of hovering is therefore the cost of a pass, not of the tilt.
GPU utilisation from Windows' per-process counter did not give a usable
comparison here (it read 2 to 8% for the same 145 passes and rose when
passes fell, because the GPU clocks down under a lighter load), so it is not
quoted; passes a second and CPU time are.

With covers off, the turning background's 33 ms request still ran at
about 58 passes a second for the reason above. The project chose 30 frames a
second; see "Now-playing background at 30 frames a second" below.

## Now-playing background at 30 frames a second, and covered windows (2026-10-17)

An installed app averaged about a quarter of a core overnight, most
likely the now-playing view: its turning background asked for a frame every
33 ms, which egui's predicted frame (16.7 ms taken off every timed request)
turned into about 60 passes a second, and on Windows a window under other
windows kept drawing, because winit reports occlusion to eframe on Wayland,
X11 and macOS but not on Windows.

- **30 frames a second.** The background asks through
  `theme::repaint_after`, which adds the predicted frame back, for its next
  frame a thirtieth of a second after the last. A pass that comes anyway
  (the player bar's, an animated cover's) within half a frame of the next one
  is that frame, and an earlier one leaves it where it was, so other passes
  add no frames of their own. The turn, the coast and the fades follow the
  clock, so they move at the same speed in half the frames (0.2° a frame
  instead of 0.1°). Its stops are unchanged: paused and settled, minimised,
  hidden, reduced motion, and an animated cover driving the passes. The
  1.2 s scale change on play and pause still asks egui for every frame, as
  every `motion::value` does.
- **Covered windows (Windows).** `occlusion::Watch` checks at most every
  500 ms whether the client area is fully under other windows: it walks the
  windows above it (`GetWindow(GW_HWNDPREV)`) and takes each visible frame
  (DWM's extended frame bounds) away from what still shows, stopping as soon
  as nothing shows. A window counts as covering only when it is visible, not
  minimised, not cloaked (another virtual desktop), not click-through, not a
  tool window or tooltip, not layered unless fully opaque, not a topmost
  composition overlay and not shaped by a region; the 8-point (scaled)
  rounded corners of a window that is not maximised are left showing. More
  than 64 uncovered pieces or 4,096 windows answer "showing". While covered,
  every pass checks, and out-of-context WinEvent hooks (window moves of
  top-level windows, show, hide, destroy, minimise, z-order, cloaking, the
  foreground window) wake the window at once; they exist only while it is
  covered. A covered window gets the drawing budget of one nobody can see
  (`theme::unseen`): the background, the player bar's progress steps, the
  lyrics' next line and glide, animated covers, skeletons and spinners ask
  for nothing. Playback, the clock's own passes, polling, the media controls,
  the tray, plugins and the network keep the window's own budget. The other
  targets never report covered here.

Measured with release demo builds of `e56a5df` (before) and this change
(after) on a signed-in profile, "Lose Yourself" (no animated
cover), Blur background, animated covers on, window 1000 by 800 at
`-4000,-4000`, unfocused, BelowNormal priority, volume 0, 60 seconds each.
"Covered" is a borderless, opaque window of the test's own, 20 pixels
larger on each side, shown without activation over the off-screen window.
Paints are from `TIDALFAST_REPAINT_TRACE`.

| Scenario | Before: CPU, paints a second | After: CPU, paints a second |
| --- | --- | --- |
| Now playing, visible | 11.4%, 60.3 | 8.4%, 30.8 |
| Now playing, covered | 11.7%, 60.3 | 2.1%, 3 (the clock's passes only) |
| Now playing, minimised | 1.7%, 0 | 1.8%, 0 |
| Now playing, paused | 0.5%, 0.1 to 0.3 | 0.3%, 0.1 |
| Home, playing | 3.0%, 3.1 | 2.5%, 3.0 |

Taking the cover away was noticed and drawn 0.5 ms after the window event
(`--verbose` log), and the next second showed 31 passes again. The check
itself is a walk of `GetWindow` and a few DWM queries in a pass that runs
anyway, at most twice a second while the window shows.

Limits: a window covered only by a mosaic of other windows, by a window with
rounded corners that exactly matches it, or by a see-through window keeps
drawing (it counts as showing). A plugin's own `<video>` and music videos
are not throttled when covered. Linux and macOS keep relying on winit's
occlusion events; this change was built and run on Windows only (the
non-Windows code paths were reasoned about, not compiled).

## Final pass before 1.0 (2026-10-19)

Windows 11, a 16-core desktop CPU, NVIDIA GPU with OpenGL, 144 Hz display, release
demo builds of `20d0eaf` (before) and this pass (after), window at
`-4000,-4000` and unfocused, run interleaved on a machine busy with other
builds. CPU is exact cycles (`QueryProcessCycleTime`) over the sampled
window divided by the processor's nominal 4.49 GHz; wake-ups are context
switches per thread from the system's thread counters; passes are from
`TIDALFAST_REPAINT_TRACE`, whose lines in demo builds now also count the
allocations made (the bench's counting allocator); GPU memory and engine use
from `\GPU Process Memory` and `\GPU Engine` counters. Where a pass's time
goes was found with a sampling profiler (the main thread suspended once a
millisecond and its stack walked, against a build with line tables only for
the measurement) and with timers around each part of `ui::show`.

| Measure | Before | After |
| --- | --- | --- |
| Idle on Home, passes (signed-in demo, nothing playing) | 1 every 4 s | 1 every 15 s |
| Idle on Home, CPU, two runs | 0.15%, 0.16% | 0.11%, 0.46% (one noisy run) |
| Home, a song playing here, CPU / passes a second | 0.56% / 2.0 | 0.62% / 2.0 |
| Now playing, CPU / passes a second | 5.1% / 30 | 5.5% / 30 |
| Now playing, minimised, CPU | 0.07% | 0.07% |
| Private memory, idle on Home | 112 to 115 MB | 114 to 117 MB |
| GPU memory, idle on Home (dedicated / shared) | 29 / 19 MB | 29 / 19 MB |
| Threads | 20 to 21 | 20 to 21 |
| First frame presented, 2 runs | 356, 405 ms | 383, 605 ms |
| Scroll CPU per frame p50 / p99, 2 runs | 1.65 to 1.68 / 2.45 to 2.49 ms | 1.40 to 1.65 / 2.41 to 2.53 ms |
| Allocations a frame while scrolling, p50 | 3,578 | 3,578 |
| Private memory after 20 pages | 128 to 133 MB | 131 to 133 MB |
| Engine thread, nothing loaded or a paused song ready, wake-ups in 1.6 s | 3 | 0 |
| Decoder thread after the end of a song is sent, wake-ups in 0.9 s | 45 | 0 to 1 |

The rows that did not change were measured to check that nothing
regressed; they move only with the machine. The covered-window case was not
repeated: the harness's covering window did not reliably engage the
occlusion check in either build, so the figures above it in this page stand.

Changes, each with a test that fails without it:

- **Idle passes.** A signed-in window's logic woke every 4 seconds for the
  playback poll, but polls go out no more often than every 15 seconds
  (`REMOTE_POLL_MIN`), so three passes in four drew the window for nothing.
  The pass is now asked for when the next poll is due, or sooner for a timer
  of `tick`'s own: a recheck asked for, the open queue panel's refresh, and
  settings or the session waiting to be saved (now on time, 2 seconds after
  the change, instead of at the next 4-second pass).
- **Engine thread.** With nothing loaded, or a paused song whose sound is
  ready, the engine looked at its stream twice a second although nothing can
  change without a message; it now sleeps until one comes. A paused song
  still loading is looked at every 500 ms, as before, instead of every 5 ms.
- **Decoder thread.** Once a song's end is sent the decoder waited for a
  possible seek by looking 50 times a second for as long as the stream
  lived: the last seconds of every song, and indefinitely for a song paused
  near its end. It now sleeps until a seek or the stream's end wakes it.

What the profile showed and was left:

- Our own interface code is a small part of a pass: about 0.2 ms of the main
  thread in the now-playing view and 0.3 to 0.7 ms on most pages while they
  repaint. The rest of the main thread is eframe, winit and the driver. On
  Windows, eframe makes the GL context current again twice a frame
  (`change_gl_context`, emilk/egui#4173), 12 to 20% of the main thread's busy
  time at 30 frames a second; the fix belongs in egui. The NVIDIA driver's
  own worker thread costs about two thirds as much again per presented frame
  and wakes about 40 times a second even when the window draws once in 15
  seconds; nothing in the app controls it.
- Home copies its whole data model each pass (its shelves, recent plays,
  top artists and tracks are cloned so they can be drawn with the app
  borrowed): about 3,000 allocations and 0.35 to 0.45 ms a pass, three to
  five times other pages. Done in the next section.
- The play history is written, with the file synced to disk, on the
  interface thread once per song (about 5 ms here), and the session every
  10 seconds while music plays. A synced write and write-through move take 2
  to 9 ms on this machine's NVMe disk, more on slow disks. Done in the next
  section: a writer thread.
- Memory: of about 103 MB of committed private memory idle on Home, the Rust
  heap is 5 MB, textures 6 MB (55 of them, the font atlas included), and about
  40 MB is the driver's write-combined memory; the rest is the driver's and
  the system libraries'. Nothing in the app's own use stood out.

## Interface passes (2026-10-19, later)

The same machine and method as the pass above, release demo builds of
`d4fb980` (before) and this pass (after), each with the `browse` run's
per-part report, run twice, interleaved. Allocations are exact and the same
on every run; microseconds move with what else the machine does (it was
busy with other builds), so they are given as the two runs.

| Per pass, once settled | Before: allocations, us | After: allocations, us |
| --- | --- | --- |
| Player bar (every page) | 223; 53 to 110 | 146; 47 to 112 |
| Sidebar (every page) | 387 to 389; 60 to 129 | 359 to 361; 54 to 126 |
| Home, the page | 2,989; 220, 285 | 610; 110, 169 |
| Home, the whole pass | 3,672; 368, 496 | 1,188; 237, 387 |
| Search, the page | 1,229; 132, 174 | 505; 91, 124 |
| Artist, the page | 798; 170, 203 | 446; 156, 179 |
| Album, the page | 923; 229, 257 | 662; 201, 252 |
| Top songs, the page | 844; 163, 197 | 658; 165, 227 |
| Liked songs (Tracks), the page | 742; 163, 175 | 599; 154, 219 |
| Playlist, the page | 549; 140, 143 | 478; 115, 159 |
| Library grids, profile, uploads, the page | 194 to 345 | 194 to 337 |

| Measure | Before | After |
| --- | --- | --- |
| Home, a song playing here, CPU, two runs | 0.55%, 0.39% | 0.31%, 0.55% |
| Now playing, CPU, two runs | 4.3%, 5.5% | 5.2%, 4.8% |
| Private memory, Home playing | 112 to 113 MB | 112 MB |
| Play history and session writes on the interface thread | each one | none |

CPU here is the process's processor time over 20 seconds, after 8 seconds
settling. Home redraws twice a second while a song plays and the
now-playing view 30 times, so a pass cut by a tenth of a millisecond is
lost in the noise of a busy machine: the passes are cheaper, the process
CPU did not measurably change. Captures of Home, a playlist, an album, an
artist, search, top songs, Tracks and the now-playing view, dark and light,
at 1280 by 800, are pixel for pixel the same before and after (the
now-playing view's moving background aside).

Changes, each with a test that fails without it:

- **Home's lists are shared, not copied.** Its shelves, recent plays, top
  artists and tracks and recommendations are held behind `Arc`
  (`model::HomeData`), so a pass clones a pointer, and a song list's rows
  and links are kept from pass to pass while the list is the same. Search
  results and an artist's top tracks are held the same way. The test draws
  Home and counts the allocations: 3,055 a pass before, 676 after.
- **What plays is read once a pass.** Every song row asked whether it was
  the playing one, and every card whether it held the playing context,
  through calls that copied the playing song (about 14 allocations) and the
  context link each time. The pass now reads both once and rows and cards
  compare against them; the player bar no longer builds the playing song's
  item twice. Top songs and Tracks keep their rows' links with their cached
  rows instead of making them each pass, and Home's cards borrow their
  titles and links. The test draws top songs, search and an artist: 1,027,
  1,275 and 906 allocations a pass before, 799, 550 and 524 after.
- **State files are written on a thread of their own.** The play history
  (once per song) and the session (every 10 seconds while music plays) are
  handed to a writer thread that writes them as before: replaced whole,
  synced, the history readable by its owner only. Writes land in order and
  a file asked for twice is written once with the newer text. Saving the
  state to quit, to sleep or when the session ends waits until everything
  asked for is on disk, so nothing is lost; a failed history write is
  written again at the next save, as before. The tests hold the writer busy
  and check that a history save returns without writing, and that quitting
  leaves the session on disk.

Left, measured and not worth the risk or the change:

- The sidebar builds its playlist rows (about 11 allocations a row) every
  pass: 110 allocations and about 12 us with the demo's ten playlists. A
  cache needs a revision of the playlist list that every change to it
  bumps, and a stale row would be a visible bug; the rest of the sidebar's
  cost is egui laying out its rows.
- Home's shortcut tiles (144 allocations, about 25 us) copy a playlist the
  account owns for its menu; the copy feeds the right-click menu.
- The rest of a pass is egui's own work for each widget drawn (laying out
  and shaping text, building shapes), which the app cannot cut without
  drawing less.

## Plugins, the pointer and covered windows (2026-10-20)

An installed 1.0.1 build was seen at about 57% of one core with seven Luna
plugins (DiscordRPC, no-remix, total-play-time, tidal-genres, Tidal Tags,
CatJam, oneko), animated covers and the Blur background. Release demo builds
of `4745d29` (before) and this pass (after), window 1240 by 800 at
`-7300,-5300` (a spot of its own: other test windows at `-4000,-4000` covered
it), the song playing here, 15 seconds settled then 15 sampled, interleaved,
on a machine busy with other builds. CPU is exact cycles over the sample
(`QueryProcessCycleTime`, per thread with `QueryThreadCycleTime`); passes from
`TIDALFAST_REPAINT_TRACE`; "allocations" there count every thread. "Seven
plugins" is the set above, installed in the demo with `TIDALFAST_DEMO_LUNA`;
"eight plugins" is no-remix, total-play-time, scroll-to-playing,
DeviceDisconnectFix, NativeFullscreen, Song Downloader, Tidal Tags and
VolumeScroll. "Pointer moving" plays a dev script that moves the pointer
every few milliseconds, which runs passes faster than a display would (about
400 a second off-screen): its pass counts are the harness's, the Luna
thread's share is not, because pointer moves reach plugins at most 30 times
a second either way. "Covered" opens an opaque test window over it.

| Scenario | Before: CPU, passes a second | After: CPU, passes a second |
| --- | --- | --- |
| Home, nothing playing | 0.08%, 0.2 | 0.09%, 0.2 |
| Home, playing | 0.60%, 2 | 0.58%, 2 |
| Now playing (Blur) | 5.4%, 30 | 5.9%, 30 (noise) |
| Now playing, animated cover | 6.3%, 15 | 6.5%, 15 |
| Queue panel open, playing | 0.67%, 2 | 0.62%, 2 |
| Eight plugins, Home playing / idle | 0.71% / 0.12% | 0.72% / 0.13% |
| Seven plugins, Home playing (CatJam's frames) | 7.2 to 7.6%, 20 | 6.7 to 8.3%, 20 |
| Seven plugins, now playing | 7.0%, 29 | 6.5 to 7.1%, 29 |
| Cotton Candy / Tidal TUI theme, Home playing | 0.96% / 0.70%, 2 | 0.93% / 0.69%, 2 |
| Seven plugins, now playing, pointer moving: Luna thread | 32% of a core | 9.1 to 9.5% |
| Seven plugins, now playing, pointer moving: process | 88 to 90% | 55 to 69% |
| Seven plugins, Home playing, covered | 7.4 to 8.0%, 21 to 24 | 1.0 to 1.6%, 2 to 4 |
| Seven plugins, now playing, covered | 5.3%, 23 | 1.1 to 1.3%, 4 to 5 |
| Private memory, Home playing (none / seven plugins) | 112 / 144 to 151 MB | 111 / 144 to 151 MB |

Where it went, each change with a test that fails without it:

- **The now-playing cover's lean.** While the pointer moves over the
  now-playing view the cover leans anew on every pass, and the lean is an
  inline style on TIDAL's tilt box, so the anchor tree went to the runtime
  on every pass, each a reconcile on the Luna thread (2,386 trees in 10
  seconds). The lean now goes out at most every 33 ms, as often as pointer
  moves reach plugins, and the pass after the wait sends the latest one
  (157 trees in 10 seconds).
- **A covered window and plugins.** A plugin's `<video>` (CatJam's cat)
  asked for each of its frames, 20 to 30 passes a second, while other windows
  covered the window; it now asks for none while the window is covered or
  hidden, and plugins see the page hidden then (frames paused, timers at
  most once a second), as they did while it was minimised.
- **An anchor tree per pass.** The player bar noted the playing song's
  position to the millisecond, so with plugins on every pass that came while
  a song played (a frame of CatJam, of the background, of a moving pointer)
  built the whole anchor tree to find it the same as the one sent, which
  shows whole seconds. Drawing Home with the seven plugins' overlay: 2,613
  allocations a pass before, 1,629 after; a playlist, 3,598 and 1,963;
  Home with Cotton Candy, 2,497 and 1,532. The demo's own song does not
  advance its position, so the table above does not show this; a real song
  does.
- **Themes' hover recheck.** With a `:hover` rule (most themes), every tree
  sent asked for one more pass to work out the element under the pointer,
  also with the pointer outside the window or the boxes unchanged (a tree
  goes out each second while a song plays). It is asked for now only with
  the pointer over the window and a box moved.
- **The clock's pass.** The clock asked for its next pass at most 1000 ms
  ahead; egui brings timed passes back a predicted frame early, so a pass
  early in a second landed just before the next and needed a second one
  50 ms later. Playback alone (a song on another device, a bar stepping once
  a second) now wakes the window once a second.

Screenshots of Home, a playlist, an album, Home in Light, the queue panel
and lyrics at 1280 by 800 are byte for byte the same before and after;
Home with Cotton Candy differs from itself between runs of the same build
by at most 1 of 255 in a few pixels, and before and after differ the same.

Memory over time: seven minutes of cycling Home, Tracks, the queue and
lyrics panels, Settings, the next song and Back (`TIDALFAST_DEV_SCRIPT`)
grow private memory by about 7 MB without plugins and by 7 to 10 MB a
minute with Tidal Tags installed. Playing on Home without navigating stays
flat with the same plugins. The growth is Tidal Tags' own: each time a
track row appears it registers a format listener
(`MediaItem.withFormat(unloads, ...)`) that holds the row's cells and is
released only when the plugin unloads, about 4 listeners a second while
navigating (931 after three and a half minutes, the document itself steady
at 343 nodes). TidaLuna's `withFormat` keeps listeners the same way, so the
plugin grows the same in TIDAL; the fix belongs in the plugin (let go of a
row's previous listener when it fills the row again). The plugin runtime's
heap is capped at 256 MB.

Not measured here: a visible window at the display's rate (the off-screen
window's passes are not paced by the display), and a long-lived real
profile; the 57% was not reproduced as such, the nearest being now playing
with the seven plugins and a moving pointer.

## Boxes after a plugin's change (2026-10-22)

A plugin that changes a style and then measures the element read the box
from before the change: radiant-lyrics widens its Lyrics tab and, at the
tab's `transitionend` or 200 ms after the click, hangs its options menu
under it, and the menu opened under the tab's old place. In Chromium a read
lays the page out at once, and the next frame has the new boxes.

Measured with a probe plugin that toggles the now-playing Lyrics tab's
`min-width` every 1.2 s and polls `getBoundingClientRect` (and a
`ResizeObserver`) until the new width shows: "idle" with nothing else
changing, "steady" with another element restyled every frame, "transition"
with `transition: min-width .12s` on the tab. Release demo builds, window
2200 by 1260 at `-4000,-4000`, focused through the dev script's new `focus`
line unless said otherwise; milliseconds from the change, median (p90).

| Case | Before | After |
| --- | --- | --- |
| Idle | 779 (827) | 9 (14) |
| Steady | 50 (60) | 9 (18) |
| While a transition runs | did not finish within the probe's 1 s wait per toggle | 8 (9) |
| Idle, window in the background | 794 (808) | 10 (13) |
| Steady, window in the background | 62 (67) | 10 (14) |
| `ResizeObserver` callbacks per change | 1 | 1 |

What held them back, each with a test that fails without its fix:

- **Timers set after an `await`.** The runtime worked out the time to the
  next timer before the promise jobs of a tick ran, so a timer set in the
  code after an `await` (`await sleep(2)` in a polling loop, and the
  overlay's own flush timer when a plugin changed a style there) waited
  for whatever woke the thread next: the playing song's next second, up to
  a second later. It asks again after those jobs, without running anything
  (`__luna_tick("peek")`), and a tick that keeps finding timers due hands
  the messages waiting (clicks, trees) their turn after 50 ms.
- **The rect throttle.** Rect-only changes went to the runtime at most
  every 100 ms. For 100 ms after an overlay that can move boxes
  (`DomOverlay::may_move_boxes`: an anchor's layout properties or text, or
  plugin elements added or removed; not colours, opacity, transforms, a
  plugin element's restyle or its words) every frame's boxes go, the frame
  after it is drawn too (a box laid out from last frame's sizes shows the
  change a pass later), and that tree says which overlay it was laid out
  with (`HostTree::laid_out`). Scrolling and animations with no change from
  plugins keep the 100 ms throttle.
- **The background wait.** In a window without focus a plugin's change
  waited for the window's next pass, up to a second. A change that can move
  boxes is drawn at once in a window someone can see; other changes still
  share passes, and a covered or minimised window still rests.
- **Reading before the boxes came back.** The first box read after such a
  change publishes it if it is still waiting for the 33 ms flush, and waits
  at most 64 ms for the tree laid out with it (`dom.wait_layout`), as a
  browser lays out on a read. radiant-lyrics' menu now opens under the
  widened tab (left 1982, width 150, as the tab), as in TIDAL; before it
  opened at the tab's old box (left 2045, width 87). Its heavy overlay
  builds (about 200 ms each with its style sheet) are its own and unchanged.

spinny-cover, up-next, quick-queue and 3d-cover draw the same before and
after (Home, a playlist with a hovered row, now playing with the pointer on
the cover), with no errors in their logs.

Passes and Luna wake-ups, the same six plugins as the pass of 2026-10-20
without no-remix (DiscordRPC, total-play-time, tidal-genres, Tidal Tags,
CatJam, oneko), same window, two interleaved runs each, 15 s settled then
15 s sampled. Other builds were running on the machine, so CPU is
indicative only (it varied by up to a factor of two between runs of the
same build); the pass and wake-up counts are what to compare.

| Scenario | Before: passes/s, Luna wakes/s, CPU | After: passes/s, Luna wakes/s, CPU |
| --- | --- | --- |
| Home, paused, background | 28, 8.5, 12 to 14% | 28, 8.2 to 8.7, 14 to 15% |
| Home, playing, background | 20, 7.3 to 7.7, 9 to 11% | 19 to 20, 7.0 to 7.6, 9 to 11% |
| Home, playing, focused | 57, 12.5, 21 to 22% | 57 to 58, 12.3 to 17.9, 22 to 26% |
| Now playing, pointer moving, background | 145, 58, 42 to 48% | 145, 57 to 58, 45 to 74% |
| Now playing, pointer moving, focused | 145, 74, 45 to 104% | 145, 74 to 76, 35 to 40% |
| Home, playing, covered | 2 to 4, 1.9 to 2.1, 3% | 1.7 to 2, 2.1 to 2.2, 3% |

## Animated covers

TIDAL's animated covers (Settings, Appearance) stay **off by default**: on
this machine the GPU decoding path keeps about 90 MB of extra private
memory alive while a cover plays, three times the 30 MB allowed for it, and
the now-playing cover adds about 3% of one core, over the 2% allowed.

How they are kept cheap when on:

- Only the now-playing cover and an album's header animate, so at most
  two clips run. Home and other shelves show still covers.
- The smallest size TIDAL serves at or above the pixels drawn is fetched,
  and Media Foundation decodes on the GPU and scales to the drawn size
  there (OpenH264, FFmpeg or GStreamer elsewhere).
- Each frame carries the time it falls due. The interface asks for a pass
  then, and a pass that comes anyway up to half a frame earlier shows it
  instead, so in the now-playing view (whose background turns 30 times
  a second while playing) the cover adds no passes. The request is
  never a zero delay, which egui answers with two passes: that doubled
  every frame's cost before.
- The texture is rewritten in place each frame, not allocated again.
- At most 24 to 30 frames a second, the clip's own rate, and 15 while
  the window is out of focus or the system saves energy. With reduced
  motion no clip starts.
- A clip not drawn (scrolled away, another page, compact mode, a paused
  song) decodes nothing; one not drawn for 3 seconds and two passes is
  stopped. An idle paused view no longer reopens its clip every few
  seconds.
- With the setting off nothing runs: no video fetch, thread, decoder,
  GPU device or texture, and turning it off stops every clip and releases
  the shared GPU device at once, unless a music video (which the setting
  does not cover) is using it. A test checks that no clip starts with
  the setting off or with reduced motion.

Measured with the demo build (`--demo-show now-playing-local` keeps a song
playing, `--demo-page album:alb0`, `TIDALFAST_DEMO_VIDEO_COVER` set to a
real cover), a window at `-4000,-4000`, 20 seconds after 35 of warm-up,
on a machine busy with other builds. The video GPU engine share is
`\GPU Engine(pid_*)\Utilization Percentage`. "Focused" forces the full
rate with `TIDALFAST_VIDEO_COVER_FOCUSED=1`; an off-screen window is
otherwise unfocused.

| Case | Passes a second | CPU, one core | GPU | Private memory |
| --- | --- | --- | --- | --- |
| Album page, covers off (before or after) | 0.25 | 0.0% | 0.0% | 113 to 116 MB |
| Album page, unfocused, before | 30 | 13.4% | 2.7% | 207 MB |
| Album page, unfocused, after | 16 | 7.0% | 3.1% | 211 MB |
| Album page, focused, before | 48 | 10.8% | 2.9% | 210 MB |
| Album page, focused, after | 24 | 6.2% | 3.5% | 209 MB |
| Now playing, covers off | 59 | 10.5% | 0.9% | 127 MB |
| Now playing, cover, unfocused, after | 59 | 13.7% | 2.0% | 218 MB |
| Now playing, cover, focused, after | 72 | 14.1% | 2.1% | 220 MB |
| Now playing, cover, software decoding | 59 | 48.5% | 1.0% | 178 MB |
| Home (no animated shelf), covers on or off | 0.25 | 0.0 to 0.3% | 0.0% | 117 to 120 MB |

Not verified: a visible window. Windows still painted the off-screen window
(every pass drew), but it may throttle such a window in ways a visible one
is not. The 59 passes a second of the now-playing view with the covers off
come from the player bar and the turning background, not from the covers.

## Memory over a long session (v7.3 follow-up)

A v7.3 window on Windows was seen at 431 MB and at 743 MB of working set
after an hour of browsing big playlists, artist pages, Explore, videos and
now playing, with downloads running. `--demo-bench session` stands in for
that hour: fifty 550-song playlists (covers from a pool of 1,500) opened
and scrolled to the end, the album, artist, library and video pages, and
now playing with lyrics, in five laps, then 15 seconds idle on Home. Each
mark reports the working set, private bytes, the textures egui holds, the
bytes Rust code holds (demo builds count their allocations) and private
bytes after compacting every process heap. Same machine, release build,
window at `-4000,-4000`, October 2026.

What the counts showed:

- **Rust's own heap is small**: 5 MB at startup and 22 to 31 MB after the
  session, page data included. Compacting the heaps freed nothing, so the
  allocator is not fragmenting and mimalloc or trimming would not help.
- **Covers cost about twice their texture bytes in private memory, and the
  driver keeps it.** A probe that uploaded 2,000 textures of 64 pixels
  raised private bytes by 85 MB with mipmaps (49 MB without) for 31 MB of
  pixels, and a hundred 512-pixel textures by 145 MB; deleting them gave
  back almost none. The OpenGL driver copies every texture into system
  memory and keeps the memory of deleted ones for reuse, so the most covers
  ever held at once sets the floor. The same session with artwork switched
  off ended at 148 MB; with it, 300 MB or more.
- **The first run of a new executable starts about 55 MB higher** (the
  driver compiling and caching shaders): compare warm runs only.
- **Audio** was not in the demo session, but by construction the current
  song, the preloaded next one and each running download were each held
  whole in memory (100 MB and more for a hi-res track), a track played
  from the cache or the offline store twice over, and up to six downloads
  at once.

Changes:

- **Streams and downloads go to a file as they arrive.** A song being
  played goes to a file in the audio cache's folder and is moved into the
  cache once whole; a download goes to a file in the offline folder and is
  moved into place. A song from the cache or the offline store is read
  from its file as it plays. The system's file cache keeps what was just
  written at hand, outside the process. A disk that refuses a write sends
  the stream back to memory. With the audio cache turned off, a played
  song stays in memory as before. Tests check that such a download holds
  nothing in memory, reads back whole, is kept by a move and leaves no file
  behind when nobody keeps it.
- **Artwork budget 64 MB to 32 MB, checked every 2 seconds instead of 20**,
  so a fast scroll never runs far past it, and covers painted in the last
  2 seconds are never let go of, so a page showing more than fits keeps its
  own covers. A test checks that covers on screen stay over the budget.
- **Explore and "View all" pages and user profiles** are kept for the
  eight most recent, like the other page caches, and read again on return.

| Measure, private bytes | Before | After |
| --- | --- | --- |
| Idle on Home after startup (warm runs) | 118 to 123 MB | 116 to 120 MB |
| Session, after the first lap (cold first run) | 338 MB | 286 MB |
| Session, end, idle on Home (cold first run) | 342 MB | 304 MB |
| Session, end, working set (cold first run) | 466 MB | 232 MB |
| Session, laps 2 to 5 | flat, 333 to 343 MB | flat, 303 to 310 MB |
| Session with artwork off (attribution) | not run | 148 MB |
| A hi-res song playing, plus the next preloaded | about 2 × 100 MB | the decoder's few MB |
| A song from the cache or offline store | 2 × its size | the decoder's few MB |
| Each running download | its whole size | 64 KB |

Both versions stay flat from lap to lap: nothing grows without bound.
Not measured yet: warm session runs of both builds side by side, the
`scroll` and `browse` scenarios after the change, a real signed-in session
with music and downloads, Luna plugins (their QuickJS heap is capped at
256 MB and was not seen in use) and video playback.

## Cover textures without mipmaps

Covers are scaled on the CPU (Lanczos3, off the UI thread) to the pixels
they are drawn at, in steps of 16 pixels instead of 32, and uploaded as
plain linear textures without mipmaps. A copy is within 15 pixels of the
size it is drawn at, so the GPU samples it about one texel to a pixel and a
mipmap chain (a third more texture, and more in the driver's own copy) has
nothing to filter. Artwork drawn unscaled (a picked cover file) keeps its
mipmaps. The source rendition is already chosen by display pixels and
screen scale (`art_target`). Only a copy standing in for a moment while its
own is made can be drawn smaller than it is.

Session benchmark, release demo build, window off-screen, warm runs after
one throwaway run of each executable (private bytes / working set at the
end, idle on Home): before 266 and 260 MB / 171 and 175 MB; after 202, 207
and 203 MB / 158, 159 and 152 MB. Screenshots of Home, an album, the
sidebar-style library list and now playing at 100% and 150% zoom differ
from before by a mean of 0.1 to 0.35 out of 255 per channel.

## Themes at rest (round 15)

How to measure a Luna theme's idle cost: not with `Process.TotalProcessorTime`.
Windows charges CPU time to whichever thread a 15.6 ms scheduler tick finds
running, so an app that wakes about three times a second can be charged
anywhere from 0 to a whole tick for each wake-up: the same binary and theme
read 16 ms, 375 ms and 906 ms (58 wake-ups, 58 ticks) in three runs, with and
without a theme. Use exact cycle counts: `QueryProcessCycleTime` for the
process and `QueryThreadCycleTime` per thread (the threads are named: `main`,
`luna`, `dev-script`, the GL driver's), read over the same 20 s after the
page settled. Release build with the `demo` feature, off-screen and unfocused,
Idle priority, Home (and a playlist page), the clock running; `TIDALFAST_LUNA_STATS=1`
logs what the Luna thread did every 10 s, including (as a `js stats` line in
the runtime log) the time of each stage of an overlay build.

What a themed app does at rest: once a second the player's clock text and
the progress bar's `style="--progress: .41"` change, the anchor tree is sent,
and the Luna thread reconciles it and builds the overlay again (about 25
million cycles with a trivial theme, the floor listed as "Luna on, empty
theme" below). Lunallena, Catppuccin Mocha Blue and Snusehunden cost five to
ten times that: each names the `style` attribute in a selector
(`[class*="_sidebarWrapper_"][style="width: 240px;"]`, `tr[style*="126, 251,
238"]`), and an inline style change then discarded the cascade of every
element, so the progress bar's change every second made all 204 elements
match every rule again (40 to 130 ms a second, `css: cascade one: 2040x` per
10 s in the stats). Now the elements the change can reach are matched again:
the element and what is inside it, the siblings after it where a rule reads
the style next to a sibling combinator, and the ancestors where a `:has()`
reads it; everything else keeps its cascade.

Millions of cycles over 20 s at rest (about 1.1% of a core per 1000 at
4.5 GHz, an assumed clock). Before: the tip of round 14 with round 15's
anchors; after: round 15 as committed. Two runs of a pair agreed within 5% except
where noted.

| Theme | Home before | Home after | Playlist before | Playlist after |
| --- | --- | --- | --- | --- |
| No theme (Luna off) | 470 | 360 | not run | 411 |
| Luna on, empty theme | 1060 to 1130 | not run | not run | not run |
| Mocha (Catppuccin) | 1161 | 1113 | 1052 | 1094 |
| Tidal Light | 1085 | 1088 | 1062 | 1030 |
| Ticua | 1275 | 1262 | 1264 | 1210 |
| Meteor Night | 1425 | 1398 | 1364 | 1334 |
| Sugarcoat | 1871 | 1671 | 1736 | 1672 |
| Cotton Candy | 1896 to 1910 | 1858 | 1964 | 1791 |
| Cotton Candy Nights | 2029 | 1939 | 1952 | 1905 |
| Drift (animated) | 2692 | 2688 | 1092 | 1084 |
| Snusehunden | 5590 | 1919 | 5670 | 2068 |
| Lunallena | 5325 to 5632 | 1973 | 5787 to 6886 | 2091 |
| Catppuccin Mocha Blue | 11150 | 1964 | 11657 | 2053 |

Lunallena on Home: 5400 before, of which the Luna thread 4800 (the UI thread
440); after 1970, the Luna thread 1330. The remainder above the empty theme
is the cascade of the changed element and the walk of the page for the
overlay (about 60 million cycles a second); Drift's Home is its background
animation on the UI thread, by design. No other theme is an outlier: all
but Drift's Home are between 1.0 and 2.1 billion cycles, about 1.1 to 2.3% of
a core with the clock and progress bar running. The no-theme figure moved
from 470 to 360 with the state of the machine, not with the code (Luna off
runs none of this).

Round 16 found what the remainder was: each overlay build matched the
`::before`/`::after` rules of every anchor again (about 200), resolved the
hundreds of root variables a theme like Cotton Candy declares, and made the
keyframes' styles again, every second, with nothing changed. They are kept
while their inputs are the same. Same method, same session, millions of
cycles over 20 s on Home (Luna thread, process): Cotton Candy 1365 and 1926
before, 697 and 1346 after; Lunallena 1372 and 1969 before, 729 and 1277
after; empty theme 529 to 551 and 1015 to 1102.

## Plugin windows (2026-10-24, round 1)

A plugin's `BrowserWindow` (see "Plugin windows" in `luna-dom-bridge.md`)
is an egui deferred viewport with its page in a runtime on a thread of its
own. Measured on Windows 11 (NVIDIA, OpenGL), debug builds with the `demo`
feature (dependencies at opt-level 2, the app crate unoptimized), the
build before the change against the change, interleaved, off-screen
windows:

| Measure | Before | After |
| --- | --- | --- |
| `--demo-bench scroll` with Luna on, CPU per frame p50 (3 runs each) | 68.25, 68.99, 65.96 ms | 71.02, 68.20, 66.23 ms |
| Allocations a frame, p50 | 273k, 274k, 282k | 283k, 275k, 278k |
| Idle CPU, miniplayer installed, no window open, 30 s after 60 s (2 runs) | 0.16 %, 0.16 % of a core | 0.16 %, 0.16 % |
| Private memory then | 125, 123 MB | 127, 124 MB |

With no plugin window open a frame does one atomic load more
(`Luna::plugin_windows`) and the renderer's accessors one thread-local
check each; the numbers above are the same within run-to-run noise.

The cost of an open window (the change, miniplayer's 700x380 window):

- Open and idle: no passes at all (the window repaints only when its page
  publishes a new overlay or the user acts on it), its page's thread
  asleep; CPU 0.16 to 0.31 % of a core for the whole process, as with no
  window; private memory about 5 MB more, one thread more.
- Changing (its lyrics button clicked twice a second, each click running
  0.3 to 0.4 s of CSS transitions): 107 passes a second for the off-screen
  window, which no vsync slows, at 2.9 ms a pass for the window's own
  layout and shapes (mean; max 6 ms), unoptimized; the app's window makes
  no passes for it. The process used 70 % of a core in that debug build.
  A release build and a frame cap for plugin windows (as the app's endless
  decorations have) are for round 2.

## Findings and decisions

- **Long lists are already virtualised.** Track tables, card grids and the
  downloads list lay out only the rows in view, and playlist rows come
  from a cached table rebuilt only when the playlist changes. Scrolling
  2,000 songs stays well under a 7 ms frame budget at p99.
- **Artwork** is fetched once into a disk cache, scaled off the UI thread
  to the pixels each cover covers (in 32-pixel steps), uploaded once, and
  the decoded copy is then dropped. Textures and pending bytes share a
  32 MB budget with least-recently-painted eviction (see above).
- **Audio**: a track streams into a file in the audio cache (memory when the
  cache is off) and plays as it arrives; the next
  song is preloaded 15 seconds before the end. A DASH stream (TIDAL's
  lossless and hi-res) now reserves room for its remaining segments after
  the first one, instead of doubling its buffer as segments arrive, and
  the finished file is shared with the audio-cache writer instead of
  copied. A hi-res track of 100 MB used to hold up to about 230 MB at the
  moment it finished; it now holds about 100 MB. Adaptive streaming
  measures each download with one clock read and a few atomic additions
  per 64 kB chunk, in the download's own thread; it allocates nothing,
  adds no timer or wakeup, and is read once per song when the next one
  opens ([Audio output](audio-output.md#adaptive-streaming)).
- **Fonts**: the fallback faces for other scripts (Chinese, Japanese,
  Korean, Indic and more) are mapped from disk (`fastframe-fonts` maps the
  regular cut, `src/fonts.rs` the bold ones), so only the pages in use are
  resident and none of it counts as private memory. They were read whole
  into memory before, about 57 MB on a typical install.
- **Release profile**: fat LTO made the executable 4% smaller with no
  measurable change in frame time, for about three times the link time.
  `lto = "thin"`, `codegen-units = 1`, `panic = "abort"` and `strip` stay.
- **Allocator**: frame CPU time is dominated by layout and painting, not
  allocation, so mimalloc was not added.
- **Network**: one HTTP client (HTTP/2, pooled connections) serves all
  TIDAL requests. Home, the library, playlists and favourite tracks are
  cached on disk per account and shown before TIDAL answers. Playback start
  latency needs a signed-in account and is not covered by the demo
  benchmark.
