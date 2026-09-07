# Pre-Batch-1 foundation measurements

## Target and current status

The authorized target is post-reboot before/after and Rust measurement,
publication in the maintained docs and roadmap, one green final-head hosted CI
run, a merge commit and green main CI. No tag or version bump.

Complete: measurement and publication are merged after green final-head CI,
main CI passed on both platforms, and both corrected measurement checkouts and
their targets have been removed. No remaining work or blocker. The user's
“go ahead” authorized the identical two-cast correction described below.

## Source, host and protocol

- Original after base: `50531b45797ae765ec8d885165c13042617ab567`.
- Original before base: `v0.3.3-preview`, `d3cc6b96104dd597687a98e9624f800a0cb3cf1e`.
- Both unchanged release suites initially failed before timing: tasks_10000:40
  and tcp_fanout:62 pass int64 range values to int32 task parameters (AU2002).
- Added a CLI regression checking all four real release benchmark inputs. It
  failed with both diagnostics before the fix and passed after the two casts.
- After explicit user authorization, applied the same two casts to the canonical
  inputs and to clean temporary commits on both bases. Corrected after:
  `4e1e48c81bae6c62a54af763a4046901820038ec`; corrected before:
  `ddeaddf74301322fc96d8c09742ea12faa1dd8c3`. The two diffs are identical:
  SHA-256 `f5161a05eff7e243e6c862282759e7945158ac1feb29c814263c87ffa11899ff`.
  Compiler/runtime and runner code remain unchanged.
- A verified Git bundle preserves both exact corrected commits. It requires the
  two original base commits and exposes the measured-before/measured-after refs.
  It creates no release tag. Source records and diffs accompany it.
- Mac14,9 / Apple M2 Pro, 10 cores, 16 GiB, arm64 macOS 26.5.2 (25F84).
  Kernel boot seconds: 1788744561, 7 September 2026 02:29:21 BST, later than
  the original main record commit c43789e. The continuation uses the same boot.
- Xcode CPython 3.9.6 / NumPy 2.0.2; Rust 1.95.0 / Cargo 1.95.0;
  concurrent Rust references pin tokio 1.53.1.
- Built both release compilers and after Rust references before timing. Each
  runner also completed its workload builds before measured observations.
- Initial host refusals were retried ten minutes apart; retry 11 became quiet.
  The approved continuation's three preflight inventories were empty. Every
  successful release/Array report has three empty inventories and clean sources.
  No override was used; no service was stopped and no unrelated work ran during
  timing. Each maintained paired protocol uses 11 observations and excluded warmup.
- Arrays and diagnostic standalone integers retain their original-base identities
  from earlier in this boot. Corrected release runs use their own exact commits
  and each base's own unchanged runner. These families are kept separate.

## Measurements and interpretation

Release protocol and V6 results, and both Array reports, are contractual.
Standalone integer/Rust comparisons are diagnostic-grade: exact checksums,
alternating pairs, warmups and quiet external pre/post inventories, but without
the release suite's three-phase host and full input/binary hash rechecks.
All Rust protocol/checksum checks passed; no workload was excluded.

| Protocol workload | Aura after ms | CPython after ms | Rust after ms | Aura/Rust ratio of medians |
| --- | ---: | ---: | ---: | ---: |
| fib30 | 89.359292 | 158.321541 | 3.012292 | 29.664884 |
| tasks_10000 | 98.692417 | 51.786208 | 3.395167 | 29.068501 |
| tcp_fanout | 104.268375 | 108.793000 | 104.597250 | 0.996856 |
| retrying_worker | 428.814667 | 522.391625 | 472.607250 | 0.907338 |

| Workload | Aura before ms | Aura after ms | Change in time |
| --- | ---: | ---: | ---: |
| fib30 | 110.397208 | 89.359292 | -19.0566% |
| tasks_10000 | 100.303375 | 98.692417 | -1.6061% |
| tcp_fanout | 104.534666 | 104.268375 | -0.2547% |
| retrying_worker | 427.788875 | 428.814667 | +0.2398% |
| aura_int32 | 36.445833 | 28.954291 | -20.5553% |
| aura_int64 | 15.148792 | 12.772500 | -15.6863% |
| Array add per operation | 1.124738 | 1.205427 | +7.1740% |
| Array sum per operation | 1.149820 | 1.148677 | -0.0994% |

After Array Aura/NumPy/Rust medians are add 1.205427/0.247190/0.244576 ms,
sum 1.148677/0.168981/0.858175 ms; Aura/Rust ratios are 4.928642/1.338511.
Standalone int32 Aura/Rust medians are 24.325417/16.429334 ms, int64
9.778250/16.337417 ms; paired median ratios are 1.478258/0.594855. Those
observations are not mixed with the qualified V6 medians to form ratios.

CPython control drift: fib +0.1489%, tasks +0.5072%, TCP +0.1845%, retry
+0.2090%, integer +2.8059%, startup +4.4642%. NumPy drift is add -0.0208%,
sum -0.0394%. All are below 5%; no repeat was required. After V6 startup-adjusted
int32/int64 medians are 24.342791/6.126791 ms; both before and after retain
11 int32 and nine int64 positive adjustments. All whole-process pairs remain.
The before standalone runner's rounded minima are supplemental, not medians.

The full foundations bundle lowers fib/integer medians by about 16–21%, leaves
concurrent task/network/retry results much closer, and makes Array addition
7.1740% slower while sum is essentially unchanged. This compares Cranelift
speed together with release/link tuning and cannot isolate the flag's effect.
Large fib/task gaps against Rust and Array kernel costs are Batch 7 inputs;
no backend change or numeric target is ratified.

