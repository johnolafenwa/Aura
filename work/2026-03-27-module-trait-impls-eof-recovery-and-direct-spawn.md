# 2026-03-27 Module Trait Impl, EOF Recovery, And Direct Spawn

## Goal

Close the remaining cross-module trait impl gap, the EOF dangling-member tooling gap, the module-qualified type-annotation parsing gap, and the direct-backend native-build failures around recursive payload bindings and spawned plain-class returns.

## Work Completed

- Added regression coverage for:
  - imported trait impls across module boundaries in compiler module tests
  - module-qualified type annotations in module tests and maintained examples
  - EOF dangling-member recovery in compiler analysis, CLI, and compiler-backed LSP bridge tests
  - direct/native builds for `examples/classes/indirect_recursive.au`
  - direct/native builds for spawned functions that return plain-class values
  - clearer diagnostics for unsupported `String(...)`, bare `Ok(...)`, `` call arguments, and list literals
- Exported/imported trait impl metadata through module namespaces and taught the checker, interpreter, MIR lowering, and compiler-backed analysis/completions to search imported module impls.
- Lowered imported trait impl methods into MIR so `run-mir` and the direct backend keep parity with `run`.
- Typed match payload bindings during MIR lowering and taught MIR/native type inference about runtime member returns like `Task.join()`, `Channel.recv()`, and related members.
- Added direct-backend spawn thunk boxing/unboxing for plain-class values so spawned functions can return plain classes natively.
- Extended recovery-only analysis sanitization so dangling-member lines can still recover a checked program when the dot is the final line in a function body.
- Updated module/tutorial docs and added a maintained module example for imported trait impls.

## Verification

- `cargo test -p aurora-compiler --test modules -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture check_fail_fixtures_match_expected_diagnostics`
- `cargo test -p aura --test cli analyze_recovers_symbols_for_dangling_dot_at_eof_stdin_buffers -- --exact --nocapture`
- `cargo test -p aura --test cli complete_recovers_member_completions_for_dangling_dot_at_eof_stdin_buffers -- --exact --nocapture`
- `cargo test -p aura --test cli build_with_direct_backend_supports_task_join_returning_plain_classes -- --exact --nocapture`
- `cargo test -p aura --test cli build_supports_task_join_returning_plain_classes -- --exact --nocapture`
- `cargo test -p aura --test cli build_with_direct_backend_runs_indirect_recursive_example -- --exact --nocapture`
- `npm test -- --runInBand test/compiler_bridge.test.js`

## Follow-up

- Add compiler-side unit tests around direct-backend thunk boxing/unboxing helpers if the native backend surface expands further.
- Keep pushing the LSP package toward enforced full coverage now that more cross-module compiler-backed behavior is under test.
