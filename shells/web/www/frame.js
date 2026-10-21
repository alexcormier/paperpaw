// The emulator frame: a device drawn into a canvas, its buttons below it,
// touch where it has a touch screen, driven by the page around it through
// postMessage (see docs/web-emulator.md). Runs in an iframe of its own
// origin; it answers the window that embeds it only.
//
// A panic in the module stops it for good (a trap): the frame then makes
// a fresh instance, starts the device again as it was last started, with
// the data plugins it had, and tells the page ({ type: "restarted" }).

import { KEYS, PaperPawDevice } from "./paperpaw-web.js";

const canvas = document.getElementById("screen");
const context = canvas.getContext("2d");
const row = document.getElementById("buttons");
const started = performance.now();

let device = null;
let info = null;
/// How the device was last started, and the data plugins on its card by
/// id (their files): what a restart after a trap starts again.
let last = null;
let plugins = {};
let timer = null;
let drawing = false;

const loading = PaperPawDevice.load(new URL("paperpaw-web.wasm", import.meta.url));

function now() {
  return performance.now() - started;
}

/// Runs `f` on the device; a trap restarts it (see above).
function guarded(f) {
  try {
    return f();
  } catch (error) {
    if (error instanceof WebAssembly.RuntimeError) {
      restart(String(error.message ?? error));
    }
    throw error;
  }
}

/// A fresh instance, started again as last time.
async function restart(reason) {
  console.error("paperpaw: the device stopped and starts again:", reason);
  clearTimeout(timer);
  const broken = device;
  device = null;
  if (!broken || !last) return;
  try {
    device = await broken.fresh();
    boot(last, plugins);
    tell({ type: "restarted", reason });
  } catch (error) {
    console.error("paperpaw:", error);
  }
}

/// Tells the page around the frame.
function tell(message) {
  if (window.parent !== window) window.parent.postMessage(message, "*");
}

/// Draws what the panel shows, and keeps drawing while it changes.
function draw() {
  drawing = false;
  if (!device) return;
  guarded(() => {
    const [width, height] = device.size();
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    context.putImageData(device.frame(), 0, 0);
    if (device.animating()) requestDraw();
  });
}

function requestDraw() {
  if (!drawing) {
    drawing = true;
    requestAnimationFrame(draw);
  }
}

/// Lets the device's time pass, draws, and waits for what is due next.
function step() {
  clearTimeout(timer);
  if (!device) return;
  const next = guarded(() => device.advance(now()));
  requestDraw();
  if (next !== null) timer = setTimeout(step, Math.max(next, 1));
}

/// The buttons below the screen: the row the device has, else all.
function buttons() {
  row.replaceChildren();
  const names = info.row.length ? info.row : info.buttons;
  for (const name of names) {
    const button = document.createElement("button");
    button.textContent = name;
    button.addEventListener("pointerdown", () => press(name, true));
    button.addEventListener("pointerup", () => press(name, false));
    button.addEventListener("pointerleave", (event) => {
      if (event.buttons) press(name, false);
    });
    row.append(button);
  }
}

function press(name, pressed) {
  if (device && guarded(() => device.button(name, pressed))) step();
}

/// Where a pointer event is on the device's screen, in its pixels.
function at(event) {
  const rect = canvas.getBoundingClientRect();
  return [((event.clientX - rect.left) * canvas.width) / rect.width, ((event.clientY - rect.top) * canvas.height) / rect.height];
}

for (const [type, phase] of [
  ["pointerdown", "down"],
  ["pointermove", "move"],
  ["pointerup", "up"],
  ["pointercancel", "cancel"],
]) {
  canvas.addEventListener(type, (event) => {
    if (!device || !info.touch || (phase === "move" && !event.buttons)) return;
    if (phase === "down") canvas.setPointerCapture(event.pointerId);
    guarded(() => device.pointer(phase, ...at(event)));
    step();
  });
}

for (const type of ["keydown", "keyup"]) {
  window.addEventListener(type, (event) => {
    const name = KEYS[event.key];
    if (!name || !device || event.repeat) return;
    event.preventDefault();
    press(name, type === "keydown");
  });
}

/// Makes the device of `how` on `device`, puts `given` (files by plugin
/// id) on its card and starts it.
function boot(how, given) {
  device.create(how);
  plugins = {};
  for (const [id, files] of Object.entries(given)) put(id, files);
  device.start();
  info = device.device();
  buttons();
  step();
}

/// Starts a device: the files go to `plugins/<id>/` on its card.
async function start({ profile, language, dark, plugins: given = [] }) {
  device = await loading;
  last = { profile, language, dark };
  boot(last, Object.fromEntries(given.map((plugin) => [plugin.id, plugin.files])));
  return { device: info };
}

function put(id, files) {
  for (const [path, bytes] of Object.entries(files)) device.putFile(id, path, bytes);
  plugins[id] = files;
}

/// Replaces a data plugin's files, whatever of it was on the card, and
/// starts the device again.
function files({ id, files: given }) {
  device.removePlugin(id);
  delete plugins[id];
  put(id, given);
  device.start();
  step();
  return {};
}

function input({ event }) {
  if (event.kind === "button") {
    press(event.name, event.pressed);
  } else if (event.kind === "key") {
    const name = KEYS[event.key];
    if (name) press(name, event.pressed ?? true);
  } else if (event.kind === "pointer") {
    device.pointer(event.phase, event.x, event.y);
    step();
  }
  return {};
}

/// The panel as a PNG, at the device's own resolution.
async function screenshot() {
  const image = device.shown();
  const offscreen = new OffscreenCanvas(image.width, image.height);
  offscreen.getContext("2d").putImageData(image, 0, 0);
  const blob = await offscreen.convertToBlob({ type: "image/png" });
  return { png: new Uint8Array(await blob.arrayBuffer()) };
}

function check({ commands }) {
  return { report: device.check(commands ?? []) };
}

const HANDLERS = { start, files, input, screenshot, check };

window.addEventListener("message", async (event) => {
  if (event.source !== window.parent || event.source === window) return;
  const message = event.data ?? {};
  const handler = HANDLERS[message.type];
  // `request` pairs the answer with the message (`id` names a plugin).
  const reply = (answer) => event.source.postMessage({ type: message.type, request: message.request, ...answer }, event.origin);
  if (!handler) return reply({ error: `unknown message type ${message.type}` });
  if (message.type !== "start" && !device) return reply({ error: "no device started yet" });
  try {
    reply(await handler(message));
  } catch (error) {
    if (error instanceof WebAssembly.RuntimeError) restart(String(error.message ?? error));
    reply({ error: String(error.message ?? error) });
  }
});

// Ready for messages once the module is loaded.
loading.then(
  () => tell({ type: "ready" }),
  (error) => console.error("paperpaw:", error),
);

// Opened on its own (not in a frame): a device to try, from the address
// (`frame.html?profile=boox-go-7&language=de&dark`).
if (window.parent === window) {
  const query = new URLSearchParams(location.search);
  start({
    profile: query.get("profile") ?? "xteink-x4",
    language: query.get("language") ?? "en",
    dark: query.has("dark"),
  }).catch((error) => console.error("paperpaw:", error));
}
