#!/usr/bin/env bash
# Unpacks a Linux CLI archive and target packs into one prefix outside the
# source tree, builds the packaged examples for each pack's target with
# no Cargo, and runs the programs:
#
#   scripts/smoke-target-packs.sh <cli-archive.tar.gz> <target-pack.tar.gz>...
#
# A program for the other architecture runs under `qemu-<arch>-static`
# (Ubuntu: qemu-user-static), and its link needs `<arch>-linux-gnu-gcc`.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: scripts/smoke-target-packs.sh <cli-archive.tar.gz> <target-pack.tar.gz>..." >&2
  exit 2
fi
for file in "$@"; do
  if [[ ! -f "$file" ]]; then
    echo "file does not exist: $file" >&2
    exit 2
  fi
done

work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-target-pack-smoke.XXXXXX")"
trap 'rm -rf "$work_root"' EXIT HUP INT TERM
prefix="$work_root/prefix"
mkdir -p "$prefix"
tar -xzf "$1" -C "$prefix" --strip-components=1
shift
aura="$prefix/bin/aura"
host_arch="$(uname -m)"
export AURA_CACHE_DIR="$work_root/cache"
export CARGO="$work_root/cargo-must-not-exist"

expected_retry=$'recover request 1\nrecover retry 4ms\nrecover request 2\nrecover result 200\nrate request 1\nrate retry 6ms\nrate request 2\nrate result 429\nexhaust request 1\nexhaust retry 3ms\nexhaust request 2\nexhaust retry 5ms\nexhaust request 3\nexhaust result 503\nrequests 7'

for pack in "$@"; do
  name="$(basename "$pack" .tar.gz)"
  if [[ ! "$name" =~ ^aura-v[0-9A-Za-z.-]+-target-((x86_64|aarch64)-unknown-linux-(gnu|musl))$ ]]; then
    echo "not a target pack: $pack" >&2
    exit 2
  fi
  target="${BASH_REMATCH[1]}"
  arch="${BASH_REMATCH[2]}"
  libc="${BASH_REMATCH[3]}"
  if [[ "$arch" == "$host_arch" && "$libc" == gnu ]]; then
    echo "$target is this machine's own target: the CLI archive covers it" >&2
    exit 2
  fi
  tar -xzf "$pack" -C "$prefix" --strip-components=1
  if [[ ! -f "$prefix/lib/aura/targets/$target/libaura_runtime.a" ]]; then
    echo "$pack did not unpack a runtime library for $target" >&2
    exit 1
  fi

  runner=()
  if [[ "$arch" != "$host_arch" ]]; then
    runner=("qemu-$arch-static")
    if [[ "$libc" == gnu ]]; then
      runner+=(-L "/usr/$arch-linux-gnu")
    fi
  fi
  case "$arch" in
    x86_64) machine=62 ;;
    aarch64) machine=183 ;;
  esac

  for example in basic_addition agents/retrying_network_worker; do
    program="$work_root/$target-$(basename "$example")"
    (cd "$work_root" && "$aura" build --target "$target" -o "$program" "$prefix/examples/$example.au")
    python3 - "$program" "$machine" "$libc" <<'PY'
import struct
import sys

path, machine, libc = sys.argv[1], int(sys.argv[2]), sys.argv[3]
data = open(path, "rb").read()
assert data[:4] == b"\x7fELF", "not an ELF file"
assert struct.unpack_from("<H", data, 18)[0] == machine, "wrong machine"
phoff = struct.unpack_from("<Q", data, 32)[0]
phentsize, phnum = struct.unpack_from("<HH", data, 54)
interpreted = any(
    struct.unpack_from("<I", data, phoff + index * phentsize)[0] == 3
    for index in range(phnum)
)
assert interpreted == (libc == "gnu"), "a musl program names no dynamic linker"
PY
    output="$("${runner[@]}" "$program")"
    case "$example" in
      basic_addition) expected=16 ;;
      *) expected="$expected_retry" ;;
    esac
    if [[ "$output" != "$expected" ]]; then
      echo "$example built for $target printed unexpected output: $output" >&2
      exit 1
    fi
  done

  # A pack of another build is refused.
  version_file="$prefix/lib/aura/targets/$target/aura-version"
  cp "$version_file" "$work_root/aura-version"
  echo "aura 0.0.1-preview (000000000000)" > "$version_file"
  if refusal="$(cd "$work_root" && "$aura" build --target "$target" -o "$work_root/refused" "$prefix/examples/basic_addition.au" 2>&1)" ||
    [[ "$refusal" != *"the installed target pack for \`$target\` is from"* ]]; then
    echo "a target pack of another build was not refused: $refusal" >&2
    exit 1
  fi
  cp "$work_root/aura-version" "$version_file"
  echo "target pack smoke test passed for $target"
done
