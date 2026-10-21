"""Builds the web emulator: the WebAssembly module from shells/web, put
next to the page's files in shells/web/www, and with --serve shows them
on http://localhost:8000 (frame.html for a device alone, index.html with a
page around it).

Usage:
    python shells/web/build.py [--debug] [--serve]

Needs the target: rustup target add wasm32-unknown-unknown
"""

import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
WWW = HERE / "www"


def main():
    args = sys.argv[1:]
    debug = "--debug" in args
    command = ["cargo", "build", "-p", "paperpaw-web", "--target", "wasm32-unknown-unknown"]
    if not debug:
        command.append("--release")
    subprocess.run(command, cwd=HERE, check=True)
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=HERE,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    built = (
        Path(metadata["target_directory"])
        / "wasm32-unknown-unknown"
        / ("debug" if debug else "release")
        / "paperpaw_web.wasm"
    )
    shutil.copyfile(built, WWW / "paperpaw-web.wasm")
    size = (WWW / "paperpaw-web.wasm").stat().st_size
    print(f"www/paperpaw-web.wasm: {size / 1024 / 1024:.1f} MB")
    if "--serve" in args:
        # Python serves .wasm as application/wasm, as streaming needs.
        subprocess.run([sys.executable, "-m", "http.server", "8000", "--directory", str(WWW)])


if __name__ == "__main__":
    main()
