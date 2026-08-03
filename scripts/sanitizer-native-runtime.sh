#!/usr/bin/env bash
set -euo pipefail

target="${1:-x86_64-unknown-linux-gnu}"
toolchain="${AURA_NIGHTLY_TOOLCHAIN:-nightly-2026-07-01}"
if [[ "$target" == *-apple-darwin ]]; then
  asan_options="detect_leaks=0:halt_on_error=1"
else
  asan_options="detect_leaks=1:halt_on_error=1"
fi
target_rustflags="CARGO_TARGET_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')_RUSTFLAGS"
existing_target_rustflags="$(printenv "$target_rustflags" || true)"
export "$target_rustflags=${existing_target_rustflags:+$existing_target_rustflags }--cfg coverage -Zsanitizer=address"
unset RUSTFLAGS
unset RUSTDOCFLAGS
ASAN_OPTIONS="$asan_options" cargo "+$toolchain" test \
  -Zbuild-std \
  --target "$target" \
  -p aura-compiler \
  --test native_runtime_ffi \
  -- --test-threads=1

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
generated_dir="$(mktemp -d)"
trap 'rm -rf "$generated_dir"' EXIT
cargo "+$toolchain" build \
  -Zbuild-std \
  --target "$target" \
  -p aura
aura_bin="$repo_root/target/$target/debug/aura"
export CC="$repo_root/scripts/cc-asan.sh"
export RUSTUP_TOOLCHAIN="$toolchain"
unset RUSTFLAGS
unset RUSTDOCFLAGS

"$aura_bin" build --backend direct -o "$generated_dir/control-plane" \
  crates/aura-compiler/tests/fixtures/run-pass/control_plane_foundations.au
ASAN_OPTIONS="$asan_options" "$generated_dir/control-plane"

"$aura_bin" build --backend direct -o "$generated_dir/queue" \
  examples/concurrency/bounded_queue.au
ASAN_OPTIONS="$asan_options" "$generated_dir/queue"
