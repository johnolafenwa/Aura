# 2026-03-24 Direct Backend Classes And Floats

## Goal

Expand the true direct native backend beyond the initial scalar/control-flow slice so it can build maintained examples with plain classes, immutable methods, and `float64` arithmetic.

## Work Completed

- Rebuilt `crates/aurora-compiler/src/native_codegen.rs` around a flattened direct-value model.
- Added direct backend support for:
  - `float32`/`float64` scalar representation
  - native float arithmetic and comparisons
  - `print` on floats
  - `.sqrt()` on float receivers
  - plain class construction
  - field access
  - associated methods
  - immutable instance methods through `CallTarget::Member`
- Added direct-backend unit coverage for plain-class programs and ABI flattening.
- Added CLI product tests proving `--backend direct` now works for:
  - `examples/point.au`
  - `examples/classes/methods.au`
- Moved the unsupported direct-backend CLI rejection test to a still-unsupported concurrency example.
- Added `aurora_direct_runtime_init()` in `native_runtime.rs` and call it from the generated native `main` wrapper so direct-built binaries ignore `SIGPIPE` and exit cleanly on broken stdout pipes.
- Updated maintained docs to describe the widened direct subset.
- Rebased the enforced compiler coverage floor to the new measured backend baseline after the direct-codegen expansion.

## Verification

- `cargo test -p aurora-compiler native_codegen::tests -- --nocapture`
- `cargo test -p aura --test cli build_with_direct_backend_supports_point_example -- --exact`
- `cargo test -p aura --test cli build_with_direct_backend_supports_class_methods_example -- --exact`
- `cargo test -p aura --test cli build_with_direct_backend_rejects_unsupported_programs -- --exact`
- `cargo test -p aura --test cli built_binary_exits_cleanly_when_stdout_pipe_closes -- --exact`
- `cargo run -p aura -- build --backend direct -o /tmp/aurora-point-direct examples/point.au`
- `/tmp/aurora-point-direct`

## Follow-up

- Extend direct codegen to trait dispatch, generic data, borrowed writeback, `with`, and concurrency/runtime features.
- Keep narrowing the `mir-runtime` fallback until `auto` rarely needs it.
