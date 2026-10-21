# The web emulator

`shells/web` runs PaperPaw in a browser: the standard app on an emulated
Xteink X4, X4 Pro or Boox Go 7, drawn into a canvas, with data plugins
handed in from the page and `paperpaw-dev`'s checks of them. It is the
desktop emulator's device model (`paperpaw-emulator` without its window)
built for `wasm32-unknown-unknown`: the same kernel, plugins, device
profiles, panel and heap budgets as `paperpaw-emu` and `paperpaw-dev`, so
a device looks and behaves as it does there.

```
shells/web/
  src/emulator.rs   the device and what a page asks of it (tested natively)
  src/exports.rs    the module's functions (wasm32 only)
  www/              paperpaw-web.js (loader), frame.html and frame.js
                    (the emulator frame), index.html (a page to try it)
  build.py          builds the module into www/ (--serve shows it,
                    --limit-mb fails where it grew too large)
```

`python shells/web/build.py --serve` builds the module (the `web`
profile: as release, function names stripped) and serves `www/`
on `http://localhost:8000`: `frame.html` is a device alone
(`frame.html?profile=boox-go-7&language=de&dark`), `index.html` a page
around it that loads a data plugin's folder and checks it.

## The device model

`paperpaw-emulator` builds without its window (`default-features =
false`, no `winit`) and for `wasm32`:

- Its storage is a folder or kept in memory (`device::Library`,
  `Device::in_memory`); a restart reads the card anew, so changed plugin
  files show after it.
- Time and random numbers come from the computer, or in a browser from
  the page (`host::set_clock`, `host::set_random`). The emulated wall
  clock is fixed (14 October 2026, 10:41), as in `paperpaw-dev`'s
  pictures.
- In a browser there is no network: requests fail at once, plugins'
  servers do not listen. Speech is logged as on the desktop.

## The emulator frame

The frame runs in an iframe, best of an origin of its own, and speaks
`postMessage` with the window that embeds it, and with no other. Every
message may carry a `request` (any value); the answer has the same
`type` and `request`, and `error` (a text) where it failed. Once its module is loaded the frame
sends `{ type: "ready", version }` (PaperPaw's version and the commit
the module was built from). A panic stops a WebAssembly module for good:
the frame then makes a fresh instance, starts the device as it was last
started with the data plugins it had, and sends
`{ type: "restarted", reason }`. The device's heap budget holds in the
browser as on the X4; where it ran out with no way around it (what
stops the device too), the frame says so before it starts again:
`{ type: "out-of-memory", bytes, device }`.

| Message to the frame | Effect | Answer |
|---|---|---|
| `{ type: "start", profile, language, dark, plugins: [{ id, files: { path: bytes } }] }` | Starts a device (`xteink-x4`, `xteink-x4-pro`, `boox-go-7`); each plugin's files go into its storage under `plugins/<id>/`, as a card would have them | `{ device: { id, name, width, height, dpi, grays, color, touch, buttons, row } }` |
| `{ type: "files", id, files }` | Replaces a data plugin's files (whatever of it was on the card goes) and starts the device again (a few milliseconds); `files: {}` takes it off | `{}` |
| `{ type: "input", event }` | `event`: `{ kind: "button", name, pressed }`, `{ kind: "button", name, hold }` (a long press, held `hold` ms), `{ kind: "key", key, pressed }` (the keyboard's keys as the desktop emulator maps them), `{ kind: "pointer", phase: "down" \| "move" \| "up" \| "cancel", x, y }` (the screen's pixels) | `{}` |
| `{ type: "screenshot", profile?, dark?, asleep? }` | The panel as it shows, at the device's resolution; with `profile`, `dark` or `asleep`, what that device, night mode or the sleep screen would show with the same card, made apart (what is shown stays) | `{ png }` (bytes) |
| `{ type: "check", commands?, plugins? }` | `paperpaw-dev`'s checks of the data plugins on the card, on this device: `package`, `lint`, `visual`, `reach` (all if none named), the libraries in memory; with `plugins`, the ids of plugins with code the module adds (see below), checked too, `perf` among the commands | `{ report }` (as `report.json`) |
| `{ type: "command", line }` | The remote control's command, as `tools/x4-remote.py` sends it to an X4 (see below), e.g. `run calibre.open` | `{ answer }` (the command's JSON) |
| `{ type: "describe" }` | What the screen shows, part by part | `{ screen, parts: [{ role, x, y, width, height, state, label }] }` |
| `{ type: "run", action }` | Runs an action, as a button bound to it would | `{ answer: {} }` |
| `{ type: "slot", slot, params? }` | Opens a slot (`core.settings`, `core.home`, `core.library` …) with parameters (`{ id: "reader" }`) | `{ answer: {} }` |
| `{ type: "show", screen, params? }` | Opens a screen by its id, with parameters | `{ answer: {} }` |
| `{ type: "open", path }` | Opens a book of the card in the reader | `{ answer: {} }` |
| `{ type: "set", setting, value }` | Sets a setting, the value as configuration files write it (`"on"`, `"14"`) | `{ answer: {} }`, or `error` with why |
| `{ type: "emit", event, params? }` | Sends an event, as a plugin would | `{ answer: {} }` |
| `{ type: "until", text, timeout? }` | Waits until the screen is `text` (its id) or shows it in a part's label, the device running meanwhile; at most `timeout` ms (30000) | `{}`, or `error` (`not shown in … ms`) |
| `{ type: "expect", text }` | The same, now | `{}`, or `error` (`not shown`) |

