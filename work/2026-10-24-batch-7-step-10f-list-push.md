# Batch 7 Step 10f: List Push Fast Path

## Goal

Implement part 10f of the step 10 plan. Store an appended scalar in
generated code when the list has room, and call the runtime only to grow
it. Keep the view and form rules from 10c: a push must reset the view as
every write does. The gate is `int_list_sum` at or under 2 times Rust in
instructions on LLVM.

## Profile

Callgrind on the LLVM build of `ae793458`, which is 10e. `int_list_sum`
appends 100,000 `int64` values and sums them. The run took 42.2 million
instructions, against Rust's 13.2 million.

- Each `append` called `aura_direct_vec_push_scalar`. The call checked
  the box, matched the form, pushed onto the `Vec`, and refreshed the
  view, about 260 instructions an append.
- The sum loop's two million reads already ran in generated code.

## Design

### Room

- `OpaqueValue::list_capacity` is a `u32` count of the elements the
  storage has room for. The box keeps its 192 bytes because the ledger
  count `owned` shrinks from a `usize` to a `u32` and the two share one
  word. A count past `u32::MAX` would need four billion live references
  to one value.
- `DirectListLayout` gives the offset of the room, and the layout
  fingerprint covers it. The ownership fingerprint records the narrower
  count, and the generated retain and release load it as 32 bits.
- Only the view refresh sets the room, and every write refreshes the
  view. The room is nonzero only for a list in the `int64`, `float64`, or
  `bool` form whose storage no other list shares. A `Values` list, a
  `str` view, a shared list, and a new box have no room.

### Generated code

- `append` on a scalar list checks the view's form, then checks that the
  view's length is below the room.
- With room, it stores the element past the view's end, one byte for a
  `bool`, and raises the view's length by one. It makes no call.
- Without room, it calls `aura_direct_vec_push_scalar` as before. That
  call grows the storage or converts the form and refreshes the view, so
  the next append has room again.
- A `str` view has no room, so an append to a list of text still takes
  the runtime path and resets the view to `Values`, as in 10c.

### Taking elements in

- The `Vec`'s own length lags behind the view while generated code
  appends. `OpaqueValue::access` checks the room before it hands out the
  value. When the room is set, a cold helper sets the `Vec`'s length to
  the view's length and clears the room. The next write sets it again.
- So every runtime read, copy, move, and write sees each appended
  element. A copy reads its original through `access`, which takes the
  room back first, and a list whose storage the copy shares has no room
  until its next write gives it storage of its own.
- Sharing a value with another thread takes the elements in first.
- The readers of a `dict`, a `set`, an enum variant, or a boxed scalar
  call `access_unlisted`, which skips the check. Those readers are hot in
  the other programs, and a list handed to one of them only reaches its
  type error.

### Inlining

The first build kept the check inside `access`, and `access` stopped
being inlined into its callers. The other five programs grew by 3% to
6%. Making the helper cold and never inlined, marking `access` inline,
having the readers above skip the check, and folding the room into the
existing form match of the refresh cut that to 0.2% to 0.6%.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `ae793458`.

| Code generator | Program | Before | After | Change |
| --- | --- | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,610,482 | 210,614,467 | 0.0% |
| Cranelift | `int_list_sum` | 73,118,474 | 47,221,868 | -35.4% |
| Cranelift | `string_list_scan` | 57,803,643 | 58,015,200 | +0.4% |
| Cranelift | `dict_build_get` | 376,368,705 | 377,147,071 | +0.2% |
| Cranelift | `set_insert_contains` | 64,765,380 | 64,952,802 | +0.3% |
| Cranelift | `record_field_reads` | 245,156,587 | 245,979,977 | +0.3% |
| LLVM | `fib(30)` | 115,021,413 | 115,022,129 | 0.0% |
| LLVM | `int_list_sum` | 42,212,310 | 16,018,077 | -62.1% |
| LLVM | `string_list_scan` | 32,734,038 | 32,940,137 | +0.6% |
| LLVM | `dict_build_get` | 349,140,385 | 349,918,889 | +0.2% |
| LLVM | `set_insert_contains` | 59,376,515 | 59,571,020 | +0.3% |
| LLVM | `record_field_reads` | 226,264,809 | 227,081,789 | +0.4% |

- `int_list_sum` runs at 1.2 times Rust's 13.2 million on LLVM. The gate
  is 2 times.
- Generated code now takes 15.4 million of the 16.0 million. The runtime
  push runs only when the list grows, about 17 times in the run.
- The other programs pay for the two-instruction check in `access` and
  the room store in the refresh.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 982,472 | 982,472 | 982,472 | 982,472 |
| Reference agent | 1,874,728 | 1,878,824 | 1,890,712 | 1,894,808 |
| JSON | 1,469,320 | 1,469,320 | 1,473,336 | 1,477,432 |
| TLS | 3,278,952 | 3,283,048 | 3,299,176 | 3,299,176 |
| WebSocket | 3,604,664 | 3,608,760 | 3,608,584 | 3,608,584 |

- Each change is one 4,096-byte page or none. The adoption helper and
  the inline append add a little text, and the section's padding takes
  it up in some programs.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs. The second table counts the built executable's
instructions under callgrind, median of 5 runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 34.9, 31.1 | 27.4, 27.2 |
| LLVM | 26.1, 32.1 | 33.3, 29.0 |

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 541,637 | 536,159 |
| LLVM | 538,655 | 539,077 |

- Wall times ranged from 26 to 35 ms with no consistent side, as in 10e.
  The instruction counts vary by about 1% from run to run, and the two
  builds overlap. Hello world appends to no list.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once in the suite run and passed when run again, as in 10e.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10e: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
- `npm run test:llvm` passes, apart from the root-only test, and the
  repeated-build test passes.
- `npm run test:backend-parity` passes in 1063 s, and
  `npm run test:backend-parity:llvm` passes in 1055 s.
- A new runtime test appends to `int64`, `float64`, and `bool` lists the
  way generated code does. It checks that most appends go in place, that
  a full list grows through the runtime, and that the next access sees
  every element and takes the room back.
- A second runtime test checks that a copy sharing storage with its
  original has no room, that each list's next write gives it room again,
  that sharing a value with another thread takes its elements in and
  leaves it no room, and that a `Values` list has no room.
- A new compiler test checks that `append` on `int64` and `bool` lists
  still references `aura_direct_vec_push_scalar` for a full list and
  never `aura_direct_vec_push_in_place`. It checks presence and absence,
  not relocation counts.
- The new run-pass fixture `list_push_in_place.au` appends to `int64`,
  `float64`, and `bool` lists past several growths. It appends to a
  slice copy without touching the original, mixes appends with `pop` and
  negative indexes, and appends to lists inside a list. It passes in
  parity on both code generators, and its output matches the 10e build
  and Python.
- The coverage run needs more build space than the container could spare
  while keeping 8 GB free, so it did not run for this step.

## Follow-Up

- `insert` and `extend` on a scalar list still call the runtime.
- A list of `str` could take the same path if its view kept room for
  text handles, but the `str` view would then need its own rule for when
  the storage owns the text.
