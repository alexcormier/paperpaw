"""Draws QuireInk's screens in the emulator, one picture each, for a look
at all of them side by side (e.g. before a release, to see that they look
alike and calm). The sample books are the library; the setup counts as
done.

Usage:
    python tools/screens.py OUT_DIR [--lang de] [PROFILE ...]

PROFILE defaults to xteink-x4 and boox-go-7. Builds the emulator first
(cargo build -p quireink-emulator).
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EMULATOR = ROOT / "target/debug/quireink-emu"

# Name, script, settings; a book open at start where the script says so.
SCREENS = [
    ("home", "", []),
    ("menu", "run:core.menu", []),
    ("tools", "run:core.tools", []),
    ("library", "run:core.library", []),
    ("settings", "run:core.settings", []),
    ("reader", "@book", []),
    ("reader-menu", "@book,run:core.menu", []),
    ("quick-panel", "run:core.quick-panel", []),
    ("toc", "@book,run:reader.show-toc", []),
    ("stats", "run:stats.open", []),
    ("transfer", "run:transfer.open,wait:500", []),
    ("calibre", "run:calibre.open,wait:500", []),
    ("catalogs", "run:opds.open", []),
    ("files", "run:file-manager.open", []),
    ("news", "run:news.open", []),
    ("battery", "run:battery.open", []),
    ("vocabulary", "run:vocabulary.open", []),
    ("browser", "run:browser.open", []),
    ("rules", "run:rules.open", []),
    ("buttons", "run:keys.open", []),
    ("plugins", "run:plugin-manager.open", []),
    ("menu-editor", "run:menu-editor.open", []),
    ("profiles", "run:profiles.open", []),
    ("setup", "run:setup.open", []),
]

BOOK = "pride-and-prejudice.epub"


def library(book):
    dir = Path(tempfile.mkdtemp(prefix="quireink-screens-"))
    books = dir / "books"
    shutil.copytree(ROOT / "samples/library/books", books)
    (dir / ".quireink/plugins/setup").mkdir(parents=True)
    (dir / ".quireink/plugins/setup/done").write_text("")
    if book:
        names = sorted(p.name for p in books.iterdir() if p.suffix == ".epub")
        name = BOOK if BOOK in names else names[0]
        state = dir / ".quireink/plugins/reader"
        state.mkdir(parents=True)
        (state / "state.ini").write_text(f"[reader]\nlast = books/{name}\n")
    return dir


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    args = sys.argv[2:]
    lang = None
    if "--lang" in args:
        at = args.index("--lang")
        lang = args[at + 1]
        del args[at:at + 2]
    profiles = args or ["xteink-x4", "boox-go-7"]
    subprocess.run(["cargo", "build", "-q", "-p", "quireink-emulator"], cwd=ROOT, check=True)
    for profile in profiles:
        for name, script, sets in SCREENS:
            book = script.startswith("@book")
            script = script.removeprefix("@book").lstrip(",")
            dir = library(book)
            shot = out / f"{profile}-{name}.png"
            command = [str(EMULATOR), "--profile", profile, "--headless",
                       "--library", str(dir), "--screenshot", str(shot),
                       "--set", "home.gesture-tips=false"]
            if not book:
                command += ["--set", "reader.open-last=false"]
            if lang:
                command += ["--set", f"core.language={lang}"]
            if script:
                command += ["--script", script]
            for setting in sets:
                command += ["--set", setting]
            result = subprocess.run(command, capture_output=True, text=True)
            status = "ok" if result.returncode == 0 else result.stderr.strip().splitlines()[-1:]
            print(f"{profile} {name}: {status}")
            shutil.rmtree(dir, ignore_errors=True)


if __name__ == "__main__":
    main()
