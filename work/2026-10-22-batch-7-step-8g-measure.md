# Batch 7 Step 8g: Measure What Step 9 Inherits

## Goal

Finish step 8g of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
Count what is left in the direct emitter, re-run the compile-time and
runtime benchmarks against the start of phase 2, and record both in the
checkpoint before step 9 starts. The results are in the checkpoint's
Step 8 Results section. This note records how they were taken.

## Work Completed

- Branch `wip-8g` from `ff35ee70`, the 8f commit in PR #112. The change is
  docs and measurement files only. No compiler or runtime source changed.
- The base point is `840f91ee`, `v0.6.0-preview`. It built in its own
  worktree with its own Cargo target directory. Both commits built
  `aura` and `aura-runtime` in release mode. Every Aura build and run had
  its own `AURA_CACHE_DIR`.

### Inventory

- `emitter_inventory.py` assigns every line of `native_codegen.rs` to one
  group and prints the totals. It lists each range with a label.
  - Translation of MIR: 6,525 lines, plus 134 in
    `native_codegen_reachability.rs`.
  - Layout, boxing, and ABI: 6,805 lines, plus 1,448 in
    `native_codegen_callables.rs`.
  - Semantic decisions: 1,612 lines in seven items.
- A range goes to the group it mostly serves. A call site that resolves a
  method and then passes arguments is split at the lookup.
- The known items checked out as follows:
  - scalar-left `ArrayBinary` by member name: semantic, 48 lines;
  - `container_mutation_callables`: in the MIR validator, not the
    emitter. It keys on member names through `member_parts`, so it also
    matches a user method named `append` or `insert`;
  - `with` cleanup exit method as a string: semantic, 147 lines with the
    inline `TaskGroup` close;
  - `Supervisor.start` defaults: semantic, 71 lines with the other
    `process` defaults;
  - borrowed-element fusion: an instruction choice, 186 lines with the
    in-place comparison;
  - casts: translation. The helper follows from the MIR types and
    reports the failure. A float narrowing cast relies on the store's
    `float32` rounding, and both backends print the same result;
  - the `int32` range check: layout, because `int32` lives in a 64-bit
    word. The emitter checks an `int32` result again when it stores it;
  - `no_call_since`: layout;
  - the `transfer_*` helpers: layout.
- New items:
  - method resolution and dispatch, 850 lines. It covers class methods by
    name, trait impls by specificity, union receivers per member,
    candidate chains on erased receivers, `clone` on a union, and the
    `From` impl behind `?`;
  - generic call type arguments, 166 lines, unified from parameter and
    argument types for the boxed ABI;
  - named-argument binding for runtime ops, 144 lines, which the shared
    validator already did.

### Compile Time

- `scripts/bench-compile-time.py` ran on both builds:
  `compile-time-base.json` and `compile-time-head.json`. Its runs are
  sequential, and their cold medians moved by about 0.2 s between runs.
- `phases.py` alternates the two builds 11 times per subject. It splits
  each build into `aura check`, the `cc` link, and the rest.
- Callgrind profiled separate builds of both commits with symbols kept,
  LTO off, and 16 codegen units. `callgrind-phases.txt` lists the
  inclusive count of each phase.
- A second profile collected only inside `insert_ownership` on the
  reference agent. That found allocation at 37% of the pass and no single
  hot function. Callgrind cannot split the recursive type walks exactly.
- Hashing the runtime archive takes 3.34 billion instructions in every
  build. Python's OpenSSL SHA-256 takes 0.19 s for the same 62 MB here.

### Runtime

- `runtime_bench.py` builds each program with both commits and runs base,
  head, and the Rust reference in turn, 31 times. Every output matched.
- The Rust references are release builds of `benchmarks/rust_baselines`
  from 2026-10-20. TCP fan-out has no binary there and was not run.
- Callgrind counted `int_list_sum`, `string_list_scan`, and
  `record_field_reads` on both builds. The counts agree within 0.1%.

## Verification

- `scripts/check-hygiene.sh` passes.
- No code changed, so the compiler gates were not run. CI skips a
  docs-only change.

## Follow-Up

- The checkpoint recommends step 8h before step 9, in two PRs: bound
  runtime op arguments, then resolved method calls. Borrowed-element
  fusion stays for step 9.
- Memoize the ownership pass's type walks, and run the pass after
  pruning.
- Stop hashing the whole runtime archive on every `aura build`. On a CPU
  without SHA extensions it costs about 0.35 s, more than half of the
  phase 2 hello world budget.
