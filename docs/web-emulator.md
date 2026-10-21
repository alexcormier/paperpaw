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
  build.py          builds the module into www/ (--serve shows it)
```

`python shells/web/build.py --serve` builds the module and serves `www/`
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
  the page (`host::set_micros`, `host::set_random`). The emulated wall
  clock is fixed (14 October 2026, 10:41), as in `paperpaw-dev`'s
  pictures.
- In a browser there is no network: requests fail at once, plugins'
  servers do not listen. Speech is logged as on the desktop.

## The emulator frame

The frame runs in an iframe, best of an origin of its own, and speaks
`postMessage` with the window that embeds it, and with no other. Every
message may carry an `id`; the answer has the same `type` and `id`, and
`error` (a text) where it failed. Once its module is loaded the frame
sends `{ type: "ready" }`.

| Message to the frame | Effect | Answer |
|---|---|---|
| `{ type: "start", profile, language, dark, plugins: [{ id, files: { path: bytes } }] }` | Starts a device (`xteink-x4`, `xteink-x4-pro`, `boox-go-7`); each plugin's files go into its storage under `plugins/<id>/`, as a card would have them | `{ device: { id, name, width, height, dpi, grays, color, touch, buttons, row } }` |
| `{ type: "files", id, files }` | Replaces a data plugin's files and starts the device again (a few milliseconds) | `{}` |
| `{ type: "input", event }` | `event`: `{ kind: "button", name, pressed }`, `{ kind: "key", key, pressed }` (the keyboard's keys as the desktop emulator maps them), `{ kind: "pointer", phase: "down" \| "move" \| "up" \| "cancel", x, y }` (the screen's pixels) | `{}` |
| `{ type: "screenshot" }` | The panel as it shows, at the device's resolution | `{ png }` (bytes) |
| `{ type: "check", commands? }` | `paperpaw-dev`'s checks of the data plugins on the card, on this device: `package`, `lint`, `visual`, `reach` (all if none named), the libraries in memory | `{ report }` (as `report.json`) |

The frame draws at the device's resolution, scaled to fit without
blurring, in the grays the panel shows, a full refresh flashing; the
device's buttons are below it, its keys on the keyboard, and the screen
takes touch where the device has a touch screen.

A code plugin cannot be added to the running module: WebAssembly shares
no Rust types between modules. A build of the whole app with the plugin
compiled in (as `paperpaw-dev`'s harness does) is the way, and it speaks
the same messages.

## The module

`www/paperpaw-web.js` wraps the module's plain functions (no generated
glue), for pages that drive a device without the frame:

```js
import { PaperPawDevice } from "./paperpaw-web.js";

const device = await PaperPawDevice.load("paperpaw-web.wasm");
device.create({ profile: "xteink-x4", language: "en" });
device.putFile("dusk", "plugin.ini", bytes);
device.start();
device.button("right", true);
const next = device.advance(performance.now()); // ms until due, or null
canvas.getContext("2d").putImageData(device.frame(), 0, 0);
const report = device.check(["package", "lint"]);
```

The module imports `paperpaw.random(ptr, len)` (fills memory with random
bytes, returns 1) and `paperpaw.log(ptr, len)` (a panic's message).
