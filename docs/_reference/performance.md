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

## Plugin windows: frame cap and release numbers (2026-10-24, round 3)

An animating page (a transition, an animation, a layout settling) no longer
asks egui for every frame: what its drawing asks for is noted for its
window (`plugin_dom::request_frame`), and the window asks for its next frame
no sooner than a sixtieth of a second with the keyboard focus, a thirtieth
without, and not at all while minimised or covered (`plugin_windows::pace`).
Off-screen, where no display paces a window, the debug build had drawn 107
passes a second while miniplayer's transitions ran (round 1; a release build
without the cap was not measured).

Release build with the `demo` feature (`lto = "thin"`, one codegen unit),
Windows 11, NVIDIA (OpenGL), the app's window and plugin windows off-screen,
BelowNormal priority, the demo paused, CPU over 30 s as a share of one core
for the whole process:

| Scenario | CPU | Private memory | Plugin window passes |
| --- | --- | --- | --- |
| lyrics-everywhere installed, no window open (Plugins page, after 60 s; 2 samples) | 0.31 %, 0.26 % | 188, 189 MB | none |
| Its lyrics window open and idle (20 s after opening; 2 samples) | 0.05 %, 0.21 % | 213 MB | none after the first second |
| miniplayer's mini player with its lyrics button clicked twice a second (0.3 to 0.4 s of transitions each time), focused; its taskbar widget open too | 20.9 % | 210 MB | 55 to 58 a second, 0.63 to 0.74 ms each (mean), 1.4 to 3.6 ms at most |

An idle plugin window costs no passes and no CPU beyond noise; its page's
runtime, document and second GL surface take about 24 MB. While a page
animates its window stays under its 60-frame cap; the process's 21 % is the
window's passes, the page thread's script and style work for each frame of
the transitions, and its overlay going to the UI. Not measured: an animating
window without the focus (the unit test
`plugin_windows::tests::an_animating_page_draws_at_the_windows_frame_cap`
checks its 30-frame cap) and a window on screen, where vsync also paces it.

## Plugin windows: see-through, egui fork (2026-10-24)

egui moved from crmne/egui apps-0.36 (ba6790fe) to the owner's fork
CIoudGuy/egui (466a177a: egui 0.36.2 with crmne's changes, plus options for
see-through windows). A see-through frameless plugin window now has no
system frame, and with Luna on eframe picks a GL config with alpha for its
windows (on X11 and macOS; Windows and Wayland keep their config). Debug
builds with the `demo` feature, Windows 11, NVIDIA (OpenGL), before against
after, interleaved, the app's window off-screen, Luna on with miniplayer
installed (its taskbar widget open off-screen):

| Measure | Before | After |
| --- | --- | --- |
| `--demo-bench scroll`, CPU per frame p50 | 46.86, 46.93, 47.18, 44.71, 45.27 ms | 46.82, 46.45, 43.06, 45.21 ms |
| The same, p95 | 66.48, 67.92, 74.46, 70.77, 69.00 ms | 71.04, 72.09, 63.53, 66.15 ms |
| Idle CPU on the home page, 30 s after 60 s | 0.26, 0.57, 0.10 % of a core | 0.31, 0.16, 0.31 % |
| Private memory then | 224, 174, 171 MB | 171, 175, 174 MB |

Three more bench runs (one before, two after) drew their frames about
300 ms apart, as an off-screen window does when the machine is busy, and
are left out. The app's window costs the same within run-to-run noise.

## Page-wide plugin scans (2026-10-24, plugin round 8)

uwu-ifier, for every node added to the page, reads `getComputedStyle(el)`
of every element in it, and the runtime's watchdog stopped it after 5 s on
an album opened from the Albums grid. Two things made that slow: each
`getComputedStyle` resolved the element's whole chain of ancestors and
every root variable again, and read the element's box (after a style
change, each read waited for the UI's next layout); and the host tree's
reconcile inserted a new page one element at a time, so an observer heard
of it in one record per element and scanned the page for each.

Now an element's cascaded result is kept with it and worked out from its
parent's while nothing it depends on changed, the declaration handed out is
built once and shared, the box is read only when a size is, and a new part
of the page goes in as one node, as React commits it (one record with its
root). Values are unchanged (the tests check them against the old path).

Measured with two synthetic probe plugins in the demo, off-screen at 2137 by
1183, debug build (the app's own crate unoptimised; QuickJS and every other
dependency at `opt-level = 2`, as in development, so the script engine's
times are close to a release build's), BelowNormal priority, before and
after on the same machine:

| Measure | Before | After |
| --- | --- | --- |
| `getComputedStyle` over the album page (452 elements), first pass | 91 ms (201 µs an element) | 21 ms (46 µs) |
| The same, again with nothing changed | 88 to 89 ms (195 to 197 µs) | 2 to 3 ms (4 to 7 µs) |
| Home (363 elements), first pass and again | 215 ms, 72 to 74 ms | 13 ms, 2 ms |
| A box read of each element of the album page | 6 to 9 ms | 2 to 3 ms |
| An observer scanning the page for every added node, the album page opened | 226 records, 62 ms a scan; stopped by the watchdog at 5 s | 1 record, one scan of 29 ms |
| The same with only the cached styles (before the one-record commit) | | 226 scans in 1,372 ms |
| The same on the Albums page | 14 records, 871 ms | 1 record, 15 ms |
| The window's pass on the album page while the probe scans (browse bench, CPU a pass) | 2.89 ms | 3.02 ms |

The window's frame time does not change: the scans run on the plugins'
thread, never on the interface's; what the change shortens is how long a
plugin's work holds that thread (and every other plugin's timers, events and
observers behind it). The first box read after the page loads now waits for
the first layout itself (126 ms on Home in the run above), which the first
`getComputedStyle` used to pay. uwu-ifier itself, live in a signed-in
session: the album opened from the Albums grid is rewritten and the plugin
keeps running.

