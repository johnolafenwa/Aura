# Pre-Batch-1 foundation measurements

## Authorized scope and current status

Collect post-reboot before/after timings and Rust comparisons, check the
Cargo-default size provenance, publish results, and merge with green branch and
main CI. No version bump or tag. Status: partially measured; publication draft
prepared, release-suite completion blocked by frozen benchmark inputs.

The user explicitly allowed the existing modification in `personal/file_ops.au`
as a clean-tree exception. It, `.swp`, and the untracked ADR-0022 draft are
preserved and excluded from this change.

## Preconditions and preparation

- Main was at `c43789e0958aabfeee97ca6c9ba4af2e8164b6a5`.
- Mac14,9 / Apple M2 Pro, 10 cores, 16 GiB, macOS 26.5.2 (25F84).
  Boot at 2026-09-07 02:29:21 BST (kernel seconds 1788744561), after the
  record commit at 02:11:06 BST.
- Three initial quiet-host inventories were empty. Xcode CPython 3.9.6,
  NumPy 2.0.2; Rust 1.95.0, Cargo 1.95.0, tokio 1.53.1 references.
- Clean detached after checkout: `50531b45797ae765ec8d885165c13042617ab567`;
  before checkout: `v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`).
- Both release compilers and pinned Rust references built successfully before
  timing. Each runner also completed its own builds before measured samples.
  Workload inputs in both checkouts remain unchanged.
- Initial root target: 4.6 GiB; available space: 92 GiB. Before documentation
  checks: target 4.6 GiB, 91 GiB free. No cleanup threshold was crossed.

## Work completed and result grades

- Both exact release-performance commands failed before timing with `AU2002`.
  Read-only checking confirmed task and TCP input errors at both pinned refs.
- Subsequent host checks detected sustained macOS background CPU activity.
  Followed the ten-minute retry policy; retry 11 became quiet at
  03:39:53 UTC. No OS service was stopped, no override was used, and no samples
  were taken during the refusals. All retry inventories are retained.
- Completed the unchanged after integer runner (11 alternating pairs and
  excluded warmups) and after/before Array runners (11 rotating pairs).
- Both Array reports are contractual: clean detached sources, three empty
  host inventories, exact protocol/checksum validation, and hash rechecks.
- Standalone after integer results are diagnostic-grade: checked outputs and
  external quiet pre/post inventories, but this runner lacks release-suite
  three-phase host inventories and full input/binary hash rechecks.
- Before standalone integer runner emits only rounded minima (33.3/11.4 ms),
  without raw observations, median, or excluded warmup. They are supplemental
  output, not before/after median evidence. No CPython or release V6 medians
  were collected because their enclosing suite failed before timing.

| After workload | Aura median ms | NumPy median ms | Rust median ms | Aura/Rust ratio of medians |
| --- | ---: | ---: | ---: | ---: |
| int32 whole process (diagnostic) | 24.325417 | — | 16.429334 | 1.480609 |
| int64 whole process (diagnostic) | 9.778250 | — | 16.337417 | 0.598519 |
| Array add per operation (contractual) | 1.205427 | 0.247190 | 0.244576 | 4.928642 |
| Array sum per operation (contractual) | 1.148677 | 0.168981 | 0.858175 | 1.338511 |

Integer paired median ratios are 1.478258/0.594855; Array paired median ratios
are 4.942400/1.338601. Rust integer optimizer barriers and startup costs are
part of the disclosed comparison, not a general compiler-speed ranking.

| Array workload | Aura before ms | Aura after ms | Aura change | NumPy before ms | NumPy after ms | Control drift |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| add | 1.124738 | 1.205427 | +7.1740% | 0.247242 | 0.247190 | -0.0208% |
| sum | 1.149820 | 1.148677 | -0.0994% | 0.169047 | 0.168981 | -0.0394% |

NumPy drift is below 5%; no Array repetition is required. CPython drift is
unavailable. This before/after comparison includes release/link tuning alongside
Cranelift `speed`; it does not isolate the optimization flag's causal effect.
No numeric target is ratified and unfavorable results are retained.

## Cargo-default size check

