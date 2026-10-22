# Batch 7 Step 8f: Explicit Access and Arithmetic Checks

## Goal

Finish step 8f of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
MIR states each access and arithmetic check. A read that an `IndexInRange`
fact proves becomes an unchecked MIR read, and each numeric operation names
its trap. The emitter's proven-access state goes away. Objects stay
byte-identical to `642f6aff`.

## Design

### What the emitter decides today

- **Proven access.** `compile_block` keeps the last `IndexInRange { list,
  index }` and asks `ProvenAccess::of` what it proves about each following
  instruction. It skips `Lend` and `EndLend`, which keep the fact.
  - `Read`: an `__index` call on `list` whose first argument is `index`
    and whose target is neither. The fact stays for the next instruction.
    `ListOp::Index` then skips `emit_vec_index_failure_guard`, and
    `ListOp::IndexScalar` skips `emit_list_view_bounds_check` on the fast
    path. Any other `__index` op ignores the proof.
  - `Advance`: `index = index + 1`. The wide integer path uses a plain
    `iadd` instead of `sadd_overflow` and the overflow trap.
  - `Lend`: a `BeginElementLoan` of `list[index]` with no projection. A
    loan of a local list element then skips `VecCheckIndex`. Other loans
    probe anyway.
  - Any other instruction ends the fact.
- **Adjacency.** The emitter reads the fact only for the instruction right
  after it, so `ownership::glued` keeps the ownership pass from inserting
  anything between the fact, the read, and the advance. Lending puts its
  `Lend` before the fact and renames the fact's list to the temporary.
- **Arithmetic.** The emitter picks each check from the operand layout:
  - `int64` and `uint64` `+`, `-`, `*`, and negation check the overflow flag
    and call `fail_integer_overflow`. Signed `//` checks zero, then
    `MIN // -1`. `%` checks zero only.
  - `int32` values live in 64-bit words. Every `int32` result and every
    store to an `int32` slot checks the range and calls
    `fail_int32_overflow`. `//` and `%` also check zero.
  - `float` `/` checks zero and calls `fail_division_by_zero`.
  - `**`, bitwise operators, shifts, float `//` and `%`, and every
    operand the backend boxes call `BinaryValue` or `UnaryValue`. The
    helper reports overflow, shift range, and zero divisors itself.
  - A cast calls a conversion helper chosen by the source and target
    kinds. The helper reports a value that does not fit.
- **Bounds and keys.** `__index` and `__set_index` check bounds and report
  `AU4003`. A dict read reports a missing key. An element loan checks at
  creation. MIR names each of these as a checked form today.
- **Other elision.** `no_call_since` skips a repeated runtime type tag on
  the same box. A tag is part of boxing, so it is layout and stays. The
  `int32` range check skips a constant that fits. That is constant folding
  and stays too.

### What MIR states after 8f

- `ArithmeticTrap` on `Rvalue::Binary` and `Rvalue::Unary` names the check
  and the trap: `None`, `Overflow`, `DivisionByZero`,
  `DivisionByZeroOrOverflow`, or `Runtime`. `Runtime` means a runtime
  helper computes the value and checks it. The trap follows from the
  operator and the operand types, by one shared table,
  `mir::checks::binary_trap` and `unary_trap`.
- `ListOp::IndexUnchecked` and `ListOp::IndexScalarUnchecked` are the
  element reads an `IndexInRange` covers. A covered advance has trap
  `None`. A covered element loan has `checked: false`.
- `mir::checks::resolve_checks` runs just before the ownership pass: in
  `lower`, and at the native boundary for an implicit module. It names
  every trap and marks what each fact covers, with the emitter's pairing
  rule. Lowering puts the fact, the read, and the advance next to each
  other, so marking before the ownership pass marks what the emitter
  marked after it. `lower_implicit` skips the pass, so `aura run` pays
  nothing for it.
- `validate_checks` runs at the native boundary. A trap must match the
  table. An unchecked access must follow a fact for the same list and
  index in the same block. Only retains, releases, lends, other facts, and
  covered reads may come between. None of them may write the index, and
  none may write, move, release, or lend the list before a read.
- The emitter follows the marks. It keeps the layout checks: the `int32`
  range check and the boxed helpers.
- The interpreter reads an unchecked read as a checked one. It still
  checks each fact.
- The marks travel with their instructions, so the ownership pass no
  longer needs the fact glued to its read.

## Work Completed

- `mir.rs` adds `ArithmeticTrap`, the `trap` field on `Rvalue::Binary` and
  `Rvalue::Unary`, and the `checked` field on `BeginElementLoan`. Both
  fields have serde defaults, so older MIR still reads.
- `mir_checks.rs` holds the trap table, the fact walker, `resolve_checks`,
  `mark_covered_accesses`, and `validate_checks`. `mir_checks_tests.rs`
  tests them.
- `runtime_ops.rs` adds `ListOp::IndexUnchecked` and
  `ListOp::IndexScalarUnchecked`. They call the same helpers as the checked
  forms.
- `native_codegen.rs` loses `ProvenAccess` and the pairing in
  `compile_block`. Each arithmetic path takes the trap and makes only the
  checks it names. The file shrinks from 14,986 to 14,942 lines.
- `mir_ownership.rs` keeps `glued` only for returned views. The fact no
  longer needs it.
- `lower` runs `resolve_checks` before the ownership pass. `lower_implicit`
  skips it. A first version ran it in `lower_implicit`, and `aura run` then
  spent 2.1M more instructions, mostly building the validator's context.

### Answer to the 8e question

The ownership pass no longer needs the fact kept next to its read. The
marks travel with their instructions, and the validator accepts retains,
releases, and lends between a fact and its access. With the glue removed,
all 750 objects still match. The lending pass still reads lowering's
order: it lends a covered element when the fact sits right before the
loan, and it renames the fact's list. That is lowering's adjacency, not a
rule the ownership pass must keep.

## Verification

- Objects: all 750 fixture objects, 124 of them run-fail, match `642f6aff`
  byte for byte. The determinism test passes.
- Parity: `backend_parity` passes with `--ignored`. Both backends print the
  same output and the same run-fail diagnostics for every runtime fixture.
- Tests: `aura-compiler` lib 1,646 and integration 1,001 pass,
  `aura-runtime` 740, and `aura` 501. Two `cli` safepoint starvation tests
  failed once under coverage load and pass alone.
- Lint: `cargo fmt --check`, the CI clippy command, and
  `scripts/check-hygiene.sh` pass.
- Coverage: lines 96.53 (floor 96.46), functions 97.39 (floor 97.33), and
  regions 95.45 (floor 95.23). `mir_checks.rs` covers 98.82% of its lines.
- Startup: `aura run` of a one-line program takes 93.33M to 93.35M
  instructions, against 93.34M to 93.62M at `642f6aff`.

## Follow-Up

- 8g should count the checks the emitter still picks from layout: the
  `int32` range check on every result and store, the boxed helpers, and
  the conversion helpers casts call. A cast names no trap yet.
- 8g should measure the compile-time cost of `resolve_checks` and
  `validate_checks`. Each builds the validator's context once per build.
