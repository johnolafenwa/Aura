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

Complete on main. PR #8 merged final head cbbd220 with merge commit
52a7ae2672f70202a21e10b4d9dda7e16bdfff9d after green Ubuntu/macOS and Docs CI.
Main CI and docs deployment are green. All local gates, contractual Array
measurements, exact-final-head size verification, documentation, evidence hashes
and cleanup are complete. No hosted failure or rerun occurred. The optional
frame optimization was reverted because its 11.10% improvement missed the 20%
gate. No remaining work or blockers in this authorized task.

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

## Delivered scope and verification

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
Initial root target was 4.7 GiB with 92 GiB free. Measurements and source
identities are recorded separately from estimates and diagnostic profiling samples.

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

## Local gate progress and final-head publication

The first full Rust pass is green: 378 CLI tests, 1,885 compiler unit tests,
all fixture categories and native-codegen acceptance. The forced MIR/direct
matrix is green; LSP and extension checks also pass. The unchanged-floor
compiler coverage run passed. No isolated timing rerun was needed.

After all local gates passed, clean detached merge-base and implementation-head
Arrays were measured with nothing else running, followed by the size table.
The size script then ran again at the exact final publication head. Its report
is included in this post-merge work-only completion record, avoiding a measured
commit identity self-reference. Branch CI, merge and main CI all completed
before cleanup. No source changed after final-head measurement.

During coverage, target reached about 17 GiB. All 38 selected profiling/sample/
disassembly identity artifacts were hashed and verified in
`profiling-artifact-SHA256SUMS`; the no-longer-used, task-owned
`target/foundations-profile` build directory was removed after checking that no
process was executing from it. Root target returned to about 15 GiB with
80 GiB free. Active coverage and native-runtime build trees were preserved.

All local gates are green. The first full CI run passed Rust tests, forced
parity, compiler coverage and 100% LSP coverage, then the reference checker
rejected the word SIMD in the new Array prose under its existing Phase 7.3
rule. Reworded that sentence to describe unchanged arithmetic policy; retained
the measured vector instructions and left the gate unchanged. Regenerated LLM
docs. Resumed reference, tutorials, docs build, audits, Clippy and hygiene all
passed (local-ci-remaining.log). No timing-sensitive failure occurred.

Exact compiler coverage: 101426/105311 lines, 6786/6980 functions,
149749/157930 regions; floors remain 96.30/97.21/94.71.

## Contractual Array publication

After local gates passed, clean detached merge base a368dce and implementation
head d9fc799 were built with Rust 1.95.0, release-profile/runtime overrides
scrubbed. Both builds completed before timing. Each checkout's unchanged runner
ran sequentially with all NumPy/Rust lanes, 11 observations per lane, excluded
warmups, no competing-process override and all three inventories empty. Both
reports are contractual, clean/detached, with verified raw-report hashes.
Measurement label: **quiet host, not post-reboot; contractual re-measure
scheduled with the 0.3.4 release session.** Raw generated-at fields record the
actual 7 September session; the work-note name follows the requested 8 September
name and is not a claim that the measurements happened on that date.

Addition: 1.244817707 to 0.249472738 ms, -79.9591%, Aura/Rust 5.039359 to
1.006524 (1.5x target met). Sum: 1.149271444 to 1.149843669 ms, +0.0498%,
Aura/Rust 1.338990 to 1.339087. NumPy add/sum drift -2.2310%/-0.7639%; Rust
add/sum drift +0.3387%/+0.0425%. Sequential reductions are unchanged; the NumPy
sum gap is chiefly the required reduction-order policy. No repeat was needed.
Full measurements and derived controls are in arrays-{before,after}-{raw,summary}
and array-publication-comparison.json.

## Executable-size publication

The clean d9fc799 release measurement records compiler 10,897,408 bytes, hello
world 1,586,968, and actual reference agent version 0 3,199,712. Compiler and
runtime were installed together; hello/agent execute with Cargo unavailable
and match pinned stdout. The script hashed all artifacts/package sources and
removed its detached checkout/target. Final-head verification then passed at
cbbd220; its report is retained in the completion record.

