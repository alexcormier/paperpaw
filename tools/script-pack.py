"""Builds a data plugin with the letters of other scripts for devices
whose built-in fonts lack them (the Xteink X4 carries Latin only): packs
of Literata's faces, read from the card a glyph at a time and asked for
what the built-in fonts lack.

Usage:
    python tools/script-pack.py OUT_DIR [SET ...]

SET defaults to cyrillic and greek (see paperpaw_text::charset::named).
The plugin is written to OUT_DIR/<id>; copy that folder into the device's
plugins folder (e.g. plugins/ on the X4's card).
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FONTGEN = ROOT / "target/release/paperpaw-fontgen"
FONTS = ROOT / "plugins/fonts-default/fonts"

# Pack, font file, weight, the built-in font it extends.
FACES = [
    ("literata", "Literata.ttf", 400, "fonts-default.literata"),
    ("literata-bold", "Literata.ttf", 700, "fonts-default.literata-bold"),
    ("literata-italic", "Literata-Italic.ttf", 400, "fonts-default.literata-italic"),
    ("literata-bold-italic", "Literata-Italic.ttf", 700, "fonts-default.literata-bold-italic"),
]

NAMES = {"cyrillic": "Cyrillic", "greek": "Greek"}


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    sets = sys.argv[2:] or ["cyrillic", "greek"]
    unknown = [s for s in sets if s not in NAMES]
    if unknown:
        sys.exit(f"unknown character sets: {', '.join(unknown)}")
    id = "script-" + "-".join(sets)
    out = Path(sys.argv[1]) / id
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    for name, file, weight, _ in FACES:
        subprocess.run([str(FONTGEN), str(FONTS / file), str(out / "fonts" / f"{name}.ppfp"),
                        "--chars", ",".join(sets), "--axis", f"wght={weight}"], check=True)
    title = " and ".join(NAMES[s] for s in sets)
    extends = "\n".join(f"{name}.ppfp = {font}" for name, _, _, font in FACES)
    (out / "plugin.ini").write_text(
        "[plugin]\n"
        f"id = {id}\n"
        f"name = {title} letters\n"
        f"description = {title} letters in Literata, for devices whose fonts lack them.\n"
        "version = 0.1.0\n"
        "\n[extends]\n"
        f"{extends}\n",
        encoding="utf-8",
    )
    shutil.copy(FONTS / "Literata-OFL.txt", out / "fonts" / "Literata-OFL.txt")
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024 / 1024:.1f} MiB")


if __name__ == "__main__":
    main()
