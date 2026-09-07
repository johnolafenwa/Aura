# Pre-Batch-1 foundations: items 6 and 7

## Target and authorized scope

Deliver the deterministic reference agent package and performance triage on
`codex/pre-batch-1-items-6-7`, then merge after green final-head CI and verify
green main CI. Base: `a368dce7e7b335c1c0cb800ba5240501a21f02bc`.
No release, version bump, tag, reboot, scheduler implementation change, semantic
change, diagnostic change, parity change, coverage-floor reduction, or fixture
adaptation is authorized. The personal file, swap file and ADR-0022 draft are
protected. The note name follows the user's requested 2026-09-08 artifact name.

## Current status

In progress: implementation, profiling, Array bit proof and stack-reuse scoping
are complete. The full local gate is running. Quiet contractual Array
measurements, final executable sizes, final documentation, hosted branch/main CI,
merge and cleanup remain. The optional frame optimization was measured and
reverted because it did not reach the required 20% improvement.
No blocker currently identified.

## Completed foundation changes

- Reference agent version 0: `fc0361bc76b8b47d300a3089b1b4bb7c2b739dfc`,
  131 Aura lines in `examples/agents/tool_runner/src/main.au`. The package has
  its manifest, lock, README and pinned stdout. The new smoke test failed before
  the program existed, then passed with byte-identical MIR/direct output.
- The execution watchdog initially included a cold native runtime build and
  timed out; compilation now completes separately before timed execution.
  The final smoke test passes. No existing fixture was changed.
- The benchmark gate discovers all 15 Aura files recursively. It exposed six
  int64 range-to-int32 calls in three scalable-runtime inputs. Explicit casts
  repair these inputs; before/after regression logs are retained.
- `AURA_NATIVE_KEEP_SYMBOLS=1` skips user-binary stripping and participates in
  native-cache identity. The CLI test failed at the missing second cache entry
  before implementation and passed afterward. Values other than exact `1`
  retain the normal policy; repeated runs reuse their corresponding entry.
- Base hosted macOS coverage is 96.3039658% lines, 97.2102387% functions and
  94.8063882% regions; exact counts and the green main run are in
  `base-coverage.json`. The base's subsequent commit changes work records only.

## Delivery and verification plan

1. Reference agent version 0: package below 400 Aura lines, typed JSON request
   and result methods, named-function dictionary registry, typed errors, retry,
   Queue streaming from a TaskGroup child, user-resource cleanup, pinned output
   on both backends. Replace the executable-size stand-in.
2. Extend the benchmark-input CLI gate to every benchmark Aura source.
3. Add cache-keyed symbol preservation for profiling. Profile Fibonacci with
   xctrace or sample and publish honest category attribution. Only take a
   semantics-free call-overhead fix if it meets the user's 20% threshold.
4. Record pre-change bit outputs, optimize floating Array elementwise kernels,
   preserve integer/trap/reduction semantics, verify both backends and release
   vector instructions, investigate the earlier regression, and measure clean
   before/after sources with quiet-host checks and NumPy/Rust controls.
5. Scope guarded task-stack reuse in ADR-0032 and Batch 8 without scheduler edits.
6. Run the full required local gates and unchanged 96.30/97.21/94.71 coverage
   floors; update all maintained documentation, generated documents and hashes;
   publish the PR, merge on green, verify main, and clean temporary/coverage builds.

## Evidence and build hygiene

Evidence belongs in `work/2026-09-08-pre-batch-1-items-6-7/` with SHA256SUMS.
Initial root target is 4.7 GiB with 92 GiB free. Measurements and source identities
will be recorded separately from estimates and diagnostic profiling samples.

## Per-call investigation

Eleven successful xctrace Time Profiler recordings contain 1,402 recovered
Fibonacci-stack samples out of 1,502 process samples. Ten rows have no recovered
backtrace; the other excluded rows are outside recovered Fibonacci stacks.
`attribute_fib.py`, lossless compressed XML, selected disassembly and the
category/PC report preserve the attribution method. Sampling skid means the
shares are estimates, not exact removable costs. UTF-8 validation alone appears
in 400/1,402 leaf samples (28.53%). No polling is emitted in this task-free input.

