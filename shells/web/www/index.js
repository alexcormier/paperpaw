// A page around the frame: it speaks the frame's messages only
// (docs/web-emulator.md).
const frame = document.getElementById("frame");
const report = document.getElementById("report");
let plugin = null;
let next = 1;
const waiting = new Map();

function send(message) {
  const request = next++;
  frame.contentWindow.postMessage({ ...message, request }, "*");
  return new Promise((resolve) => waiting.set(request, resolve));
}

window.addEventListener("message", (event) => {
  if (event.source !== frame.contentWindow) return;
  if (event.data.type === "ready") return start();
  if (event.data.type === "restarted") {
    report.textContent = `The device stopped and was started again: ${event.data.reason}`;
    return;
  }
  waiting.get(event.data.request)?.(event.data);
  waiting.delete(event.data.request);
});

async function start() {
  const answer = await send({
    type: "start",
    profile: document.getElementById("profile").value,
    language: document.getElementById("language").value,
    dark: document.getElementById("dark").checked,
    plugins: plugin ? [plugin] : [],
  });
  report.textContent = answer.error ?? `${answer.device.name}: ${answer.device.width} x ${answer.device.height}, ${answer.device.grays} grays`;
}

for (const id of ["profile", "language", "dark"]) {
  document.getElementById(id).addEventListener("change", start);
}

/// The id plugin.ini names, if it names one.
function manifestId(bytes) {
  let section = "";
  for (const line of new TextDecoder().decode(bytes).split("\n")) {
    const text = line.trim();
    if (text.startsWith("[")) section = text.slice(1, -1).trim();
    const [key, ...value] = text.split("=");
    if (section === "plugin" && key.trim() === "id" && value.length) return value.join("=").trim();
  }
  return null;
}

document.getElementById("folder").addEventListener("change", async (event) => {
  const files = {};
  let folder = null;
  for (const file of event.target.files) {
    const [top, ...rest] = file.webkitRelativePath.split("/");
    folder ??= top;
    // Hidden files and folders (.git, .DS_Store) are no part of it.
    if (rest.some((part) => part.startsWith("."))) continue;
    files[rest.join("/")] = new Uint8Array(await file.arrayBuffer());
  }
  if (!folder) return;
  const id = (files["plugin.ini"] && manifestId(files["plugin.ini"])) || folder;
  if (plugin && plugin.id !== id) await send({ type: "files", id: plugin.id, files: {} });
  plugin = { id, files };
  const answer = await send({ type: "files", id, files });
  report.textContent = answer.error ?? `${id}: ${Object.keys(files).length} files on the card`;
});

document.getElementById("check").addEventListener("click", async () => {
  report.textContent = "Checking ...";
  const answer = await send({ type: "check" });
  const findings = answer.report?.findings.map((f) => `${f.level} ${f.check} ${f.place}: ${f.message}`) ?? [];
  report.textContent = answer.error ?? (findings.join("\n") || "No findings.");
});
