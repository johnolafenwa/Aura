# Batch 7 Step 10a: Frame Stack Fast Path

## Goal

Implement part 10a of the step 10 plan. Generated code records each Aura
call in its task's frame stack without a runtime call, on Cranelift and
LLVM. Traces, task ancestry, and the depth limit must stay exact, and
`fib(30)` must stay at or under 3 times Rust in instructions.

## Design

### The frame stack

- `DirectFrameStack` in `native_frame_stack.rs` is `#[repr(C)]`. Its
  header holds `len`, `limit`, `floor`, `flags`, and `entries` at offsets
  0, 8, 16, 24, and 32. Generated code reads only the header.
- Each task's runtime state allocates one stack and frees it when the task
  ends. The stack keeps its address for the task's whole life.
- An entry is a descriptor's address. A frame record the runtime builds,
  such as a task's entry frame, is boxed and stored with its low bit set.
- The first 64 entries live inside the stack. A push past them calls the
  runtime, which doubles the entries into a heap block and keeps it, so
  later deep recursion does not allocate again.
- `limit` is the smaller of the capacity and `floor + 256`. A push at the
  limit calls the runtime, which raises `AU4001` exactly as before.
- `floor` replaces the separate call depth. Cleanup that runs during a
  trap sets `floor` to `len`, so its calls count their own depth and never
  pop the trapped calls. The depth is `len - floor`.
- `flags` is nonzero while a call hands mutable sinks to its callee, while
  a returned-view record is live, or while a boxed record is on the stack.
  The runtime refreshes it after each such change. Every push and pop
  then takes the runtime path, which keeps the sink and view rules in one
  place.

### Generated code

- A function's entry calls `aura_direct_frame_stack`, loads the header,
  and stores its descriptor at `entries[len]` when `len < limit` and
  `flags == 0`. Otherwise it calls `aura_direct_enter_frame`.
- A return subtracts one from `len` when `len > floor` and `flags == 0`.
  Otherwise it calls `aura_direct_exit_call`.
- The emitter writes both paths once, through `FunctionBackend`, so
  Cranelift and LLVM share them.
- Two backend hints are new. `ModuleBackend::mark_task_invariant` gives
  the accessor `memory(none)`, `nounwind`, and `willreturn` on LLVM, so
  LLVM merges a function's accessor calls into one. `set_cold_block`
  marks the runtime paths cold: Cranelift places them last, and LLVM
  weighs the branch 1 to 2000.
- `runtime_abi.rs` takes the header offsets from the runtime. The main
  wrapper passes the layout's fingerprint to
  `aura_direct_check_frame_stack_layout` at startup, as it does for lists.

### The accessor

- The scheduler keeps the current task's stack in a thread-local slot. It
  clears the slot whenever it enters or leaves a task, and whenever the
  task table changes. The accessor reads the slot and falls back to a
  separate cold function that looks the task up and fills it.
- The plan asked for a reload after any call that can yield. That turned
  out to be unnecessary. A task's stack never moves, and a task resumes
  only on its own frames, so every accessor call within one function
  returns the same address. LLVM may merge the calls across a yield, and
  the task-switch tests confirm it.

### Startup validation

- The old runtime push checked each descriptor's UTF-8 on its first call
  and cached the result in the descriptor. An inline push cannot afford
  that check.
- The main wrapper now emits `aura_frame_descriptors`, a table of every
  descriptor's address, and passes it to
  `aura_direct_validate_frame_descriptors` before the first call. The
  inline push then stores only validated descriptors.
- A corrupt descriptor now traps at startup instead of at its function's
  first call. The compiler always writes valid UTF-8, so only forged
  objects see the difference.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds of `d188a35a` and of this change. The method matches 9c.

