#!/usr/bin/env bash
# Installs the static LLVM libraries that the `llvm` cargo feature links.
#
# On Linux, downloads the official LLVM release archive, checks its SHA-256
# before extracting anything, and keeps only what llvm-sys needs:
# llvm-config, the LLVM and LLVM-C headers, and the static libLLVM and
# libPolly archives.
#
# On macOS, arm64 or x86-64, installs Homebrew's llvm@22 instead. The
# official macOS archive's static libraries hold LLVM bitcode, which Apple's
# linker cannot read, and Homebrew's bottle holds machine code. Homebrew
# checks the bottle's checksum itself. The script also points llvm-sys at
# Homebrew's static zstd, so `aura` loads only system libraries.
#
# Usage: scripts/install-llvm.sh [prefix]
#
# The default Linux prefix is ~/.cache/aura/llvm/<version>, and the prefix
# argument applies to Linux only. The script prints the settings to build
# with, and appends them to $GITHUB_ENV under GitHub Actions. It does
# nothing when the prefix already holds this version.
set -euo pipefail

version=22.1.2

# Fails before `brew install` when Homebrew has no llvm@22 bottle for this
# Mac. Building LLVM from source takes hours, longer than any CI job allows.
check_bottle() {
  brew info --json=v2 --formula llvm@22 | python3 -c '
import json, platform, sys
macos = ["big_sur", "monterey", "ventura", "sonoma", "sequoia", "tahoe"]
majors = {"11": "big_sur", "12": "monterey", "13": "ventura", "14": "sonoma", "15": "sequoia", "26": "tahoe"}
try:
    files = json.load(sys.stdin)["formulae"][0]["bottle"]["stable"]["files"]
    host = majors[platform.mac_ver()[0].split(".")[0]]
except (KeyError, IndexError, TypeError, ValueError):
    sys.exit(0)  # Unknown shapes leave the decision to brew.
prefix = "arm64_" if platform.machine() == "arm64" else ""
usable = ["all"] + [prefix + name for name in macos[: macos.index(host) + 1]]
if not any(tag in files for tag in usable):
    print(f"install-llvm.sh: Homebrew has no llvm@22 bottle for {prefix}{host}; "
          f"it has {sorted(files)}", file=sys.stderr)
    sys.exit(1)
'
}

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    archive="LLVM-${version}-Linux-X64.tar.xz"
    sha256=ff32497b6801267ee427bc69cdaeecfb2d19578af8c2a942e864c45215f9a2ac
    ;;
  Darwin-arm64 | Darwin-x86_64)
    check_bottle
    brew list llvm@22 >/dev/null 2>&1 || brew install llvm@22
    prefix=$(brew --prefix llvm@22)
    installed=$("$prefix/bin/llvm-config" --version)
    if [ "${installed%%.*}" != "${version%%.*}" ]; then
      echo "install-llvm.sh: Homebrew's llvm@22 is LLVM ${installed}" >&2
      exit 1
    fi
    echo "Using Homebrew's LLVM ${installed} in ${prefix}"
    # LLVM names zstd, and Homebrew's libzstd is a dylib under
    # /opt/homebrew or /usr/local, which an installed aura cannot load on a
    # Mac without Homebrew. llvm-sys searches LLVM_SYS_SYSTEM_LIBRARY_DIRS
    # before Homebrew's lib directory, so a directory that holds only the
    # static libzstd makes the linker take that one.
    zstd_static="$(brew --prefix zstd)/lib/libzstd.a"
    if [ ! -f "$zstd_static" ]; then
      echo "install-llvm.sh: Homebrew's zstd has no static library at ${zstd_static}" >&2
      exit 1
    fi
    static_dir="${XDG_CACHE_HOME:-$HOME/.cache}/aura/llvm/static-system-libs"
    mkdir -p "$static_dir"
    ln -sf "$zstd_static" "$static_dir/libzstd.a"
    echo "export LLVM_SYS_221_PREFIX=${prefix}"
    echo "export LLVM_SYS_SYSTEM_LIBRARY_DIRS=${static_dir}"
    if [ -n "${GITHUB_ENV:-}" ]; then
      echo "LLVM_SYS_221_PREFIX=${prefix}" >> "$GITHUB_ENV"
      echo "LLVM_SYS_SYSTEM_LIBRARY_DIRS=${static_dir}" >> "$GITHUB_ENV"
    fi
    exit 0
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
