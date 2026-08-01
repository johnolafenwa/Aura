#!/usr/bin/env bash
set -euo pipefail

target="${1:-x86_64-unknown-linux-gnu}"
toolchain="${AURA_NIGHTLY_TOOLCHAIN:-nightly-2026-07-01}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export RUSTFLAGS="${RUSTFLAGS:-} -Zsanitizer=thread"
export RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -Zsanitizer=thread"
export RUSTUP_TOOLCHAIN="$toolchain"
export TSAN_OPTIONS="${TSAN_OPTIONS:-halt_on_error=1}"
export CC="$repo_root/scripts/cc-tsan.sh"

# The scheduler, worker, reactor-facing queue/task registrations, reachability
# analysis, and blocking-pool coordination all live in runtime_value's unit-test
# module. Run the complete module so naming changes cannot silently remove a
# cross-worker path from the race-detection gate.
cargo "+$toolchain" test \
  -Zbuild-std \
  --target "$target" \
  -p aura-compiler \
  --lib \
  'runtime_value::tests::' \
  -- --test-threads=1

# Keep these as filters rather than an enumerated list so every regression
# added to the maintained four-worker fixture family automatically joins the
# data-race gate. The join filter also covers the loaded four-worker
# reachability regression, whose test name describes its semantic contract.
for filter in four_worker task_group_join; do
  cargo "+$toolchain" test \
    -Zbuild-std \
    --target "$target" \
    -p aura \
    --test cli \
    "$filter" \
    -- --test-threads=1
done
