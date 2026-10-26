#!/usr/bin/env python3
"""Install CI's pinned espflash without compiling it or installing development headers."""

import hashlib
import io
import pathlib
import platform
import shutil
import subprocess
import sys
import urllib.request
import zipfile

# Official esp-rs/espflash v4.6.0 release asset, checked against GitHub's
# SHA-256 digest. The static musl executable needs no build-time libudev headers.
# Review a new archive and its digest when versions.env changes.
ASSETS = {
    ("4.6.0", "Linux", "x86_64"): (
        "espflash-x86_64-unknown-linux-musl.zip",
        "d515ee13ae44ba913c31a2ca58dc5fd72d3362994ee1d7487a16b289dfbb6e4a",
    ),
}
MAX_BYTES = 32 * 1024 * 1024


def matches(executable, version):
    try:
        result = subprocess.run([str(executable), "--version"], capture_output=True,
                                text=True, encoding="utf-8", timeout=10, check=True)
        return result.stdout.strip() == f"espflash {version}"
    except (OSError, subprocess.SubprocessError):
        return False


def fetch(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("espflash archive is larger than expected")
    return data


def install(version, destination):
    key = version, platform.system(), platform.machine()
    if key not in ASSETS:
        raise ValueError(f"no reviewed espflash archive for {key}; update tools/ci/espflash.py")
    existing = shutil.which("espflash")
    if existing and matches(existing, version):
        print(f"Reusing espflash {version}: {existing}", file=sys.stderr)
        return pathlib.Path(existing).resolve()
    name, checksum = ASSETS[key]
    data = fetch(f"https://github.com/esp-rs/espflash/releases/download/v{version}/{name}")
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError("espflash release archive SHA-256 differs from the reviewed pin")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        info = archive.getinfo("espflash")
        if info.is_dir() or info.file_size > MAX_BYTES or len(archive.infolist()) != 1:
            raise ValueError("unexpected espflash release archive contents")
        executable = archive.read(info)
    destination = pathlib.Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    temporary = destination / "espflash.tmp"
    temporary.write_bytes(executable)
    temporary.chmod(0o755)
    binary = destination / "espflash"
    temporary.replace(binary)
    if not matches(binary, version):
        raise ValueError("the verified espflash executable does not report the pinned version")
    print(f"Verified official espflash {version}: {name}", file=sys.stderr)
    return binary.resolve()


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: tools/ci/espflash.py <version> <install-directory>")
    try:
        print(install(sys.argv[1], sys.argv[2]))
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        sys.exit(f"espflash setup: {error}")


if __name__ == "__main__":
    main()
