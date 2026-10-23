#!/usr/bin/env bash
# The Android version code of a PaperPaw version: Android installs a
# version over another only if its code is higher, so the codes are in
# the order the update plugin puts versions in (SemVer's, as
# `paperpaw_sdk::firmware::compare_versions` has it). Of 1.2.3:
#
#   1.2.3-beta.1 to -beta.49   1020301 to 1020349
#   1.2.3-dev                  1020350 (a build in work, after the betas)
#   1.2.3-rc.1 to -rc.48       1020351 to 1020398
#   1.2.3                      1020399
#
# Each of major, minor and patch is 0 to 99, so that no two versions
# share a code; any other version is refused.
#
#   shells/android/version-code.sh 0.5.0-beta.2    # 50002
set -euo pipefail
fail() {
    echo "version-code.sh: $1: $2" >&2
    exit 1
}
version="${1%%+*}"
[[ "$version" =~ ^(0|[1-9][0-9]?)\.(0|[1-9][0-9]?)\.(0|[1-9][0-9]?)(-(.+))?$ ]] \
    || fail "$1" "no version of numbers from 0 to 99"
major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
pre="${BASH_REMATCH[5]}"
if [[ -z "$pre" ]]; then
    last=99
elif [[ "$pre" == dev ]]; then
    last=50
elif [[ "$pre" =~ ^beta\.([1-9][0-9]?)$ ]] && (( BASH_REMATCH[1] <= 49 )); then
    last=$((BASH_REMATCH[1]))
elif [[ "$pre" =~ ^rc\.([1-9][0-9]?)$ ]] && (( BASH_REMATCH[1] <= 48 )); then
    last=$((50 + BASH_REMATCH[1]))
else
    fail "$1" "a pre-release is beta.1 to beta.49, dev or rc.1 to rc.48"
fi
echo $(( ((major * 100 + minor) * 100 + patch) * 100 + last ))