## egui fork, round 1: where time goes (2026-10-24)

Release builds with the `demo` feature, Windows 11, a 16-core desktop CPU,
NVIDIA GPU with OpenGL, the demo window off-screen and repainting
continuously (the bench asks for every pass), BelowNormal priority on a
machine busy with other builds. Where a pass goes inside egui, epaint,
egui_glow and eframe was read from their own `profiling` scopes, recorded
with puffin in a measurement build (not committed) and summed per scope on
the main thread: microseconds a frame of each scope's own time, without the
scopes inside it.

| Main thread, us a frame | Scroll 2,000 songs | Home | Album, playlist, Home | Now playing, lyrics | Cotton candy, Home | Cotton candy, scroll |
| --- | --- | --- | --- | --- | --- | --- |
| All scopes | 1,585 | 903 | 898 | 501 | 1,778 | 20,540 |
| eframe making the GL context not current and current again (two a frame) | 610 | 409 | 421 | 288 | 444 | 853 |
| `App::ui`: the app's pass and egui's widgets in it | 470 | 223 | 212 | 79 | 773 | 2,659 |
| `App::logic` (Luna's overlay with a theme) | 21 | 14 | 16 | 14 | 86 | 15,890 |
| egui_glow painting: buffer uploads and draws | 131 | 95 | 95 | 33 | 126, and 88 in backdrop callbacks | 251, and 138 |
| Tessellation | 71 | 70 | 58 | 17 | 132 | 236 |
| Image loading (`try_load_image`, `try_load_texture`) | 144 | 18 | 17 | 5 | 19 | 79 |
| egui-winit reading the window's state | 19 | 15 | 16 | 17 | 19 | 25 |
| Text layout (galley cache misses) | 25 | under 1 | under 1 | under 1 | under 1 | 107 |
| `swap_buffers` (no vsync off-screen) | 18 | 13 | 14 | 12 | 19 | 36 |

What it showed:

- The largest cost egui itself added was eframe's GL context switch on
  Windows: 38 to 57% of the main thread's time a frame without a theme
  (25% on Home with cotton candy), outside the frame time eframe reports. glutin's WGL `is_current` compares only the context,
  which every window shares, so eframe turned its early-out off on Windows
  and made the context not current (which flushes) and current again twice
  a frame (emilk/egui#4173).
- Painting re-specified the vertex and index buffers for every mesh: two
  `glBufferData` calls per mesh, 33 to 131 us a frame.
- Not costs: font atlas uploads (1 us a frame or less), text layout
  outside fast scrolling (the galley cache), repaint scheduling.
  The rest of a pass is the app's own and egui's widget work in `App::ui`;
  with a theme, Luna's per-pass work in `App::logic` dwarfs everything
  egui does (16 ms a frame while scrolling with cotton candy).
- Startup (process start to first frame presented, 224 to 259 ms) is the
  window and GL context (154 to 172 ms, the driver), the first frame 8 to
  10 ms, fonts and loaders 0 ms. Idle on Home the window makes no passes and
  the process uses 0.25 to 0.38% of a core.

Changes in the fork (CIoudGuy/egui, branch `tidalfast`), each measured on
its own with the same profile:

| Commit | Change | Measured |
| --- | --- | --- |
| 406ad072 | eframe remembers which window surface the context was last made current with and switches only when another one is painted | context switching 400 to 690 us a frame before, under 1 us after; main thread a frame on Home 895 to 486 us, scrolling 1,259 to 740 us |
| 052d8172 | egui_glow uploads a frame's meshes in one vertex and one index upload, binds a texture only when it changes | painting 77 to 21 us a frame on Home, 103 to 18 us scrolling; eframe's frame CPU scrolling p50 0.67 to 0.56 ms, p95 0.98 to 0.81 ms |
| 36760640 | `FontColorTransferFunction::PerColor`: glyph coverage adjusted per text colour in egui_glow's shader, as Chromium's Skia does | see "Text" below |

Before (466a177a) against after (36760640 and the app using it), release
builds, interleaved, three runs each unless two are given:

| Measure | Before | After |
| --- | --- | --- |
| Scroll, eframe's frame CPU p50 | 0.67, 0.80, 1.05 ms | 0.57, 0.58, 0.59 ms |
| Scroll, p95 | 1.00, 1.20, 2.10 ms | 0.83, 0.89, 1.01 ms |
| Scroll, p99 | 1.27, 1.54, 2.62 ms | 1.08, 1.17, 1.53 ms |
| Allocations a frame while scrolling, p50 | 2,407 to 2,410 | 2,407 to 2,410 |
| Main thread a frame, profile build: scroll / Home / now playing | 1,259 to 1,716 / 895 to 1,102 / 501 to 538 us | 627 to 742 / 413 to 433 / 243 us |
| Now playing with lyrics, CPU over 20 s (two runs) | 11.5, 11.7% of a core | 11.2, 11.2% |
| Idle on Home, CPU over 20 s (two runs) | 0.36, 0.38% | 0.25, 0.27% |
| First frame presented | 224, 235, 248 ms | 235, 247, 259 ms |
| Private memory: idle on Home / after scrolling / after the browse pages | 114 to 118 / 128 to 135 / 129 to 130 MB | 116 to 122 / 132 to 137 / 131 to 132 MB |

The context switch is not in eframe's frame time, so the scroll numbers
show the upload change; the switch shows in the main thread's time a
frame. Whole-process CPU in the now-playing view moves less than the main
thread: at 30 passes a second the switch cost about 0.9% of a core, and
most of the view's CPU is the GL driver's own threads. First-frame times
and memory move within the run-to-run range.

### Text

TIDAL draws its text with Chromium, which on Windows turns a glyph's
coverage into ink with Skia's mask gamma: a curve for the text's luminance,
heavier for light text on dark and close to linear for dark text on light.
egui applies one curve to every colour (2c - c² in the dark theme, linear
in the light one). Headless Chrome 155 drew seven lines of Hanken Grotesk
(the app's own font) at 1.5x with grayscale antialiasing, and the same lines
were drawn with epaint's glyphs and egui_glow's blending; ink is the sum of
the text's normalised pixel values against Chrome's, difference the mean
absolute difference in levels of 255 at the best whole-pixel alignment:

| Line | Before: ink, difference | After: ink, difference |
| --- | --- | --- |
| 14 px 600 white on black | 0.93, 0.88 | 0.96, 0.64 |
| 14 px 600 #8c8c8c on black | 1.08, 0.81 | 0.98, 0.58 |
| 14 px 700 white | 0.95, 0.76 | 0.97, 0.54 |
| 12 px 600 #b0b0b0 | 1.04, 0.54 | 0.98, 0.41 |
| 16 px 700 white | 0.95, 0.75 | 0.97, 0.56 |
| 32 px 800 white | 0.98, 0.59 | 0.99, 0.43 |
| 14 px 600 black on white (linear before) | 1.01, 0.55 | 0.99, 0.55 |

Chrome's curve was read off these lines (contrast 1.0 and gamma 2.2 fit
best; Chromium's old defaults, 0.2 and 1.2, fit worse than egui's own), and
on Windows the app now uses it in both themes
(`theme::chromium_text_coverage`). The demo's Home, an album, a playlist and
the now-playing view at 1440 by 900 change in 1.5 to 2.1% of their pixels,
94 to 98% of them grey text on black or grey (grey text is a little lighter,
white text's edges a little heavier); covers, icons and shapes are
unchanged, and the now-playing view's moving background differs between any
two shots.

Seen and left for a next round: TIDAL's own text (Square Sans) has no
vertical antialiasing, because its `gasp` table asks DirectWrite for its
natural mode (rows snap to pixels, edges spread over two pixels
horizontally), while Chromium draws Hanken Grotesk, whose `gasp` asks for
symmetric smoothing, antialiased both ways as egui does. Matching TIDAL's
crisper rows would need a rasterizer mode of its own in epaint.

### TIDAL's elements on Home and in My Collection (plugin round 9)

Home's title and vibe tabs, the cards' play and favourite buttons and My
Collection's cells with their words are anchors the window notes while Luna
runs (with Luna off nothing is noted). Browse bench, debug demo build, Luna on
with no plugin, BelowNormal priority, two runs each, the commit before the
round against the round's last (the page's part of a window pass, first visit
of each page):

| Page | Before | After |
| --- | --- | --- |
| Home | 1,113 allocations, 823 and 835 µs | 1,154 allocations, 894 and 896 µs |
| My Collection's Albums | 380 allocations, 164 µs (both runs) | 492 allocations, 203 and 219 µs |
| An album | 1,871 allocations, 1,013 to 1,204 µs | 1,876 allocations, 1,209 to 1,233 µs (within the runs' spread) |

Memory after the 20-page walk is the same (Rust heap 6.8 MB in every run,
private 152 to 161 MB either way). The cost is the words and boxes noted per
visible card while Luna runs; layout itself (the flex alignment of plugin
elements) costs a lookup of last frame's height per item, only in rows that
centre or end their items.

### Every Home card in the page (plugin round 10)

Home now notes every card of every shelf for plugins, drawn or not (TIDAL's
page holds them all); with Luna off nothing is noted. Browse bench, debug
demo build, Luna on with no plugin, BelowNormal priority, two runs each,
`tidal` 11310160 against plug10 (the page's part of a window pass):

| Page | Before | After |
| --- | --- | --- |
| Home | 1,154 allocations, 821 to 981 µs | 1,273 allocations, 874 to 1,045 µs |

The Rust heap after the 20-page walk is 7.6 MB instead of 7.3. While Home
scrolls (20 s of wheel scrolling, `TIDALFAST_LUNA_STATS`, same build) the
Luna thread is less busy than before: 30 % and 32 % of a core (104 and 105
DOM syncs, 22 and 26 overlay builds in 10 s) against 45 % and 48 % (217 and
226 syncs, 59 and 64 overlay builds). Each sync carries more cards, but the
set of cards no longer changes as cards come into view, so the runtime
restructures the page and rebuilds the overlay far less. A signed-in Home
in lunatest held about 165 cards.

## Themes, speed round 1: where time goes (2026-10-25)

Release builds with the `demo` feature, the same machine and method as the
egui fork's round above (off-screen, BelowNormal, other builds sometimes
running), before and after run one after the other, three scroll runs and
two browse and idle runs each. Before: tidal 11310160; after: the five
commits below on top of it.

**Where the 16 ms went.** Not the themes: Luna being on with a long list
open. A probe build (scoped timers on the window's thread, not committed)
while the bench scrolls 2,000 songs, microseconds a frame:

| Window's thread, us a frame | Luna off | Luna on, no theme | Cotton candy |
| --- | --- | --- | --- |
| `App::logic` | 15 | 15,597 | 15,467 |
| of it the open page's track list for plugins | 0 | 15,386 | 15,086 |
| `App::ui` | 420 | 2,218 | 2,837 |
| of it `end_frame` (anchor tree, compare, a deep copy for the runtime) | 0 | 869 | 890 |

Every pass made TIDAL JSON for every loaded song of the open page only to
compare the list's length with the one plugins had. At rest the runtime's
thread built the whole overlay every second for the progress bar's
`--progress` (about 15 ms with cotton candy, 10 ms with no theme), which
came out the same.

| Commit | Change | Measured on its own (round 1's builds) |
| --- | --- | --- |
| c822a7ce | the open page's list is counted each pass and made only when the page or its length changes | cotton candy scroll p50 20.62 to 2.02 ms, allocations a frame 174,528 to 20,738 |
| 7e52420e | the anchor tree goes to the runtime shared (`Arc`), not copied | the copy for the runtime, 163 us to about 1 us a tree |
| ec40d676 | an overlay build keeps each app element's entry while its inputs are unchanged | 9.4 to 6.0 ms a build (sample page, cotton candy) |
| f949732d | a host's inline custom property no rule reads builds no overlay | cotton candy at four tree syncs a second: 512 to 179 ms busy in 10 s |
| e9833e50 | the theme caches stay while a plugin changes only its own element | cotton candy on Home with a plugin changing its own element ten times a second: the window's thread 1,391 to 1,470 M cycles in 20 s before, 1,205 to 1,379 after, about 0.2 ms for each such overlay |

Before against after, the five together:

| Measure | Luna off | No theme | Blur | Meteor Night | Tidal TUI | Cotton candy (animated) |
| --- | --- | --- | --- | --- | --- | --- |
| Scroll, frame CPU p50, ms | 0.55-0.61 / 0.54-0.58 | 18.0-20.2 / 0.71-0.79 | 18.3-19.0 / 0.89-0.98 | 18.9-21.0 / 0.86-1.04 | 18.7-19.9 / 0.92-1.01 | 19.4-21.2 / 1.56-2.26 |
| Scroll, p99, ms | 0.96-1.19 / 1.06-1.16 | 24.2-27.2 / 1.78-2.32 | 23.8-25.1 / 2.26-2.53 | 25.9-29.0 / 2.22-2.88 | 24.2-27.6 / 2.43-2.71 | 27.2-29.4 / 3.75-4.32 |
| Scroll, allocations a frame p50 | 2,415 / 2,415 | 174,524 / 4,320 | 175,763 / 5,290 | 174,968 / 4,711 | 176,097 / 4,655 | 180,314 / 16,492 |
| Browse, the page's part of a pass over 20 pages, ms | 4.4-4.5 / 4.4-4.5 | 5.4-5.7 / 5.5 | 7.2 / 6.9-7.1 | 6.7-7.0 / 6.9-7.0 | 6.7-7.0 / 6.9-7.2 | 14.1-15.3 / 14.5-15.0 |
| Idle on Home, process, M cycles in 20 s | 336-382 / 307-353 | 1,735-2,099 / 1,011-1,067 | 2,410-2,447 / 1,964-1,966 | 2,790-2,881 / 1,469-1,625 | 2,594-2,636 / 1,436-1,598 | 3,117-3,385 / 2,063-2,132 |
| of it the runtime's (`luna`) thread | 0 / 0 | 1,296-1,539 / 572-644 | 2,101-2,151 / 1,601-1,678 | 1,890-1,963 / 681-740 | 2,324-2,341 / 1,090-1,301 | 2,206-2,559 / 1,163-1,357 |

Scrolling with Luna on is as fast as with it off, give or take the theme's
own drawing; at rest the runtime's thread does half the work or less. Blur
gains least at rest: its selectors read the `style` attribute, so the
progress bar's variable still builds an overlay (with the entries kept).
The browse bench times only the page's drawing, which the round did not
change. A theme's own cost while scrolling is now visible: cotton candy
spends about 12,000 allocations a frame more than no theme (track rows'
anchor keys and lookups, the anchor tree's build), round 2's subject.

**Nothing drawn changed.** The theme parity checker on tidal 19f9c975 and
the five commits on it, no theme, Blur, cotton candy, Cotton Candy Nights,
Meteor Night and Sugarcoat: the style reports are identical item for item
and the pixel numbers the same (Sugarcoat's album page 7.85 against 7.86 %,
its animation). The demo's shots of the four pages are pixel-identical with
no theme, Blur and Meteor Night; with the three animated themes they differ
by at most 12 levels, only in the boxes that animate, as much as a build's
two runs differ from each other. What the caches keep is checked end to end
(`luna::tests::dom_tests::every_change_of_an_app_element_reaches_its_drawn_look_at_once`):
a class, the pointer, the window's size and a custom property a rule reads
each reach the drawn look on the first frame after the runtime has them, and
the test fails with any of the invalidations taken out.

### Round 2: what plugins read of the queue

Every pass with Luna on made the queue's rows for plugins again, copied
every queue song's JSON into a new snapshot, compared it whole with the one
plugins had, and made the favourites again (92f5092a keeps all three while
they are the same, shared with the snapshot). Measured with `--demo-show
big-playlist,long-queue` (2,000 liked songs as the paused queue) while the
bench scrolls 2,000 songs, before (the commit before it) against after:

| Measure | No theme | Cotton candy |
| --- | --- | --- |
| Scroll, frame CPU p50, ms | 20.2-20.9 / 6.9-7.5 | 24.0-24.4 / 7.4-8.1 |
| Scroll, p99, ms | 27.7-30.8 / 9.7-10.9 | 35.4-45.9 / 11.9-16.7 |
| Allocations a frame p50 | 122,696 / 10,718 | 127,759 / 15,771 |
| Idle on Home, process / window's thread, M cycles in 20 s | 101-117 / 80-96 before, 42-46 / 25 after | |

Still about 6 ms a frame more than with the demo's 12-song queue: not yet
found (a probe build is round 3's first step).

## egui fork, round 2 (2026-10-25)

Release builds with the `demo` feature, the same machine and method as the
fork's round 1 (off-screen, BelowNormal, other builds running at times).
Where a frame goes was read from egui's `profiling` scopes with puffin and
from a sampling profiler of the window's thread (its stack every
millisecond, named from the build's line tables), both in a measurement
build that is not committed. Each change was then measured on its own,
switched off and on at run time in that build, in interleaved runs.

### Where a frame goes

The window's thread, microseconds a frame, before the round and after it
(measurement builds; both scroll columns compare the round's first profile
build with the last, the others the same build with the changes off and
on):

| | Scroll 2,000 songs | Home | Album, playlist, Home | Now playing, lyrics | A plugin's element on Home | Cotton candy, Home | Cotton candy, scroll |
| --- | --- | --- | --- | --- | --- | --- | --- |
| All scopes | 718 / 566 | 480 / 440 | 456 / 414 | 279 / 255 | 626 / 591 | 1,281 / 1,174 | 2,580 / 2,073 |
| `App::ui`: the app's pass and egui's widgets in it | 426 / 300 | 230 / 228 | 225 / 220 | 84 / 86 | 344 / 348 | 784 / 772 | 1,789 / 1,444 |
| Tessellation | 61 / 41 | 67 / 45 | 54 / 36 | 17 / 13 | 73 / 51 | 161 / 102 | 220 / 132 |
| egui_glow's buffer upload | 12 / 12 | 17 / 17 | 12 / 13 | 4 / 5 | 19 / 19 | 54 / 55 | 64 / 57 |
| Image loading (`try_load_texture`, 15 to 104 images) | 27 / 23 | 15 / 15 | 15 / 15 | 4 / 4 | 16 / 16 | 18 / 18 | 47 / 37 |
| egui-winit reading the window's state | 16 / 0 | 14 / 0 | 15 / 0 | 17 / 0 | 16 / 0 | 21 / 0 | 22 / 0 |
| eframe's own, freeing the frame's meshes among it | 18 / 14 | 14 / 11 | 14 / 11 | 9 / 8 | 15 / 12 | 21 / 14 | 41 / 25 |
| `swap_buffers` (no vsync off-screen) | 24 / 30 | 23 / 24 | 22 / 22 | 26 / 35 | 23 / 24 | 31 / 32 | 32 / 35 |

The plugin is a probe that keeps a fixed element's text changing ten times
a second. What it showed:

- **egui's widgets are a small share of `App::ui`.** In the samples taken
  inside it while scrolling, egui's own functions are about 15% of the time
  (the largest, `Context::get_response`, 2%); the rest is the app's work,
  the heap and hashing. The largest single cost was the app's: an editable
  playlist's page collected every song's link into a set every pass, to
  leave them out of its recommendations, 130 us a pass on the 2,000-song
  playlist (27% of `App::ui`). The set is now kept until the playlist's
  songs change.
- **The heap**: allocating and freeing were about 12% of the window's
  thread while scrolling. Tessellation grew every frame's meshes from
  nothing, and eframe freed them after painting.
- **The window's state**: egui-winit asked winit for it every frame, 14 to
  22 us, most of it two messages to the window for its title. On the
  machine measured those messages pass through another program's title-bar
  hook in `DefWindowProc`, which also costs about 35 us a frame in winit's
  own handling of its paint message: that part is the machine's, not
  Tidalfast's.
- **Image loading** costs about 0.3 us an image a frame: egui's loader made
  the image's address into a new string to look it up, and Tidalfast's
  `texture_fit` loader, asked first, hashes it with the standard (SipHash)
  hasher.
- Not costs worth a change: the GL buffer upload (one copy of the frame's
  vertices), text layout outside fast scrolling, the plugin hooks (4 to 5
  us). fastframe-emoji's output hook builds its map of the frame's text
  again every frame (fastframe's, not changed here).

Changes in the fork (CIoudGuy/egui, branch `tidalfast`, 36760640 to
ab59df76), each measured on its own with the same profile:

| Commit | Change | Measured |
| --- | --- | --- |
| f9b6cafd | crmne/winit 0bed9dc0 (two macOS fixes) | no change on Windows |
| 000c1eed | `GlyphRasterization::HorizontalOnly`: glyphs antialiased across only, each row sampled once, as DirectWrite's natural mode draws them | see "Text" below |
| 3bb2799f | egui-winit reads the window's state for `ViewportInfo` only after a window event that changes it (size, position, scale, focus, occlusion), after viewport commands, when the zoom changes, and at least every half second | 14 to 22 us a frame to under 0.2 |
| 35b4e0ee | a viewport's next frame is tessellated into the meshes of its last one, each mesh back in the place its primitive had (`TessellatorBuffers`) | tessellation 67 to 77 to 43 to 52 us a frame scrolling, 158 to 167 to 102 to 117 with cotton candy on Home; eframe's own time (freeing the meshes) 20 to 22 to 15 to 17 us; allocations a frame 4,352 to 3,942 scrolling |
| 81b3c975 | egui's texture loader finds a loaded texture by the borrowed address, and `has_extension` reads the name's last dot instead of parsing a path | image loading 27 to 21 to 25 us a frame scrolling (80 images) |
| d45c9a3e | a blurred fill (a shadow, a glow) fades as a Gaussian with half the blur as standard deviation, as CSS draws a box-shadow; its mesh is kept with the buffers above while its size stays | see "Shadows and rounded corners" below; tessellation with cotton candy within the runs' spread (cached) |
| ab59df76 | rounded rectangles keep the corner radius asked for (egui added 0.4) | see below |

Before (tidal 56024542) against after (the eguiperf2 branch: the fork at
ab59df76, the playlist set kept, glyphs antialiased across only on
Windows), ordinary release builds, interleaved, three runs each unless two
are given:

| Measure | Before | After |
| --- | --- | --- |
| Scroll, eframe's frame CPU p50 | 0.58, 0.59, 0.63 ms | 0.39, 0.42, 0.43 ms |
| Scroll, p95 | 0.89, 0.89, 0.99 ms | 0.60, 0.64, 0.67 ms |
| Scroll, p99 | 1.18, 1.20, 1.37 ms | 0.75, 0.82, 0.88 ms |
| Scroll, allocations a frame p50 | 2,415 | 1,917 |
| Cotton candy scroll, frame CPU p50 | 2.20, 2.35, 2.46 ms | 1.39, 1.40, 1.51 ms |
| Cotton candy scroll, p99 | 4.31, 4.45, 4.77 ms | 3.64, 3.71, 3.77 ms |
| Cotton candy scroll, allocations a frame p50 | 18,073 to 18,103 | 15,055 to 15,792 |
| Browse, the app's part of a pass over 20 pages (two runs) | 4.46, 4.59 ms | 4.42, 4.47 ms |
| Now playing with lyrics, CPU over 20 s (two runs) | 17.0, 17.8% of a core | 17.6, 17.8% |
| Idle on Home, CPU over 20 s (two runs) | under 0.1% | under 0.1% |
| First frame presented | 234, 237, 260 ms | 224, 228, 251 ms |
| Private memory: idle on Home / after scrolling / after the browse pages | 117 to 119 / 130 to 133 / 135 to 138 MB | 116 to 120 / 132 to 133 / 130 to 136 MB |

The scroll numbers include the playlist set, which the frame CPU counts
(it is in `App::ui`); the window's state and freeing the meshes are
outside eframe's frame CPU and show in the table above. The now-playing
view moves less than the window's thread: most of its CPU is the GL
driver's threads. Memory stays within the runs' range: the kept meshes
hold about as much as one frame's.

### Text

TIDAL's text looks sharper than Tidalfast's because its rows are crisp:
Chromium on Windows draws a font in DirectWrite's natural mode unless the
font's `gasp` table asks for symmetric smoothing, and TIDAL's font does not
ask for it. In that mode a glyph is antialiased across only: each pixel
row samples the outline once, along its middle, and its coverage is spread
a little sideways by ClearType's filter (Skia averages the three
sub-pixels to gray). Hanken Grotesk asks for symmetric smoothing, so
Chrome draws it antialiased both ways, as egui does. In TIDAL's capture of
the album page and queue (1.5x), 7.6% of the horizontal steps between
neighbouring text pixels are full jumps, against 20.5% in Tidalfast's
before; its rows have no partly covered row above the round letters.

The oracle was Chrome 155 drawing Hanken Grotesk with its `gasp` table
changed to leave symmetric smoothing out, so it draws the app's own font
the way it draws TIDAL's. epaint's new mode was fitted to it: a row sampled
at its middle and sub-pixels weighted 1, 2, 3, 2, 1 came closest (other
row offsets and filters were further off). Seven lines (12 to 32 px, 600 to
800, white, grey and black on white), mean difference in levels of 255:

| Scale | Chrome natural against egui before (both ways) | against egui after (across only) | Chrome both ways against egui before |
| --- | --- | --- | --- |
| 1x | 1.92 | 0.60 | 1.23 |
| 1.25x | 1.44 | 0.50 | 0.96 |
| 1.5x | 1.23 | 0.35 | 0.79 |

The ink stays the same (0.99 to 1.01 of Chrome's per line at 1.5x).
`theme::chromium_text_rasterization` sets the mode on Windows in both
themes, with the per-colour coverage of round 1. On the demo's album page
at 1.5x the share of full horizontal jumps goes from 20.5% to 10.7%
(TIDAL 7.6%), and the tops and bottoms of round letters sit on whole rows.

### Shadows and rounded corners

Chrome 155 drew boxes with CSS box-shadows (blur 8 to 32 px, spread,
offsets, a pink glow) and rounded corners, and epaint drew the same as
egui shapes, rasterized the way egui_glow does. egui's blur was a linear
ramp as wide as the blur radius; a browser's is a Gaussian with half the
radius as standard deviation, which fades over about three times that
width. Mean difference in levels, and pixels more than 4 levels off, at
1.5x:

| Box | Before | After |
| --- | --- | --- |
| Menu, 0 6px 24px #0009 on #121212 | 0.36, 27 | 0.18, 0 |
| Card, 0 4px 16px #000c on #3a3a3a | 0.60, 7,131 | 0.06, 0 |
| White card, 0 4px 16px #0008 on white | 1.48, 15,739 | 0.17, 19 |
| Pink glow, 0 0 20px on #101010 | 1.32, 18,778 | 0.42, 12 |
| Square box, 0 2px 8px 4px spread | 0.53, 7,166 | 0.12, 0 |
| Radius 24, 0 8px 32px #000c | 1.66, 21,488 | 0.11, 0 |
| White box, radius 12, no shadow (largest pixel difference) | 87 levels | 32 levels |
| White box, radius 4, no shadow (largest pixel difference) | 114 levels | 61 levels |

A blurred fill is now a mesh of rings round the box, each vertex carrying
the Gaussian coverage at its place (exact across the box, integrated along
it), so corners and boxes smaller than their blur fade as Chrome's do.
About 500 vertices a shadow instead of about 100; the mesh is kept while
the shadow keeps its size, so a frame only copies it. Chrome's glow tail
is a little shorter than a Gaussian's; that remains (0.42). The pill
(radius 50) differs by a few levels along its curve, as both are
antialiased differently. Tidalfast's own see-through boxes keep their
outside-only shadow rings (`plugin_dom::outer_shadow_shape`), and a tilted
cover's shadow is now egui's blur projected.

### Demo shots

The demo's Home, album, playlist, now-playing view and album with the
queue at 1440 by 900, before against after (two before runs are identical
to the pixel): 1.6 to 2.5% of the pixels change, nearly all of them text
(rows crisper, horizontal edges a little softer). Outside text, the album
cover's shadow changes by 1 to 3 levels (softer and wider) and the cover's
and buttons' rounded corners by a few pixels each (0.4 px less radius).
Icons, covers and the layout do not move. A dialog (the sign-in permission
prompt over Home) changes in 3.1% of the pixels: its text, and its shadow,
which now darkens what is behind it further out, as a CSS shadow of the
same blur does.

## Themes, speed round 3 (2026-10-25)

Release builds with the `demo` feature, the same machine and method as
round 1 (off-screen, BelowNormal, before and after run one after the
other): tidal f21a6706 against the five commits below on it. Scroll:
three runs each, with the demo's 12-song queue and with 2,000 liked
songs queued (`--demo-show big-playlist,long-queue`); browse two, idle
three (the first run before filled the artwork cache and is left out).

On this base the bench's window is paced at the display's 144 Hz even
off-screen. Round 1's and round 2's runs, and the commits' own numbers
below, were not paced (800 frames a second and more, which reach the
list's end in about a second, so their p50 was mostly a frame at rest).
Now every frame of the five seconds down scrolls: p50 is a scrolling
frame, p95 and p99 sit at the 6.9 ms frame interval, and the frames
drawn in the ten seconds show the frames missed. Compare within the
table, not with round 1's.

**Where the long queue's 6 ms went.** A probe build put 4.7 ms of
every pass in one place: the host kept its old snapshot of what plugins
read when the app's new one compared equal, so once the app had made
the queue's songs again (the song cache grows when a page opens), every
pass compared 2,000 songs element by element instead of by pointer.

| Commit | Change | Measured on its own (round 3's builds, on tidal 56024542, not paced) |
| --- | --- | --- |
| a56524ae | the snapshot plugins read is replaced even when it is equal | 2,000 queued, scroll p50 no theme 7.64-9.00 to 0.85-1.29 ms, cotton candy 8.07-9.24 to 2.29-3.09 |
| edde7eb8 | a theme's lengths in `vw` and `vh` follow the window's size (they kept the size they were resolved at until the overlay changed, a bug) | no cost |
| 2e9c599d | the saved flags count their changes; a queued song's id is read without a `split` | 2,000 queued, p50 no theme 0.95-1.43 to 0.83-0.94 ms, cotton candy 2.73-2.81 to 1.68-2.52 |
| 4a6553d8 | a read of the store gets only the large parts that changed | below |
| fa64685d | a tree the window sent is freed on a thread of its own (0.13 to 0.17 ms a tree) | p95 cotton candy, median of six, 3.39 to 3.14 ms |

Before against after, the five together:

| Measure | Luna off | No theme | Blur | Meteor Night | Tidal TUI | Cotton candy (animated) |
| --- | --- | --- | --- | --- | --- | --- |
| Scroll, frame CPU p50, ms | 1.52-1.88 / 1.57-1.69 | 2.70-2.88 / 2.44-2.63 | 2.86-2.98 / 2.74-3.08 | 2.88-3.04 / 2.71-2.84 | 2.30-3.21 / 2.75-3.17 | 3.69-3.90 / 3.26-3.73 |
| Scroll, p99, ms | 6.82-6.87 / 6.80-6.84 | 6.85-6.93 / 6.86-6.92 | 6.80-6.90 / 4.25-6.89 | 6.88-6.98 / 6.81-6.97 | 6.88-7.00 / 6.78-6.92 | 6.95-7.16 / 6.86-6.94 |
| 2,000 queued: scroll p50, ms | 1.33-1.76 / 1.47-1.67 | 7.47-8.74 / 2.16-2.92 | 7.48-9.54 / 2.52-2.93 | 7.91-8.46 / 2.66-2.94 | 6.99-8.25 / 2.67-3.07 | 8.76-9.02 / 3.77-4.00 |
| 2,000 queued: p99, ms | 6.81-6.86 / 6.79-6.85 | 10.73-12.52 / 6.80-6.89 | 10.99-13.60 / 6.85-6.91 | 11.09-14.25 / 6.88-6.91 | 9.93-14.37 / 6.89-6.95 | 12.33-14.01 / 6.78-6.87 |
| 2,000 queued: frames in the 10 s (1,440 at 144 Hz) | 1,440-1,442 / 1,441-1,442 | 1,067-1,249 / 1,441-1,442 | 1,017-1,272 / 1,436-1,441 | 1,099-1,222 / 1,441 | 1,122-1,328 / 1,441 | 1,024-1,087 / 1,404-1,436 |
| Browse, a pass summed over the 20 pages, ms | 13.1-14.2 / 12.7-14.2 | 13.3-15.2 / 14.2-15.3 | 17.4-17.9 / 17.8-17.9 | 18.7-20.1 / 14.6-17.7 | 15.5-17.0 / 16.4-16.6 | 20.4-26.2 / 26.3-27.0 |
| Idle on Home, process, M cycles in 20 s | 415-1,778 / 313-2,493 | 2,189-4,569 / 2,104-3,809 | 4,280-5,119 / 3,829-4,761 | 3,609-4,365 / 3,613-4,837 | 4,210-4,590 / 3,339-4,107 | 4,674-5,572 / 4,528-5,827 |
| of it the runtime's (`luna`) threads | 0 / 0 | 1,659-1,953 / 1,646-1,898 | 3,744-3,837 / 3,358-3,678 | 1,974-1,983 / 1,763-1,939 | 2,493-2,921 / 2,273-2,778 | 2,334-2,754 / 2,214-2,514 |

With 2,000 songs queued a scrolling frame now costs what it costs with
12, and every theme holds the display's rate. The round changed no
drawing, so the 12-song rows, the browse pages and rest on Home are the
same within the runs' spread. Idle on the playlist page with 2,000 songs
queued and a probe plugin reading the store every second (as Total Play
Time does), the runtime's thread over 20 s: 16,935-17,033 / 186-195 M
cycles (the process 17,025-17,148 / 341-370). Private memory after
scrolling is 211-232 / 189-190 MB with no theme; with cotton candy it
varies from 340 to 490 MB between runs of either build (the script
runtime's heap; Rust's own is 45 to 62 MB).

**Nothing drawn changed.** The theme parity checker on the same two
commits (debug builds), no theme, Blur, Meteor Night, Sugarcoat, cotton
candy and Cotton Candy Nights: the style reports are identical item for
item and the pixel numbers the same (cotton candy's theme-touched share
4.32 against 4.33 %, its animation). The demo's shots of the four pages
are pixel-identical with no theme, Blur and Meteor Night; with the three
animated themes they differ only in the boxes that animate (the album's
list panel, the playlist's title, Sugarcoat's bar button), within 12
levels, where two runs of one build differ as much.

### Round 4

Measured against the commit before each, release builds interleaved,
three scroll runs each (the 2,000-song playlist, both queues), with the
anchor tree's build timed in a probe build. While a list scrolls the
window builds and sends a tree on every frame: 630 elements and 7,700
allocations a build with no theme, 680 and 8,500 with cotton candy,
0.45 to 0.6 ms of the window's thread.

| Commit | Change | Measured |
| --- | --- | --- |
| addbe9d0 | a track row's list id is borrowed and its anchor key made once | allocations a frame 12,327 to 11,966 (no theme), 17,951-17,977 to 17,532-17,538 (cotton candy); frame time the same |
| 839aa55a | the demo's `luna-dump.json` (5 MB with cotton candy, every two seconds) is made and written on a thread of its own | cotton candy: the scroll bench's slowest frame 23.3-26.8 to 7.2-12.9 ms; idle on Home, the window's thread 2,131-3,837 to 524-568 M cycles in 20 s |
| 0b04f94f | a tree's CSS-module classes are found in a map and written straight into the element's class | allocations a tree 7,711-7,723 to 7,361-7,377 (no theme), 8,496-8,500 to 8,111-8,117 (cotton candy); the build's time the same |

The demo's dump was in every idle number above with Luna on: at rest
with cotton candy most of the window's thread was that dump, and what
is left (524 to 568 M cycles in 20 s, the theme's endless animations at
15 frames a second out of focus) is about 0.45 ms a frame.

What is left of Luna's cost, found here: while a list scrolls, the
runtime's thread is busy all the time (one core, 4.5 G cycles a second,
with or without a theme, both builds) reconciling a tree a frame, each
sent whole as JSON; and at rest, while a song plays, one tree a second
(the player bar's time) costs the runtime about 18 ms (no theme) to
20 ms (cotton candy), 2% of a core (`TIDALFAST_LUNA_STATS=1`).

## Findings and decisions

- **Luna's work follows changes, not the page.** With Luna on, a pass made
  the open list's and the queue's TIDAL JSON and the favourites again, and the
  runtime built the overlay for an inline variable no rule reads: 16 to 20 ms
  of every frame while scrolling a long list. Each is now made when what it is
  made of changes ("Themes, speed round 1" above).
- **A long queue costs a scrolling frame nothing more.** With 2,000 songs
  queued every pass compared the queue's songs element by element, about
  6 ms a frame; it compares them by pointer now ("Themes, speed round 3").
  What Luna still costs is the anchor tree: built and sent whole on every
  frame a list scrolls, and reconciled by the runtime as fast as it can.
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
