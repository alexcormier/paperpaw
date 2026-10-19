# 0023. The plugin store

- Status: Accepted
- Date: 2026-10-19

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

- **One list for the device and the website.** `plugins.json` stays
  format 1 and grows only by fields, which readers that do not know
  them pass over:

  ```json
  {"format": 1, "plugins": [{
    "id": "airy-theme", "name": "Airy",
    "summary": "Light headers, wide margins, roomy rows and rounded corners.",
    "i18n": {"de": {"name": "Luftig", "summary": "…"}},
    "kind": "data", "group": "look", "tags": ["theme"],
    "author": "…", "licence": "Apache-2.0", "source": "https://…",
    "devices": ["android", "x4", "x4-pro"],
    "built_in": false, "off_by_default": false,
    "latest": {
      "version": "1.0.0", "sdk": "0.4", "formats": 1, "date": "2026-10-19",
      "url": "/plugins/airy-theme/airy-theme-1.0.0.zip",
      "size": 2345, "sha256": "…",
      "files": [
        {"path": "plugin.ini", "url": "/api/v1/plugins/airy-theme/1.0.0/plugin.ini",
         "size": 412, "sha1": "…"},
        {"path": "i18n/de.ini", "url": "…", "size": 230, "sha1": "…"}
      ]}}]}
  ```

  - `i18n`: the name and summary in other languages, from an optional
    `[i18n.<lang>]` table of the catalogue entry; English otherwise.
  - `devices`: `android`, `x4` and `x4-pro` (the X4 Pro is a device of
    its own); every device when the entry names none.
  - `latest.formats`: the data format the plugin says in its
    `plugin.ini` (`formats = 1`, ADR 0020's data formats), read by the
    server from the package.
  - `latest.files`: every file of a data plugin's package, its path in
    the plugin's folder, an address relative to the list's, its size and
    SHA-1. The server serves them out of the package it already holds
    (`GET /api/v1/plugins/{id}/{version}/{*path}`), so a device needs
    no unpacking. A `plugin.ini` is always among them.
- **The device's store** is a plugin (`store`), the same for X4 and X4
  Pro, by buttons and by touch. It lists the data plugins fit for this
  device (its `DeviceInfo::id`: `xteink-x4` is `x4`, `xteink-x4-pro` is
  `x4-pro`), made for an SDK this one reads and a data format it knows,
  by group as the website shows them, with fonts as one more entry that
  opens the fonts browser. A plugin's page tells what it is, who made
  it, its licence and size, and installs, updates or removes it.
  Built-in plugins and plugins with code are not listed yet.
- **Installing is a building block of the SDK**, not of one plugin: the
  download into `plugins/.<id>.part`, file after file, each checked by
  size and SHA-1, what is left kept on the card so it goes on after the
  screen is closed, then the folder renamed and loaded. The fonts browser
  moves onto it.
- **The list's address** is a setting for developers (`store.catalog`,
  `https://paperpaw.org/api/v1/plugins.json` unless set), as the fonts
  browser's is.

## Consequences

- One catalogue entry makes a plugin appear on the website and on every
  device it fits; authors change nothing to reach the X4.
- The server keeps every file of a package by its SHA-1 as well as the
  ZIP's SHA-256; a package checked once is served both ways.
- Plugins with code need a firmware built with them (O6's second half,
  a build service): they get fields of their own then, and devices that
  do not know those fields keep passing over them.
