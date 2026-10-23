#!/usr/bin/env bash
# Builds a native PaperPaw plugin for the Xteink X4 and the X4 Pro as the
# plugin library rebuilds it, packs both files, signs them with the
# author's key, and puts them into dist/ with the plugin's folder as ZIPs
# and their SHA-256 sums:
#
#   ./build.sh                       unsigned, to try the build
#   ./build.sh --key <author.key>    signed, to publish
#
# The library rebuilds each file from its source with the sample's
# aliases (`cargo x4`, `cargo +esp x4pro`, with --locked) and compares.
# Paths end up in a build (in panic messages), so they are written as the
# same on every computer, in whatever folder: the source's top (the
# repository) as /plugin and Cargo's home as /cargo. The rebuild does the
# same. The core libraries come built: rustup's for the X4, the PaperPaw
# release's for the X4 Pro.
#
# Needs: Linux or macOS (on Windows, paths in the code keep their `\`,
# so a build there matches only other builds on Windows), the X4's Rust
# from rust-toolchain.toml, espup's `esp` toolchain with its environment
# (`. ~/export-esp.sh`) and the release's core libraries for it
# (paperpaw-xtensa-std-<version>.tar.gz, installed with PaperPaw's
# tools/xtensa-std/install.sh), paperpaw-sign of the PaperPaw release
# built for, and zip. Never set RUSTFLAGS: it would replace
# .cargo/config.toml's flags, which link the plugin.
set -euo pipefail
cd "$(dirname "$0")"

# The binary crate that makes the file (`[[bin]]` in Cargo.toml).
bin=${PLUGIN:-hello}

key=()
case "$#" in
    0) ;;
    2) [[ "$1" == --key ]] || { echo "usage: ./build.sh [--key <author.key>]" >&2; exit 2; }
       key=(--key "$2") ;;
    *) echo "usage: ./build.sh [--key <author.key>]" >&2; exit 2 ;;
esac

ini() { sed -n "s/^$1 *= *//p" plugin/plugin.ini | tr -d '\r' | head -n 1; }
id=$(ini id)
version=$(ini version)
[[ -n "$id" && -n "$version" ]] || { echo "plugin/plugin.ini names no id or version" >&2; exit 1; }

# The paths remapped, added to .cargo/config.toml's flags for a target.
top=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cargo_home=${CARGO_HOME:-$HOME/.cargo}
if command -v cygpath >/dev/null; then
    # Git Bash on Windows: the paths as the compiler sees them (C:/...).
    top=$(cygpath -m "$top")
    cargo_home=$(cygpath -m "$cargo_home")
fi
std="$(rustc +esp --print sysroot)/lib/rustlib/xtensa-esp32s3-none-elf/lib"
if [[ ! -d "$std" ]]; then
    echo "no core libraries for the X4 Pro in the esp toolchain: install the PaperPaw" >&2
    echo "release's paperpaw-xtensa-std-<version>.tar.gz with tools/xtensa-std/install.sh" >&2
    exit 1
fi
remap() {
    local target=$1
    shift
    local flags="'--remap-path-prefix=$top=/plugin', '--remap-path-prefix=$cargo_home=/cargo'"
    for extra in "$@"; do flags+=", '$extra'"; done
    printf 'target.%s.rustflags = [%s]' "$target" "$flags"
}

cargo x4 --locked --config "$(remap riscv32imc-unknown-none-elf)"
cargo +esp x4pro --locked --config "$(remap xtensa-esp32s3-none-elf)"

rm -rf dist
mkdir dist
paperpaw-sign pack "target/riscv32imc-unknown-none-elf/release/$bin" "dist/$id-$version-x4.ppn" "${key[@]}"
paperpaw-sign pack "target/xtensa-esp32s3-none-elf/release/$bin" "dist/$id-$version-x4-pro.ppn" "${key[@]}"
# The plugin's folder, plugin.ini at the top, without hidden files: what
# readers install, and with store/ for the
# upload form.
(cd plugin && zip -q -X -r -D "../dist/$id-$version.zip" . -x '.*' -x '*/.*' -x 'store/*')
if [[ -d plugin/store ]]; then
    (cd plugin && zip -q -X -r -D "../dist/$id-$version-upload.zip" . -x '.*' -x '*/.*')
fi
(cd dist && sha256sum -- * > SHA256SUMS)
for device in x4 x4-pro; do
    echo "$device: fingerprint $(paperpaw-sign plugin "dist/$id-$version-$device.ppn" | sed -n 's/^fingerprint: *//p')"
done
cat dist/SHA256SUMS
