# Items 6 and 7 evidence

The parent work note records scope, current completion state, gate outcomes,
measurement grades and cleanup. `SHA256SUMS` is the final artifact manifest.
No compiled executable is committed here.

## Reference agent, profiling variable and benchmark gate

`reference-agent-{red,green}.log`, `keep-symbols-{red,green}.log`, and
`benchmark-input-gate-*.log` retain the test-first evidence. The benchmark gate
first found task-argument width mismatches, then Queue payload widths; all
15 benchmark Aura sources pass after six explicit casts. The reference agent
version-0 commit is `fc0361bc76b8b47d300a3089b1b4bb7c2b739dfc`.

`size-script-{red,green}.log` records the new real-package default, pinned stdout
verification and the one-ref release measurement mode. The size reports record
exact clean refs, profile settings, compiler/runtime installation, package source
identities and standalone execution with Cargo unavailable.

## Fibonacci attribution

`profiling-provenance.json` records tool versions, source/implementation commits,
profile settings and exact binary sizes/hashes. `profiles/` contains eleven
lossless gzip-compressed xctrace time-profile XML tables, their record logs, exact
record/export commands, stdout checks and compressed/uncompressed hashes.
Every process label in these tables belongs to the targeted `fib30-before` run.
`collect_fib_profiles.py` reproduces collection from a supplied native binary.

`attribute_fib.py` reproduces `fib-attribution.json` from those tables and the
retained `fib-before-selected.asm`. Its instruction ranges intentionally refer
to this exact before binary, not arbitrary rebuilt layouts. All retained active
samples have a one-millisecond weight. Sampling skid, inline code and missing
backtraces limit precision; the report distinguishes 1,402 recovered Fibonacci
samples from 100 excluded process samples.

`compare_fib.py` reuses the release runner's bounded READY/GO/DONE implementation
for diagnostic-only comparisons. `fib-spill-comparison.json` retains two warmups
and eleven alternating observations per binary. The safe storage experiment in
`fib-spill-experiment.patch` improved 11.10%, below the required 20%, and was
reverted. This patch is historical evidence, not part of the shipped runtime.
No unchecked frame-metadata ABI was implemented.

## Arrays

`array-prechange-corpus.json` identifies the runtime sources and frozen expected
values recorded before optimization. The original test module was committed at
`b1c40b6eff649e7b28c6359c996b7b577e8db42a`, then renamed to
`array_kernel_corpus_tests.rs` to follow the existing test-file convention.
Its 1,008 expected results remain in `array_kernel_expected.json` in compiler
source. MIR operator/method evaluation and direct ABI calls compare against the
same immutable bit/diagnostic fingerprints. The temporary recording switch was
removed. Recording, baseline and optimized debug/release logs are retained;
the expected-value JSON was not updated after changing kernels.

`array-division-invariants.log` additionally covers empty division, signed-zero
traps and allocation-error precedence. Integer kernels and sequential reduction
implementations are unchanged.

`array-profile-investigation.json` describes the old default/tuned loop-selection
difference and its limits. Full gzip-compressed disassemblies and selected kernel
blocks support the observation. `array-vector-instructions.json` records actual
arm64 `.4s` and `.2d` add/subtract/multiply/divide sites, including outlined scalar
division. `compare_array_profiles.py` reuses the numeric runner protocol for a
quiet diagnostic comparison; raw observations are in `array-profile-timings.json`.
These symbol-preserving profile controls are separate from the contractual
clean-detached NumPy/Rust before/after publication reports.

The publication label is: **quiet host, not post-reboot; contractual re-measure
scheduled with the 0.3.4 release session.** Runner qualification and reboot
qualification are distinct. No competing-process override is authorized.

## Gates and integration

`base-coverage.json` preserves exact verified counts from the green baseline
hosted macOS gate. Local full-CI output, final coverage, documentation checks,
branch/main hosted CI identities and final-head size verification are recorded
as their steps complete. Timing-sensitive failures, if any, are retained and
classified separately from isolated reruns. Coverage floors remain unchanged.

## Publication artifacts

`arrays-{before,after}-build.{json,log}` records the clean release builds.
`arrays-{before,after}-{raw,summary}.json` contains contractual 11-observation
NumPy/Rust comparisons; each summary links its raw report by hash.
`array-publication-comparison.json` derives the published medians/ratios and
control drift. Addition improves 79.96% to 1.0065x Rust; sum remains 1.3391x.
Both runs have three empty quiet-host inventories and no override.

`executable-sizes.json` records the clean implementation-head release sizes
(compiler 10,897,408; hello 1,586,968; reference agent 3,199,712 bytes).
The final PR-head repeat is retained as `final-head-executable-sizes.json` in
the post-merge completion record, avoiding a commit identity self-reference.
`after-coverage.json` records exact LLVM coverage totals. `local-ci.log` and
`local-ci-remaining.log` cover all local gates; `publication-gates*.log` records
final document verification and its corrected metadata/wording failures.
Protected personal-file content is excluded from the publication log.

## Integration completion

`branch-ci.json` and `main-ci.json` retain the successful Ubuntu/macOS runs
on final head cbbd220 and merge 52a7ae2. The matching Docs records include the
successful main deployment. `pull-request.json` records PR #8 and its merge.
No hosted failure or rerun occurred. Compressed branch/main job logs and
per-platform coverage JSON retain the exact gate evidence; every floor passed.

`final-head-executable-sizes.json` and `final-head-size-verification.json`
confirm all three published byte counts and standalone outputs at the exact
final PR head. Compiler hashes differ because each build embeds its commit.
`pre-cleanup-SHA256SUMS` freezes the evidence before deleting the task-owned
checkouts/profiling builds. `cleanup.json` and `cleanup.log` record removal,
coverage cleaning and disk checks. The final SHA256SUMS also covers those
completion records.
