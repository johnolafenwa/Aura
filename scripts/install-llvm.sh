#!/usr/bin/env bash
# Installs the static LLVM libraries that the `llvm` cargo feature links.
#
# Downloads the official LLVM release archive for this host, checks its
# SHA-256 before extracting anything, and keeps only what llvm-sys needs:
# llvm-config, the LLVM and LLVM-C headers, and the static libLLVM and
# libPolly archives.
#
# Usage: scripts/install-llvm.sh [prefix]
#
# The default prefix is ~/.cache/aura/llvm/<version>. The script prints the
# LLVM_SYS_221_PREFIX setting to build with, and appends it to $GITHUB_ENV
# under GitHub Actions. It does nothing when the prefix already holds this
# version.
set -euo pipefail

version=22.1.2

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    archive="LLVM-${version}-Linux-X64.tar.xz"
    sha256=ff32497b6801267ee427bc69cdaeecfb2d19578af8c2a942e864c45215f9a2ac
    ;;
  Darwin-arm64)
    archive="LLVM-${version}-macOS-ARM64.tar.xz"
    sha256=7b290ec4529c309b6b952d64a3513595c816dff1c217d54466a586d6e35dcec9
    ;;
  *)
    echo "install-llvm.sh: LLVM ${version} has no release archive for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

prefix=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/aura/llvm/${version}}
url="https://github.com/llvm/llvm-project/releases/download/llvmorg-${version}/${archive}"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

if [ -x "$prefix/bin/llvm-config" ] && [ "$("$prefix/bin/llvm-config" --version)" = "$version" ]; then
  echo "LLVM ${version} is already installed in ${prefix}"
else
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  echo "Downloading ${url}"
  curl --fail --location --retry 3 --silent --show-error --output "$work/$archive" "$url"
  actual=$(sha256_of "$work/$archive")
  if [ "$actual" != "$sha256" ]; then
    echo "install-llvm.sh: ${archive} has SHA-256 ${actual}, expected ${sha256}" >&2
    exit 1
  fi
  top=${archive%.tar.xz}
  # GNU tar needs --wildcards for the library patterns; bsdtar matches them
  # by default and rejects the option.
  wildcards=()
  if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    wildcards=(--wildcards)
  fi
  mkdir "$work/extract"
  tar -xJf "$work/$archive" -C "$work/extract" ${wildcards[@]+"${wildcards[@]}"} \
    "$top/bin/llvm-config" \
    "$top/include/llvm" \
    "$top/include/llvm-c" \
    "$top/lib/libLLVM*.a" \
    "$top/lib/libPolly*.a"
  rm "$work/$archive"
  rm -rf "$prefix"
  mkdir -p "$(dirname "$prefix")"
  mv "$work/extract/$top" "$prefix"
  echo "Installed LLVM ${version} in ${prefix}"
fi

echo "export LLVM_SYS_221_PREFIX=${prefix}"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "LLVM_SYS_221_PREFIX=${prefix}" >> "$GITHUB_ENV"
fi