| Program | Cranelift before | Cranelift after | LLVM before | LLVM after | Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 522,952,331 | 210,620,034 | 483,919,495 | 115,032,693 | 39.5 M |
| `int_list_sum` | 73,117,854 | 73,117,019 | 42,212,477 | 42,220,324 | 13.2 M |
| `string_list_scan` | 150,828,113 | 150,910,395 | 137,470,454 | 137,559,234 | 13.1 M |
| `dict_build_get` | 942,020,543 | 940,408,179 | 936,860,479 | 935,263,992 | 179.0 M |
| `set_insert_contains` | 148,267,110 | 148,869,562 | 144,529,995 | 144,674,980 | 31.6 M |
| `record_field_reads` | 693,083,697 | 693,165,470 | 690,499,528 | 690,485,244 | 36.0 M |

- `fib(30)` on LLVM drops 76%, to 2.91 times Rust. On Cranelift it drops
  60%, to 5.3 times Rust.
- The other five programs move by under 0.5%. Their time is in runtime
  collection and ownership calls, which later parts of step 10 target.
- On LLVM, `fib(30)` spends about 98 million instructions in generated
  code and 15 million in the accessor. The accessor still saves and
  restores a register around its thread-local read. The linker relaxes
  the general dynamic access but keeps that alignment code.
- Cranelift cannot merge the two accessor calls in each function, so they
  cost 43 million instructions in `fib(30)`.
- The plan expected 2.37 times Rust from its prototype. The first full
  version reached 3.39 times Rust: its accessor kept a cache keyed by
  task and read two thread-locals per call. The scheduler slot and
  startup validation brought it to 2.91.
- Wall times are noisy here. `fib(30)` on LLVM went from about 48 ms to
  about 19 ms, against 6 to 10 ms for Rust.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 977,632 | 978,056 | 977,632 | 978,056 |
| Reference agent | 1,853,904 | 1,870,712 | 1,815,424 | 1,833,800 |
| JSON | 1,464,048 | 1,464,480 | 1,459,728 | 1,460,384 |
| TLS | 3,266,816 | 3,267,240 | 3,245,928 | 3,250,792 |
| WebSocket | 3,592,608 | 3,597,136 | 3,588,336 | 3,588,880 |

- The inline paths add about 1% to the reference agent, which has the
  most Aura functions.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 29.5, 31.6 | 28.0, 23.9 |
| LLVM | 27.9, 26.9 | 29.2, 27.5 |

- The differences are within this container's noise.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, the compiler
  library, all 59 compiler test targets, and all 14 CLI test targets
  besides parity pass, with these exceptions:
  - the same eleven emitter tests as in 9b and 9c, which pin Cranelift's
    object shape, and the root-only test above;
  - `native_run_cache_keys_on_the_code_generator`, which expects the
    default code generator to be Cranelift. Its fixture run inherits
    the variable, so it fails under any ambient `llvm` setting.
- `npm run test:llvm` passes, apart from the root-only test. That
  includes the repeat-build test on LLVM.
- `npm run test:backend-parity` passes in 994 s, and
  `npm run test:backend-parity:llvm` passes in 1007 s.
- The new CLI tests check exact traces and ancestry while 20 tasks yield
  at every level of their recursion, on MIR and direct. The direct
  depth-limit test checks that a task traps at its own 256th call while
  40 other tasks are deep in theirs.
- The runtime's stress test runs 200 tasks to depth 100, yields at every
  level, and checks each task's trace and its stable stack address
  mid-yield.
- Coverage is 96.56% of lines, 97.46% of functions, and 95.50% of
  regions, above the floors. `native_frame_stack.rs` is fully covered.
  This container lacks the disk for one profile per process, so the run
  merged profiles per binary with `LLVM_PROFILE_FILE` set to
  `wt-9a-%4m.profraw`. The tests and the report are the same as
  `npm run coverage:compiler:check`.

## Follow-Up

- Cranelift could share one accessor call between push and pop by
  keeping the address in a variable. Step 11 removes Cranelift, so this
  is not worth doing.
- The accessor's thread-local read could drop its register save with the
  initial-exec TLS model, which stable Rust does not offer for a
  `thread_local!`.
- The MIR interpreter runs out of task stack in the deep task test and
  traps with `AU4005` before the depth limit. So the depth-limit task test
  runs on the direct backend only.
