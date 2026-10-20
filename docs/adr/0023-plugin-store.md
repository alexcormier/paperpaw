# 0023. The plugin store

- Status: Accepted
- Date: 2026-10-19, the lists for each device 2026-10-20

## Context

paperpaw.org keeps a plugin library (`catalogue/<id>.toml`, one file a
plugin) and offers it as `GET /api/v1/plugins.json`. Data plugins come
as ZIPs, which the server packs from a folder or copies from elsewhere
and checks by SHA-256. On the device only fonts can be had so far: the
`fonts-browser` plugin reads `/api/v1/fonts.json` and downloads a font's
files one by one, each checked by its size and SHA-1, into a folder of
its own, then loads it (`Command::LoadPlugin`).

The X4 and the X4 Pro should find, install, update and remove every kind
of data plugin (themes, home layouts, translations, key maps, script
packs, presets) on the device itself (PLANNING O6). They have a few
hundred kilobytes of memory: a list must be read as it streams, a
download written as it arrives, nothing unpacked in memory.

## Decision

- **A list made for each device.** `GET /api/v1/plugins.json` without
  parameters stays format 1, the whole library for the website and
  other readers. With them, `?device=<x4|x4-pro|android>&lang=<xx>`, it
  is format 2: only the plugins for that device, already in the order
  to show them (by the website's groups, then by name in that
  language), each with what a list needs and no files:

  ```json
  {"format": 2, "plugins": [{
    "id": "airy-theme", "name": "Luftig", "summary": "…",
    "group": "look", "author": "…", "licence": "Apache-2.0",
    "kind": "data",
    "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 2345}}]}
  ```

  - `name`, `summary`: in the language asked for (from an optional
    `[i18n.<lang>]` table of the catalogue entry), English otherwise.
  - `group`: one of `reading`, `formats`, `library`, `sync`, `home`,
    `look`, `system`, in that order.
  - `kind`: `data` or `code`; `built_in` where true.
  - `latest.formats`: the data format the plugin says in its
    `plugin.ini` (`formats = 1`, ADR 0020's data formats), read by the
    server from the package. `latest.size`: the bytes a device
    downloads (its files together). `latest` is left out (or `null`)
    where no package is ready.
- **A page for each plugin**: `GET /api/v1/plugins/{id}.json`, with the
  same parameters, is `{"format": 2, "plugin": {…}}`: the fields of the
  list and `latest.files`, every file of a data plugin's package, its
  path in the plugin's folder (plain names and `/`, none starting with a
  dot), an address relative to this page, its size and SHA-1. The server
  serves them out of the package it already holds
  (`GET /api/v1/plugins/{id}/{version}/{*path}`), so a device needs no
  unpacking. A `plugin.ini` is always among them. Plugins with code get
  `latest.builds` there later.
- **The device's store** is a plugin (`store`, *More plugins*), the same
  on every device, by buttons and by touch. The shell says what plugin
  libraries call its device (`KernelBuilder::catalog_device`, read as
  `Env::catalog_device`: `x4`, `x4-pro`, `android`); a device without
  one is told that there is nothing for it. The store lists the data
  plugins made for an SDK this one reads and a data format it knows, by
  group, with *Search* (by name and summary) above them and fonts as
  one more entry that opens the fonts browser. It keeps the list as an
  index on the storage (a line for each plugin and where each line
  begins); in memory are only the runs of plugins of one group and the
  page shown (`ui::Sections`), so the list may grow without limit. A
  plugin's page fetches the plugin's own page for its files, tells what
  it is, who made it, its licence and size, and installs, updates or
  removes it. Built-in plugins and plugins with code are not listed
  yet.
- **Installing is a building block of the SDK** (`install`), not of one
  plugin: the download into `plugins/.<id>.part`, file after file, each
  checked by size and SHA-1, what is left kept on the card so it goes on
  after the screen is closed, each job told apart from the ones before
  so that late answers are passed over. Then the kernel installs it
  (`Command::InstallPlugin`): in place of every version of that plugin
  on the storage, loaded, switched off or refused at start, in the
  folder named as it; a plugin switched off stays off, and what was
  chosen from the version before (its font for reading) stays chosen.
  The fonts browser uses it too.
- **The list's address** is a setting for developers (`store.catalog`,
  `https://paperpaw.org/api/v1/plugins.json` unless set), as the fonts
  browser's is; a plugin's page is `plugins/<id>.json` beside it.

## Consequences

- One catalogue entry makes a plugin appear on the website and on every
  device it fits; authors change nothing to reach the X4.
- The website sorts and filters, so a device does neither: it reads the
  list as it streams, whatever its length, and fetches files only for
  the plugin it installs.
- The server keeps every file of a package by its SHA-1 as well as the
  ZIP's SHA-256; a package checked once is served both ways.
- Plugins with code need a firmware built with them (O6's second half,
  a build service): they get fields of their own then (`latest.builds`
  on a plugin's page), and devices that do not know those fields keep
  passing over them.
