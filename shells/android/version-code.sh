#!/usr/bin/env bash
# The Android version code of a PaperPaw version: Android installs a
# version over another only if its code is higher, and a beta comes
# before its release. 1.2.3 is 1020399, its betas 1020301 to 1020398 by
# their number; another pre-release (a build in work, `-dev`) 1020300.
#
#   shells/android/version-code.sh 0.5.0-beta.2    # 50002
set -euo pipefail
version="${1%%+*}"
[[ "$version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-(.*))?$ ]] \
    || { echo "version-code.sh: no version: $1" >&2; exit 1; }
major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
pre="${BASH_REMATCH[5]}"
if [[ -z "$pre" ]]; then
    last=99
elif [[ "$pre" =~ ^beta\.([0-9]+)$ ]] && (( 10#${BASH_REMATCH[1]} >= 1 && 10#${BASH_REMATCH[1]} <= 98 )); then
    last=$((10#${BASH_REMATCH[1]}))
elif [[ "$pre" =~ ^beta\. ]]; then
    echo "version-code.sh: a beta's number is 1 to 98: $1" >&2
    exit 1
else
    last=0
fi
echo $(( ((10#$major * 10000 + 10#$minor * 100 + 10#$patch) * 100) + last ))
