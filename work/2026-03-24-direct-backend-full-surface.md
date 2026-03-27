# 2026-03-24 Direct Backend Full Surface

## Goal

Bring the direct native backend up to the full currently implemented Aurora language surface so runnable maintained programs no longer require the runtime-linked MIR fallback to build natively.

## Work Completed

- Added direct-backend support for free-function `borrow mut` writeback and mutating method receiver writeback through an ABI that appends mutable borrow values to native call results.
- Added direct support for `range(...)` and MIR `ForRange`, including named builtin argument handling for `range(stop=...)` and `range(start=..., stop=...)`.
- Added direct runtime helpers for native `Range` values and iteration state updates.
- Fixed native runtime artifact resolution in the CLI build path so direct builds reliably link against the canonical Aurora static runtime library instead of stale hashed archives under `target/*/deps`.
- Extended direct backend product coverage with failing-first CLI tests for:
  - `examples/basics/borrow_parameters.au`
  - `examples/classes/mutating_methods.au`
  - `examples/control_flow/for_range.au`
- Added CLI regression tests for native runtime staticlib path selection so the direct build path keeps preferring `target/<profile>/libaurora_compiler.a` and only falls back to the newest hashed archive when the canonical staticlib is missing.
- Updated outdated native-codegen tests so they reflect the current direct-backend capabilities for trait dispatch and runtime-backed opaque types.
- Verified that every runnable maintained example under `examples/` now builds with `--backend direct`.
- Verified parity by comparing direct-built binary output against `aura run` for every runnable maintained example.
- Updated README, CLI README, examples README, tutorials, and the task board to describe the direct backend as the full current maintained surface.
- Rebased the enforced compiler coverage gate to the new measured baseline after the backend expansion.

## Verification

- `cargo test`
- `npm run test:lsp`
- `npm run coverage:compiler:check`
- `npm run ci`
- direct build sweep over runnable `examples/*.au`
- direct output parity sweep against `aura run` for runnable `examples/*.au`

## Follow-up

- Decide whether to remove or deprecate `--backend mir-runtime` now that the maintained Aurora surface has direct coverage.
- Continue raising compiler coverage toward 100%, especially in `native_codegen.rs`, `native_runtime.rs`, `analysis.rs`, and `mir_runtime.rs`.
