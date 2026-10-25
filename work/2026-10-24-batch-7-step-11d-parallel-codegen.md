# Batch 7 Step 11d: Parallel Codegen Units

## Goal

Bring a cold `aura build` of the reference agent to 1.5 s or less, the
phase 2 target in checkpoint section E, without slowing generated code.
Step 11 measured 1.89 s. LLVM's `default<O3>` passes took 0.61 to 0.70 s
and code generation 0.51 to 0.58 s, all on one thread.

## Design

`aura build` now splits a large program into codegen units, as rustc
does. Threads optimize and write the units in parallel, and `cc` links
their objects.
[`native_backend_llvm_units.rs`](../crates/aura-compiler/src/native_backend_llvm_units.rs)
holds the split.

- **Weight.** A function weighs the number of instructions the emitter
  recorded for it. The split reads nothing else.
- **One unit for small programs.** A module under 4,000 stays one unit.
  That unit is the exact object `finish` writes, so small programs build
  the same executables as before.
- **Units.** A larger module gets one unit per 3,000, from 2 to 16. Each
  unit takes a run of consecutive functions in declaration order. A
  function belongs to the unit its first instruction falls in. Data
  belongs to the first unit that refers to it, or to unit 0.
- **Cross-unit symbols.** A local function or data object that another
  unit refers to becomes a hidden global in its own unit, and a hidden
  declaration in the others. The executable exports nothing new, and
  `strip -x` still drops the names.
- **Copies.** A unit that calls a function of at most 64 instructions in
  another unit, or one marked for inlining, gets an `available_externally`
  copy of it, so LLVM can still inline it. A unit that reads constant data
  of another unit gets an `available_externally` copy, so LLVM can still
  fold the loads. LLVM drops every copy before it writes the object.
  Writable data is never copied, so each mutable object exists once.
- **Threads.** `unit_objects` runs up to `available_parallelism` threads,
  the calling thread included. Each takes the next unit, largest first,
  and lowers, optimizes, and writes it in its own LLVM context. Objects
  return in unit order.
- **Determinism.** The split depends only on the module, never on the
  host's cores. A unit's object depends only on the unit. So the bytes do
  not depend on scheduling or on the thread count.
- **Interface.** `ModuleBackend::finish_units` writes the units, and
  `emit_host_objects_with_metadata` returns their objects.
  `emit_host_object` and `finish` still write one object, which the
  emitter tests read. `aura build` writes each object to its own temporary
  file and passes them to `cc` in unit order.
- **Optimizer switch.** `without_optimization` is a thread-local switch in
  tests. `unit_objects` reads it on the calling thread and passes it to
  the workers.

The reference agent weighs 21,279 and splits into 8 units. Hello world
weighs 109. The six phase 2 benchmark programs weigh 311 to 2,108, so they
stay one unit. `unix_tls_roundtrip` weighs 6,600 and splits into 3, but
one function holds 4,179 of that.

### Cheap Wins Tried

- **Cold functions.** The emitter marks blocks cold, never whole
  functions, so no function can skip `O3` on that basis.
- **Redundant passes.** The pipeline is one `default<O3>`. Nothing else
  runs, and step 11 showed `O1` alone would not reach the target. Nothing
  was cut.
- **Copy limit.** With 4 or 8 units, copy limits of 0, 16, and 64
  instructions built the reference agent in the same time, within noise,
  and gave the same instruction counts. 64 keeps the most inlining.

## Measurements

The four-core Linux x86-64 container, Rust 1.95.0, LLVM 22.1.2. Before is
release `aura` at `c65c35f7`. Both used one runtime archive.

### Build Time

`scripts/bench-compile-time.py --repetitions 10`, the before and after
runs back to back. Cold builds, in seconds:

| Program | Before median | After median | After range |
| --- | ---: | ---: | ---: |
| Hello world | 0.410 | 0.404 | 0.357 to 0.764 |
| Reference agent | 1.908 | 0.960 | 0.895 to 1.060 |
| `unix_tls_roundtrip` | 1.069 | 0.927 | 0.818 to 1.459 |

- The reference agent meets the 1.5 s target with room. A first before
  run gave 1.865 s.
- Hello world builds the same object as before, so the difference is
  noise. It meets its 0.6 s target.
