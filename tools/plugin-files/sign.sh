#!/usr/bin/env bash
# Signs the plugin files build.sh made, apart from building them, so the
# key is on the disk only while it signs (never while cargo builds):
#
#   tools/plugin-files/sign.sh <dir> <paperpaw-sign> --key <file>
#
# <paperpaw-sign> is the tool, built before (`cargo build --release -p
# paperpaw-sign`: target/release/paperpaw-sign). Every <dir>/*.ppn gets
# the signature of the key in <file> and is checked; it fails at the
# first file whose signature does not hold.
set -euo pipefail

[[ $# -eq 4 && "$3" == --key ]] || { echo "usage: $0 <dir> <paperpaw-sign> --key <file>" >&2; exit 2; }
dir=$1
sign=$2
key=$4

shopt -s nullglob
files=("$dir"/*.ppn)
[[ ${#files[@]} -gt 0 ]] || { echo "$dir: no plugin files" >&2; exit 1; }
for file in "${files[@]}"; do
    "$sign" sign-plugin "$file" --key "$key" > /dev/null
    "$sign" check-plugin "$file" > /dev/null
done
echo "${#files[@]} files signed"
