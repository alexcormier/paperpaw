#!/usr/bin/env bash
# Builds core, alloc and compiler_builtins for the ESP32-S3
# (xtensa-esp32s3-none-elf) with espup's `esp` toolchain and packs them
# as the toolchain keeps a target's libraries, for install.sh:
#
#   tools/xtensa-std/build.sh <out-dir>
#
# makes <out-dir>/paperpaw-xtensa-std-<version>.tar.gz, <version> the
# workspace's. The release builds it once, on Linux, and publishes it;
# code plugins for the X4 Pro and the X4 Classic then build against it
# as plugins for the X4 build against rustup's libraries, without
# `-Zbuild-std`. Built so, every plugin's code is the same wherever it is
# built (see README.md).
#
# Needs: espup's `esp` toolchain of shells/xteink/versions.env with its
# environment, and GNU tar (for an archive the same on every run). Never
# set RUSTFLAGS: the flags are given here.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target=xtensa-esp32s3-none-elf

[[ $# -eq 1 ]] || { echo "usage: $0 <out-dir>" >&2; exit 2; }
mkdir -p "$1"
out="$(cd "$1" && pwd)"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
[[ -n "$version" ]] || { echo "no version in $root/Cargo.toml" >&2; exit 1; }

# The libraries' sources lie in the toolchain: their paths, which end up
# in panic messages, are written as /rust wherever it is installed. Their
# bitcode goes in beside their code, for the plugins' link time
# optimisation.
sysroot="$(rustc +esp --print sysroot)"
command -v cygpath >/dev/null && sysroot="$(cygpath -m "$sysroot")"
flags="'-C', 'embed-bitcode=yes', '--remap-path-prefix=$sysroot=/rust'"

# A target folder of its own, so that nothing built before is packed.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
(cd "$here" && env -u RUSTFLAGS cargo +esp build --release --locked --target "$target" \
    -Zbuild-std=core,alloc --target-dir "$work/target" \
    --config "target.$target.rustflags = [$flags]")

lib="$work/stage/lib/rustlib/$target/lib"
mkdir -p "$lib"
for crate in core alloc compiler_builtins; do
    found=("$work/target/$target/release/deps/lib$crate"-*.rlib)
    [[ ${#found[@]} -eq 1 && -f "${found[0]}" ]] || { echo "not one lib$crate: ${found[*]}" >&2; exit 1; }
    cp "${found[0]}" "$lib/"
done
# The compiler they were built by: install.sh refuses another.
rustc +esp -V > "$work/stage/lib/rustlib/$target/paperpaw-rustc.txt"
# The Rust project's licence for the libraries, PaperPaw's and its notices.
cp "$here/NOTICE-rust.txt" "$work/stage/lib/rustlib/$target/"
cp "$here/../../LICENSE" "$here/../../NOTICE" "$work/stage/lib/rustlib/$target/"

file="$out/paperpaw-xtensa-std-$version.tar.gz"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu \
    -C "$work/stage" -cf - lib | gzip -9n > "$file"
echo "$file"