## Publication verification

Final reference/Manual blocks, tutorials, docs build, 15 identity tests and
hygiene pass. The size-command illustrative fence changed, so its existing
reference-integrity metadata hash was refreshed. Two wording checks rejected
SIMD and historical; the prose now uses measured instruction names and exact
earlier-session identities. Both existing checks remain unchanged. Initial
failures and successful reruns are retained. No existing executable fixture
was adapted. A scoped whitespace check passes; the broad check reported only
pre-existing whitespace in protected personal/file_ops.au, which is untouched
and whose content is omitted from publication evidence.

Exact-final-head size verification, PR/branch green, merge, main green and
cleanup/completion recording are complete. No release, tag or version bump.

## Hosted branch verification and merge

Final PR head cbbd220cc3f0492e3e52d1704126d0526649b98e was rebuilt cleanly
under the release profile. All published byte counts match exactly, and both
standalone outputs match with Cargo unavailable. The exact-head report and
comparison are retained for this work-only completion record.

PR https://github.com/johnolafenwa/Aura/pull/8 merged with merge commit
52a7ae2672f70202a21e10b4d9dda7e16bdfff9d after one complete green CI run:
https://github.com/johnolafenwa/Aura/actions/runs/34147227517
Both Ubuntu and macOS passed; Docs run 34147227808 also passed. No hosted
failure or rerun occurred. Hosted macOS coverage exactly matches local totals;
Ubuntu covered four additional lines and seven additional regions, with the
same function count. Compressed job logs and coverage records are retained.

The merge has the expected two parents and a tree identical to the final PR
head. Local main was fast-forwarded while preserving the protected personal
changes. Main CI https://github.com/johnolafenwa/Aura/actions/runs/34154181644
passed on both Ubuntu and macOS. Main Docs run 34154181549 built and deployed
successfully. No release/tag/version bump.

## Final verification and cleanup

| Compiler coverage | Baseline hosted macOS | Final main hosted macOS |
| --- | ---: | ---: |
| Lines | 96.303966% | 96.309028% |
| Functions | 97.210239% | 97.220630% |
| Regions | 94.806388% | 94.818591% |

Floors stay 96.30/97.21/94.71. Final main Ubuntu measured 96.312826% lines,
97.220630% functions and 94.822390% regions. Local and branch macOS measured
96.310927/97.220630/94.819857. All are above the floors; small covered-line/
region variation did not justify rerunning a green job. LSP coverage is 100%.
All existing fixtures pass unchanged, and all requested local/hosted gates pass.

The 111 pre-cleanup evidence artifacts were hashed and both earlier manifests
verified. Open-file checks were empty for each task-owned detached/profile
directory and the root coverage target. Removed the before/after detached
checkouts and their targets, profiling directory and task-owned temporary helpers;
`cargo llvm-cov clean --workspace` completed successfully. Root target fell from
16.279 to 12.738 GiB; free disk rose from 74.731 to 80.199 GiB. Exact byte counts
and commands are in cleanup.json. Earlier obsolete profiling output was already
removed under the build-hygiene gate. Unrelated build trees, worktrees, caches,
source files and protected personal changes were preserved.

The final SHA256SUMS includes hosted CI/log/coverage records, exact-head size
verification and cleanup evidence. This completion commit changes work records
only; the green merge commit remains the tested source/docs tree. The workflows
exclude work-only changes, so this record does not require another CI run.

## Follow-up

The 0.3.4 release session re-measures Arrays after reboot. Batch 7 evaluates the
recorded call-bookkeeping/backend options; Batch 8 evaluates guarded-stack reuse.
Later usability batches compare the maintained agent against version 0 fc0361b.
These are roadmap follow-ups, not unfinished items 6 or 7. No release is prepared.
