# Pre-Batch-1 foundations, items 1–4

## Goal and authorized scope

Deliver Cranelift speed optimization (subject to parity), a backend boundary
inventory, equivalent Rust benchmark lanes, release-profile/link improvements,
and deterministic executable-size evidence for the 0.3.4 update. No release,
version bump, tag, reboot, or publication-grade timing measurement is authorized.
Branch: `codex/pre-batch-1-foundations`. Preserve the existing personal file,
`.swp`, and the untracked ADR-0022 draft.

## Progress and verification

- Baseline recorded release coverage: 96.307730% lines, 97.210239% functions,
  94.810026% regions (prior release evidence, not a new measurement).
- Enforced floors remain 96.30/97.21/94.71.
- Test-first flag regression failed with `None` versus `Speed` as expected.
- Test-first platform-link regression failed because the helper was absent.
- COMPLETE: implementation, local gates, final-head size confirmation, branch
  and main hosted CI, merge integration, and measurement handoff are delivered.

## Remaining work

No implementation or integration work remains. Publication-grade optimization
and Rust-baseline timings are reserved for the separate post-reboot session.

## Verification notes

- All nine Rust workloads pass one exact protocol/checksum smoke run; no timing
  is published. Standalone `cargo audit` found no vulnerabilities in 10 packages.
- Python runner/size/provenance tests pass (44). Release packaging tests pass (36).
- Reference gate exposed a pre-existing post-publication metadata test that still
  required an unreleased 0.3.3 changelog. Updated it to require the published
  2026-09-06 entry and new 0.3.4 unreleased section; manifest versions stay 0.3.3.
- Plain `git diff --check` reports existing whitespace in protected
  `personal/file_ops.au`; the repository hygiene gate explicitly excludes it
  and passes. The file remains untouched.

- The first full Rust run passed 374/375 CLI tests but failed the cache entry-id
  recovery assertion during concurrent runtime-archive refreshes from other
  compiler gates. Daybreak's read-only investigation identified content-key
  drift; the exact isolated cache regression passed (1/1). The isolated
  direct/MIR stripped source-frame test also passed (1/1). No link reversion
  is indicated. Subsequent compiler gates run without competing builds.
- Cache format v6 invalidates the prior artifact construction pipeline, keeping
  product and semantic-schema versions unchanged. Its regression failed first
  with v5, then passed after the single constant change.

- The uncontended outer Rust run passed 374/375 CLI tests, including cache
  recovery. Its one failure was the manual CFG/view object's linker reporting
  `target/debug/libaura_compiler.a` absent during another test's archive rebuild
  (cli.rs:1291), before execution. This is shared parallel-test build state,
  not a generated-code mismatch; an isolated rerun is recorded below.

- The isolated CFG/view regression passed (1/1). All compiler suites passed:
  1,882 compiler unit/native-codegen tests, all fixture suites, and the remaining
  package, FFI, semantic-interface, runtime, and diagnostic integration suites.
  No fixture expectation or codegen workaround was changed.

- Forced MIR/direct parity passed all 385 runtime fixtures (291 run-pass and
  94 run-fail), fallback disabled and loopback available. Exact stdout and
  diagnostics matched the unchanged fixture oracles. Cranelift speed and both
  link steps passed this checkpoint; final coverage and hosted confirmation
  are recorded below.

- Clippy passed with warnings denied. After parity, `cargo clean --profile dev`
  removed obsolete debug outputs: physical `target/` usage fell from about
  20 GiB to 6.7 GiB, with 64 GiB free before coverage. Coverage retains the
  exact existing 96.30/97.21/94.71 floors.

- The coverage workspace run passed all 375 CLI integration tests serially,
  including both regressions affected by archive replacement during parallel
  runs. The CLI unit suite passed 34/34; native-codegen, FFI, backtrace, and
  package acceptance suites also passed.

- Full serial workspace coverage passed: 101,387/105,275 lines (96.306815%),
  6,760/6,954 functions (97.210239%), and 149,598/157,790 regions (94.808289%).
  All three unchanged floors are met. The complete workspace test run was green.
- Coverage outputs and the now-obsolete coverage-only uninstrumented runtime
  target are cleaned before the separate release-profile diagnostic check.

- Added a test-first regression requiring Rust baseline builds to use the
  shared owned-process-group helper. It failed against the original subprocess
  call, then passed after using the existing timeout/interrupt cleanup path.
  All 43 benchmark tooling tests remain green; workload protocols are unchanged.

