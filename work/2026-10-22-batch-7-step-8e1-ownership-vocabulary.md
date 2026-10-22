# Batch 7 Step 8e, PR 1 of 4: Ownership Vocabulary in MIR

## Goal

Give MIR the words it needs to state ownership, with no change to what
either backend emits. This is the first of four PRs for step 8e of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
PR 2 adds the ownership pass and checker in shadow mode, PR 3 switches the
direct emitter to the MIR's decisions, and PR 4 moves lending into MIR.

## Work Completed

- MIR has two new instructions and one new field:
  - `Retain { place }` creates one new reference to the value at `place`;
  - `Release { place }` drops the value of a root local and leaves it empty;
  - `Rvalue::Try` gains `releases_on_error`, the root locals its `Err` edge
    releases, in name order.
- `MirModule.ownership` is `Implicit` or `Explicit`. Lowering produces
  `Implicit`, and MIR without the field reads as `Implicit`.
- `MirClass.copy` and `MirEnum.copy` mark a type whose values need no drop.
  Lowering fills them with `type_is_copy_in_program`, the rule union layout
  plans use, asked about the type with its own type parameters. A generic
  class and an enum with a type-parameter payload are not marked. MIR
  without the mark needs drop.
- The new fields serialize only when set, so the JSON of every lowered
  module is unchanged except for the `copy` marks.
- The shared validator, which runs at both public boundaries, refuses:
  - a `Retain`, a `Release`, or `try` releases in an implicit module;
  - a release of a projection, an unknown local, or a local that an active
    loan holds;
  - a retain of an unknown place;
  - `try` releases out of name order or listed twice;
  - a Copy mark on a generic class, or on a class or enum with a field or
    payload that needs drop.
- A `Release` may follow a returned loan, like the other cleanup
  instructions.
- `mir::ownership` holds the operand-role table. `operand_roles` and
  `terminator_roles` give every operand a `Consume` or `Borrow` role, as the
  plan's section 3 lists. A method call's receiver consumes, because the
  callee owns every parameter. `ReborrowElement` selectors consume like
  `BeginElementLoan` selectors. Only its tests read the table for now. PR 2
  is its first user.
- `RuntimeOp::arg_ownership(index)` and `RuntimeOp::receiver_ownership()`
  are exhaustive over the op families. A member op consumes exactly its
  `own` parameters. A private op consumes the key and value an index write
  stores and the values a span restore puts back. Every receiver borrows.
- The interpreter and the Cranelift emitter treat `Retain` and `Release`
  as no-ops and ignore `try` releases.
- The direct runtime's `consume_owned_value` and
  `consume_owned_untracked_value` copy the value when the box has another
  reference, instead of emptying it. A test shows the hazard: a string
  retained once and appended to a list read as `None` through the other
  reference. A box with one reference is still emptied without a copy.
- `AURA_RUNTIME_STATS` reports `retains`, `releases`, and `clones`. The
  direct runtime counts calls to `aura_direct_retain_value`,
  `aura_direct_release_value`, and `aura_direct_clone_value`, and each
  shared-box copy above. The counters run only after
  `aura_direct_runtime_init` sees the report requested, so a retain costs
  one relaxed load otherwise. The interpreter reports zero for them.
- `scripts/bench-representation.py` reads all eight counters. It rejected
  the `cloned_elements` counter before.

## Verification

- `cargo fmt`, the CI clippy command, and `scripts/check-hygiene.sh` pass.
  `cargo clippy --workspace --all-targets -D warnings` still fails on test
  lints from before this PR, none in changed lines.
- Tests pass with the root-only `package_io_helpers_cover_local_error_paths`
  skipped: aura-compiler 2578, aura-runtime 739, aura 500.
- Backend parity passes.
- Every run-pass fixture emits a byte-identical object at base and head
  (627 of 627).
- Startup of `print("hi")`, median of 30 debug runs: base 38.3 ms, head
  35.8 ms.
- A traced run found the shared-box copy in 8 of 663 run-pass fixtures, 13
  times in all, every one a callable or closure. Their output is unchanged.

## Follow-Up

- PR 2: the ownership pass and checker, run in shadow mode at both
  boundaries. The pass must not insert anything between `IndexInRange`
  and the read it covers.
- PR 3: compare each fixture's `retains` plus `clones` against the base
  counters recorded with this PR.
