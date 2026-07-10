#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/smoke-cli-archive.sh <archive.tar.gz>" >&2
  exit 2
fi

archive="$1"
install_root="$(mktemp -d)"
output_root="$(mktemp -d)"
trap 'rm -rf "$install_root" "$output_root"' EXIT

top_level="$(tar -tzf "$archive" | head -n 1 | cut -d/ -f1)"
tar -xzf "$archive" -C "$install_root"
packaged="$install_root/$top_level/bin/aura"

env CARGO="$output_root/missing-cargo" "$packaged" --version
env CARGO="$output_root/missing-cargo" "$packaged" check examples/basic_addition.au
env CARGO="$output_root/missing-cargo" "$packaged" run examples/basic_addition.au
env CARGO="$output_root/missing-cargo" "$packaged" build --backend direct -o "$output_root/basic-addition" examples/basic_addition.au
test "$("$output_root/basic-addition")" = "16"