- Actual fat-LTO release-profile direct/MIR launcher regression passed, retaining
  source frames with absent source files and unavailable Cargo in child execution.
  Rust 1.95.0's matching `llvm-nm` confirms the runtime archive retains
  `aura_native_run` and 356 direct runtime exports. Apple's older `nm` cannot
  parse the newer embedded LLVM attributes; this is a reader-version mismatch,
  not a link failure. The archive is not post-link stripped.
- Size-command cleanup also has a red/green regression for the shared owned
  process-group helper. All 44 runner/size/provenance tests now pass.
- Full local gates passed (parallel build-state failures classified above).
  Size publication, final docs checks, hosted branch/main CI, merge, handoff,
  and cleanup subsequently completed as recorded below.

## Executable-size evidence

The initial clean-ref measurement uses v0.3.3-preview and implementation commit
`11ce755dd97386c2aa230ba16b5f7e26a675e2cf`. The final branch head was confirmed
separately before integration, as recorded below. The canonical raw report is
`work/2026-09-07-pre-batch-1-executable-sizes.json`.

| Subject | Before bytes | After default bytes | After tuned bytes | Reduction |
| --- | ---: | ---: | ---: | ---: |
| `aura` compiler | 15,424,032 | 15,866,008 | 10,897,392 | 29.35% |
| Native hello world | 23,646,792 | 1,702,088 | 1,586,968 | 93.29% |
| Retrying-worker stand-in | 23,715,816 | 4,049,240 | 3,666,600 | 84.54% |

All three clean builds passed standalone hello/worker execution with identical
source bytes and full output, and Cargo unavailable. The tuned profile uses fat
LTO; no fallback to thin was needed locally. All size worktrees and target trees
were removed after hashing. Timing measurements remain pending after reboot.

- Final documentation gates passed after registering the new size-command fence:
  Manual reference replay (129 verified Aura blocks), 340 tutorial fences,
  generated LLM check, production docs build, 15 identity tests, 36 packaging
  tests, and repository hygiene. The fence is orchestration metadata; no
  language fixture expectation was changed.

## Hosted verification

- PR: https://github.com/johnolafenwa/Aura/pull/6.
- Initial hosted CI at `9f24820` failed on both hosts in the existing scalable
  runner test: its integer-helper mock returned empty output and expected the
  old label. The stricter integer runner correctly requires `10000000` plus a
  newline. This same failure was present in an earlier local log and was missed
  when the surrounding shell command's final status was inspected.
- Updated only that mock and its current label, preserving checksum validation.
  All 100 benchmark-tooling tests now pass together. No language fixture or
  runtime behavior changed. Final-head confirmation below supersedes the
  earlier size confirmation at `9f24820`.

## Final integration and provenance

- Final branch head: `7e1c54c0f505f3b8b31d3d51dc39ebed920ae753`.
- One complete hosted branch CI run passed on both macOS and Linux:
  https://github.com/johnolafenwa/Aura/actions/runs/34063004465.
- Branch Docs passed:
  https://github.com/johnolafenwa/Aura/actions/runs/34063004483.
- PR https://github.com/johnolafenwa/Aura/pull/6 merged with merge commit
  `50531b45797ae765ec8d885165c13042617ab567`. Its complete file tree matches
  the final measured branch head; no squash or rebase was used.
- One complete hosted main CI run passed on both macOS and Linux:
  https://github.com/johnolafenwa/Aura/actions/runs/34067729115.
- Main Docs passed:
  https://github.com/johnolafenwa/Aura/actions/runs/34067729084.
- Hosted branch and main gates include the ordinary parallel Rust suites, forced
  backend parity, serial coverage, all nine Rust protocol smoke programs,
  packaging/identity, LSP/extension, Manual/tutorial/docs, audits, and hygiene.
  The classified local parallel-build failures did not recur in branch CI;
  all 375 CLI integration tests passed on each host.
- Final clean-ref size confirmation at the branch head reproduced every table
  byte count and all standalone outputs. Canonical raw report SHA-256:
  `f3502eaab570f64d4924ed1829672caa80ba8e3a33dc8c973b49e8b4aea02cc0`.
  The canonical JSON now records that final branch head, replacing the initial
  implementation-head report. All detached size checkouts and targets are gone.
