# 2026-10-11 Batch 4 Move Fixes

## Goal

Fix two defects found on `main` at `5652e46`:

1. The checker accepted a later use of a non-copy name that a tuple or
   collection literal had moved, when the literal sat in a shared position.
   `print((pair, 2))` followed by `print(pair)` passed `aura check`, then
   trapped with `AU4001 unknown MIR place` on MIR and with a null opaque
   pointer on the direct backend.
2. A direct binary released a shared-loan lambda capture twice.
   `f = lambda [values]: len(values)`, `f()`, then scope exit trapped with
   `AU4001 attempted to release an already-released direct runtime value`.

## Work Completed

- Root cause 1: sema typed a tuple, list, set, or dict literal without
  consuming its elements. Consumption only ran when an owned context replayed
  the literal through `consume_value_expr`. Shared positions such as
  `print`, `len`, f-string interpolation, equality operands, bare parameters,
  `for` sources, and bare `match` scrutinees never replayed it. Both backends
  always move the elements into the new aggregate, as the tuple and ownership
  manual pages require. Sema now consumes each element in evaluation order
  while it types the literal (`consume_aggregate_literal_element`). An owned
  context still replays from the pre-literal state and merges the same moves.
  A borrowed parameter used as an element now reports `AU3002` instead of
  failing at run time.
- Root cause 2: the direct `invoke` adapter for a closure retained only
  by-value captures before calling the lowered lambda. A direct function
  releases its shared parameters at exit, and direct call sites retain shared
  arguments for that reason. The adapter now retains every capture except a
  `mut` capture, whose updated value the function returns into the
  environment. `mut` and `own` captures were already balanced.
- `union_values_compare_and_flow_through_generic_choices` in
  `tests/batch1_coverage_diagnostics.rs` compared `[tagged] == [other]` and
  then passed `tagged` to an owned parameter. Both backends move `tagged` into
  the list, so that program was a latent use after move. The test now
  compares lists built from separate union bindings.
- Docs: `docs/manual/ownership-and-borrowing.md` lists tuple literals as an
  owned position and states the shared-position rule with a `check-fail`
  example. `docs/manual/tuples.md` gains a short construction rule.

## Regression Tests

- `check-fail`: `print_tuple_literal_moves_element`,
  `print_list_literal_moves_element`, `len_dict_literal_moves_value`,
  `fstring_set_literal_moves_element`,
  `tuple_literal_equality_operand_moves_element`,
  `shared_parameter_tuple_literal_moves_element`,
  `shared_parameter_tuple_literal_borrowed_element`,
  `match_tuple_literal_moves_element`, `for_list_literal_moves_element`.
- `run-pass` (in the forced MIR/direct parity matrix):
  `shared_aggregate_literal_arguments` keeps copy elements and explicit clones
  usable after shared literal arguments. `lambda_loan_capture_calls` calls
  shared, multi-capture, `mut`, and `own` capture closures repeatedly and then
  uses the captured values.

## Verification

- Both new `run-pass` fixtures match their `.stdout` on `run --backend mir`
  and on a `build --backend direct` binary.
- `cargo test -p aura-compiler --test fixtures` passes after the merge with
  `origin/main`. Every other `aura-compiler` and `aura` test binary passes
  except tests that failed on the shared workstation for environmental
  reasons: a root-run permission check in `package_tests`, wall-clock
  latency tests under a load average near 17 on 4 cores, and `cli` tests
  whose runtime build hit a full disk.
- `cargo fmt --all --check`, `npm run check:clippy`,
  `npm run check:doc-examples`, and `npm run check:hygiene` pass.
- Local coverage and the full parity matrix were left to hosted CI because
  the shared workstation is short on disk.

## Follow-Up

- None known. Hosted CI runs the coverage floors and the full parity matrix.
