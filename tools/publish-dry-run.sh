#!/usr/bin/env bash
# Whether the workspace's crates publish to crates.io as <version> (e.g.
# a beta, 0.5.0-beta.1; the plugins as their own): in a worktree of HEAD aside, the version set
# there (never in this checkout: the tag sets it), `cargo publish
# --dry-run` across the workspace, each crate packaged and built from
# its package as crates.io would have it, in the order they depend on
# each other.
#
#   tools/publish-dry-run.sh 0.5.0-beta.1
set -euo pipefail
[[ $# -eq 1 ]] || { echo "usage: tools/publish-dry-run.sh <version>" >&2; exit 2; }
version=$1
cd "$(git rev-parse --show-toplevel)"
current="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
aside="$(mktemp -d)/paperpaw"
git worktree add -q --detach "$aside" HEAD
trap 'git worktree remove --force "$aside"' EXIT
# The workspace's version and every path dependency's on our crates.
sed -i "s/\"$current\"/\"$version\"/g" "$aside/Cargo.toml"
# Plugins keep their own versions (plugins/*): one whose version is on
# crates.io already did not change, and is taken from there.
skip=()
for manifest in plugins/*/Cargo.toml; do
    crate="$(sed -n '/^\[package\]/,/^\[/s/^name = "\(.*\)"/\1/p' "$manifest")"
    own="$(sed -n '/^\[package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$manifest")"
    if curl -sf "https://index.crates.io/${crate:0:2}/${crate:2:2}/$crate" | grep -q "\"vers\":\"$own\""; then
        skip+=(--exclude "$crate")
    fi
done
(cd "$aside" && cargo publish --dry-run --workspace --allow-dirty "${skip[@]}")
