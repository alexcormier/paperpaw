"""Builds the data plugins of Literata that the X4's firmware leaves out
to keep room for updates, for the font library: its larger sizes (36, 40
and 48 px) as packs that extend the built-in faces, and its bold italic
face at every size, bound to the reading role. Both are read from the
card a glyph at a time.

Usage:
    python tools/literata-packs.py OUT_DIR

Writes OUT_DIR/literata-large and OUT_DIR/literata-bold-italic; copy a
folder into the device's plugins folder (plugins/ on the X4's card), or
offer it in the font library.
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FONTGEN = ROOT / "target/release/paperpaw-fontgen"
FONTS = ROOT / "plugins/fonts-default/fonts"

# Sizes the firmware leaves out of the reading faces (it keeps up to
# 32 px, plugins/fonts-default/build.rs), and all sizes.
LARGE = "36,40,48"
ALL = "20,24,28,32,36,40,48"

# Pack, font file, weight, the built-in face it extends.
FACES = [
    ("literata", "Literata.ttf", 400, "fonts-default.literata"),
    ("literata-bold", "Literata.ttf", 700, "fonts-default.literata-bold"),
    ("literata-italic", "Literata-Italic.ttf", 400, "fonts-default.literata-italic"),
]


def pack(file, weight, sizes, out):
    subprocess.run([str(FONTGEN), str(FONTS / file), str(out), "--sizes", sizes,
                    "--chars", "latin,ligatures", "--axis", f"wght={weight}"], check=True)


def plugin(out, id, name, description, sections):
    (out / "plugin.ini").write_text(
        "[plugin]\n"
        f"id = {id}\n"
        f"name = {name}\n"
        f"description = {description}\n"
        "version = 1.0.0\n"
        "formats = 3\n"
        f"\n{sections}",
        encoding="utf-8",
    )
    shutil.copy(FONTS / "Literata-OFL.txt", out / "fonts" / "Literata-OFL.txt")
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024:.0f} KiB")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    base = Path(sys.argv[1])

    out = base / "literata-large"
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    for name, file, weight, _ in FACES:
        pack(file, weight, LARGE, out / "fonts" / f"{name}-large.ppfp")
    extends = "\n".join(f"{name}-large.ppfp = {face}" for name, _, _, face in FACES)
    plugin(out, "literata-large", "Literata, larger sizes",
           "Book text in Literata larger than the firmware's sizes.",
           f"[extends]\n{extends}\n")

    out = base / "literata-bold-italic"
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    pack("Literata-Italic.ttf", 700, ALL, out / "fonts" / "literata-bold-italic.ppfp")
    plugin(out, "literata-bold-italic", "Literata Bold Italic",
           "Bold italic book text in Literata, where bold stands in for it.",
           "[family]\nname = Literata\nbold-italic = literata-bold-italic.ppfp\n\n"
           "[fonts]\ncore.font-reading-bold-italic = literata-bold-italic.bold-italic\n")


if __name__ == "__main__":
    main()
