# 2026-03-27 Regression Hardening And CLI Stdin

## Goal

Turn externally reported Aurora regressions into maintained compiler/CLI coverage, then fix the affected checker, interpreter, MIR, runtime, and direct-backend paths so the maintained surface behaves consistently again.

## Work Completed

- Added compiler regression fixtures covering:
  - generic trait dispatch on builtin types through generic bounds
  - generic functions calling other generic functions, including explicit specialization
  - non-consuming borrowed field reads in free functions
  - `match ` over indirect borrowed fields
  - large negative literals that need `int64`/`int128`
  - float special-value rendering
  - long chained binary expressions
  - the improved `range(...)` arity diagnostic
  - move-out-of-borrowed-field rejection as an explicit check-fail case
- Added CLI/product regression tests for:
  - stdin-backed `check`, `run`, and `run-mir` with local module imports
  - `run-mir` execution of generic constructor specialization
  - interpreter execution of long binary-expression chains
  - maintained example parity for `simple_example.au`, `generic_constructor_specialization.au`, and `namespace_import_types.au` through both default builds and direct builds
- Fixed checker move analysis so borrowed free-function field reads and borrowed-match projections no longer fail as moves, while true move-out cases still reject.
- Fixed generic call inference so generic functions can compose with other generic functions instead of treating in-scope caller type parameters as unresolved.
- Fixed interpreter runtime type instantiation for generic parameters so generic dispatch on builtin trait impls resolves against actual runtime argument types.
- Fixed interpreter evaluation/coercion so long binary chains stay fast while `float32` arithmetic and large negative literals still preserve the intended narrowed types.
- Fixed MIR type inference for explicitly specialized `Channel[T](...)` constructor calls so `for item in jobs:` lowers correctly through `run-mir` and built binaries.
- Fixed the direct backend to recover declared field types from opaque classes, which restores native handling of mixed classes like `Person { age: int32, name: String }`.
- Fixed direct-backend `float32` coercion and native float rendering so built binaries print `float32` results with the same user-facing surface as the interpreter and MIR runtime.
- Fixed native runtime equality so opaque values like enums compare correctly in built binaries.
- Updated CLI docs and tutorials so stdin-backed `check`, `run`, `run-mir`, and `build` behavior matches the now-supported local-module execution path.

## Verification

- `cargo test -p aurora-compiler -- --nocapture`
- `cargo test -p aura -- --nocapture`

## Follow-up

- Keep converting externally reported regressions into maintained fixtures and CLI product tests instead of relying on ad hoc example sweeps.
- Continue tightening native-backend coverage around opaque-field classes and mixed numeric paths so new parity gaps surface immediately in CI.
