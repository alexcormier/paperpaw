#!/usr/bin/env bash
# Builds PaperPaw's own plugins that the store offers (plugins.txt) as
# native plugin files for one reader, as the release pipeline does, packs
# each (signed with --key), and checks that it fits the firmware:
#
#   tools/plugin-files/build.sh <device> <out-dir> [--key <file>] [--fingerprint <hex>]
#
# <device> is x4, x4-pro or x4-classic; the files are
# <out-dir>/<id>-<version>-<device>.ppn, <version> the workspace's. With
# --fingerprint (the firmware's, `paperpaw-sign fingerprint <elf>`) each
# file must have it. Run from anywhere; paperpaw-sign is built from this
# repository.
#
# Needs: the X4's Rust from rust-toolchain.toml, for the X4 Pro and the
# X4 Classic espup's `esp` toolchain with its environment. Never set
# RUSTFLAGS: it would replace .cargo/config.toml's flags, which link the
# plugins.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

usage() { echo "usage: $0 <x4|x4-pro|x4-classic> <out-dir> [--key <file>] [--fingerprint <hex>]" >&2; exit 2; }
[[ $# -ge 2 ]] || usage
device=$1
out=$2
shift 2
key=()
fingerprint=
while [[ $# -gt 0 ]]; do
    case "$1" in
        --key) [[ $# -ge 2 ]] || usage; key=(--key "$2"); shift 2 ;;
        --fingerprint) [[ $# -ge 2 ]] || usage; fingerprint=$2; shift 2 ;;
        *) usage ;;
    esac
done

case "$device" in
    x4) build=(cargo x4 --locked); target=riscv32imc-unknown-none-elf; chip=5 ;;
    x4-pro | x4-classic) build=(cargo +esp x4pro --locked); target=xtensa-esp32s3-none-elf; chip=9 ;;
    *) usage ;;
esac

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
[[ -n "$version" ]] || { echo "no version in $root/Cargo.toml" >&2; exit 1; }

(cd "$here" && env -u RUSTFLAGS "${build[@]}")
target_dir="${CARGO_TARGET_DIR:-$here/target}"
sign() { (cd "$root" && cargo run -q --release -p paperpaw-sign -- "$@"); }

mkdir -p "$out"
out="$(cd "$out" && pwd)"
for id in $(sed -e 's/#.*//' "$here/plugins.txt" | awk 'NF { print $1 }'); do
    file="$out/$id-$version-$device.ppn"
    sign pack "$target_dir/$target/release/$id" "$file" "${key[@]}"
    shown="$(sign plugin "$file")"
    if ! grep -Eq "^chip: .*\($chip\)\$" <<< "$shown"; then
        echo "$file: not for chip $chip" >&2
        echo "$shown" >&2
        exit 1
    fi
    if [[ -n "$fingerprint" ]] && ! grep -Eq "^fingerprint: +$fingerprint\$" <<< "$shown"; then
        echo "$file does not fit the firmware (fingerprint $fingerprint)" >&2
        echo "$shown" >&2
        exit 1
    fi
    echo "$id: $(wc -c < "$file") bytes"
done
