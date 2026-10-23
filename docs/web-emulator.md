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
  src/emulator/commands.rs  the remote control's commands, as on an X4
  src/network.rs    plugins' requests answered from the page's recorded answers
  src/exports.rs    the module's functions (wasm32 only)
  tests/            the emulator, its commands, network and a code plugin, natively
  www/              paperpaw-web.js (loader), frame.html and frame.js
                    (the emulator frame), index.html (a page to try it)
  build.py          builds the module into www/ (--serve shows it,
                    --limit-mb fails where it grew too large)
```

`python shells/web/build.py --serve` builds the module with the `web`
profile, which is the release profile with symbols stripped. It then
serves `www/` on `http://localhost:8000`. There, `frame.html` is a
device alone, as in `frame.html?profile=boox-go-7&language=de&dark`.
`index.html` is a page around it that loads a data plugin's folder and
checks it.

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
- In a browser plugins' requests get the page's recorded answers
  (below); none goes through a server of PaperPaw's. Plugins' servers do
  not listen. Speech is logged as on the desktop.

## The emulator frame

The frame runs in an iframe, best of an origin of its own, and speaks
`postMessage` with the window that embeds it, and with no other. Every
message may carry a `request`, which can be any value. The answer has
the same `type` and `request`, and an `error` text where it failed.
Once its module is loaded the frame
sends `{ type: "ready", version }` (PaperPaw's version and the commit
the module was built from). A panic stops a WebAssembly module for good:
the frame then makes a fresh instance, starts the device as it was last
started with the data plugins it had, and sends
`{ type: "restarted", reason }`. The device's heap budget holds in the
browser as on the X4; where it ran out with no way around it (what
stops the device too), the frame says so before it starts again:
`{ type: "out-of-memory", bytes, device }`. Before `start`, only
`fixtures` and `network` are taken; other messages answer with `error`.

