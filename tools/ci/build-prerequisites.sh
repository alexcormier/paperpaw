#!/usr/bin/env bash
# GitHub's Ubuntu image already supplies these commands. Install only what
# is absent, without pulling duplicate CMake/Ninja packages over apt mirrors.
set -euo pipefail
: "${MBEDTLS_CLANG_PACKAGE:?the release must name its mbedTLS clang}"
: "${GITHUB_ENV:?the action must provide its environment file}"

missing=()
for pair in "cmake:cmake" "ninja:ninja-build" "$MBEDTLS_CLANG_PACKAGE:$MBEDTLS_CLANG_PACKAGE"; do
  tool=${pair%%:*}
  command -v "$tool" >/dev/null || missing+=("${pair#*:}")
done
if (( ${#missing[@]} )); then
  sudo apt-get update
  sudo apt-get install -y "${missing[@]}"
fi

cmake --version
ninja --version
"$MBEDTLS_CLANG_PACKAGE" --version
{
  echo "CMAKE_GENERATOR=Ninja"
  echo "CLANG_PATH=$(command -v "$MBEDTLS_CLANG_PACKAGE")"
} >> "$GITHUB_ENV"
