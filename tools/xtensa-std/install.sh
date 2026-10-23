#!/usr/bin/env bash
# Installs the ESP32-S3's core libraries of a PaperPaw release
# (paperpaw-xtensa-std-<version>.tar.gz, made by build.sh) into espup's
# `esp` toolchain, where rustc finds a target's libraries:
#
#   tools/xtensa-std/install.sh <paperpaw-xtensa-std-<version>.tar.gz>
#
# Those of another release go first: two of each would be ambiguous.
# Refused where the toolchain is not the one they were built by.
set -euo pipefail
target=xtensa-esp32s3-none-elf

[[ $# -eq 1 && -f "$1" ]] || { echo "usage: $0 <paperpaw-xtensa-std-<version>.tar.gz>" >&2; exit 2; }
archive="$1"

sysroot="$(rustc +esp --print sysroot)"
command -v cygpath >/dev/null && sysroot="$(cygpath -u "$sysroot")"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work"
built="$(cat "$work/lib/rustlib/$target/paperpaw-rustc.txt")"
here="$(rustc +esp -V)"
if [[ "$built" != "$here" ]]; then
    echo "built by $built, but the esp toolchain is $here" >&2
    exit 1
fi

rm -rf "${sysroot:?}/lib/rustlib/$target"
mkdir -p "$sysroot/lib/rustlib"
cp -r "$work/lib/rustlib/$target" "$sysroot/lib/rustlib/"
echo "installed in $sysroot/lib/rustlib/$target:"
ls "$sysroot/lib/rustlib/$target/lib"
