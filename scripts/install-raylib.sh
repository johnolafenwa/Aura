#!/usr/bin/env bash
# Installs raylib for the tests of `examples/packages/raylib`, which skip
# on a host that cannot link it.
#
# On Linux, builds the pinned raylib release from a verified source archive as
# a shared library under a user-owned prefix (defaulting to
# ~/.cache/aura/raylib/<version>), because Ubuntu 24.04 has no raylib package.
# It exports LIBRARY_PATH and LD_LIBRARY_PATH for later steps, installs Xvfb,
# and under GitHub Actions starts Xvfb and sets DISPLAY so window tests run on
# a host with no screen.
#
# On macOS, installs Homebrew's raylib. The programs that open a window do
# not run there unless AURA_RAYLIB_WINDOW_TESTS is set.
#
# On both, also installs SDL2 and GLFW from the system's packages, for the
# FFI acceptance tests that call them.
#
# Usage: scripts/install-raylib.sh [prefix]
#
# The script does nothing when the C compiler already links raylib.
set -euo pipefail

version=6.0
commit=dbc56a87da87d973a9c5baa4e7438a9d20121d28
sha256=81b06ce7c19cf3b634b0271c23c361ba6ad8bf45fb8b036abbfeb4260ec1e126
prefix="${1:-${XDG_CACHE_HOME:-$HOME/.cache}/aura/raylib/${version}}"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

links_raylib() {
  local scratch status=0
  scratch="$(mktemp -d)"
  printf 'int GetRandomValue(int, int);\nint main(void) { return GetRandomValue(0, 0); }\n' \
    > "$scratch/probe.c"
  cc "$scratch/probe.c" -lraylib -o "$scratch/probe" > /dev/null 2>&1 || status=$?
  rm -rf "$scratch"
  return $status
}

case "$(uname -s)" in
  Darwin)
    links_raylib || HOMEBREW_NO_AUTO_UPDATE=1 brew install raylib </dev/null
    # The SDL and GLFW tests skip on a host that cannot link them.
    brew list sdl2 > /dev/null 2>&1 || HOMEBREW_NO_AUTO_UPDATE=1 brew install sdl2 </dev/null
    brew list glfw > /dev/null 2>&1 || HOMEBREW_NO_AUTO_UPDATE=1 brew install glfw </dev/null
    ;;
  Linux)
    sudo apt-get update
    sudo apt-get install -y cmake curl xvfb libgl1-mesa-dev libgl1-mesa-dri \
      libx11-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev \
      libwayland-dev libxkbcommon-dev libasound2-dev libsdl2-dev libglfw3-dev
    if ! links_raylib; then
      if ! { [ -d "$prefix/lib" ] && LIBRARY_PATH="$prefix/lib${LIBRARY_PATH:+:$LIBRARY_PATH}" links_raylib; }; then
        work="$(mktemp -d)"
        trap 'rm -rf "$work"' EXIT HUP INT TERM
        archive="raylib-${commit}.tar.gz"
        url="https://github.com/raysan5/raylib/archive/${commit}.tar.gz"
        curl --fail --location --retry 3 --silent --show-error \
          --proto '=https' --tlsv1.2 --output "$work/$archive" -- "$url"
        actual="$(sha256_of "$work/$archive")"
        if [ "$actual" != "$sha256" ]; then
          echo "install-raylib.sh: ${archive} has SHA-256 ${actual}, expected ${sha256}" >&2
          exit 1
        fi
        mkdir -p "$work/src"
        tar -xzf "$work/$archive" -C "$work/src"
        cmake -S "$work/src/raylib-${commit}" -B "$work/build" \
          -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
          -DBUILD_SHARED_LIBS=ON -DBUILD_EXAMPLES=OFF -DCMAKE_BUILD_TYPE=Release
        cmake --build "$work/build" --parallel
        cmake --install "$work/build"
        rm -rf "$work"
        trap - EXIT HUP INT TERM
      fi
      lib_dir="$prefix/lib"
      new_library_path="${lib_dir}${LIBRARY_PATH:+:${LIBRARY_PATH}}"
      new_ld_library_path="${lib_dir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
      new_pkg_config_path="${lib_dir}/pkgconfig${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
      echo "export LIBRARY_PATH=${new_library_path}"
      echo "export LD_LIBRARY_PATH=${new_ld_library_path}"
      echo "export PKG_CONFIG_PATH=${new_pkg_config_path}"
      if [ -n "${GITHUB_ENV:-}" ]; then
        echo "LIBRARY_PATH=${new_library_path}" >> "$GITHUB_ENV"
        echo "LD_LIBRARY_PATH=${new_ld_library_path}" >> "$GITHUB_ENV"
        echo "PKG_CONFIG_PATH=${new_pkg_config_path}" >> "$GITHUB_ENV"
      fi
    fi
    if [ -n "${GITHUB_ENV:-}" ] && [ -z "${DISPLAY:-}" ]; then
      nohup Xvfb :99 -screen 0 1280x720x24 > /dev/null 2>&1 &
      echo "DISPLAY=:99" >> "$GITHUB_ENV"
    fi
    ;;
  *)
    echo "install-raylib.sh: no raylib install for $(uname -s)" >&2
    exit 1
    ;;
esac