The pinned Cargo 1.95 reference lists `strip="none"`, including the release
profile example. Decision 8 therefore requires retaining the column and
existing provenance JSON without remeasurement; no size-script changes were
made. The column explicitly sets `none`. Cargo's implementation may strip
pre-existing debug information automatically when strip is omitted and no
compiled package needs debuginfo, so explicit none is not equivalent to omission.
The Manual records this qualification. Reference URLs, full-source hash, Cargo
version and relevant lines are in `cargo-default-strip-check.json`.

## Blocker and concrete proposed resolution

- `benchmarks/release_performance/tasks_10000.au:40` passes an int64 range
  value to `task_value`'s int32 parameter.
- `benchmarks/release_performance/tcp_fanout.au:62` passes an int64 range
  index to `serve_one`'s int32 parameter.
- Both errors occur at both immutable measurement refs. The runner has no
  workload-selection option and builds every input before timing; consequently
  it produces no fib/task/TCP/retry or release V6 observations.
- Identical explicit `as int32` casts at those two call sites pass checking on
  both pinned compilers in scratch copies. The concrete zero-context patch and check output
  are retained under `proposed-input-fix/` (apply only after authorization, using
  `git apply --unidiff-zero`). This patch has NOT been applied.
- Authorization to make identical corrections in clean temporary commits is
  pending because decision 5 explicitly prohibits source changes in either
  checkout. Otherwise, the blocked workloads must remain unavailable.

The Performance and Numeric Arrays chapters, four benchmark READMEs, changelog,
roadmap, ADR-0064 and generated LLM documents now describe the available evidence
and limitation. Items 1 and 3 remain partially measured; marking them complete
would overstate the evidence. No source/compiler/runtime behavior changed.

## Evidence and hashes

All session evidence is copied into
[`2026-09-07-foundations-measurements/`](2026-09-07-foundations-measurements/).
Raw reports are byte-for-byte copies; original temporary paths inside them are
preserved. Summary-to-raw hashes match the copied files. `SHA256SUMS` covers
all measurement files, build logs, refusal logs, checks and unapplied proposal.

| File | SHA-256 |
| --- | --- |
| `aura-foundations-after-arrays-raw.json` | `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f` |
| `aura-foundations-before-arrays-raw.json` | `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd` |
| `aura-foundations-after-integer-loops.json` | `8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb` |
| `cargo-default-strip-check.json` | `8b8f957d2ac519fbbea2371bbd3e3cd82157915eb8e95f0087132fe8f58e1410` |
| `published-measurements.json` | `074a00c2f6d9c575c42602ecd4f0b5ec8e08a90d90d43b65711ddb9a2fcbf871` |
| `SHA256SUMS` | `db6f659c3466c16a8001639e6aca33ef6d56423ca5d2eab62e4d137b42c7f999` |

## Verification and remaining work

Passed: 98 benchmark-tooling unit tests; 15 identity tests; two LLM-generation
tests; generated-document freshness; complete reference gate including Manual
executable blocks; 340 tutorial fences (215 pass, five expected failures, 120
fragments); VitePress production build; scoped documentation whitespace and evidence hashes. The exact compiler error
logs retain their trailing blank diagnostic line.
Initial documentation checks exposed new fence hashes and forbidden narrative
wording, both corrected. The generation tests require direct script invocation;
the first module-style invocation could not resolve their local import.

Draft branch: `codex/foundations-measurements`. Remaining: source-freeze decision,
qualified release suites and their control drift/repetition handling, completion
of the remaining tables/statuses, final-head hosted branch CI, merge commit and
green main CI. No version bump or tag. Cleanup/hosted run status is recorded
below when available.

## Draft publication and cleanup

[PR #7](https://github.com/johnolafenwa/Aura/pull/7) is open as a draft, titled
“Publish pre-Batch-1 foundation measurements.” Hosted validation for the current
head is linked from its [checks page](https://github.com/johnolafenwa/Aura/pull/7/checks).
This is an evidence/publication draft, not a claim that the complete measurement
mission or merge gate is satisfied. No merge commit or main CI exists for it.

After all 37 manifest files were verified both tracked and hash-correct, both
clean detached measurement checkouts were removed with `git worktree remove`,
including their target trees (810 MiB after, 637 MiB before). No live process
used either checkout. Root target remains 4.6 GiB and available space is 92 GiB.
The personal file SHA-256 still matches preflight. Other user files and unrelated
worktree registrations remain untouched. Temporary evidence staging is retained
as a second copy; the versioned evidence is self-contained.
