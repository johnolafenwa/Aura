# Batch 7 Step 10e: Scalar-Key Hashing

## Goal

Implement part 10e of the step 10 plan. Keep the keyed SipHash hasher,
which the owner approved as the default. Hash a scalar key with one write
that still agrees across integer widths when the values compare equal.
Add direct `int64` key probes for sets and maps. The gate is
`set_insert_contains` at or under 3 times Rust in instructions on LLVM.
Equal integers of different widths must still meet, and iteration order
must not change.

## Profile

Callgrind on the LLVM build of `a15db860`, which is 10c.
`set_insert_contains` adds 20,000 `int64` values to a set and makes
200,000 `in` tests. The run took 135 million instructions, against Rust's
31.6 million.

- Hashing took 81 million instructions, 60% of the run. `hash_value`
  wrote each `int64` key in three writes: the variant's discriminant, a
  sign byte, and a 128-bit magnitude. Each write is an out-of-line call
  into the SipHash state, about 68 instructions, and `hash_value` itself
  took about 135 more. That is 340 instructions a hash.
- An `add` hashed its key twice: once in `contains_key`, and again in
  `push_new` to index the new entry. There were 240,000 hashes for
  220,000 operations.
- `add` boxed its element with `aura_direct_box_i64` before the insert,
  about 280 instructions. The insert itself then took about 1,350
  instructions, and an `in` test about 400.
- `in` already passed its needle unboxed, but built a `Value` for it on
  the stack and compared candidates with `Value` equality.

`dict_build_get` showed the same pattern for `str` keys: a discriminant
write before the bytes, and a second hash on every insert.

## Design

### One write

- A scalar or `str` key, the common case, takes a direct path in
  `hash_value`. Unions are stripped first, as before.
- An integer writes its value as one `i64` when it fits, and as its 128
  bits otherwise. The word depends on the value alone, never on the
  width or the representation, so an `int8`, an `int32`, a `uint64`, and
  an untyped literal with the same value write the same word.
- A `float` writes its bits, with `-0.0` as `0.0`. A `bool` writes one
  byte. A `str` writes its bytes.
- The direct path writes no discriminant. Equal values are always of one
  kind, so each kind only needs to agree with itself. Keys of different
  kinds that write the same word only share a bucket, and equality still
  tells them apart.
- Every other key takes the structural walk, which is unchanged. A
  scalar inside a tuple or a list still hashes through it, so the two
  paths never need to agree with each other.
- `hash_str` and the new `hash_i64` write the same words as the direct
  path, so a probe by text or by `int64` finds what a probe by value
  finds.

### One hash an insert

- `IndexedEntries::probe` finds a key and returns the hash it computed. A
  small collection scans and hashes nothing, as before.
- `SetElements::insert` and `DictEntries::insert` pass that hash to the
  push, so a new key is hashed once.

### `int64` probes

- `position_i64` hashes the word with `hash_i64` and compares it with
  stored keys of any integer width without building a value.
  `value_of_i64` and `insert_i64` build on it. `insert_i64` builds the
  `Value` only when the key is new.
- MIR picks `Set.AddInt64` for `add` on a `set[int64]`, beside the
  existing `Set.ContainsInt64`. It picks `Dict.GetInt64` and
  `Dict.ContainsKeyInt64` for `get` and `in` on a dict with `int64` keys.
  Each unspecializes to the public op, so the checker, the MIR
  interpreter, and the validator see the op they saw before.
- The new runtime entries are `aura_direct_set_insert_int64_in_place`,
  `aura_direct_map_get_int64`, `aura_direct_map_get_int64_payload`, and
  `aura_direct_map_contains_int64`. A `get` kept as a payload slot uses
  the payload form, as `GetStr` does.
- A `get` on a dict that is a lent list element keeps the boxed key, as
  any non-literal key did before.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `a15db860`.

