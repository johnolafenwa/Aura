# Batch 3 phase 3b: typed context managers

Started 2026-10-11 on `batch-3b-managers` from `main` at `1a3e4174`. Design
authority: [ADR-0060](../architecture_docs/decisions/0060-typed-context-managers.md)
and the Batch 2–3 design checkpoint sections F1–F5 and G1. Phase 3a
(`__init__`, definite initialization) runs in parallel on another branch and
is integrated separately.

## Goal

- Replace the `close(mut self)`-only `with` contract with typed context
  managers: `__exit__(mut self) -> None | Result[None, E]`, optional
  `__enter__(mut self) -> None | view [mut] T from self`, close-only classes
  as managers whose exit is `close`, builtin resources unchanged, generic
  classes admitted. Bad shapes report the new `AU2017`.
- Multi-item headers `with a, b:` entered left to right and exited in
  reverse, with later items seeing earlier bindings; malformed headers
  report `AU1101`.
- Entry views: the item name binds the `__enter__` view and the manager
  lives in a hidden managed binding; escape is `AU3010`, conflicts `AU3002`.
- `Result` exits propagate like `try` on normal paths, are dropped under
  `try` propagation and cancellation, and attach as a note under a trap.
- Exits run with cancellation pending; scheduler abandonment runs none.
- Typed MIR cleanup actions with validator checks at both public
  boundaries, and byte-identical MIR and direct behavior.

## Work Completed

- Parser: `parse_with_stmt` loops over comma-separated items and nests them
  as single-item `WithStmt`s, each with its own span. New messages for a
  missing comma, a trailing comma, and a missing `as name`.
- Checker: `sema/managers.rs` classifies a type as a manager
  (`ManagerEntry`, `ManagerExit`, `ManagerShape`) and renders the four
  `AU2017` shape messages plus the exit-result compatibility message.
  `check_with` prepends a synthetic `view [mut] name = with#L_C.__enter__()`
  for view entries and rewrites the hidden name in body diagnostics to "the
  manager of the `with` item at L:C".
- MIR: `PushCleanup` carries `action: CleanupAction` (`ResourceClose` or
  `ManagerExit { exit_result: Unit | Result }`, serde-defaulted), and
  `PopCleanup` carries `run`. Normal paths pop without running and emit an
  explicit `__exit__` call, wrapped in `Rvalue::Try` for a `Result` exit.
  Abnormal paths run the action from the runtime cleanup stack.
- Validator: `validate_manager_exit_actions` rejects a manager registered as
  a `close` resource, a `ManagerExit` on a place that is not a class with
  `__exit__`, a declared exit result that differs from `__exit__`'s return
  type, and a registration before `__enter__` returns. A `ManagerExit` push
  is not checked as a read, because the entry view holds its loan there;
  the pop is still checked as a mutation, so the view must end first.
- MIR runtime: the cleanup stack holds `(place, action)`; trap unwinding
  collects `Result.Err` exits as notes on the trap
  (`` `Class.__exit__` also failed during cleanup: payload ``).
  `close` and `__exit__` share `call_instance_exit`.
- Direct backend: cleanup thunks and inline cleanups call `__exit__` for
  manager actions. On trap paths a `Result` exit's value is handed to the
  new `aura_direct_record_manager_exit_failure`, which stores the rendered
  note in task runtime state; `runtime_diagnostic_error` and the task
  boundary attach the notes, and cancellation clears them.
- Analysis and LSP: a `with` name hovers as the entry view when the manager
  declares one, and its definition is the name's own position, including
  the `expression as name` form and multi-item headers.
- Fixtures: 5 run-pass, 4 run-fail (with `.stdout` checked on both
  backends), 18 check-fail, 3 parse-fail, 1 check-pass; the fixture and
  parity harnesses compare a run-fail fixture's partial stdout when a
  `.stdout` sidecar exists. `tests/manager_validator.rs` forges six MIR
  corruptions and checks both boundaries report the same reason.
- Docs: statements (`with` rewrite), classes, execution model, static
  semantics, grammar, ownership and borrowing, generics and traits,
  diagnostics (`AU2017`), CLI code table, learn/from-python, tutorial 11,
  tutorials README, examples README, regenerated `llms` files. New example
  `examples/resources/context_managers.au` with CLI run and direct tests.

## Verification

- `cargo build -p aura`, `cargo fmt --all --check`,
  `cargo clippy -p aura-compiler -p aura -- -D warnings`: clean.
- `RUST_MIN_STACK=33554432 cargo test -p aura-compiler`: all 49 integration
  test binaries pass; the library suite passes except
  `package::tests::package_io_helpers_cover_local_error_paths`, which fails
  only because the sandbox runs as root and can write to a `0o500` directory.
- `cargo test -p aura --test backend_parity -- --ignored`: see the final
  report for this branch.
- `npm run check:doc-examples`: all 739 examples pass.
- LSP: the new `compiler_bridge` test passes.
- Coverage gates were not run on this branch: the shared workstation had
  under 6 GiB free while another agent's build used the rest.

## Follow-Up

- Run `npm run coverage:compiler:check` and `npm run coverage:lsp:check`
  after integration with phase 3a.
