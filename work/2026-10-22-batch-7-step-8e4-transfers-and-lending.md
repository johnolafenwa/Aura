# Batch 7 Step 8e, PR 4 of 4: Lending in MIR and Eager Retains

## Goal

Finish step 8e of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
The direct emitter should decide no ownership on a MIR operand. A future
LLVM emitter must reproduce what native code does from MIR alone. This PR
lands in two commits, each with its own gates:

- Part 4a moves lending into MIR. It deletes the emitter's lending
  peephole.
- Part 4b is part 3b: the operand-role table matches what the emitter
  borrows, and each `Retain` makes its reference where MIR states it.

Lending goes first. An eager `Retain` of a field the emitter used to lend
would copy the field.

## Work Completed

### Part 4a: lending

- New instructions `Lend { place, temp, mutable }` and
  `EndLend { place, temp, mutable }`. A lend moves the value out of
  `place` into a fresh `%tN` temporary for one call. The end moves it back.
  `aura mir` shows both.
- `mir::lending` runs first in the ownership pass. It plans the same
  places the emitter used to lend:
  - a field of a class instance given to a shared parameter, or to a `mut`
    parameter whose writeback is the field;
  - the receiver of a builtin `list`, `dict`, or `set` operation when it
    is a field or an element a loan selects;
  - the receiver of a class method when a loan selects it;
  - an element read that an `IndexInRange` covers. The fact then names the
    temporary.
- The overlap rules moved with it. No other operand may overlap the place
  or reach a different root through it. No argument may hold an instance
  or a callable. The call's target may not overlap the place.
- The pass covers the temporaries. A shared temporary gets a `Retain`,
  because the callee owns its parameter. A mutable temporary moves into
  the call and comes back through the writeback, with no retain.
- The loan validator checks a lend as a read of the place into the
  temporary, and the end as a write back. `validate_lends` checks the
  shape and re-plans the call, so a hand-written lend must be one the pass
  would make. The ownership checker treats `Lend` as a read and a fill, and
  `EndLend` as a move and a read.
- The interpreter lends a copy. `strip_ownership` removes lends, and the
  pass makes them again.
- The emitter lowers `Lend` as a take by name out of the instance, or out
  of the collection through the loan's element path. `EndLend` stores the
  value back. Deleted `lendable_arguments`, `lendable_receiver`,
  `compile_lent_member_call`, `take_lent_receiver`, their types, and five
  helpers they used.
- Borrowed-element fusion stays. It picks a runtime helper for `len` or
  `get` on a borrowed element of a local list, and moves nothing. The MIR
  already says the operand is borrowed, so the planner leaves those calls
  alone.

### Part 4b: eager retains

- `ownership::borrowed_receiver` finds the receivers a call reads where
  they lie: a `Member` or `TraitMember` receiver whose type is a type
  parameter, and the receiver of a union's `clone`. `operand_roles_with`
  borrows them. The pass, the checker, and the emitter share it, so MIR
  states no `Retain` for them.
- `Retain` now makes the reference where MIR states it, by layout. The
  consuming operand that follows takes it as an owned temporary.
  - A consumer that stores the value inline turns the retain into a copy.
  - A consumer that only reads the value leaves the reference. Its retain
    and releases then cancel at the end of the instruction. This covers
    opaque handles and inline callables.
  - A method call on a union receiver hands the active member to the
    member's method.
  - Each arm of a branch inside one instruction takes the same references.
- The transfer helpers stay. A consuming MIR operand now always reaches
  them owned, so they move it. Their retain path still serves values MIR
  does not track: boxed Copy values, and temporaries the emitter makes.

### Line counts

`native_codegen.rs` had 14991 lines at `f20e13a2`. Part 4a takes it to
14701. Part 4b takes it to 14986, as eager retains need their own
tracking. `mir_lending.rs` adds 860 lines and its tests 346.

## Findings

### `record_field_reads`

Part 3 saw this benchmark run slower at equal instruction counts. The call
pattern did not change. Callgrind with branch and cache simulation on the
release binaries:

