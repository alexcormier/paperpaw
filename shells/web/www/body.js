// The device's body around its screen, with its keys where they sit on
// the real one: on its edges, and on its face below or beside the screen.
// Drawn from the profile's body (`device.body`, see docs/web-emulator.md),
// in the embedding page's look (CSS variables set by `start`'s `look`).

const SVG = "http://www.w3.org/2000/svg";

/// How thick a key on an edge is drawn, and its gap to the body, in CSS
/// pixels.
export const EDGE_KEY = 22;
export const EDGE_GAP = 4;

/// How keys are labelled, as the hint bar shows their use: a word, or an
/// icon with its name for screen readers; the button's name where none is
/// here.
const LABELS = {
  back: { text: "Back" },
  confirm: { text: "Select" },
  home: { text: "Home" },
  menu: { text: "Menu" },
  power: { icon: "power", spoken: "Power" },
  left: { icon: "left", spoken: "Left" },
  right: { icon: "right", spoken: "Right" },
  up: { icon: "up", spoken: "Up" },
  down: { icon: "down", spoken: "Down" },
  "page-back": { icon: "left", spoken: "Page back" },
  "page-forward": { icon: "right", spoken: "Page forward" },
};

/// Icons as the hint bar draws them, in the key's colour.
const ICONS = {
  left: ["0 0 10 16", "polyline", { points: "8,2 2,8 8,14" }],
  right: ["0 0 10 16", "polyline", { points: "2,2 8,8 2,14" }],
  up: ["0 0 16 10", "polyline", { points: "2,8 8,2 14,8" }],
  down: ["0 0 16 10", "polyline", { points: "2,2 8,8 14,2" }],
  power: ["0 0 16 16", "path", { d: "M8 1.5v6M4.2 4.2a5.5 5.5 0 1 0 7.6 0" }],
};

function icon(name) {
  const [box, shape, attributes] = ICONS[name];
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", box);
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add(`icon-${name}`);
  const part = document.createElementNS(SVG, shape);
  for (const [key, value] of Object.entries(attributes)) part.setAttribute(key, value);
  svg.append(part);
  return svg;
}

/// Whether `place` runs from top to bottom (its keys stand upright).
const upright = (place) => ["left", "right", "beside"].includes(place);
const edge = (place) => ["top", "right", "bottom", "left"].includes(place);

/// A key's label: page keys stacked on one upright place point up and
/// down (the X4's rocker), elsewhere back and forward.
function label(key, keys) {
  const stacked = upright(key.place) && keys.filter((k) => k.place === key.place && k.button.startsWith("page-")).length > 1;
  if (stacked && key.button === "page-back") return { icon: "up", spoken: "Page back" };
  if (stacked && key.button === "page-forward") return { icon: "down", spoken: "Page forward" };
  return LABELS[key.button] ?? { text: key.button };
}

/// What room the keys on the edges take around the body: top, right,
/// bottom, left, in CSS pixels.
export function margins(body) {
  const out = EDGE_KEY + EDGE_GAP;
  return ["top", "right", "bottom", "left"].map((side) => (body.keys.some((k) => k.place === side) ? out : 0));
}

/// The body's bezels in CSS pixels for a screen `width` wide: top, right,
/// bottom, left.
export function bezels(body, width) {
  return body.bezel.map((part) => Math.round((part * width) / 100));
}

/// Makes the keys of `body` in `element` (the body, around the screen),
/// each pressing its button through `press(name, pressed)`. Returns the
/// keys made, by button.
export function keys(element, body, press) {
  const made = new Map();
  for (const key of body.keys) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `key ${edge(key.place) ? "edge" : "face"} ${key.place}`;
    button.dataset.button = key.button;
    const { text, icon: shown, spoken } = label(key, body.keys);
    if (shown) {
      button.append(icon(shown));
      button.setAttribute("aria-label", spoken);
    } else {
      button.textContent = text;
    }
    const pressed = key.keys.length ? ` (${key.keys.map((k) => (k === " " ? "Space" : k)).join(", ")})` : "";
    button.title = `${spoken ?? text}${pressed}`;
    button.addEventListener("pointerdown", (event) => {
      button.setPointerCapture(event.pointerId);
      press(key.button, true);
    });
    button.addEventListener("pointerup", () => press(key.button, false));
    button.addEventListener("pointercancel", () => press(key.button, false));
    element.append(button);
    made.set(key.button, { key, button });
  }
  return made;
}

/// Puts each key where it sits on a body `width` by `height` (CSS pixels)
/// with `bezel` (top, right, bottom, left).
export function place(made, width, height, bezel) {
  const [, right, bottom] = bezel;
  for (const { key, button } of made.values()) {
    const along = upright(key.place) ? height : width;
    const length = Math.round((key.length * along) / 100);
    const middle = (key.at * along) / 100;
    // Keys on the face: as thick as the bezel allows, with room around.
    const room = key.place === "below" ? bottom : key.place === "beside" ? right : EDGE_KEY;
    const thick = edge(key.place) ? EDGE_KEY : Math.max(18, Math.min(36, Math.round(room * 0.42)));
    const [w, h] = upright(key.place) ? [thick, length] : [length, thick];
    let x;
    let y;
    switch (key.place) {
      case "top":
        [x, y] = [middle - w / 2, -EDGE_GAP - h];
        break;
      case "bottom":
        [x, y] = [middle - w / 2, height + EDGE_GAP];
        break;
      case "left":
        [x, y] = [-EDGE_GAP - w, middle - h / 2];
        break;
      case "right":
        [x, y] = [width + EDGE_GAP, middle - h / 2];
        break;
      case "below":
        [x, y] = [middle - w / 2, height - bottom + (bottom - h) / 2];
        break;
      default:
        // beside
        [x, y] = [width - right + (right - w) / 2, middle - h / 2];
    }
    // Words smaller where the key is narrow (a phone).
    button.classList.toggle("tight", w < 64);
    Object.assign(button.style, {
      left: `${Math.round(x)}px`,
      top: `${Math.round(y)}px`,
      width: `${w}px`,
      height: `${h}px`,
    });
  }
}

/// The page's look for the frame: the CSS variables it may set.
const LOOK = { ink: "--ink", body: "--body", onBody: "--on-body", font: "--font" };

/// Sets `look` (`{ ink, body, onBody, font }`, CSS values) on the frame.
export function apply(look = {}) {
  const style = document.documentElement.style;
  for (const [name, property] of Object.entries(LOOK)) {
    if (typeof look[name] === "string") style.setProperty(property, look[name]);
    else style.removeProperty(property);
  }
}
