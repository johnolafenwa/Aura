## Goal

Add a true direct native backend for a supported MIR subset, expose backend selection through `aura build`, and keep the compiler coverage gate green after introducing the new codegen modules.

## Work Completed

- Added `aura build --backend auto|direct|mir-runtime`.
- Implemented a direct native backend in `crates/aurora-compiler/src/native_codegen.rs` using Cranelift object emission for the current scalar/control-flow subset.
- Added direct-runtime helper exports in `crates/aurora-compiler/src/native_runtime.rs`.
- Kept `auto` as the default build mode so supported programs use direct native codegen first and unsupported programs fall back to the broader runtime-linked MIR artifact backend.
- Added CLI regression tests for direct-backend success and unsupported-program rejection.
- Added compiler-side direct-backend tests so the new codegen modules are exercised under `cargo llvm-cov`.
- Updated the root README, CLI README, running-programs tutorial, examples README, current-language-surface tutorial, and task board to describe the new backend matrix accurately.

## Verification

- `cargo test -p aura --test cli build_with_direct_backend_produces_runnable_binary_for_supported_program -- --exact`
- `cargo test -p aura --test cli build_with_direct_backend_rejects_unsupported_programs -- --exact`
- `cargo test`
- `npm run coverage:compiler:check`
- `npm run ci`

## Follow-Up

- Expand the direct backend beyond the current scalar/control-flow subset so fewer programs need the runtime-linked MIR artifact path.
- Add more product-level coverage around mixed backend behavior in `auto`, especially as direct codegen grows to cover more of the language.