- Total instructions: 695.94 million at `725acde`, 694.61 million at
  `f20e13a2`, and 693.17 million at head.
- `aura_direct_tag_value_type`: 45.0 million instructions and 275,001
  calls in all three builds, from the same callers with the same counts.
- Simulated branch misses in that function: 296,815 at `725acde`, 299,574
  at `f20e13a2`, and 130,143 at head. The hot functions sit 16
  bytes later in the head binary.

The same work with misses that swing both ways is code layout. Part 3 also
saw one unrelated function move base by 4%. No fix is needed.

### Native cache key

The object cache key hashes the MIR, the runtime archive, the schema
version, and `CARGO_PKG_VERSION`. A change to the emitter alone keeps the
key. Every release from 0.2.0 to 0.6.0 has its own version, so a user who
upgrades never reuses an object. Two dev builds of one version can share a
key, and this work hit stale objects once while it compared builds. That
affects only compiler developers, who can set `AURA_CACHE_DIR`. No fix is
needed for users.

## Verification

Both parts ran the fixture and library gates. The rest ran on part 4b.

- Every run-pass fixture passes on the direct backend in a debug build,
  663 of 663, with the ledger and the over-release check on. This holds
  after each part.
- Counters, `f20e13a2` against each part:
  - part 4a: retains 8474 to 8458, releases 71239 to 71045, clones 3636
    to 3636;
  - part 4b: retains 8457, releases 71214, clones 3636;
  - retains plus clones are lower in 8 fixtures and equal in 655. None
    is higher.
  - `task_group_cancel` and `yield_now_fairness` releases vary run to
    run, as in part 3.
- Tests pass with `package_io_helpers_cover_local_error_paths` skipped:
  aura-compiler 2629 (1636 in the library), aura-runtime 740, and aura
  500.
  - `loop_backedge_safepoints_prevent_socket_readiness_starvation` fails
    on its interpreter half under `RUST_MIN_STACK=33554432`. Run by hand,
    its program prints `false` in 2 of 6 runs with that variable and in
    none of 12 without it. The test passes 3 of 3 runs without it.
    `aura run` builds implicit MIR, which holds no lend, so this PR does
    not change that path.
- Backend parity passes, and so does
  `direct_objects_are_identical_across_repeated_builds`.
- `cargo fmt`, the CI clippy command, and `scripts/check-hygiene.sh` pass.
- `aura run` of `print("hi")`, debug builds: 93.61 million instructions
  at `f20e13a2` and 93.61 to 93.62 million at head. The lend check first
  cost 79 thousand. The loan validator now skips it in an implicit
  module, which can hold no lend.
- Release builds, `725acde`, `f20e13a2`, then head, interleaved medians
  over 31 runs:
  - `string_list_scan` 16.7, 17.1, and 18.3 ms;
  - `record_field_reads` 107.7, 114.2, and 110.4 ms;
  - `int_list_sum` 10.7, 11.6, and 11.5 ms;
  - `dict_build_get` 127.9, 136.0, and 131.0 ms;
  - `set_insert_contains` 22.9, 22.9, and 22.1 ms.
  The head binaries of every benchmark but `record_field_reads` are
  byte-identical to `f20e13a2`, so their spread is noise. An earlier run
  of 15 put `record_field_reads` at 136.0 ms on head, with the same
  binary. See the finding above.
- Binary sizes of the five benchmark programs equal `f20e13a2`.
  `record_field_reads` is 1208192 bytes, 4 KiB under `725acde`.
- Coverage meets the floors: lines 96.58%, functions 97.38%, regions
  95.49%.
- Coverage filled the disk to about 1 GB free. Its instrumented build
  takes 7.2 GB, and each test process writes a profile. Folding finished
  profiles into one while the tests ran kept it from running out.

## Follow-Up

- Borrowed-element fusion is an instruction choice that MIR does not
  name. An LLVM emitter can make the same choice from the loan and the
  operand role, or call the general helper.
- Step 8 continues with the next sub-step on the task board.
