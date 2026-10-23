# Batch 7 Step 9c: LLVM Tuned and Measured

## Goal

Implement part 9c of the step 9 plan. Pick LLVM's pass pipeline from
measurements, give LLVM objects per-function sections, keep LLVM objects
repeatable, and run the size gates on LLVM. Steps 10 and 11 then have
numbers to start from. The raw reports are in
`2026-10-23-batch-7-step-9c-measurements/`.

## Design

### Pipeline

- LLVM now runs `default<O3>`. The code generator keeps LLVM's `Default`
  code generation level.
- `O1`, `O2`, and `O3` build the same executable for four of the six phase
  targets. `fib30` gets one executable at `O1` and another at `O2` and
  `O3`, with the same instruction count. Only `int_list_sum` gains: `O3`
  runs it in 22% fewer instructions than `O2`.
- The gain comes from loop unswitching, which only `O3` does. The sum loop
  checks the list's inline layout on every element. `O3` moves the check
  out of the loop, and the loop shrinks to a load, an add with an overflow
  check, and the loop branch.
- `O3` costs 0.3% more `aura` instructions than `O2` on the reference
  agent and 0.1% more on `json_codec_derived`. Both are within wall-clock
  noise. The reference agent builds in 1.27 s, inside the phase 2 target
  of 1.5 s.
- So `O3` is the default: it is the only level with a measured runtime
  gain, and its compile cost is too small to see.

### Per-function sections

- After the passes run, every defined function on ELF gets its own section
  named `.text.<symbol>`. `--gc-sections` can then drop functions nothing
  calls, as it does with Cranelift's `per_function_section`. Placing the
  sections after the passes also covers any function the passes create.
- The code calls `LLVMSetSection` directly. inkwell's `set_section`
  prefixes a comma on a macOS host, for Mach-O's `segment,section` form,
  so the ELF test failed there with `,.text.first`.
- Cranelift names every one of its sections `.text.subsection`. LLVM uses
  the symbol, as rustc does. The linker treats both the same.
- On Mach-O, Cranelift puts every function in one `__text` section and
  marks the object `MH_SUBSECTIONS_VIA_SYMBOLS`, so `-dead_strip` works
  per symbol. LLVM marks its Mach-O objects the same way, so LLVM needs no
  change there.
- Two unit tests build objects for the four host triples and inspect them
  with the `object` crate:
  - `elf_objects_put_every_function_in_its_own_section` failed before the
    change, finding `.text` where it expected `.text.first`.
  - `mach_o_objects_are_split_by_symbol` checks the flag and the one
    shared section.
- The sections add 389 KB to the 629 dumped fixture objects at `O1`, from
  6.11 MB to 6.50 MB. Each section brings its own header and relocation
  table.

### Determinism

- `npm run test:llvm` now also runs
  `direct_objects_are_identical_across_repeated_builds` with
  `AURA_NATIVE_CODEGEN=llvm`. That test builds every third run-pass
  fixture twice in one process and compares the objects. It passes.
- Two separate dumps of all 629 fixture objects at `O3` match byte for
  byte.

### Size gates

- `scripts/check-native-binaries.py` passes on LLVM at every level, with
  the Cranelift baselines. All three levels link the same executables.
- The `native-binaries` CI job now installs LLVM and builds one release
  `aura` with the `llvm` feature. It runs the gate once on Cranelift and
  once with `AURA_NATIVE_CODEGEN=llvm`. The feature does not change what
  Cranelift emits. The extra cost is installing LLVM, which Linux
  caches, and about 80 s more release build, as 9b measured.
- `scripts/bench-binary-size.py` builds `aura` from clean git refs and
  clears `AURA_*` variables, so it cannot measure LLVM without changes.
  It also needs 25 GiB free, which this container lacks. The native
  binaries gate covers its two subjects. `npm run test:bench-binary-size`
  passes.

## Measurements

The tables are medians of the runs the README lists. The container is
noisy: identical executables differ by up to 40% in median time. Callgrind
counts settle what the times leave open.

### Runtime

Time in milliseconds, 51 rounds. `fib(30)` times GO to DONE. The rest
time the whole process.

| Program | Cranelift | LLVM `O1` | LLVM `O2` | LLVM `O3` | Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 48.48 | 43.76 | 39.68 | 55.83 | 5.10 |
| `int_list_sum` | 12.34 | 11.45 | 10.95 | 9.87 | 4.09 |
| `string_list_scan` | 20.68 | 16.50 | 17.71 | 17.25 | 4.22 |
| `dict_build_get` | 132.41 | 137.31 | 126.38 | 124.26 | 25.31 |
| `set_insert_contains` | 25.51 | 24.07 | 31.17 | 23.48 | 7.37 |
| `record_field_reads` | 94.93 | 95.48 | 100.51 | 99.28 | 11.50 |

