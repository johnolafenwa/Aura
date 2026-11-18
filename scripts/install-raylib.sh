#!/usr/bin/env bash
# Installs raylib for the tests of `examples/packages/raylib`, which skip
# on a host that cannot link it.
#
# On Linux, builds the pinned raylib release from source as a shared
# library under /usr/local, because Ubuntu 24.04 has no raylib package. It
# also installs Xvfb, and under GitHub Actions starts it and sets DISPLAY
# for the later steps, so the programs that open a window run on a host
# with no screen.
#
# On macOS, installs Homebrew's raylib. The programs that open a window do
# not run there unless AURA_RAYLIB_WINDOW_TESTS is set.
#
# Usage: scripts/install-raylib.sh
#
# The script does nothing when the C compiler already links raylib.
set -euo pipefail

version=6.0

links_raylib() {
  local scratch
  scratch="$(mktemp -d)"
  printf 'int GetRandomValue(int, int);\nint main(void) { return GetRandomValue(0, 0); }\n' \
    > "$scratch/probe.c"
  cc "$scratch/probe.c" -lraylib -o "$scratch/probe" > /dev/null 2>&1
  local status=$?
  rm -rf "$scratch"
  return $status
}

case "$(uname -s)" in
  Darwin)
    links_raylib || brew install raylib
    ;;
  Linux)
    sudo apt-get update
    sudo apt-get install -y cmake git xvfb libgl1-mesa-dev libgl1-mesa-dri \
      libx11-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev \
      libwayland-dev libxkbcommon-dev libasound2-dev
    if ! links_raylib; then
      source_dir="$(mktemp -d)"
      git clone --depth 1 --branch "$version" https://github.com/raysan5/raylib.git "$source_dir"
      cmake -S "$source_dir" -B "$source_dir/build" \
        -DBUILD_SHARED_LIBS=ON -DBUILD_EXAMPLES=OFF -DCMAKE_BUILD_TYPE=Release
      cmake --build "$source_dir/build" --parallel
      sudo cmake --install "$source_dir/build"
      sudo ldconfig
      rm -rf "$source_dir"
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