- A timed experimental build put LLVM's work on the reference agent at
  1.22 s in one unit. In 8 units, each unit took 45 to 110 ms of passes
  and 70 to 130 ms of code generation. Summed over the units, the work
  matched the one-unit build, so the threads split it rather than add to
  it.
- With 4, 6, 8, 12, or 16 units the reference agent built in 0.9 to
  1.1 s. The rest of the build is the front end, the emitter, and the
  link, which units do not change.
- A cold build also copies the 63 MB runtime archive to a temporary file
  before it links. On this container that write takes about 0.27 s, in
  every cold build, hello world included. It is the next lever and is left
  alone here.

### Runtime

Callgrind instructions for one run, startup included. `fib30` reads its
`GO` line first.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `fib(30)` | 115,033,242 | 115,025,097 | 0.0% |
| `int_list_sum` | 16,019,822 | 16,015,568 | 0.0% |
| `string_list_scan` | 32,937,912 | 32,942,620 | 0.0% |
| `dict_build_get` | 344,783,971 | 344,784,267 | 0.0% |
| `set_insert_contains` | 59,563,622 | 59,570,448 | 0.0% |
| `record_field_reads` | 135,095,156 | 135,096,748 | 0.0% |
| Reference agent | 3,718,132 | 3,720,025 | 0.1% |

- The six programs stay one unit, and their executables are
  byte-identical before and after. The differences are callgrind's own
  noise.
- The reference agent splits, and prints its pinned output.
- To see what a split costs, an experimental build forced the six
  programs into 2 and 4 units. Their counts moved by at most 0.15%, in
  `set_insert_contains`. Their hot loops sit in one function and call the
  runtime.

### Sizes

The reference agent grows by 18,040 bytes, 1.0%, and the TLS subject by
6,496 bytes, 0.2%. A function another unit calls stays out of line, where
one module could inline it into its only caller and drop it. Every subject
stays inside the 10% size gate.

## Tests

- `unit_counts_follow_the_recorded_weight_alone` pins the unit count.
- `small_modules_write_the_one_object_finish_writes`: a small module's one
  unit is `finish`'s object.
- `large_modules_split_into_runs_of_consecutive_functions` checks the runs
  and data ownership.
- `units_reach_each_other_through_hidden_symbols_and_copy_small_code`
  reads each unit's IR: hidden definitions and declarations across units,
  internal functions within one, and `available_externally` copies of the
  small function, the inline one, and constant data, never writable data.
- `unit_objects_join_at_link_time_whatever_the_thread_count` compares the
  objects from 1 and 4 threads and from `finish_units`, and checks that
  each symbol a unit leaves undefined another unit defines once.
- `unoptimized_unit_objects_keep_every_function` checks that the optimizer
  switch reaches the worker threads.
- `large_programs_emit_several_objects_and_small_programs_one` checks the
  emitter: hello world is one object equal to `emit_host_object`'s, and a
  generated 40-function program splits into disjoint objects with one
  `main`, the same on a repeated build.
- `reference_tool_runner_package_has_pinned_output_on_both_backends` now
  builds the reference agent twice and, on Linux, checks the two
  executables are identical. On macOS, ld64 folds each object's temporary
  path into the executable's UUID and names the output file in its ad hoc
  signature, so the bytes differ there by design. The backend tests check
  the units' objects on every host.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, and
  `scripts/check-hygiene.sh` pass.
- The whole workspace passes, 4,026 tests, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- `npm run test:backend-parity` passes in 979 s.
- The size gate passes. Hello world is 982,472 bytes, the reference agent
  1,912,848, the JSON subject 1,477,432, the TLS subject 3,305,672, and the
  WebSocket subject 3,608,584. No subject links LLVM.
  `scripts/check-aura-libraries.py` passes.
- The release `aura` built from the committed code links the reference
  agent and TLS executables byte-identical to the ones measured above.
- Coverage meets its floors: lines 96.51% (floor 96.46), functions
  97.36% (floor 97.33), and regions 95.45% (floor 95.23). The run used
  `--no-fail-fast`, so the root-only failure did not stop it.
  `native_backend_llvm_units.rs` covers 98.87% of its lines and every
  function.
- Not verified here: macOS. Mach-O hidden symbols and
  `available_externally` work the same way in LLVM, and the native
  binaries CI job builds the size gate subjects on macOS arm64.
