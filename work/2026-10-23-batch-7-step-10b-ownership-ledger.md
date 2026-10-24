# Batch 7 Step 10b: Ownership Ledger

## Goal

Implement part 10b of the step 10 plan. Replace each task's hash map of
owned references with an intrusive list, then emit retain and release
inline on both code generators. A task that fails or is cancelled must
still free every value it held. The plan's target is 25% fewer
instructions for `record_field_reads` and `dict_build_get`.

## Design

### The ledger

- `DirectOwnedValues` in `native_owned_values.rs` lives in the task's
  `DirectFrameStack`. The stack's address names the task.
- `OpaqueValue` gains four fields: `owner`, `owned`, `prev`, and `next`.
  A value confined to its thread links into the ledger of the first task
  that owns it. `owner` names that task's stack, and `owned` counts the
  references the task holds. Adding and removing a reference is O(1) and
  needs no hashing.
- A shared value never links, because another thread may read its
  header. Neither does a value another task's ledger already links. The
  ledger counts those references by address in a small map.
- `share_direct_value` moves a linked value's count into that map, and
  unlinks the value, before it publishes the lock. So a linked value is
  always confined, and only its own thread reads the four fields.
- A value freed while linked leaves the list in its `Drop`. A ledger that
  is dropped forgets its links without releasing, so no value names a
  freed stack.
- A failed or cancelled task's teardown takes every entry with
  `take_all` and releases each one its count of times, as before.
- While the representation counters count ownership, no value links. Then
  every retain and release reaches the runtime, and the counts stay exact.
- The runtime finds the current stack through the scheduler slot that 10a
  added. A separate cold function looks the task up when the slot is empty.

### Generated code

- Each module defines two local functions, `aura_owned_retain` and
  `aura_owned_release`. LLVM marks them `alwaysinline`. Cranelift calls
  them. The runtime table names them in place of the runtime's exports,
  so every existing call site and every pass that rewrites those calls
  keeps working.
- Both check, in order: the value is not null, the lock word is zero, and
  `owner` is the running task's stack. Another thread may write a shared
  value's header, so `owner` is read only once the lock word shows the
  value is confined.
- Retain adds one to the count and to `owned` unless the count is full.
- Release subtracts one from both while both stay above one. When the
  ledger holds the last reference it links, release calls the new
  `aura_direct_release_last_owned_value`. It unlinks the value without a
  lookup, and frees it when its count reaches zero.
- Every other case calls `aura_direct_retain_value` or
  `aura_direct_release_value`, in a cold block.
- `runtime_abi.rs` takes the offsets of `DirectOwnershipLayout` from the
  runtime. The main wrapper passes its fingerprint to
  `aura_direct_check_ownership_layout` at startup.

### The box cache

- A second commit adds `native_box_cache.rs`, a per-thread list of up to
  256 freed boxes. Boxing a value takes one from the list before it calls
  the allocator. glibc keeps only seven blocks of each size in its own
  per-thread cache, so most frees in these programs went to its slower
  bins.
- The cache is a separate commit so it can be reviewed or dropped on its
  own. It changes no semantics.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `fb4caf82`, which is 10a. Ledger is the first commit
alone, and Cache adds the box cache. The method matches 10a.

| Code generator | Program | Before | Ledger | Cache | Ledger change | Cache change |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,620,234 | 210,615,352 | 210,615,361 | 0.0% | 0.0% |
| Cranelift | `int_list_sum` | 73,114,035 | 73,121,391 | 73,116,931 | 0.0% | 0.0% |
| Cranelift | `string_list_scan` | 150,922,502 | 147,641,494 | 145,123,600 | -2.2% | -3.8% |
| Cranelift | `dict_build_get` | 940,411,061 | 875,273,027 | 785,861,944 | -6.9% | -16.4% |
| Cranelift | `set_insert_contains` | 148,897,021 | 144,245,014 | 141,932,235 | -3.1% | -4.7% |
| Cranelift | `record_field_reads` | 693,162,094 | 640,871,858 | 588,291,778 | -7.5% | -15.1% |
| LLVM | `fib(30)` | 115,028,528 | 115,032,275 | 115,026,843 | 0.0% | 0.0% |
| LLVM | `int_list_sum` | 42,212,925 | 42,219,042 | 42,211,215 | 0.0% | 0.0% |
| LLVM | `string_list_scan` | 137,548,742 | 133,436,344 | 130,914,297 | -3.0% | -4.8% |
| LLVM | `dict_build_get` | 935,257,929 | 854,220,873 | 764,796,072 | -8.7% | -18.2% |
| LLVM | `set_insert_contains` | 144,648,296 | 138,854,516 | 136,518,947 | -4.0% | -5.6% |
| LLVM | `record_field_reads` | 690,486,333 | 623,169,779 | 570,595,535 | -9.7% | -17.4% |

