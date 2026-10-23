# Runtime Archive Copy Race in CLI Tests

## Goal

PR #120's macOS `verify` job failed in
`native_run_cache_rejects_symlink_and_fifo_members_without_blocking_or_leaking`.
The test copies the workspace runtime archive into an installed layout, and
the copy found no file.

## Cause

Cargo refreshes an uplifted artifact such as `target/debug/libaura_runtime.a`
by removing the old file and then linking the new one. When a sibling test's
`aura run` makes Cargo refresh the runtime, a copy that runs in that gap
returns `NotFound`.

## Change

- `copy_native_runtime_archive` in `crates/aura/tests/cli.rs` retries a
  `NotFound` copy every 50 ms for up to 30 seconds. Any other error, or a
  file still missing after that, fails as before.
- The three tests that install the archive use it.

## Verification

- The three tests pass.
- `cargo fmt --check`, `npm run check:clippy`, and `scripts/check-hygiene.sh`
  pass.
