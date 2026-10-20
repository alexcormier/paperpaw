"""Builds PaperPaw's icon font: the icons it uses, taken from Tabler Icons.

Usage:
    python tools/icons/subset.py path/to/tabler-icons.ttf

The font comes from the npm package `@tabler/icons-webfont` (MIT licence,
see plugins/fonts-default/fonts/TablerIcons-MIT.txt), file
`dist/fonts/tabler-icons.ttf`, version TABLER below. Needs fontTools.

Writes:
- plugins/fonts-default/fonts/PaperPawIcons.ttf: only the icons in ICONS,
  at Tabler's code points (private use area);
- crates/paperpaw-sdk/src/icon_table.rs: the names and code points of all
  of Tabler's icons, sorted by name, for `paperpaw_sdk::icons` (plugins
  name any; icon packs bring the drawings the font lacks, see pack.py).

To add an icon, add its Tabler name to ICONS (https://tabler.io/icons)
and run this again.
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

TABLER = "3.35.0"

# Icons by their Tabler names; PaperPaw uses the same names.
ICONS = sorted({
    # Places
    "home", "books", "book", "book-2", "bookmark", "bookmarks", "list",
    "list-details", "folder", "folder-open", "file", "file-text", "files",
    "wifi-off", "inbox",
    # Reading
    "search", "typography", "text-size", "letter-case", "adjustments",
    "adjustments-horizontal", "layout", "layout-bottombar", "layout-grid",
    "columns", "highlight", "cursor-text", "clock-play", "arrows-horizontal",
    "pencil",
    "note", "hash", "arrow-bar-to-right", "language", "a-b", "link",
    "clipboard", "copy", "selector", "volume", "tag",
    # Light and display
    "sun", "sun-low", "moon", "temperature", "brightness", "contrast",
    "refresh",
    # Tools
    "chart-bar", "calendar", "clock", "wifi", "world", "cloud",
    "cloud-download", "download", "upload", "rss", "qrcode", "keyboard",
    "plug", "puzzle", "vocabulary", "transfer", "apps", "camera",
    "book-download", "article", "devices", "device-desktop", "stack-2",
    "automation", "circle-arrow-up", "list-check",
    # System
    "settings", "menu-2", "dots", "dots-vertical", "info-circle", "power",
    "battery", "device-tablet", "user", "lock", "palette", "bug",
    "hourglass", "hand-click", "hand-finger", "wand", "key", "eraser",
    "sparkles", "bulb", "paw", "leaf",
    # Controls
    "chevron-left", "chevron-right", "chevron-up", "chevron-down",
    "arrow-left", "arrow-right", "arrow-up", "arrow-down", "x", "check", "plus",
    "minus", "trash",
    "edit", "star", "eye", "sort-ascending", "filter", "circle",
    "circle-check", "toggle-left", "toggle-right",
})


def tabler_codes(path):
    """Every icon of Tabler's font at `path`: its name and code point."""
    return {name: code for code, name in TTFont(path).getBestCmap().items()}


def icon_font(path, codes):
    """Tabler's font at `path` with only the icons at `codes`, its metrics
    set from the drawings."""
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ["*"]
    options.glyph_names = True
    options.notdef_outline = True
    subsetter = subset.Subsetter(options)
    font = TTFont(path)
    subsetter.populate(unicodes=codes)
    subsetter.subset(font)
    # Tabler's metrics put every left side bearing at 0 although the
    # drawings start further right; renderers that follow the metrics
    # (as PaperPaw's does) would draw icons off centre. The bearings are
    # set from the drawings, and the ascent and descent to the drawings'
    # extent, so an icon centred by its line box is centred by its ink.
    glyf, hmtx = font["glyf"], font["hmtx"]
    top, bottom = 0, 0
    for name in font.getGlyphOrder():
        glyph = glyf[name]
        if glyph.numberOfContours == 0:
            continue
        glyph.recalcBounds(glyf)
        advance, _ = hmtx[name]
        hmtx[name] = (advance, glyph.xMin)
        top, bottom = max(top, glyph.yMax), min(bottom, glyph.yMin)
    font["hhea"].ascent, font["hhea"].descent = top, bottom
    font["OS/2"].sTypoAscender, font["OS/2"].sTypoDescender = top, bottom
    font["OS/2"].usWinAscent, font["OS/2"].usWinDescent = top, -bottom
    return font


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = Path(__file__).resolve().parents[2]
    names = tabler_codes(sys.argv[1])
    missing = [name for name in ICONS if name not in names]
    if missing:
        sys.exit(f"not in Tabler Icons {TABLER}: {', '.join(missing)}")
    codes = {name: names[name] for name in ICONS}

    font = icon_font(sys.argv[1], codes.values())
    # Named for what it is now.
    for record in font["name"].names:
        if record.nameID in (1, 4, 6):
            record.string = "PaperPaw Icons" if record.nameID != 6 else "PaperPawIcons"
    out = root / "plugins/fonts-default/fonts/PaperPawIcons.ttf"
    font.save(out)

    # Every icon's name, so that plugins name any of them; the drawings of
    # those the font lacks come from icon packs (tools/icons/pack.py).
    table = root / "crates/paperpaw-sdk/src/icon_table.rs"
    # Without pointers (one text of all names, and where each ends): a
    # table of string slices went partly into RAM on the X4.
    ordered = sorted(names.items())
    ends, end = [], 0
    for name, _ in ordered:
        end += len(name)
        ends.append(end)
    text = "".join(name for name, _ in ordered)
    lines = [
        "// Generated by tools/icons/subset.py from Tabler Icons "
        f"{TABLER}; do not edit.",
        "",
        "/// The names of all of Tabler's icons, sorted, one after another; the",
        "/// icon font holds those PaperPaw uses, packs add others.",
        "pub(crate) static NAMES: &str = concat!(",
    ]
    lines += [f'    "{text[i:i + 80]}",' for i in range(0, len(text), 80)]
    lines += [
        ");",
        "",
        "/// Where each name of [`NAMES`] ends, and its code point.",
        "pub(crate) static ICONS: &[(u32, char)] = &[",
    ]
    lines += [f"    ({e}, '\\u{{{code:x}}}')," for e, (_, code) in zip(ends, ordered)]
    lines.append("];")
    table.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(codes)} icons in the font, {len(names)} names, {out.stat().st_size} bytes")


if __name__ == "__main__":
    main()