- The plan asked for 25% fewer instructions in `dict_build_get` and
  `record_field_reads`. The ledger alone gives 9% and 10% on LLVM, and
  the cache brings them to 18% and 17%. Both miss the target.
- The plan overestimated what the ledger cost. Before 10b, registering a
  value and the runtime boundary around retain and release took about 15%
  of each run: 151 million instructions in `dict_build_get` and 104
  million in `record_field_reads`. Removing all of it could not reach
  25%.
- After 10b, registering a box costs about 41 instructions, and releasing
  its last reference about 50. The rest of each run is allocation,
  cloning, hashing, and type tags. `aura_direct_tag_value_type` alone
  takes 45 million instructions in `record_field_reads`. 10d and 10e
  target those.
- Nearly every release in these programs drops the last reference. So
  the inline release rarely subtracts in place, and the lean call to
  `aura_direct_release_last_owned_value` does most of the work. That call
  is not marked cold for this reason.
- Before the cache, `malloc` and `free` cost about 63 and 96 instructions
  a call in `record_field_reads`. glibc's per-thread cache holds seven
  blocks of each size, and these programs free boxes in bursts.
- On LLVM, `fib(30)` and `int_list_sum` are unchanged. Their hot paths
  hold no boxes.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 978,056 | 982,488 | 978,056 | 982,488 |
| Reference agent | 1,870,712 | 1,874,992 | 1,833,800 | 1,891,328 |
| JSON | 1,464,480 | 1,468,904 | 1,460,384 | 1,477,096 |
| TLS | 3,267,240 | 3,271,520 | 3,250,792 | 3,287,840 |
| WebSocket | 3,597,136 | 3,601,416 | 3,588,880 | 3,601,352 |

- LLVM inlines both fast paths at every retain and release, which adds
  3% to the reference agent. Cranelift calls them, so it grows by about
  4 KB.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 23.7, 25.3 | 24.0, 23.3 |
| LLVM | 24.5, 24.6 | 23.9, 24.4 |

- The differences are within this container's noise.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10a: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once in the full run and passed alone.
- `npm run test:llvm` passes, apart from the root-only test.
- `npm run test:backend-parity` passes in 988 s, and
  `npm run test:backend-parity:llvm` passes in 1022 s.
- The new runtime test starts a task that holds values in every form the
  ledger records, then traps. A second task does the same and is
  cancelled. Each box holds a reference to one type name, and the test
  checks that the name's count returns to where it started. Making
  teardown skip linked values fails the test.
- Unit tests cover linking, unlinking in any order, shared values, values
  another task links, overflow, freeing a linked value, a dropped ledger,
  and the generated code's rules against `DirectOwnershipLayout`.
- Coverage is 96.57% of lines, 97.45% of functions, and 95.52% of
  regions, above the floors. `native_box_cache.rs` is fully covered, and
  `native_owned_values.rs` covers 99.7% of its lines. The run merged
  profiles per binary, as in 10a.

## Follow-Up

- `aura_direct_tag_value_type` interns each type name on every call. 10d
  replaces it with static ids.
- Registering a fresh box could link it without the checks a retain
  needs, which would save a few instructions per box.
- The box cache keeps up to 256 boxes per thread, about 48 KB. A worker
  that stops frees its cache.