## Size decision

After timing, rechecked the pinned Cargo 1.95 reference: its release defaults
list strip=none. Decision 8 therefore retains the existing size table and
provenance without remeasurement or script changes. Explicit none is distinct
from omitting strip, for which Cargo may remove existing debug information
automatically. Both the original reference check and after-timing recheck are
retained with reference URL, source hash and exact Cargo version.

## Published surface and evidence

Updated Performance and Numeric Arrays chapters, all four benchmark READMEs,
0.3.4 Unreleased changelog, roadmap items 1 and 3 (measured and published),
ADR-0064 Batch 7 inputs, Manual shell-block metadata and generated llms files.
The two canonical benchmark casts and the CLI regression are the only code/test
changes. User personal files, .swp and the ADR-0022 draft remain untouched.

The versioned [evidence directory](2026-09-07-foundations-measurements/) retains
raw/summary JSON byte-for-byte, original failures, build logs, host checks,
authorization/source records, corrected-commit bundle and regression evidence.
Every file is covered by SHA256SUMS. The prior partial manifest and aggregate
are retained with initial- prefixes; originals inside approved/ retain their
own recorded temporary paths. Each summary's raw-report hash matches its copy.

| Evidence | SHA-256 |
| --- | --- |
| `approved/aura-foundations-after-release-raw.json` | `a2a31328af601c32783519d9658ab164d9b415046b03b5b7856b6e09c4c79441` |
| `approved/aura-foundations-before-release-raw.json` | `16aead5c0c9fe73ff2155e66b74edf982c8ec81958ab7ae3b4b2a6ae82498ec7` |
| `aura-foundations-after-arrays-raw.json` | `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f` |
| `aura-foundations-before-arrays-raw.json` | `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd` |
| `aura-foundations-after-integer-loops.json` | `8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb` |
| `approved/approved-measurement-commits.bundle` | `4db67544d886e7c15cae84609ddc6a6c784b0493cad9aac3c0e2df3e119e0cff` |
| `published-measurements.json` | `c11f387be513597e6355637543e1188e0a9cbe9fac546962fb29396ee2b7be00` |
| `SHA256SUMS` | `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67` |

## Verification and integration

Local checks passed: the new CLI regression (red then green); formatting;
100 benchmark/Rust-reference unit tests; 36 packaging tests; 15 identity tests;
two generated-document tests and freshness; complete Manual/reference gate;
340 tutorial fences (215 check-pass, five check-fail, 120 fragments); production
docs build; 58 manifest entries and all four raw/summary links; source-bundle
verification. The identity wording check caught one defensive sentence, corrected
before its successful rerun. Documentation/source whitespace checks pass; exact
logs and unified-diff evidence preserve their original diagnostic/context bytes.

[PR #7](https://github.com/johnolafenwa/Aura/pull/7), “Publish pre-Batch-1
foundation measurements,” contains the work. The prior partial draft's full
hosted CI [34081578524](https://github.com/johnolafenwa/Aura/actions/runs/34081578524)
passed on both Ubuntu and macOS. An earlier packaging text assertion was fixed
by documenting Aura / CPython explicitly; its final version has the full table.
The completed publication passed one full green final-head run before merge,
followed by one green main run. No version bump or tag.

Root target was 4.6 GiB with 92 GiB free before continuation builds; after local
checks it was 4.7 GiB with 90 GiB free. Both original temporary checkouts were
removed after their evidence was hashed. After main CI passed, the two corrected
checkouts were confirmed clean and lsof reported no open files beneath them;
both were removed with git worktree remove, including their 810 MiB / 637 MiB
targets. Their two temporary local refs were deleted with expected-old-SHA
checks; the verified versioned bundle preserves the exact commits. Unrelated
worktree registrations were left alone. Root target remains 4.7 GiB and free
space is now 92 GiB. User files remain unchanged by this work.

## Completed integration

Final-head CI [34102193853](https://github.com/johnolafenwa/Aura/actions/runs/34102193853)
passed on macOS and Ubuntu at `5e045cdfb7a5d927cb5bddadfeb21747a26c0e20`.
Final-head Docs [34102193940](https://github.com/johnolafenwa/Aura/actions/runs/34102193940)
also passed. PR #7 merged with merge commit
`052d342658c8b3344ad291d4136cc5705746d2d4`; its tree is byte-identical to the
validated PR head. The local checkout is now main with user files preserved.

Main CI [34109940364](https://github.com/johnolafenwa/Aura/actions/runs/34109940364)
passed at the merge commit: macOS completed at 11:19:30 UTC and Ubuntu at
11:50:33 UTC on 7 September 2026. Main Docs [34109940381](https://github.com/johnolafenwa/Aura/actions/runs/34109940381)
passed and deployed. An HTTP 200 check of
https://johnolafenwa.github.io/Aura/manual/performance verified the new fib/Rust
values, corrected source commit and current manifest hash. The returned HTML
SHA-256 was `d812c9b0a6b4a26ab16c7b75d7038553b780f1a2a6a46b1c6e51a37c794e573e`.

All 58 evidence manifest entries and the corrected-source bundle verified again
before cleanup; the manifest hash remains unchanged. This work note and task
board record the final CI and cleanup results in a work-only follow-up on main.
The repository's CI path filters exclude work-only changes; the complete green
main run above validates the merged implementation and publication tree.
