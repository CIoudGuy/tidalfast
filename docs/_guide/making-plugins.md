---
title: Making Plugins for Tidalfast
description: Build a TidaLuna plugin with TidaLuna's template and test it in Tidalfast with live reload, logs and source maps.
nav_order: 4
---

Tidalfast runs plugins made for [TidaLuna](https://github.com/Inrixia/TidaLuna).
There is no separate Tidalfast plugin format: you build a TidaLuna plugin with
TidaLuna's own template and library, and the same plugin runs in TIDAL with
TidaLuna and in Tidalfast. This page shows how to test one in Tidalfast.

## What you need

- [Node.js](https://nodejs.org/) and [pnpm](https://pnpm.io/), as TidaLuna's
  [plugin template](https://github.com/Inrixia/luna-template) asks for.
- Tidalfast with **Enable TidaLuna plugins** turned on in
  **Settings > Plugins (TidaLuna)**.

## Start from the template

```sh
git clone https://github.com/Inrixia/luna-template.git luna-plugins
cd luna-plugins
pnpm install
pnpm run watch
```

`pnpm run watch` builds every plugin in `plugins/` into `dist/` each time you
save, and serves `dist/` on `http://127.0.0.1:3000`. Edit
`plugins/Example/src/index.ts` to start. The files in `dist/` are named after
the package, with `@` removed and `/` turned into `.`: the package
`@luna/example` is built as `luna.example.mjs`, `luna.example.json` and
`luna.example.mjs.map`.

## Load it in Tidalfast

Open **Settings > Plugins (TidaLuna)**, then use either way.

- **Install from URL.** On the **Plugin Store** tab, paste
  `http://127.0.0.1:3000/luna.example` (your plugin's file name, without
  `.mjs`). A folder such as your `dist/` folder, or the path of a built
  `.mjs` file, works too. This is a dev install: the plugin shows `[DEV]`
  next to its version and reloads by itself whenever the build changes.
- **The dev store.** While `pnpm run watch` is running, the **Plugin Store**
  tab lists your plugins in a store marked `[DEV]`. Install from there, then
  turn on **Enable live reloading (for development)** on the plugin's card.
  Live reload is off after every start, as in TidaLuna.

Only addresses on this computer (`http://127.0.0.1`) and local files can be
dev installs. Everything else must be HTTPS.

## Live reload

Save a file, wait for the build, and the plugin reloads within about a second.
Tidalfast does not restart: the old copy is unloaded (its `unloads` run), the
new build starts, and the plugin's saved settings stay. If the build is caught
half written, Tidalfast looks again a second later.

## Logs

Everything a plugin writes with `console.log`, `console.info`, `console.warn`,
`console.error`, `console.debug` and TidaLuna's `Tracer` is kept per plugin,
with its level.

- On the plugin's card, open **Logs** to see its own lines.
- On **Settings > Plugins (TidaLuna) > Settings**, under **Plugin
  development**, all plugins' lines are listed together, newest first, with
  the time, the level and the plugin's name. Debug lines (the runtime's own
  notes) are hidden until you turn on **Show debug lines**.

For a dev plugin, an error that nothing caught, and a promise rejection that
nothing handled, are logged as errors with their stack.

## Source maps

The template writes a source map (`luna.example.mjs.map`) next to the build.
Tidalfast reads it, so a stack in the log names your TypeScript files and
lines instead of the built file:

```
Uncaught Error: Example plugin crashed on purpose
    at L (plugins/Example/src/explode.ts:2:44)
    at <anonymous> (plugins/Example/src/index.ts:24:12)
```

The line is exact. The column is the nearest mapped position in the built
file, so it can be a little off.

It reads the map for dev installs and for plugins served from
`http://127.0.0.1`, and keeps it across restarts. A build without a map shows
positions in the built file. Function names in a stack are the names in the
built file, which the template minifies.

## Settings and permissions

A `Settings` component that you export is drawn on the plugin's card, the same
as for a store plugin. The card's **Permissions** list shows what the plugin
has actually done: network, files, programs and so on. Use it to check that
your plugin does only what you mean it to.

## What is supported

Tidalfast follows TidaLuna's plugin API (`@luna/core`, `@luna/lib`, `@luna/ui`
and `@luna/lib.native`, at the version named in the Settings tab). See
[Luna compatibility](https://github.com/CIoudGuy/tidalfast/blob/main/docs/_reference/luna-compat.md)
for what is supported and what is not, and the
[API surface](https://github.com/CIoudGuy/tidalfast/blob/main/docs/_reference/luna-api-surface.md)
for every export compared with TidaLuna.

Tidalfast is a native app. It has no TIDAL web page, so code that depends on
TIDAL's web bundle or on Electron's main process cannot work. Downloading
tracks to files is refused, and nothing that touches DRM is supported.

When your plugin uses something Tidalfast does not have, it does not fail in
silence. The plugin's log names it:

- `@luna/lib.<name> is not available in Tidalfast's Luna ...`: an export that
  Tidalfast lacks. Calling it does nothing and returns an inert stand-in, and a
  feature check such as `"name" in lib` still tells you it is missing.
- `Redux action <type> does nothing in Tidalfast: <reason>` (or `is not an
  action type TIDAL has`, or `was not understood`): an action only TIDAL's own
  web app handles, with the reason. Every one of TIDAL's 694 action types
  either acts or says this. Once per action for a plugin under development.
- `findModuleProperty No module matches selector ...`: a lookup in TIDAL's
  web modules, which do not exist here.
- `ipcRenderer.invoke Unsupported channel ...`: an IPC channel Tidalfast does
  not answer.

## Publish it

The template includes a GitHub Action that builds and publishes a release.
Allow it to write: in your repository, **Settings > Actions > General >
Workflow permissions > Read and write permissions**. After it runs, your
release holds the plugin files and a `store.json`.

Tell people to install it from **Settings > Plugins (TidaLuna) > Plugin
Store > Install from URL**:

- the store, `https://github.com/<you>/<repo>/releases/download/latest/store.json`,
  which lists all of your plugins, or
- one plugin, `https://github.com/<you>/<repo>/releases/download/latest/luna.example`.

Plugins and stores install over HTTPS only, and Tidalfast checks installed
plugins for updates. You can also open a pull request on
[TidaLuna](https://github.com/Inrixia/TidaLuna) to add your store to its
default list.