| Code generator | Program | Before | After | Change |
| --- | --- | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,616,106 | 210,610,482 | 0.0% |
| Cranelift | `int_list_sum` | 73,112,038 | 73,118,474 | 0.0% |
| Cranelift | `string_list_scan` | 57,807,819 | 57,803,643 | 0.0% |
| Cranelift | `dict_build_get` | 403,287,563 | 376,368,705 | -6.7% |
| Cranelift | `set_insert_contains` | 140,469,111 | 64,765,380 | -53.9% |
| Cranelift | `record_field_reads` | 245,590,358 | 245,156,587 | -0.2% |
| LLVM | `fib(30)` | 115,030,190 | 115,021,413 | 0.0% |
| LLVM | `int_list_sum` | 42,215,750 | 42,212,310 | 0.0% |
| LLVM | `string_list_scan` | 32,736,019 | 32,734,038 | 0.0% |
| LLVM | `dict_build_get` | 376,068,275 | 349,140,385 | -7.2% |
| LLVM | `set_insert_contains` | 135,092,104 | 59,376,515 | -56.0% |
| LLVM | `record_field_reads` | 226,694,034 | 226,264,809 | -0.2% |

- `set_insert_contains` runs at 1.9 times Rust's 31.6 million on LLVM.
  The gate is 3 times.
- A hash now takes about 146 instructions: 90 in `hash_i64` and one
  56-instruction write. An `in` test takes about 210, and an `add` about
  470, including the index's growth.
- `dict_build_get` drops 7.2% on LLVM, from the single write for `str`
  keys and the single hash on insert.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 982,472 | 982,472 | 982,472 | 982,472 |
| Reference agent | 1,874,728 | 1,874,728 | 1,890,712 | 1,890,712 |
| JSON | 1,469,320 | 1,469,320 | 1,473,336 | 1,473,336 |
| TLS | 3,278,952 | 3,278,952 | 3,299,176 | 3,299,176 |
| WebSocket | 3,604,664 | 3,604,664 | 3,608,584 | 3,608,584 |

- No size moved. `hash_value` is 80 bytes smaller, and the new helpers
  are dropped from programs that do not call them, but the text section's
  padding absorbs both.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs. The second table counts the built executable's
instructions under callgrind, median of 5 runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 34.9, 31.4 | 27.6, 24.8 |
| LLVM | 30.4, 34.5 | 32.1, 35.5 |

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 538,312 | 537,981 |
| LLVM | 536,222 | 539,470 |

- The container was busy during these runs, and wall times ranged from
  25 to 35 ms with no consistent side. The instruction counts vary by
  about 1% from run to run, and the two builds overlap. Hello world
  builds no `dict` or `set`.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once in the suite run and passed when run again.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10c: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
- `npm run test:llvm` passes, apart from the root-only test.
- `npm run test:backend-parity` passes in 1080 s, and
  `npm run test:backend-parity:llvm` passes in 1072 s.
- New hashing tests check that equal integers of every width and both
  representations hash alike, including values past `i64`, and that
  `hash_i64` matches `hash_value`.
- New probe tests check `position_i64` against `position` for sets and
  dicts whose keys mix widths, below and above the scan threshold, and
  that `insert_i64` keeps insertion order and refuses an equal key of
  another width.
- New runtime tests check `aura_direct_set_insert_int64_in_place` against
  boxed inserts, including `i64::MIN`, `i64::MAX`, and repeats, and the
  three map helpers against boxed lookups on `int32` keys.
- A new compiler test checks that `add`, `in`, and `get` with `int64`
  keys reference the unboxed helpers and not the boxed ones. It checks
  presence and absence, not relocation counts.
- The new run-pass fixture `int64_keyed_collections.au` adds, finds, and
  orders `int64` set elements and dict keys, including the extremes and a
  dict inside a list. It passes in parity on both code generators, and its
  output matches the 10c build and Python.
- Iteration order is unchanged: `hashed_dict_and_set_order.au` and the
  order tests pass, and entries still live in insertion order.
- The coverage run needs more build space than the container could spare
  while keeping 8 GB free, so it did not run for this step.

## Follow-Up

- The SipHash state still takes an out-of-line write and a full finish
  for one word. A one-word SipHash-1-3 with the same keys would cut a
  hash to about 60 instructions, but `RandomState` does not expose its
  keys.
- `d[k]` and `d[k] = v` on a dict with `int64` keys still box the key.
