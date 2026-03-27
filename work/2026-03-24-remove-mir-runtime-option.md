# 2026-03-24 Remove Mir Runtime Build Option

## Goal

Remove the `--backend mir-runtime` product option now that the direct backend covers the full currently implemented Aurora language surface.

## Work Completed

- Added a failing-first CLI regression test that verifies `aura build --backend mir-runtime ...` is rejected.
- Removed `mir-runtime` from `aura build` argument parsing and usage text.
- Simplified build backend behavior so both `auto` and `direct` resolve to the direct native backend.
- Removed the old runtime-artifact build path from the CLI implementation so the product surface and code path no longer diverge.
- Updated the root README, CLI README, tutorials, and task board to document `aura build` as `--backend auto|direct` only.

## Verification

- `cargo test -p aura`
- `cargo test`
- `npm run ci`

## Follow-up

- Decide whether `run-mir` should remain as a debugging/runtime comparison command or eventually be retired as well.
