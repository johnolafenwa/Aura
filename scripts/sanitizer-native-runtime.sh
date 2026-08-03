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

# This integration target executes the public native ABI directly, including
# arrays, queues, tasks, files, sockets, HTTP, and resource cleanup. Do not
# invoke aura from this sanitizer process: aura would spawn a nested Cargo
# runtime build that does not own this script's -Zbuild-std ABI contract.
