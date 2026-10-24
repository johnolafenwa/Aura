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

## Part 11c: Cranelift Removed

### Design

- `native_backend_cranelift.rs` and the five Cranelift crates are gone,
  with 20 more crates only they used. inkwell is a plain dependency of
  `aura-compiler`, and the `llvm` cargo feature is gone from both crates.
  The tests read objects with the `object` crate, a dev-dependency now.
- `CodeGenerator`, `selected_code_generator`, and `AURA_NATIVE_CODEGEN`
  are gone. `host_module_backend` builds the LLVM module. A set
  `AURA_NATIVE_CODEGEN` is ignored, so an old shell setting cannot break a
  build.
- The native cache format is `aura-native-cache-v10`. The key names
  `llvm` as a constant instead of the selected code generator, so programs
  that Cranelift built under v9 never reach an LLVM `aura`.
- The size gate's `cranelift` component is now `llvm`, matching `llvm::`,
  `inkwell::`, and `LLVM`, and every subject must lack it. So the gate
  checks that user programs never link LLVM. `aura-runtime`'s dependency
  test forbids `inkwell` and `llvm-sys`.
- CI drops `test:llvm`, `test:backend-parity:llvm`, and
  `check:clippy:llvm`. `test:rust` and `test:backend-parity` run on LLVM,
  and the repeat-build test runs inside `test:rust`. The native binaries
  job builds once and runs the gate once.
- `docs.yml` builds `aura` for the doc examples, and `safety.yml` builds
  `aura-compiler` in the fuzz, ASan, and stress and benchmark jobs. Each
  now installs LLVM, with the CI cache. The ThreadSanitizer job tests
  `aura-runtime` alone and needs none.
- `fuzz/Cargo.lock` was already stale before this step, and `cargo fuzz`
  refreshes it. It is left as it was.
- Docs: CONTRIBUTING, SUPPORTED_PLATFORMS, the CLI README, the
  install page, architecture docs 00, 07, 08, and the README, and the
  manual pages that named Cranelift frames.

### Tests Retired

| Test | Reason |
| --- | --- |
| `direct_backend_enables_stack_returns_for_flattened_mutable_writeback` | It checked Cranelift's flags. Cranelift is gone |
| `unset_empty_and_cranelift_settings_select_cranelift` | The switch is gone |
| `other_settings_name_both_code_generators` | The switch is gone |
| `llvm_setting_needs_the_llvm_feature` | The feature is gone |
| `llvm_setting_selects_llvm` | The switch is gone |
| `native_run_cache_keys_on_the_code_generator` | Replaced by `native_run_ignores_the_retired_code_generator_setting`, which runs `cranelift`, `llvm`, `gcc`, and an empty setting and expects each to reuse the one cached program. It does not depend on the ambient environment |
| `test_ci_installs_pinned_llvm_and_runs_the_llvm_gates` | Replaced by a test that every workflow building the compiler installs pinned LLVM |
| `test_native_binary_gate_checks_both_code_generators` | Replaced by a test that no gate builds a second code generator |

Two LLVM backend tests lost `like_cranelift` from their names.

### Verification

- `cargo fmt --check`, `npm run check:clippy`, and
  `scripts/check-hygiene.sh` pass.
- The whole workspace passes, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root. The CLI test binary passes 404 tests.
- `npm run test:backend-parity` compares LLVM with the MIR interpreter
  and passes in 1047 s.
- The size gate passes on the LLVM-only `aura`. Hello world is 982,472
  bytes, the reference agent 1,894,808, the JSON subject 1,477,432, the
  TLS subject 3,299,176, and the WebSocket subject 3,608,584. No subject
  links LLVM.
- `scripts/check-aura-libraries.py` passes on the LLVM-only `aura` on
  Linux.
- The Python gates pass, and `check_doc_examples` checks 812 examples.
- Coverage meets its floors, with `native_backend_llvm.rs` in the report
  and the Cranelift file gone: lines 96.51% (floor 96.46), functions
  97.37% (floor 97.33), and regions 95.44% (floor 95.23). The run used
  `--no-fail-fast`, so the root-only failure did not stop it.
  `native_backend_llvm.rs` alone covers 92.06% of its lines.
- The LLVM-only `aura` release binary is 66.33 MB stripped, against
  68.69 MB with both code generators and 17.8 MB for Cranelift alone
  at 9b.
- Not run here: `npm run docs:build`, since this worktree has no
  VitePress install. The regenerated `llms-full.txt` comes from
  `npm run docs:llms`, and `npm run docs:llms:check` passes.