- `fib(30)` at `O2` and `O3` is the same executable, 39.68 and 55.83 ms.
  `set_insert_contains` at all three levels is one executable, 23.48 to
  31.17 ms. The differences between LLVM levels in this table are noise,
  except for `int_list_sum` at `O3`.

Instructions in millions, one run each, startup included:

| Program | Cranelift | LLVM `O1` | LLVM `O2` | LLVM `O3` | Rust | `O3` / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 523.0 | 483.9 | 483.9 | 483.9 | 39.5 | 12.3x |
| `int_list_sum` | 73.1 | 54.2 | 54.2 | 42.2 | 13.2 | 3.2x |
| `string_list_scan` | 150.8 | 137.5 | 137.5 | 137.5 | 13.1 | 10.5x |
| `dict_build_get` | 942.0 | 936.9 | 936.9 | 936.9 | 179.0 | 5.2x |
| `set_insert_contains` | 148.3 | 144.5 | 144.5 | 144.5 | 31.6 | 4.6x |
| `record_field_reads` | 693.1 | 690.5 | 690.5 | 690.5 | 36.0 | 19.2x |

- LLVM at `O3` runs 0.4% (`record_field_reads`) to 42%
  (`int_list_sum`) fewer instructions than Cranelift.
- Most of the time stays in the runtime: `dict_build_get`,
  `set_insert_contains`, and `record_field_reads` move by under 3%.
  Step 10's runtime inlining is where those can move.

### Compile time

Cold `aura build`, 11 rounds in seconds, and the `aura` process's
instructions in millions:

| Program | Cranelift | LLVM `O1` | LLVM `O2` | LLVM `O3` |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 0.727, 3,469 | 0.746, 3,479 | 0.682, 3,479 | 0.706, 3,479 |
| Reference agent | 1.052, 4,983 | 1.302, 5,778 | 1.285, 5,892 | 1.272, 5,910 |
| `json_codec_derived` | 1.195, 6,010 | 1.646, 7,677 | 1.633, 7,813 | 1.748, 7,822 |

- LLVM at `O3` costs the reference agent 927 million more instructions
  than Cranelift, 19% more, and about 0.22 s. `O1` already pays 86% of
  that.
- A cold build of hello world hashes the 62 MB runtime archive, about
  3,340 million instructions, whichever code generator runs. A build
  with a warm cache reuses the digest.
- `scripts/bench-compile-time.py` ran once per setting. Its cold medians
  for the reference agent are 1.038 s on Cranelift and 1.130, 1.282, and
  1.245 s on LLVM `O1`, `O2`, and `O3`. Its runs are sequential, so they
  carry more drift than the table above.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
All three LLVM levels link the same executables.

| Subject | Cranelift | LLVM | Recorded Linux size |
| --- | ---: | ---: | ---: |
| Hello world | 977,632 | 977,632 | 973,016 |
| Reference agent | 1,853,904 | 1,815,424 | 1,915,000 |
| JSON | 1,464,048 | 1,459,728 | 1,506,456 |
| TLS | 3,266,816 | 3,245,928 | 3,303,264 |
| WebSocket | 3,592,608 | 3,588,336 | 3,608,752 |

- The 629 dumped fixture objects come to 6.50 MB at `O1`, 6.50 MB at
  `O2`, and 6.51 MB at `O3`. `O2` changes 157 objects from `O1`, and `O3`
  changes 52 from `O2`.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, and
  `npm run check:clippy:llvm` pass. `scripts/check-hygiene.sh` passes.
- Compiler library tests: 1666 of 1667 pass on the default build and 1678
  of 1679 with the `llvm` feature. The one failure in each is
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- With `AURA_NATIVE_CODEGEN=llvm`, the same eleven emitter tests as in 9b
  fail, because they pin Cranelift's object shape. The repeat-build test
  passes.
- The LLVM parity gate, `npm run test:backend-parity:llvm`, passes on all
  788 run-pass and run-fail fixtures at `O3`, in 18 minutes.
- Cranelift objects for all 629 fixtures match the dumps taken before 9a
  byte for byte.
- The 33 release packaging tests pass, including a new one for
  the `native-binaries` job.
- Coverage on the default build is 96.56% of lines, 97.44% of functions,
  and 95.48% of regions, above the floors. The default build does not
  compile `native_backend_llvm.rs`, so the report leaves it out.

## Follow-Up

- Step 10 can hoist the inline layout check out of list loops in MIR, as
  `O3`'s unswitching does now. Then re-measure `O2` against `O3`.
- Eleven emitter tests pin Cranelift's object shape and fail on LLVM. Each
  could check an LLVM form, in a later step:
  - the tests that look for local functions or unused imports could read
    the module before the passes, or only the relocations, which both code
    generators keep;
  - `the_emitter_emits_only_the_checks_mir_names` could count trap calls
    before the passes, since LLVM folds checks it can prove;
  - a test-only way to build an object without passes would serve all of
    them.
- The archive hash dominates cold builds of small programs, on both code
  generators.
