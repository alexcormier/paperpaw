#!/usr/bin/env bash
# Checks that a code plugin's file is the same wherever it is built, as
# the plugin library's rebuild needs: the sample plugin (samples/
# hello-plugin) as its author's repository has it, with the SDK from a
# registry, built for one reader in two folders as samples/hello-native's
# build.sh builds, packed, and compared byte for byte:
#
#   tools/plugin-files/anywhere.sh <x4|x4-pro> [<work-dir>]
#
# The registry is a folder of the crates `cargo package` makes of the SDK
# and the crates below it, as crates.io would have them: Cargo hashes a
# path dependency outside the plugin's workspace by its whole path, so a
# plugin built against the SDK's source in a checkout differs from folder
# to folder. For the X4 Pro, the core libraries of tools/xtensa-std are
# installed (install.sh) first.
#
# Needs: Linux or macOS, the X4's Rust of samples/hello-native's
# rust-toolchain.toml, for the X4 Pro espup's `esp` toolchain with its
# environment. Never set RUSTFLAGS.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

usage() { echo "usage: $0 <x4|x4-pro> [<work-dir>]" >&2; exit 2; }
[[ $# -ge 1 && $# -le 2 ]] || usage
case "$1" in
    x4)
        toolchain="+$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/samples/hello-native/rust-toolchain.toml")"
        alias=x4
        target=riscv32imc-unknown-none-elf ;;
    x4-pro)
        toolchain=+esp
        alias=x4pro
        target=xtensa-esp32s3-none-elf ;;
    *) usage ;;
esac
work="${2:-$(mktemp -d)}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"

# The SDK and the crates below it, packed as published, and the crates of
# crates.io they need, from Cargo's cache.
tree="$(cd "$root" && cargo tree -p paperpaw-sdk -e normal,build --target all --prefix none --format '{p}' | sort -u)"
ours=($(sed -n 's/^\([^ ]*\) v[^ ]* (\/.*$/\1/p' <<< "$tree" | sort -u))
theirs=($(sed -n 's/^\([^ ]*\) v\([^ ]*\)$/\1-\2/p; s/^\([^ ]*\) v\([^ ]*\) (\*)$/\1-\2/p' <<< "$tree" | sort -u))
(cd "$root" && cargo fetch --locked -q && cargo package -q --no-verify --allow-dirty \
    --target-dir "$work/package" $(printf -- '-p %s ' "${ours[@]}"))
crates=("$work"/package/package/*.crate)
for name in "${theirs[@]}"; do
    found=("$cargo_home"/registry/cache/index.crates.io-*/"$name.crate")
    [[ -f "${found[0]}" ]] || { echo "$name is not in Cargo's cache" >&2; exit 1; }
    crates+=("${found[0]}")
done

# The plugin's repository: its crate, Cargo.lock, and the sample's
# configuration, the registry replaced by the folder.
plugin="$work/template"
rm -rf "$plugin"
mkdir -p "$plugin/src" "$plugin/.cargo" "$plugin/vendor"
for crate in "${crates[@]}"; do
    tar -xzf "$crate" -C "$plugin/vendor"
    name="$(basename "$crate" .crate)"
    printf '{"files":{},"package":"%s"}' "$(sha256sum "$crate" | cut -d ' ' -f 1)" \
        > "$plugin/vendor/$name/.cargo-checksum.json"
done
sed -n '/^\[package\]/,/^\[dependencies\]/p' "$root/samples/hello-native/Cargo.toml" \
    | sed 's/^name = .*/name = "hello"/; /^\[dependencies\]/d' > "$plugin/Cargo.toml"
cat >> "$plugin/Cargo.toml" <<EOF
[dependencies]
paperpaw-sdk = "=$version"

$(sed -n '/^\[profile.release\]/,$p' "$root/samples/hello-native/Cargo.toml")
EOF
grep -v '^#!\[' "$root/samples/hello-plugin/src/lib.rs" > "$plugin/src/hello.rs"
# What it includes: its texts.
cp -r "$root/samples/hello-plugin/how" "$root/samples/hello-plugin/i18n" "$plugin/"
printf '#![no_std]\n#![no_main]\n\nextern crate alloc;\n\nmod hello;\n\npaperpaw_sdk::plugin!(hello::Hello);\n' \
    > "$plugin/src/main.rs"
cp "$root/samples/hello-native/.cargo/config.toml" "$plugin/.cargo/config.toml"
# The format's linker script, which the configuration links with.
cp "$root/samples/hello-native/paperpaw-plugin.x" "$plugin/"
printf '\n[source.crates-io]\nreplace-with = "vendored"\n\n[source.vendored]\ndirectory = "vendor"\n' \
    >> "$plugin/.cargo/config.toml"
(cd "$plugin" && cargo "$toolchain" generate-lockfile --offline -q)

# Built in two folders, each written as /plugin, packed, compared.
sign() { (cd "$root" && cargo run -q --release -p paperpaw-sign -- "$@"); }
files=()
for dir in "$work/a/plugin" "$work/b/elsewhere/plugin"; do
    rm -rf "$dir"
    mkdir -p "$(dirname "$dir")"
    cp -r "$plugin" "$dir"
    (cd "$dir" && env -u RUSTFLAGS -u CARGO_TARGET_DIR cargo "$toolchain" "$alias" --locked --offline -q \
        --config "target.$target.rustflags = ['--remap-path-prefix=$dir=/plugin', '--remap-path-prefix=$cargo_home=/cargo']")
    sign pack "$dir/target/$target/release/hello" "$dir/hello.ppn" > /dev/null
    files+=("$dir/hello.ppn")
done
sha256sum "${files[@]}"
cmp "${files[@]}"
echo "$1: the same in both folders"