| Message to the frame | Effect | Answer |
|---|---|---|
| `{ type: "start", profile, language, dark, plugins: [{ id, files: { path: bytes } }] }` | Starts a device (`xteink-x4`, `xteink-x4-pro`, `boox-go-7`, or another profile of the emulator); each plugin's files go into its storage under `plugins/<id>/`, as a card would have them | `{ device: { id, name, width, height, dpi, grays, color, touch, buttons, row, version } }` |
| `{ type: "files", id, files }` | Replaces a data plugin's files (whatever of it was on the card goes) and starts the device again (a few milliseconds); `files: {}` takes it off | `{}` |
| `{ type: "input", event }` | `event`: `{ kind: "button", name, pressed }`, `{ kind: "button", name, hold }` (a long press, held `hold` ms), `{ kind: "key", key, pressed }` (the keyboard's keys as the desktop emulator maps them), `{ kind: "pointer", phase: "down" \| "move" \| "up" \| "cancel", x, y }` (the screen's pixels) | `{}` |
| `{ type: "screenshot", profile?, dark?, asleep? }` | The panel as it shows, at the device's resolution; with `profile`, `dark` or `asleep`, what that device, night mode or the sleep screen would show with the same card, made apart (what is shown stays) | `{ png }` (bytes) |
| `{ type: "check", commands?, plugins? }` | `paperpaw-dev`'s checks of the data plugins on the card, on this device: `package`, `lint`, `visual`, `reach` (all if none named), the libraries in memory; with `plugins`, the ids of plugins with code the module adds (see below), checked too, `perf` among the commands | `{ report }` (as `report.json`) |
| `{ type: "command", line }` | The remote control's command, as `tools/x4-remote.py` sends it to an X4 (see below), e.g. `run calibre.open` | `{ answer }` (the command's JSON) |
| `{ type: "describe" }` | What the screen shows, part by part | `{ screen, parts: [{ role, x, y, width, height, state, focused, on, label }] }` |
| `{ type: "run", action }` | Runs an action, as a button bound to it would | `{ answer: {} }` |
| `{ type: "slot", slot, params? }` | Opens a slot (`core.settings`, `core.home`, `core.library` …) with parameters (`{ id: "reader" }`) | `{ answer: {} }` |
| `{ type: "show", screen, params? }` | Opens a screen by its id, with parameters | `{ answer: {} }` |
| `{ type: "open", path }` | Opens a book of the card in the reader | `{ answer: {} }` |
| `{ type: "set", setting, value }` | Sets a setting, the value as configuration files write it (`"on"`, `"14"`) | `{ answer: {} }`, or `error` with why |
| `{ type: "emit", event, params? }` | Sends an event, as a plugin would | `{ answer: {} }` |
| `{ type: "type", text, enter? }` | Types into the text field shown (the keyboard's) as if on its keys; with `enter: true` then confirms it | `{ answer: {} }`, or `error` (`no text field`) |
| `{ type: "until", text, timeout? }` | Waits until the screen is `text` (its id) or shows it in a part's label, the device running meanwhile; at most `timeout` ms (30000) | `{}`, or `error` (`not shown in … ms`) |
| `{ type: "expect", text }` | The same, now | `{}`, or `error` (`not shown`) |
| `{ type: "fixtures", fixtures: [{ method?, url, status?, headers?, body?, delay? }] }` | Recorded answers for plugins' requests, in place of those before; kept for every device started after (also before `start`) | `{}` |
| `{ type: "network", mode }` | `"fixtures"` (the default): requests no recorded answer answers fail as without a network; `"real"`: the frame fetches them itself, only where it is served on this computer (`localhost`, `127.0.0.1`, `[::1]`, `*.localhost`) | `{}`, or `error` elsewhere |

**Plugins' requests.** By default nothing leaves the browser: a request
gets the first recorded answer whose `method` (any where none is named)
and `url` match. `url` is a pattern for the whole address, its query
too, `*` standing for any text: `https://api.example.com/v1/feed*`
matches `…/v1/feed?page=2`, `…/v1/feed` does not. Scheme and host are
compared in any case, path and query as written. `status` is 200 unless
said. `headers` is an object: `content-type`, `etag` and the headers a
plugin asks for come from it. `body` is text, bytes or an object, which
is sent as JSON. Bytes are an `ArrayBuffer` or a typed array; read a
`Blob` first. `delay` is in ms of the device's time. A delay longer
than the request waits makes it fail as timed out. A redirect (a 3xx with `location`) is followed through
the table, at most ten, as the desktop's `curl -L` does: the plugin gets
the last answer, with the address it came from. Answers are cut to what
a request takes, as a device does. A request no recorded answer answers
fails as without a network (status 0). In `"real"` mode, only where the
frame is served on this computer, the frame fetches it instead
(`fetch()` without the page's cookies, within the browser's rules:
servers without CORS, and addresses the frame's Content-Security-Policy
leaves out, fail with status 0 and the reason), so a local setup can let
plugins reach the internet; paperpaw.org forwards nothing. Every request
is told to the page as it is made:

`{ type: "request", id, method, url, source, status }`. Here `source` is
one of three: `"fixture"` with the fixture's `status`; `"none"`, failed
with `status` 0; or `"fetch"`, fetched by the frame with `status` null.

The frame serves the page that wrote to it first: messages from other
origins are passed over, and what it tells (plugins' addresses among
it) goes to that page's origin alone; only `ready` goes to any. A server
that hosts the frame should also say which pages may embed it
(`Content-Security-Policy: frame-ancestors …`).

On an X4 profile PaperPaw manages the radio as on the device: a plugin
that waits for Wi-Fi asks for it to be joined first (the pretend radio's
network `PaperPaw Test`, any password but an empty one or `wrong`); on the Boox the
network is the system's.

**Commands.** `command` takes the remote control's lines. They are
parsed as on an X4 (`paperpaw_x4::remote`) and carried out with the
same kernel calls. As on the device, texts that may hold spaces are
written in hexadecimal, and `-` stands for none. The messages `slot`,
`show`, `open`, `set` and `emit` do this for a page.
They answer in JSON:

| Line | Answer |
|---|---|
| `run <action>`, `slot <slot> [<key>=<hex> …]`, `show <screen> [<key>=<hex> …]`, `open <hex path>`, `set <setting> <hex value>`, `emit <event> [<key>=<hex> …]`, `type <hex>`, `type-enter <hex>`, `press <button>`, `hold <button> <ms>`, `touch <down\|move\|up> <x> <y>`, `time set <unix ms>`, `mkdir <path>`, `rm <path>`, `mv <hex> <hex>` | `{}` |
| `screen` | `{ "screen": id }` |
| `screens` | `{ "screens": [ids, the one shown last] }` |
| `describe` | `{ "screen": id, "parts": [{ "role", "x", "y", "width", "height", "state", "focused", "on", "label" }] }`; `state` is `focused`, `on`, `off` joined by `+`, or `-`, the same as `focused` (true, false) and `on` (true, false, null: no switch) |
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

The frame draws at the device's resolution. Where the device fits on
the screen at least once, each of its pixels covers a whole number of
the screen's pixels, with devicePixelRatio counted, and is drawn crisp.
Where it has to be made smaller, it is smoothed. It shows the grays the
panel shows, in e-ink's tones on gray panels, so its black is lighter
than a screen's. A full refresh flashes. Screenshots have the panel
model's own colours. The device's buttons are below it, its keys are on
the keyboard, and the screen takes touch where the device has a touch
screen.

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
device.setFixtures([{ url: "https://api.example.com/*", body: { items: [] } }]);
device.requests(); // [{ id, method, url, source, status }]
device.shows("calibre.screen"); // true
const next = device.advance(performance.now()); // ms until due, or null
canvas.getContext("2d").putImageData(device.frame(), 0, 0);
const report = device.check(["package", "lint"]);
```

Buttons and touch take the time they happen at, as `advance` does: the
device lets time pass to then first, so that a press is as long as it
was held, also after a while in which nothing was due. A full
refresh flashes by the page's clock (`animating()` while it does).

The module imports three functions. `paperpaw.random(ptr, len)` fills
memory with random bytes and returns 1. `paperpaw.log(ptr, len)` takes a
panic's message. `paperpaw.now()` is `performance.now()`, for the
panel's animations and for the time plugins take, which `perf`
measures.
