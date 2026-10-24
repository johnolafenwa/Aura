# Batch 7 Step 11: LLVM As The Only Native Backend

## Goal

Remove the Cranelift code generator, so LLVM 22 through inkwell is Aura's
only native backend. The MIR interpreter stays: `aura run` uses it, and
the parity matrix compares it with LLVM. John decided this on 2026-10-24:
"make the full switch to llvm, we will discontinue the cranelift backend
rather than supporting multiple backends." The plan and the inventory are
in `step-11-plan.md` in the project's batch 7 folder. Step 11 has three
parts, one commit each.

## Measurements Before Deleting

Release `aura` at bb29b251 with both code generators, on the four-core
Linux x86-64 container. `scripts/bench-compile-time.py` ran twice for each
code generator, alternating, with five repetitions each. Medians of the
ten pooled samples, in seconds:

| Program | Mode | Cranelift | LLVM | Phase 2 target |
| --- | --- | ---: | ---: | ---: |
| Hello world | cold | 0.457 | 0.453 | 0.6 |
| Hello world | edit | 0.376 | 0.373 | |
| Reference agent | cold | 0.792 | 1.893 | 1.5 |
| Reference agent | edit | 0.734 | 1.797 | |
| `unix_tls_roundtrip` | cold | 0.509 | 1.004 | |
| `unix_tls_roundtrip` | edit | 0.507 | 1.010 | |

- **The reference agent misses the 1.5 s target on LLVM.** It takes
  1.89 s, against 1.27 s at 9c. Step 10's inline fast paths made the IR
  larger.
- Hello world meets the 0.6 s target on both.
- A timed `aura` split one reference agent build. LLVM's `default<O3>`
  passes take 0.61 to 0.70 s, and instruction selection and object
  writing 0.51 to 0.58 s, all on one thread. Lowering to LLVM IR takes
  13 to 23 ms. `default<O1>` passes take 0.37 to 0.44 s, so `O1` alone
  would not meet the target either. Without passes, code generation still
  takes 0.46 to 0.58 s.
- The `aura` release binary with both code generators is 68.69 MB
  stripped.

## Part 11a: Generator-Neutral Emitter Tests

### Design

- `native_backend::without_optimization` runs a closure with the code
  generator's optimizer off on the current thread. The LLVM backend then
  skips its pass pipeline and writes the object the emitter asked for.
  Cranelift has no separate optimizer, so its objects do not change. The
  switch exists only in test builds.
- `emit_unoptimized_object` in `native_codegen_tests.rs` wraps
  `emit_host_object` with it.
- Eleven tests pin what the emitter defines and calls: the ABI of
  callables, closures, default binders, and host builtins, which builtin
  wrappers the object holds, which retain and release hooks it calls, and
  which arithmetic checks it emits. On LLVM they failed: LLVM inlines the
  local functions they look up, drops wrappers nothing calls, and folds
  checks it proves. They now read the unoptimized object, so they check
  the emitter on both code generators:
  - `dead_assignments_do_not_poison_reachable_cleanup_type_inference`
  - `direct_backend_emits_retain_and_release_hooks_for_opaque_call_and_local_flow`
  - `direct_callable_objects_pin_defaults_capability_writebacks_and_task_handoff_abis`
  - `direct_closure_objects_pin_environment_call_default_binding_and_task_abis`
  - `direct_dynamic_json_surface_uses_the_host_builtin_abi`
  - `direct_monotonic_time_uses_scalar_runtime_abi_without_generic_host_boxing`
  - `direct_object_emits_only_the_builtin_wrappers_the_program_reaches`
  - `direct_try_moves_noncopy_ok_payloads_through_the_destructive_runtime_path`
  - `native_codegen_function_value_binding_skips_named_slots_for_later_positionals`
  - `the_emitter_emits_only_the_checks_mir_names`
  - `ticket9_expected_uint64_operands_avoid_generic_literal_boxing`
- `generated_retains_and_releases_count_in_place_and_keep_the_runtime_paths`
  asked which code generator ran, since LLVM removes the program's retains.
  It reads the unoptimized object too, and expects the retain's cold call
  on both.
- LLVM's ELF objects refer to a local function or data object through
  its section's symbol and an addend, where Cranelift names the symbol.
  `relocation_symbol_name` maps such a relocation to the symbol defined at
  that place, so the helpers that list referenced symbols read both
  forms.
- One assertion pinned Cranelift's object shape and is retired:
  `direct_closure_objects_pin_environment_call_default_binding_and_task_abis`
  expected the symbol `aura_direct_function_call` in the object. The
  emitter declares it for the contract invoke adapter, but this program
  never calls it. Cranelift writes every declared import into the symbol
  table, used or not, and LLVM writes only the ones a call uses. The
  test's other checks, including every call it expects, hold on both.

### Verification

- With `AURA_NATIVE_CODEGEN=llvm` and with Cranelift, the compiler library
  passes 1707 of 1708 tests. The one failure on each is
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- 11a changes no code outside tests. The switch compiles only in test
  builds, and a release build runs the passes as before.

## Part 11b: LLVM In Every Release Build

### Design

- The Release workflow's `cli` job installs LLVM on all three runners and
  builds `aura` with the `llvm` feature. `ubuntu-24.04` uses the pinned
  Linux archive, and `macos-15` and `macos-15-intel` use Homebrew's
  `llvm@22`, each for its own architecture. The release restores no
  cache. Cranelift stays the default until 11c.
- `install-llvm.sh` handles `Darwin-x86_64` as it handles `Darwin-arm64`.
  Before `brew install`, it reads `brew info --json=v2` and stops when no
  bottle fits this Mac, since building LLVM from source takes hours. An
  older macOS bottle counts, as Homebrew uses one.
- A static LLVM still names system libraries. On Linux the LLVM `aura`
  loads `libstdc++.so.6`, `libz.so.1`, and `libzstd.so.1` besides glibc
  and `libgcc_s`. `--as-needed` drops `libxml2`, which `llvm-config` also
  names. On macOS, Homebrew's LLVM names Homebrew's zstd, a dylib under
  the Homebrew prefix, which a Mac without Homebrew lacks. The smoke test
  would not notice, since it runs on the build machine.
  - `install-llvm.sh` links Homebrew's `libzstd.a` into a directory of its
    own and exports it as `LLVM_SYS_SYSTEM_LIBRARY_DIRS`. llvm-sys passes
    those directories to the linker before Homebrew's `lib`, and Apple's
    linker takes the first directory that has the library, so `aura`
    links zstd statically.
  - `scripts/check-aura-libraries.py` reads `readelf -d` or `otool -L`
    and fails on a library outside the system set. The Release workflow
    runs it before packaging, and the native binaries CI job runs it on
    every pull request, on Linux and macOS arm64.
- SUPPORTED_PLATFORMS lists the libraries the Linux `aura` loads.

### Verification

- The six new library-check tests and the 35 release packaging tests
  pass. Two packaging tests are new: one pins the Release steps and their
  order, and one pins the macOS bottle check and the static zstd.
- On Linux, the check passes on the release `aura` built at bb29b251
  with LLVM.
- Not verified here, since the container has no macOS: the static zstd
  route, the bottle check on a real Homebrew, and the Release workflow as
  a whole. The native binaries job checks the first on the next pull
  request. Whether Homebrew still bottles `llvm@22` for Intel macOS is
  unknown until the Release workflow runs on `macos-15-intel`.