**Commands.** `command` takes the remote control's lines, parsed as on
an X4 (`paperpaw_x4::remote`) and carried out with the same kernel
calls; texts that may hold spaces are hexadecimal (`-` for none), as on
the device, which `slot`, `show`, `open`, `set` and `emit` do for a page.
They answer in JSON:

| Line | Answer |
|---|---|
| `run <action>`, `slot <slot> [<key>=<hex> …]`, `show <screen> [<key>=<hex> …]`, `open <hex path>`, `set <setting> <hex value>`, `emit <event> [<key>=<hex> …]`, `press <button>`, `hold <button> <ms>`, `touch <down\|move\|up> <x> <y>`, `time set <unix ms>`, `mkdir <path>`, `rm <path>`, `mv <hex> <hex>` | `{}` |
| `screen` | `{ "screen": id }` |
| `screens` | `{ "screens": [ids, the one shown last] }` |
| `describe` | `{ "screen": id, "parts": [{ "role", "x", "y", "width", "height", "state", "label" }] }`; `state` is `focused`, `on`, `off` joined by `+`, or `-` |
| `setting <id>` | `{ "value": text }` |
| `settings [<prefix>]` | `{ "values": { id: text } }` |
| `actions [<prefix>]` | `{ "actions": [ids] }` |
| `plugins` | `{ "plugins": [{ "id", "on", "version", "kind" }] }` |
| `version` | `{ "version", "device" }` |
| `time` | `{ "unix_ms", "ms" }` (the wall clock, the device's own time) |
| `usage` | `{ "refreshes": [fast, partial, full], "pixels", "inputs" }` |
| `ls [<path>]` | `{ "entries": [{ "name", "dir", "size" }] }` |
| `stat <path>` | `{ "kind": "f" \| "d" \| "-", "size" }` |

Secrets are never said (`•`), and while a PIN or a password is entered
`describe` gives where the parts are, not what they say. What only a
device has (its panel, radio, flash, sleep) answers with `error`.

The frame draws at the device's resolution: each of its pixels a whole
number of the screen's (devicePixelRatio counted), crisp, where it fits
at least once, smoothed where it has to be made smaller; in the grays
the panel shows, in e-ink's tones on gray panels (its black lighter than
a screen's), a full refresh flashing; screenshots are the panel model's
own colours; the
device's buttons are below it, its keys on the keyboard, and the screen
takes touch where the device has a touch screen.

A code plugin cannot be added to the running module: WebAssembly shares
no Rust types between modules. A module of its own is: a crate with
`paperpaw-web` as a library (its `#[no_mangle]` functions are the new
module's too) and `crate-type = ["cdylib"]`, which exports `pp_plugins`
calling `paperpaw_web::set_plugins` with the plugins to add. The loader
calls `pp_plugins` first where a module has it; the frame and its
messages stay the same, and `check` with `plugins: ["<id>"]` looks at
the plugin as `paperpaw-dev check --plugin <id>` does (`lint`, `visual`,
`reach`, `perf`).

## The module

`www/paperpaw-web.js` wraps the module's plain functions (no generated
glue), for pages that drive a device without the frame:

```js
import { PaperPawDevice } from "./paperpaw-web.js";

const device = await PaperPawDevice.load("paperpaw-web.wasm");
device.create({ profile: "xteink-x4", language: "en" });
device.putFile("dusk", "plugin.ini", bytes);
device.start();
device.button("right", true, performance.now());
device.command("run calibre.open", performance.now()); // {}
device.shows("calibre.screen"); // true
const next = device.advance(performance.now()); // ms until due, or null
canvas.getContext("2d").putImageData(device.frame(), 0, 0);
const report = device.check(["package", "lint"]);
```

Buttons and touch take the time they happen at, as `advance` does: the
device lets time pass to then first, so that a press is as long as it
was held, also after a while in which nothing was due. A full
refresh flashes by the page's clock (`animating()` while it does).

The module imports `paperpaw.random(ptr, len)` (fills memory with random
bytes, returns 1), `paperpaw.log(ptr, len)` (a panic's message) and
`paperpaw.now()` (`performance.now()`: the panel's animations, the time
plugins take, which `perf` measures).
