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
# A published plugin is taken from crates.io only when its packaged
# sources still match. Otherwise its own version must be raised first.
py="$(command -v python3 || command -v python)"
published="$("$py" tools/published_crates.py)"
skip=()
while IFS= read -r crate; do
    [[ -z "$crate" ]] || skip+=(--exclude "$crate")
done <<< "$published"
(cd "$aside" && cargo publish --dry-run --workspace --allow-dirty "${skip[@]}")
