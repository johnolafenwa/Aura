#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

IGNORE_REGEX='crates/aurora-compiler/src/.*_tests\.rs$|crates/aura/.*'

cargo llvm-cov clean --workspace
RUST_MIN_STACK=33554432 cargo llvm-cov \
  --workspace \
  --no-report \
  -- \
  --test-threads=1

# `aurora-compiler` also emits a static archive for packaged native builds. The
# archive has a different LLVM coverage map from the test-profile rlib, so it
# must not be presented to llvm-cov when the test profiles are reported.
rm -f \
  target/llvm-cov-target/debug/libaurora_compiler.a \
  target/llvm-cov-target/debug/deps/libaurora_compiler-*.a

report_args=(
  report
  --ignore-filename-regex "$IGNORE_REGEX"
)
if [[ "${1:-}" == "--check" ]]; then
  report_args+=(
    --fail-under-lines 96.07
    --fail-under-functions 96.81
    --fail-under-regions 94.29
  )
fi

cargo llvm-cov "${report_args[@]}"
