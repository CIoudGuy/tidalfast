---
layout: home
title: Tidalfast
description: A fast, lightweight, native TIDAL client for Linux and Windows.
permalink: /
hero:
  name: Tidalfast
  text: TIDAL, native and fast
  tagline: A lightweight TIDAL client for Linux and Windows. Lossless playback on your computer, your favourites and playlists, plugins, and a now-playing view with lyrics.
  actions:
    - theme: brand
      text: Download
      link: /download/
    - theme: alt
      text: What is Tidalfast?
      link: /what-is-tidalfast/
    - theme: alt
      text: GitHub
      link: https://github.com/CIoudGuy/tidalfast
  image:
    src: /assets/images/readme-home-dark.jpg
    alt: "Tidalfast Home with the playlist library, recommendations, queue, and player visible"
    width: 1600
    height: 1000

features:
  - icon: ⚡
    title: Lightweight
    details: Opens in well under a second and typically uses just 100–250 MB of memory.
  - icon: 🦀
    title: Native, in Rust
    details: "Built with [egui](https://www.egui.rs) and [fastframe](https://fastframe.dev), the framework behind a family of native desktop apps. No browser engine inside."
  - icon: 🔊
    title: TIDAL, played locally
    details: Sign in with your TIDAL account and play lossless FLAC (or AAC) straight from your computer. A TIDAL subscription is required.
  - icon: 📚
    title: Library and search
    details: Browse your favourites, playlists, albums, artists, and Mixes & Radio. Search the catalogue and edit playlists you own.
  - icon: 🎨
    title: Themes
    details: Choose TIDAL's dark or light look, or follow your system as it changes, while the music keeps playing.
  - icon: 🎧
    title: Now playing
    details: Click the player bar to slide up the big cover with similar tracks, credits, and lyrics, and an equalizer in Settings.
  - icon: 🧩
    title: Plugins
    details: Optional TidaLuna plugin support, off by default, with one switch in Settings. No built-in plugins.
  - icon: ⌨️
    title: Desktop controls
    details: Use keyboard shortcuts and your keyboard's media keys. Keep the music playing after you close the window.
  - icon: 🔓
    title: Open source
    details: Free to use, study, and improve. Explore the code, report a problem, or help build the next version.
    link: https://github.com/CIoudGuy/tidalfast
    link_text: Read the source
---

## WhatsApp, just as fast

**Want WhatsApp just as fast and native?** [ZapFast](https://zapfast.rocks) is
Tidalfast's sibling: the same native interface, for WhatsApp. Both are built on [fastframe](https://fastframe.dev), the shared
foundation for native Rust apps built with [egui](https://www.egui.rs).

<style>
  /* The hero image slot is sized for a square logo; the screenshot needs the
     room. Page-scoped overrides, so the theme stays untouched. */
  .VPHero .image-container {
    width: 100% !important;
    height: auto !important;
    transform: none !important;
  }
  .VPHero .image-src {
    position: relative !important;
    top: auto !important;
    left: auto !important;
    transform: none !important;
    width: 100% !important;
    height: auto !important;
    max-width: 100% !important;
    max-height: none !important;
    padding: 0 !important;
    border-radius: 12px;
    box-shadow: 0 12px 48px rgba(0, 0, 0, 0.45);
  }
  @media (max-width: 959px) {
    .VPHero .image {
      margin: 0 0 24px !important;
    }
  }
</style>
