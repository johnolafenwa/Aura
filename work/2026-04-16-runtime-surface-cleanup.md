## Goal

Remove leftover redundant compiler/runtime surface after interpreter removal, with the immediate focus on deleting the `*_via_mir` compiler aliases and aligning internal tests and maintained docs with the reduced two-path model.

## Session

- Start time: 2026-04-16 21:22:00 BST

## Work Completed

- Removed the redundant compiler-side `run_source_via_mir(...)`, `run_path_via_mir(...)`, and `run_path_with_source_via_mir(...)` aliases from `crates/aurora-compiler/src/lib.rs`.
- Reworked the internal compiler integration surface so tests now use the canonical public `run_*` entrypoints or the explicit MIR helpers `lower_*_to_mir + run_mir(...)` when MIR-level runtime coverage is intentional.
- Deleted duplicate fixture/integration coverage that only repeated the same `run` path under old `*_via_mir` names.
- Renamed stale CLI tests in `crates/aura/tests/cli.rs` that still implied a removed `run-mir` execution path even though they already exercised the canonical `run` command.
- Hardened git dependency checkout caching in `crates/aurora-compiler/src/package.rs` so restricted environments can fall back from a home-directory cache root to a temp-directory cache root when materializing git dependencies.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`

## Follow-up

- No immediate follow-up for this cleanup pass. The remaining runtime surface is the intended two-path model: public `run` via MIR and `build` via native codegen, plus explicit MIR lowering/runtime helpers where tests or internal callers need them.
