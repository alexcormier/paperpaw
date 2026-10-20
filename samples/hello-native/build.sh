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
# Paths end up in a build, and the rebuild works in the paths of GitHub's
# runners, so only files built by the CI job (.github/workflows/build.yml)
# match their source; builds made elsewhere are for trying.
#
# Needs: the X4's Rust from rust-toolchain.toml, espup's `esp` toolchain
# with its environment (`. ~/export-esp.sh`), paperpaw-sign of the
# PaperPaw release built for, and zip. Never set RUSTFLAGS: it would
# replace .cargo/config.toml's flags, which link the plugin.
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

cargo x4 --locked
cargo +esp x4pro --locked

rm -rf dist
mkdir dist
paperpaw-sign pack "target/riscv32imc-unknown-none-elf/release/$bin" "dist/$id-$version-x4.ppn" "${key[@]}"
paperpaw-sign pack "target/xtensa-esp32s3-none-elf/release/$bin" "dist/$id-$version-x4-pro.ppn" "${key[@]}"
# The plugin's folder, plugin.ini at the top, without hidden files: what
# readers install (for the catalogue's url), and with store/ for the
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
