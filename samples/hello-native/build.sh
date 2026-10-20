#!/usr/bin/env bash
# Builds a native PaperPaw plugin for the Xteink X4 and the X4 Pro, packs
# both files, signs them with the author's key, and puts them into dist/
# with the plugin's folder as a ZIP and their SHA-256 sums:
#
#   ./build.sh                       unsigned, to try the build
#   ./build.sh --key <author.key>    signed, to publish
#
# Reproducibly: the same source, Cargo.lock and toolchains give the same
# files on every Linux computer, in whatever folder, so the plugin
# library can rebuild them from the source and show that they match.
# Paths are written as /plugin (this folder), /cargo (Cargo's home) and
# /rust (the X4 Pro's core and alloc), never as they are here.
#
# Needs: Linux (paths in the code use `/`), the X4's Rust from
# rust-toolchain.toml, espup's `esp` toolchain with its environment
# (`. ~/export-esp.sh`), paperpaw-sign of the PaperPaw release built for,
# and zip. Never set RUSTFLAGS: it would replace .cargo/config.toml's
# flags, which link the plugin.
set -euo pipefail
cd "$(dirname "$0")"

# The binary crate that makes the file (`[[bin]]` in Cargo.toml).
bin=hello

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

cargo_home=${CARGO_HOME:-$HOME/.cargo}
# The target's rustflags, added to those of .cargo/config.toml.
flags() {
    local target=$1 extra=${2:-}
    printf 'target.%s.rustflags = ["--remap-path-prefix=%s=/plugin", "--remap-path-prefix=%s=/cargo"%s]' \
        "$target" "$PWD" "$cargo_home" "$extra"
}

x4=riscv32imc-unknown-none-elf
x4pro=xtensa-esp32s3-none-elf
cargo build --release --locked --target "$x4" --config "$(flags "$x4")"
sysroot=$(rustc +esp --print sysroot)
cargo +esp build --release --locked --target "$x4pro" -Zbuild-std=core,alloc \
    --config "$(flags "$x4pro" ", \"--remap-path-prefix=$sysroot=/rust\"")"

rm -rf dist
mkdir dist
paperpaw-sign pack "target/$x4/release/$bin" "dist/$id-$version-x4.ppn" "${key[@]}"
paperpaw-sign pack "target/$x4pro/release/$bin" "dist/$id-$version-x4-pro.ppn" "${key[@]}"
# The plugin's folder, plugin.ini at the top, without hidden files.
(cd plugin && zip -q -X -r -D "../dist/$id-$version.zip" . -x '.*' -x '*/.*')
(cd dist && sha256sum -- * > SHA256SUMS)
for device in x4 x4-pro; do
    echo "$device: fingerprint $(paperpaw-sign plugin "dist/$id-$version-$device.ppn" | sed -n 's/^fingerprint: *//p')"
done
cat dist/SHA256SUMS