- Cranelift speed, fat LTO, dead-section collection, and post-link stripping are
  adopted. Hosted jobs passed within their configured budget. No reversion,
  coverage-floor change, language-fixture adaptation, or compatibility path was
  required. No version bump, tag, release preparation, reboot, or publication-
  grade timing run occurred.
- The standalone Rust audit found no vulnerabilities. The root audit gate also
  passed with the repository's existing allowlisted `rustls-pemfile`
  unmaintained-dependency advisory, `RUSTSEC-2025-0134`; the allowlist is unchanged.
- This post-main-CI update changes only work records and measured provenance.
  The measurement `after_commit` remains the verified merge commit below.

- Final `cargo llvm-cov clean --workspace` completed successfully after the
  handoff was written. No local Cargo/rustc/size jobs were active. `target/`
  occupies 4.7 GiB and the host has 65 GiB free. Protected personal files,
  `.swp`, and the ADR-0022 draft remain untouched.

## Handoff to measurement session

before_ref = v0.3.3-preview
after_commit = 50531b45797ae765ec8d885165c13042617ab567

Status: COMPLETE for pre-Batch-1 items 1–4 implementation and integration.
Optimization-level and Rust-baseline timings are not yet published. Reboot is
reserved for this measurement session; the implementation task did not reboot.

Adopted:

- Cranelift `opt_level=speed`: all 385 forced MIR/direct runtime fixtures and
  compiler/CLI acceptance gates passed, without changing fixture expectations.
- Backend boundary inventory and illustrative builder contract:
  `architecture_docs/15-backend-boundary.md`; no backend refactor or switch.
- Pinned Rust 1.95.0 / tokio 1.53.1 references: all nine exact protocol/checksum
  smoke checks passed. The three schema-2 runners enable their Rust lanes by
  default. Integer loops retain checked arithmetic and per-iteration
  `black_box`; Array add allocates fresh results and sum is sequential. TCP
  retains 20 pre-bound listeners and retries retain the existing local schedule.
- Release profile: level 3, fat LTO, one codegen unit, stripped compiler symbols,
  no debug data, default unwinding. Both native build paths use macOS
  `-Wl,-dead_strip` / Linux `-Wl,--gc-sections` and user-binary `strip -S -x`.
  Standalone execution, source frames, task ancestry, native caches, and
  packaging gates passed. Runtime archive globals remain linkable. Cache format
  v6 invalidates artifacts from the earlier construction pipeline.

Reverted: none. No version bump, tag, or release preparation was performed.

After reboot, use a clean detached checkout of `after_commit` on the maintained
Mac14,9 / Apple M2 Pro host. Keep the host quiet and do not use the competing-
process override. The commands below measure the after source with Rust lanes;
`before_ref` is the immutable baseline source for the before comparison.

Build the compiler and the standalone Rust references:

```bash
cargo +1.95.0 build --release --locked -p aura
cargo +1.95.0 build --manifest-path benchmarks/rust_baselines/Cargo.toml --release --locked --bins
```

Release-performance runner:

```bash
python3 scripts/bench-release-performance.py \
  --label foundations-after-rust \
  --aura target/release/aura \
  --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  --pairs 11 \
  --raw-json /tmp/aura-foundations-after-release-raw.json \
  --summary-json /tmp/aura-foundations-after-release-summary.json
```

Integer-loop runner:

```bash
python3 scripts/bench-direct-integer-loops.py \
  --aura target/release/aura \
  --repeats 11 \
  --raw-json /tmp/aura-foundations-after-integer-loops.json
```

Numeric-Array runner:

```bash
python3 scripts/bench-numeric-arrays.py \
  --label foundations-after-rust \
  --aura target/release/aura \
  --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  --pairs 11 \
  --raw-json /tmp/aura-foundations-after-arrays-raw.json \
  --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

Executable-size subjects:

- compiler executable: `target/release/aura`;
- hello-world source: `examples/basics/hello_world.au`;
- reference-agent stand-in: `examples/agents/retrying_network_worker.au`, until
  pre-Batch-1 item 6 lands.

Size measurements are already published in `docs/manual/performance.md` with
canonical provenance in `work/2026-09-07-pre-batch-1-executable-sizes.json`.
The before/default-after/tuned-after table is 15,424,032/15,866,008/10,897,392
bytes for the compiler, 23,646,792/1,702,088/1,586,968 for hello world, and
23,715,816/4,049,240/3,666,600 for the worker stand-in.
