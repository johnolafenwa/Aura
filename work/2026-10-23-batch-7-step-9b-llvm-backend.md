# Batch 7 Step 9b: The LLVM Code Generator

## Goal

Implement part 9b of the step 9 plan: give the native backend interface
from 9a a second implementation, with LLVM 22 through inkwell, behind an
`llvm` cargo feature that is off by default. `AURA_NATIVE_CODEGEN=llvm`
selects it at run time, and the whole parity matrix must pass on it. CI
installs LLVM and runs the matrix on both code generators.

## Design

### Selection

- `native_backend::selected_code_generator` reads `AURA_NATIVE_CODEGEN`.
  Unset, empty, or `cranelift` selects Cranelift. `llvm` selects LLVM, or
  fails with a message when this `aura` was built without the feature.
  Any other value fails and names both choices.
- The native run cache key carries the code generator's name, so an LLVM
  run never reuses a Cranelift program, or the other way around. A setting
  that selects nothing gets a key of its own, so the build reports it. A
  CLI test failed before the fix: an `aura` without LLVM ran the cached
  Cranelift program for `AURA_NATIVE_CODEGEN=llvm`.

### Recording, then lowering

- `native_backend_llvm.rs` records each body in a small instruction list
  of its own while the emitter builds it. `finish` lowers the whole object
  to LLVM IR in one context, so no LLVM type outlives a call, and the
  module needs no self-referencing lifetime.
- The peepholes inspect and rewrite the recorded instructions, with the
  same answers Cranelift gives: `only_calls_since` walks the current block
  back to the instruction, the `replace_with_*` methods rewrite it in
  place and keep its results, and `integer_constant` reads an `iconst`.
- `use_var` answers like Cranelift's SSA builder where the peepholes can
  tell: from the variable's last definition in the current block, or
  through sealed blocks with one predecessor each, and with zero for a
  variable no path defines. Elsewhere it loads from the variable's alloca.
- `define_function` reports a body's own error before it checks the
  definition, as Cranelift does, so the emitter's tests that build a
  scratch body get the same error on both.

### Lowering

- Blocks become basic blocks and block parameters become phis. When both
  edges of a `brif` reach one block with parameters, the second goes
  through a block of its own, since a phi tells edges apart by their
  predecessor.
- An entry block holds an alloca for each variable, set to zero, and for
  each stack slot. `default<O1>` promotes the variables.
- Values stay 64-bit integers. Loads and stores turn the address into a
  pointer and add the offset with a byte GEP. A trusted access assumes
  natural alignment, and any other access none.
- An Aura-convention function with many results returns one struct, for
  direct and indirect calls alike. The C convention uses the C ABI. No C
  signature the emitter builds returns more than one value.
- Divisions trap on a zero divisor and on the overflowing quotient, as
  Cranelift's do, and `x srem -1` is zero, where LLVM leaves each
  undefined. Traps call `llvm.trap`.
- Each function has an asynchronous unwind table, since task failures
  unwind through generated frames to the runtime's task boundary.
- Local symbols are `internal`, so LLVM may inline them or drop unused
  ones. Data holding addresses becomes a packed struct of byte arrays and
  pointers.
- The target machine uses the host triple, the generic CPU, and PIC. On
  macOS the triple names the deployment target rustc builds the runtime
  for. Per-function sections and the pipeline are left to 9c.

### Building and CI

- `scripts/install-llvm.sh` downloads the official LLVM 22.1.2 archive
  for Linux x86-64 or macOS arm64, checks its SHA-256 before extracting,
  and keeps `llvm-config`, the headers, and the static libLLVM and
  libPolly archives, about 360 MB.
- Both verify jobs install `llvm-config`'s system libraries (Ubuntu's
  `libzstd-dev` and `libxml2-dev`, Homebrew's `zstd`), cache the prefix
  keyed on the version and the script, and run the script.
- `npm run ci` adds `test:llvm` (the compiler library tests with the
  feature), `test:backend-parity:llvm` (the parity matrix with
  `AURA_NATIVE_CODEGEN=llvm`), and `check:clippy:llvm`.

## Verification

- Parity: all 788 fixtures (665 run-pass and 123 run-fail) match their
  oracles on LLVM, with no change to the emitter or the fixtures.
  `npm run test:backend-parity:llvm` passes, in 17 minutes here.
- Cranelift objects: all 629 dumped fixture objects match `wip-9a` byte
  for byte.
- LLVM objects are deterministic: two dumps of all 629 match.
- Tests: the compiler library passes with and without the feature, apart
  from `package_io_helpers_cover_local_error_paths`, which fails because
  the container runs as root. The new CLI test passes. With
  `AURA_NATIVE_CODEGEN=llvm` set, 11 native codegen tests fail because they
  pin Cranelift's object shape: they look for local functions LLVM inlined
  or dropped, for runtime imports Cranelift declares even when nothing
  calls them, or for checks LLVM folded away. They stay Cranelift-only.
- Lint: `cargo fmt --check`, both clippy commands, and
  `scripts/check-hygiene.sh` pass. The release packaging tests pin the new
  CI timeout, the LLVM install step, both archive pins, and the new gates.
- Coverage, on the default build: lines 96.56%, functions 97.44%, and
  regions 95.48%, above the floors. The default build does not compile
  `native_backend_llvm.rs`, so the report leaves it out.

## Measurements

Rough numbers on the Linux x86-64 container, with release builds:

- `aura` is 17.8 MB with Cranelift alone and 68.4 MB with the `llvm`
  feature, so static LLVM adds 50.7 MB, as the plan estimated.
- `aura build` of hello world takes 1.01 s with Cranelift and 1.03 s with
  LLVM, best of three; linking dominates. The `json_codec_derived` fixture
  takes 1.54 s and 1.90 s.
- LLVM objects are smaller, mostly because `default<O1>` inlines and drops
  local functions: `json_codec_derived` is 544 KB on Cranelift and 309 KB
  on LLVM, `json_dynamic_values` 136 KB and 73 KB, and
  `scheduler_mixed_wakeups` 99 KB and 34 KB. All 629 dumped objects come
  to 21.6 MB and 6.1 MB. The linked hello world executable is 978 KB on
  both, since the runtime dominates it.

## Follow-Up

- 9c tunes the pipeline, adds per-function sections, and sets the size
  gates and benchmarks on LLVM.