The safe in-place Spill push/pop experiment retained UTF-8 validation and all
state transitions. Two warmups and eleven alternating GO/DONE observations per
lane measured 91.165375 ms before and 81.045542 ms after: 33.85854 versus
30.10007 ns per 2,692,537 logical naive-Fibonacci calls, an 11.10% improvement.
These are diagnostic quiet-host timings, not contractual release results.
The experiment failed the user's 20% eligibility gate and was reverted; its
exact patch and binary hashes are preserved. No optional call-overhead fix ships.

The repository-mandated Daybreak ABI review found that a new unchecked exported
entrypoint would remain reachable through process-global FFI and handwritten
objects; skipping UTF-8 validation could replace existing invalid-metadata
diagnostics with undefined behavior. That proposal was not implemented.
Any future validation amortization requires a design that preserves the current
trust boundary, or a separately approved contract decision.

## Array correctness baseline

The fixed-seed corpus records 1,008 cases per backend across int32/int64/float32/
float64, lengths 0/1/7/8/9/1,000,001, checked/wrapping/saturating arithmetic,
Array/Array and legal scalar broadcasts, all four reductions, payload-bearing
NaNs, positive/negative infinities, signed zero, and first traps at 0/7/8.
MIR operator and method evaluation and the direct exported Array ABI produce
identical pre-change fingerprints. The frozen JSON hashes every output element's
exact little-endian bits and shape, or retains the exact reduction bits and
diagnostic code/message. Recording happened before the shared kernel rewrite;
the temporary recording switch was removed. Baseline source and golden hashes
are recorded in `array-prechange-corpus.json`. Existing fixtures are untouched.

## Kernel implementation and investigation

Only the shared `runtime_value.rs` float32/float64 elementwise slice kernels
change: operation dispatch moves outside loops, preallocated output uses
exact-length iterator extension, and division validates the first zero divisor
after allocation. Reductions and integer kernels are unchanged. Both runtime
paths pass all 1,008 frozen cases in debug and optimized release builds. A
separate regression verifies empty division and allocation-error precedence.
The corpus test module uses the existing `_tests.rs` naming/coverage exclusion;
no coverage policy changes.

The release user-binary disassembly contains fadd/fsub/fmul/fdiv in both `.4s`
and `.2d` vector forms, including scalar broadcasts and outlined scalar-division
iterator specializations. Full compressed and selected disassemblies are retained.

The old default profile selects separate scalar loops; tuned code instead
performs fadd/fsub/fmul and two fcsel operations per element. Both call the
kernel out of line. This supports a loop-selection explanation rather than a
lost inlining boundary, so no speculative attribute was added. Default and
tuned controls differ in LTO and codegen units; the historical regression cannot
be attributed to LTO alone. Eleven paired diagnostic observations reproduced
+7.4285% (1.128668375 to 1.212511557 ms/add). Vectorized addition took
0.246133057 ms/add, a 79.7006% improvement. Pre/post inventories were quiet;
these are diagnostic controls, separate from the required detached publication.

## Other completed preparation

- ADR-0032 and roadmap Batch 8 scope bounded guarded-stack reuse, preserving
  override bounds, pinning, cleanup/ancestry and TSan. The acceptance comparison
  is 9.869 microseconds per Aura task against 0.340 for tokio. No scheduler edit.
- Size tooling now defaults to the actual tool_runner package, verifies its
  exact pinned stdout, and records manifest/lock/Aura-source identities. It
  explicitly omits the nonexistent reference subject at historical before refs;
  `--after-only` measures the requested final ref's release profile once.
  Three new tests failed before implementation; all eight size-script tests pass.
- The reference smoke test counts and copies every Aura source in the package,
  enforcing the total under-400-line requirement, not just the entry file.

Build hygiene before full CI: root target 8.4 GiB, detached diagnostic target
532 MiB, 87 GiB free. The detached merge-base checkout is retained for the later
contractual before run. Full local CI output is captured in `local-ci.log`.
