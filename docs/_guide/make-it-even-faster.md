---
title: Make It Even Faster
description: Scrolling performance, and how to keep large libraries light.
nav_order: 6
---

## Scrolling and large libraries

The sidebar is drawn alongside every page, so its sorting work can affect
scrolling elsewhere in the window. Library ordering looks up each row's
position once per sort instead of repeatedly searching the full saved order,
which keeps large libraries smooth while preserving your chosen sort, pins,
and folders.

The main window draws as needed and uses the platform's frame pacing.

## Plugins

Plugins you do not use cost nothing: each native plugin checks its own switch
first, and the Luna compatibility runtime only runs while it is turned on in
**Settings > Plugins**.

## Network delays

If TIDAL is slow to answer, the top bar shows a spinner. Your playlists and
Home show what they showed last time until the update arrives.
