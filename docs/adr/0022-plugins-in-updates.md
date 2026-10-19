# 0022. Plugins in updates

- Status: Accepted
- Date: 2026-10-19

## Context

The X4's firmware has its code plugins built in, and an update replaces
the kernel and all of them at once (ADR 0019). Once readers choose code
plugins of their own (a firmware built with them, later ordered from
paperpaw.org), an update must bring the same ones, and the web flasher
must be able to choose them again for a new firmware. An update that
cannot bring one (not in the plugin library, or not built for the new
version yet) must say so before anything changes, never fail half way.

- An app image begins with its header and the application descriptor
  (`esp_app_desc_t`, 256 bytes); the bootloader and the update check
  read them, and the web flasher can read the first bytes of an app
  partition over USB in a moment, but not search megabytes.
- esp-hal's linker script places the sections `.flash.appdesc` and then
  `.flash.appdesc.*` first in the read-only data.
- Updates were only found on the card (ADR 0019); none were fetched.

## Decision

- **The image lists its plugin crates** (`paperpaw_update::Plugins`):
  `PPlugins`, a format number, a length, then a line per crate, its
  name and version (`paperpaw-plugin-stats 0.4.0`). The firmware's
  `build.rs` writes it from the firmware's lockfile (every crate named
  `paperpaw-plugin-*`) into the section `.flash.appdesc.plugins`, so it
  sits at offset 0x120 of every image, right after the descriptor. The
  firmware reads its own from flash, no copy in memory
  (`Firmware::plugins`); *About* shows it. `paperpaw-sign plugins` prints
  it, and a release fails without it; the release carries it as
  `paperpaw-x4-<version>-plugins.txt` beside the update file.
- **An update says what it leaves out**: `Update::plugins` is the list
  in an update file, `Update::leaves_out` the crates the running
  firmware has and it lacks. The update page names them, and its button
  becomes *Update without them*; *Not now* waits. Older images, without
  a list, say nothing.
- **Updates from the update server**, only when the reader asks (the
  page's *Search online*, or the action `update.check`, e.g. from a
  rule). The setting `update.server` (developers) names the server,
  `https://paperpaw.org` unless set. The device sends
  `POST <server>/api/v1/updates`:

  ```json
  {"project": "paperpaw-xteink", "chip": 5, "version": "0.4.0",
   "plugins": ["paperpaw-plugin-stats 0.4.0", "chess-for-paperpaw 1.0.0"]}
  ```

  and the server answers `200` with the newest update for that device and
  selection, or `"version": ""` when there is none:

  ```json
  {"version": "0.5.0",
   "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
   "size": 6194784,
   "plugins": ["paperpaw-plugin-stats 0.5.0"],
   "missing": ["chess-for-paperpaw not-ready"]}
  ```

  `url` is the update file's address, or a path on the server asked.
  `plugins` are the crates the update has built in; `missing` may say
  why a crate of the request is not there (`unknown`: not in the plugin
  library, `not-ready`: not built for this version yet). Until
  paperpaw.org builds firmware with chosen plugins, its answer is the
  release, with what the release lacks as `missing`. The device downloads
  the file into `updates` (as `updates/.download` until it is whole),
  and installs it as one copied there: its own list, its signature and
  its device are checked again then.
- **The web flasher** reads which app partition runs (the OTA data),
  then the list at 0x120 of that partition, and offers the same plugins
  for the image it writes.

## Consequences

- About 1.3 KB of flash for the list of 43 crates; no memory while
  running.
- The device never trusts the server's word alone: what it installs is
  what the signed file lists.
- The server learns the device's version and plugin crates when the
  reader asks for an update, nothing else; no account, no identifier.
- Plugins are named by their crates. First-party crates
  (`paperpaw-plugin-<id>`) show their plugin's translated name; others
  their crate's name.
- A server of one's own can answer the same question (e.g. with one's
  own builds, signed with one's own key).
- Building firmware with chosen plugins on paperpaw.org (O6) only
  changes the server's answer: devices need nothing new for it.
