"""Builds the web emulator: the WebAssembly module from shells/web (the
`web` profile: optimised, function names stripped), put next to the
page's files in shells/web/www, and with --serve shows them on
http://localhost:8000 (frame.html for a device alone, index.html with a
page around it). It prints the module's size, also gzipped as a server
sends it; --limit-mb N fails where it is larger, for CI.

Usage:
    python shells/web/build.py [--debug] [--limit-mb N] [--serve]

Needs the target: rustup target add wasm32-unknown-unknown
"""

import gzip
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
WWW = HERE / "www"


def main():
    args = sys.argv[1:]
    debug = "--debug" in args
    profile = "dev" if debug else "web"
    command = ["cargo", "build", "-p", "paperpaw-web", "--target", "wasm32-unknown-unknown",
               "--profile", profile]
    # The commit it is built from, for its version (`device.version`).
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=HERE,
                            capture_output=True, text=True).stdout.strip()
    env = dict(os.environ, PAPERPAW_COMMIT=commit)
    subprocess.run(command, cwd=HERE, check=True, env=env)
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
        / ("debug" if debug else profile)
        / "paperpaw_web.wasm"
    )
    module = WWW / "paperpaw-web.wasm"
    shutil.copyfile(built, module)
    data = module.read_bytes()
    size = len(data) / 1024 / 1024
    zipped = len(gzip.compress(data, 9)) / 1024 / 1024
    print(f"www/paperpaw-web.wasm: {size:.1f} MB, {zipped:.1f} MB gzipped")
    if "--limit-mb" in args:
        limit = float(args[args.index("--limit-mb") + 1])
        if size > limit:
            sys.exit(f"the module is larger than {limit} MB")
    if "--serve" in args:
        # Python serves .wasm as application/wasm, as streaming needs.
        subprocess.run([sys.executable, "-m", "http.server", "8000", "--directory", str(WWW)])


if __name__ == "__main__":
    main()
