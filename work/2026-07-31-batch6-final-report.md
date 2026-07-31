# Aurora Batch 6 final report

Date: 2026-07-31

## Release verdict

Aurora 0.2.0 technical preview is ready for the user's final remote-runner
verification and publish decision.

The release candidate passed the complete local CI contract, the final
compiler and language-server coverage gates, the 30-program fresh-eyes corpus,
the post-reboot benchmark qualification, the claims audit, and installed
archive smoke tests. No commit or tag was pushed, and no release was
published.

Release identity:

- local annotated tag: `v0.2.0-preview`;
- tag object:
  `093ef98ece62bb4fb262f29b078b4e8e6ec8bcd1`;
- peeled implementation commit:
  `003ca88502077ee1706686722de16cc01f4c8b96`;
- implementation subject: `Ratchet final compiler coverage floors`;
- release-preparation commit: `b6230af`;
- final behavior-focused coverage closure: `b2fdfdc`;
- one-time final coverage re-ratchet: `003ca88`; and
- product version: `0.2.0` in Cargo, npm, language-server, and extension
  manifests and locks.

The main worktree intentionally still contains the user's modified
`personal/file_ops.au` and untracked
`architecture_docs/decisions/0022-implicit-shared-capability-syntax.md`.
Neither file was modified, staged, committed, packaged, or otherwise absorbed
by Batch 6. Every release build used a clean detached checkout of the peeled
tag commit.

## Final gates

The exact clean `npm run ci` replay ran with Node 22.14.0 from detached commit
`003ca88502077ee1706686722de16cc01f4c8b96` and passed:

- formatting;
- 54 scalable-runtime benchmark-harness tests;
- 10 numeric-Array benchmark-harness tests;
- 23 consolidated release-performance harness tests;
- 9 release-packaging tests;
- 334 CLI tests;
- 1,498 compiler-library tests and every compiler integration target;
- 6 retry, 4 FFI acceptance, and 2 closure acceptance tests;
- the full forced MIR/direct fixture matrix in 772.00 seconds;
- 101 language-server tests;
- extension build/check and 19 extension tests;
- compiler coverage;
- 100% language-server coverage;
- reference integrity and documentation build;
- npm and Cargo audits;
- Clippy with warnings denied; and
- repository hygiene.

Final CI evidence:

- log:
  `/private/tmp/aurora-b6-final-ci-003ca88.log`;
- log SHA-256:
  `a111b1e4f2094a33038caa2639659c81731fac56a6b42341986914f29b19bc45`;
- coverage JSON:
  `/private/tmp/aurora-b6-final-ci-003ca88-coverage.json`; and
- coverage JSON SHA-256:
  `7a8ddfb2b98795c731554c13d3f43fc61652c62f5018b0f5e45464ac72b62f8c`.

The final CI coverage replay covered:

- 86,655/90,002 compiler lines: 96.28119375124997%;
- 5,706/5,870 compiler functions: 97.206132879046%;
- 126,863/134,075 compiler regions: 94.62092112623532%; and
- 938/938 LSP statements, 251/251 branches, 49/49 functions, and 938/938
  lines: 100%.

The single end-of-Batch-6 downward-truncated compiler floors are
`96.28/97.20/94.62`, raised from the Batch 5 floors
`96.18/96.97/94.62`. The deterministic pre-ratchet evidence at `b2fdfdc`
covered 126,862 regions; the final CI replay covered one additional
scheduler-dependent runtime region. The floor was not changed a second time.

The coverage closure used grouped/tuple lambda-type diagnostics and the
ordinary syntax-error behavior of slice and literal lambda bodies followed by
an extra colon. No synthetic execution-only test or coverage exclusion was
added. An experimental slice-span assertion and a redundant mixed-tuple case
were discarded because they did not add a distinct behavior or coverage
claim.

The final Manual inventory contains 37 pages, 260 fenced blocks, and 126
verified blocks. Every feature page has its normative sections and a verified
executable example. Reference integrity, all migration manifests, and the
rendered documentation build passed.

## Six-batch evidence ledger

### Batch 1 — normative reference and Phase 1.5

Phase 1 recovery landed in `5c2c297`, `86a93a9`, `9a50cca`, `c817060`, and
`cf9dbc5`. Phase 1.5 landed in `856054c`, `9b49e0d`, `f82b6a3`, `21365e6`,
`e250ef4`, and `683b0cf`; coverage closure was `c30ebfe`; the reference
freeze was `a15df0f`.

The checkpoint passed 242 CLI, 552 compiler, 53 LSP, and 8 extension tests,
plus forced backend parity. The reference had 29 pages, 226 fences, and 101
verified blocks. Coverage was 96.065820% lines, 96.796738% functions, and
94.158721% regions; floors became `96.06/96.79/94.15`.

### Batch 2 — Phases 3, 3.5, and 4

B2.0 commits were `8bca972`, `8590cc3`, `19d8de6`, and `2acdbb1`.
Phase 3 commits were `97d0c7c`, `b268c72`, `c3df960`, `5889227`, `38f8cad`,
`f7e7965`, and `9ff7e82`. Phase 3.5 commits were `456a99a`, `1380b8d`,
`f50b206`, `a96f115`, `575962c`, and `6b56d1a`. Phase 4/V6 commits were
`7898659`, `6ffbe40`, `9eca65b`, and `8fbb2c7`. The checkpoint and ruling
records were `e61b6e6`, `4929bab`, and `19a10f4`.

Every semantic commit passed full CI. Final coverage was 96.076910% lines,
96.810244% functions, and 94.298491% regions; floors became
`96.07/96.81/94.29`. Forced parity, 100% LSP coverage, extension, reference,
docs, audits, Clippy, and hygiene passed. The checkpoint does not preserve one
authoritative final LSP/extension count, so this report does not invent one.

### Batch 3 — capability-syntax migration

B3.0 landed in `6afe47c`, `fc22696`, `e05c5e6`, `79174dd`, and `7998cc7`.
The migration was `d9382a0`, `9f7cb3f`, `aae9498`, `ec90ad5`, and `3d7827b`.
The initial checkpoint was `91e0d5f`; corrective closure was `1c249ab`.

The final gate passed 268 CLI, 928 compiler, 79 LSP, and 13 extension tests,
with 732.74 seconds of parity. The reference had 34 pages, 246 fences, and
118 verified blocks. Coverage was 96.134971% lines, 96.885813% functions,
and 94.349671% regions; floors became `96.13/96.89/94.35`. No synthetic
coverage test or exclusion was added.

The old same-name/different-type sibling match-arm binding-slot report is
closed, not a release limitation. Commit `1c249ab` allocates distinct typed
MIR slots; the maintained generalized regression passes, and the original
`random.Rng`/`Holder` shape now produces identical output on MIR and direct.
Older Batch 3 follow-up prose is superseded by that corrective commit.

### Batch 4 — Phase 5 scalable runtime

B4.0 was `5cb4476`, `4f0461e`, and `665d540`.

- 5.1: `850e906`, `7420bc2`, `1de9cf7`, `df104fa`;
- 5.2: `d22ae10`, `57e3816`;
- 5.3: `a339c61`, `af03d15`, `f8fcf8`;
- 5.4: `5af134a`, `0dddb43`, `f72fd2f`;
- 5.5: `ea92897`, `015db33`;
- 5.6: `7dcdd70`, `8d7f984`;
- 5.7: `6fb5efb`, `f601fc7`;
- 5.8: `ec3fd61`, `3e15b8a`, `dcb7667`;
- 5.9: `cc450c9`, `d921313`, `7df4df2`; and
- 5.10: `ad6bef6`, `29ff7f6`, `1e1263d`, `e171420`, `c3278c4`, `181204b`.

The checkpoint was `77c999d`; its final verification record was `4c9d9a2`.
The gate passed 45 benchmark tests, 300 CLI, 1,150 compiler, 90 LSP, and 13
extension tests, with 685.76 seconds of parity. Coverage was 96.131917%
lines, 96.903646% functions, and 94.466446% regions; floors became
`96.13/96.90/94.46`. LSP coverage remained 100%.

### Batch 5 — Phase 6 callables, closures, and FFI

B5.0 was `616ac71`, `e25387d`, `22a9073`, `e93e789`, `8dc509b`, `93d9f4f`,
`90fe059`, `14f2b8b`, and `af80b3f`. Phase 6.1 was `8a6dbd9`; 6.2 was
`de91f41`; 6.3 was `e1feb04`; 6.4 was `3c8b0cd`; the design-only Phase 6.5
record was `5b87b0b`; the checkpoint was `8131ebe`.

The final gate passed 49 runtime-harness, 320 CLI, 1,385 compiler, 6 retry,
4 FFI, 2 closure, 97 LSP, and 15 extension tests. Coverage was 96.181573%
lines, 96.970796% functions, and 94.629319% regions; floors became
`96.18/96.97/94.62`. LSP coverage remained 100%. No synthetic coverage test
or exclusion was added.

### Batch 6 — Phase 7, audit, and release preparation

B6.0 landed in `ce1258d`, `3616c11`, `1865415`, `659b877`, `49ae8bb`, and
`f29cd3e`. Comprehensions landed in `c7170b5`, `e8c7af1`, `5609d74`, and
`6291344`. Slices landed in `1903aae` and `fd465b4`. Arrays landed in
`0511adf`, `0609cda`, `9371be3`, `bff9899`, `f844b87`, `465d0a0`, and
`86ff95a`. Fresh-eyes closure was `4c4fba2`; consolidated performance
harness/evidence were `18c45ac` and `366e2f0`; release preparation was
`b6230af`; final coverage closure was `b2fdfdc`; final re-ratchet was
`003ca88`.

Stage gates grew from B6.0's 320 CLI/1,386 compiler/97 LSP/15 extension to
334 CLI/1,498 compiler/101 LSP/19 extension after Arrays. Comprehensions
passed 324/1,417/99/17 and slices passed 326/1,436/100/18. The final exact
gate is recorded above.

## Complete decisions ledger

No provisional ADR remains open. ADR-0038 is an Accepted future design with an
explicit implementation boundary, not an unresolved provisional decision.

1. **ADR-0001 — contextual `None` and `Option` equality.** Accepted.
2. **ADR-0002 — integer division and modulo.** Accepted; amended by Phase 3
   floor-division semantics.
3. **ADR-0003 — default integer type.** Accepted: `int` is `int64`.
4. **ADR-0004 — Unicode String semantics.** Accepted; amended for `int64`
   lengths and owned scalar-indexed slices.
5. **ADR-0005 — method receivers.** Accepted; spelling amended by ADR-0022.
6. **ADR-0006 — parameter, loop, Queue, and default modes.** Accepted and
   amended: bare is universally shared, Queue iteration is receive semantics,
   Range modifiers are rejected, shared/owned defaults are allowed, and
   mutable defaults are rejected.
7. **ADR-0007 — Duration.** Accepted: signed i128 nanoseconds.
8. **ADR-0008 — task-result ownership.** Accepted; static mechanism amended by
   ADR-0033.
9. **ADR-0009 — borrowed-return containment.** Superseded in part by ADR-0022:
   borrowed-return syntax is removed; owned-return containment remains.
10. **ADR-0010 — comparison chains.** Accepted with Python-style semantics.
11. **ADR-0011 — typed errors and assertions.** Accepted.
12. **ADR-0012 — conditions.** Accepted: conditions require `bool`.
13. **ADR-0013 — callable sequencing and ownership.** Accepted; amended by
    ADR-0022 and ADR-0037, with ADR-0038 as future loan work.
14. **ADR-0014 — Map behavior.** Accepted: duplicate keys, indexing, and
    assignment follow the settled ownership contract.
15. **ADR-0015 — explicit/default argument evaluation order.** Accepted.
16. **ADR-0016 — retained-place sequencing.** Accepted; spelling amended by
    ADR-0022.
17. **ADR-0017 — iteration-source selection.** Accepted; the source is
    selected once and later rulings cover Range/Queue distinctions.
18. **ADR-0018 — fixed resource limits.** Accepted.
19. **ADR-0019 — Duration conversions and timer policy.** Accepted.
20. **ADR-0020 — randomness and security boundary.** Accepted; amended for
    Transfer.
21. **ADR-0021 — JSON model and codec policy.** Accepted.
22. **ADR-0022 — capability syntax.** Accepted, ratified, and implemented:
    bare shared, `mut` mutable, `own` transfer; `borrow` is retired.
23. **ADR-0023 — bytes, codecs, and SHA-256.** Accepted; destination ceiling
    amendment included.
24. **ADR-0024 — assertion evaluation and diagnostics.** Accepted.
25. **ADR-0025 — delimiter continuation and layout.** Accepted.
26. **ADR-0026 — minimal tuples.** Accepted; amended for structural equality.
27. **ADR-0027 — conditional expressions.** Accepted.
28. **ADR-0028 — membership and comparison chains.** Accepted.
29. **ADR-0029 — `enumerate` and `zip`.** Accepted; amended for binding-slot
    isolation.
30. **ADR-0030 — `len` and `str`.** Accepted; amended for unified `int64`
    lengths and reserved names.
31. **ADR-0031 — CLI backend defaults.** Accepted: MIR for `aura run`, auto
    for `aura build`.
32. **ADR-0032 — guarded task stacks.** Accepted.
33. **ADR-0033 — structural Transfer and task-result consumption.** Accepted.
34. **ADR-0034 — heterogeneous typed `select`.** Accepted.
35. **ADR-0035 — configurable blocking pool.** Accepted.
36. **ADR-0036 — native structured diagnostic frames.** Accepted.
37. **ADR-0037 — value-capturing expression closures.** Accepted.
38. **ADR-0038 — place loans and views.** Accepted as design only;
    implementation is unstarted, targets Aurora 0.3, and changes no 0.2
    grammar or behavior.
39. **ADR-0039 — comprehensions.** Accepted and implemented.
40. **ADR-0040 — owned Vec/String slices.** Accepted and implemented.
41. **ADR-0041 — contiguous numeric Arrays and integer arithmetic modes.**
    Accepted and implemented.

## Fresh-eyes corpus

Thirty programs were written from a Python developer's point of view without
consulting compiler fixtures. They cover:

- 01–08: foundations, classes, traits, and ownership;
- 09–16: strings, files, bytes, JSON, process, and Result;
- 17–23: comprehensions, slices, collections, and integer modes; and
- 24–30: Arrays, tasks, Queue, retry, and HTTP.

Results:

- 30/30 `check` passed;
- 30/30 `fmt --check` passed;
- 60/60 forced MIR/direct executions passed;
- all 30 stdout streams were byte-identical; and
- consolidated TSV SHA-256:
  `0ff7a962c32116050c78685d881e1d3159d54ce19e74f3d39009c796dddbe13d`.

Findings and dispositions:

- `AU3005` for non-Copy Map indexing was intentional; its guidance led to the
  correct migration.
- Bare match is shared; moving payloads requires `match own`.
- String has no builtin `Ord[String]`; the docs now give concrete migrations.
- A real MIR `int16` wrapping panic was fixed test-first; all six boundary
  arithmetic methods now have backend-parity coverage.
- Apparent repeated native rebuilds were distinct cold keys plus changing
  runtime-archive identities; progress text now says “building native
  program.”
- The previously reported heterogeneous match-arm slot collision is already
  fixed and regression-pinned, as described in the Batch 3 section.

## Consolidated post-reboot performance evidence

The contractual report used the clean detached benchmark commit
`18c45ac63a02887328b434c06ce3ba08d046cea3` on:

- Mac14,9;
- Apple M2 Pro, 10 cores;
- 16 GiB memory;
- boot time 2026-07-30 23:02:25 +0100; and
- Xcode CPython 3.9.6, not free-threaded Python 3.13+.

Each lane used one excluded warmup and 11 rotating pairs. The raw report
SHA-256 is
`06cc1223630b1063c8a6806bf590449d6121a3be8d33e8dc1b0ffd17cee93ccb`;
the summary SHA-256 is
`4490e0d169d9a031ae57f04ade772d22169189f71a949356234f529d40e56236`.
The measured release `aura` binary SHA-256 is
`5d95d54345bb268aa7eeaef070142bcbca410ee8f82383126d6a0390df2b087e`.

| Workload | Aurora median | Comparator median | Aurora/comparator |
|---|---:|---:|---:|
| Naive `fib(30)` | 93.875250 ms | CPython 158.491666 ms | 0.592304 |
| Create/join 10,000 tasks | 101.743042 ms | CPython asyncio 51.950667 ms | 1.958455 |
| 20-client TCP fan-out | 104.505375 ms | CPython asyncio 108.605459 ms | 0.962248 |
| Retrying HTTP worker | 429.291292 ms | CPython asyncio 520.447791 ms | 0.824850 |
| V6 `int32`, whole process | 36.620333 ms | CPython 321.096625 ms | 0.114048 |
| V6 `int64`, whole process | 13.724042 ms | CPython 321.096625 ms | 0.042741 |
| Array float64 add, 1M | 1.142461 ms | NumPy 2.0.2 0.251602 ms | 4.540751 |
| Array float64 sum, 1M | 1.150392 ms | NumPy 2.0.2 0.174065 ms | 6.608975 |

Startup-adjusted V6 medians were 31.037083 ms for Aurora `int32` versus
295.458959 ms for CPython, and 7.737813 ms for Aurora `int64` versus
296.966042 ms for CPython over the 10 valid aligned pairs.

Array evidence was qualified separately. Its raw SHA-256 is
`f51b979977519b5cbca9be4119a77bb3aff1d1a2874e1cdd4269f315bc1f9e7d`;
its summary SHA-256 is
`f6fc84c1f0fadfb4b93a5f07befb5a33cbaa6926d54ef88a795e103106b410ab`.

These are exact workload measurements, not portable speed claims. TCP used
20 pre-bound listeners because accepted streams are not Transfer (`AU3008`).
CPython uses arbitrary-precision integers while Aurora lanes use fixed-width
`int32`/`int64`. The current Array kernels are scalar; no float-SIMD claim is
made. The earlier broad 100,000-sleeper RSS claim remains withdrawn.

## Claims and positioning audit

The maintained claims inventory reviewed 228 matched statements. Performance,
concurrency, and safety wording is now either connected to a passing gate or
retained measurement, explicitly scoped as historical/roadmap language, or
removed.

The root README and `docs/positioning.md` describe Aurora as a
Python-inspired compiled language for agent and systems control planes,
centered on compiler-checked ownership, scoped task concurrency, and typed
control-plane failures. The unsupported “memory safety of Rust” equivalence
was removed. “Deterministic ownership” is explicitly limited to move, sharing,
and cleanup behavior; it does not promise deterministic task scheduling.
Comparisons with Mojo, Nim, Go, and free-threaded Python are qualified rather
than reduced to slogans. The 0.2.0 technical-preview and unsafe FFI boundaries
are stated plainly.

## Release artifacts

All products below were built from the clean peeled tag commit
`003ca88502077ee1706686722de16cc01f4c8b96` and copied to the ignored local
directory `release/v0.2.0-preview-003ca8850207/`.

| Artifact | Target | Bytes | SHA-256 |
|---|---|---:|---|
| `aurora-v0.2.0-preview-aarch64-apple-darwin.tar.gz` | macOS arm64 | 22,804,780 | `2ef8377aedaf1eb5238c6244d4738245bb69ab6ce820db8830cd2fafc7115311` |
| `aurora-v0.2.0-preview-x86_64-apple-darwin.tar.gz` | macOS x86_64 | 23,708,610 | `d9d93637eb223956ff53c8ed9c70ecf09e5b17158fe00f7c5798f5b20d2a9916` |
| `aurora-v0.2.0-preview-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 glibc | 25,816,239 | `576991cc7a9479ea44a8a0e3972ec7f2dfae4263e9a552c205903709992f9d60` |
| `aurora-language.vsix` | VS Code | 212,140 | `f7cbd748385c5040a8799af1ca0f9c54a7511b2010067aa8a0df423a833ab225` |
| `aurora-docs-v0.2.0-preview.tar.gz` | VitePress `/Aurora/` | 2,110,826 | `c7d9758e17c4e8eabe6024afbb8337c0e034b3330d8859f9635efbda5a3a40b9` |

`SHA256SUMS` is 538 bytes with SHA-256
`da62866c627f3548d3692b8299b29e38dabfe06db9e331bc1365d8042658988a`;
all five entries pass `shasum -a 256 -c`.

Each CLI archive has one matching top-level directory with the executable,
compiler static library, nonempty native link manifest, root/CLI READMEs,
license, and packaged basic and retrying-worker examples.

All three CLI archives passed architecture inspection and installed smoke
outside the checkout with Cargo deliberately unavailable and an initially
absent isolated native cache. Each printed exactly `aura 0.2.0`; the basic
example printed `16`; the retry oracle ended in `requests 7` after all 15
expected lines. The macOS x86_64 proof used the exact Rust 1.95.0 Intel
toolchain under Rosetta and a temporary `clang -arch x86_64` wrapper so the
Apple-Silicon host linked Intel objects. The Linux archive was built and
smoked in an Ubuntu 24.04 amd64 container.

The VSIX has valid zip integrity, version 0.2.0 in both manifests, and bundled
extension and language-server JavaScript. The docs archive has a root index,
Manual index, `/Aurora/` base, the 0.2.0 technical-preview label, and the exact
full implementation-commit stamp.

These locally produced x86_64 products prove the archive contract, but they
do not replace later hosted-runner provenance from `macos-15-intel` and
`ubuntu-24.04`. The workflow's nonpublishing dispatch should still be used
after the commit is available remotely.

## Remaining limitations

- Aurora 0.2.0 is a technical preview, not a production-stability promise or
  an untrusted-code sandbox.
- Supported archives are glibc Linux x86_64 and macOS x86_64/arm64. Native
  builds still require a host C compiler/linker.
- Tasks are cooperative and pinned: there is no preemption, migration, work
  stealing, detached task model, deterministic sibling order, or universal
  speedup promise.
- The robust 100,000-sleeper RSS claim is withdrawn; maintained task evidence
  is limited to qualified 10,000-task/sleeper workloads.
- Slices are owned copies. Place/view returns and mutable/in-loan closure
  capture await ADR-0038.
- Arrays are CPU-only and intentionally narrow: no views, general
  broadcasting, mixed-dtype promotion, shape transforms, autograd, or
  accelerators. They are slower than NumPy in the retained add/sum lanes.
- FFI v0 is a trusted synchronous direct-call boundary. It has no callbacks,
  variadics, returned views, nullable handles, or dynamic library selection.
- Package manifests and Git/path dependencies exist, but no Aurora package
  registry or package publishing implementation exists.
- String has no builtin ordering contract.
- A persistent kernel-level `mio::Waker` failure still lacks a formal portable
  recovery path without a second control primitive.
- `AU3006` retains unconditional clone wording; this is diagnostic-polish
  debt, not a semantic defect.

## User handoff commands

Nothing below was executed during Batch 6.

First make the implementation/report commits available, without pushing the
tag:

```bash
git fetch origin
git push --dry-run origin main
git push origin main
```

Then run the supported hosted matrix without publishing:

```bash
gh workflow run release.yml --ref main \
  -f source_ref=003ca88502077ee1706686722de16cc01f4c8b96 \
  -f release_tag=v0.2.0-preview \
  -f publish=false
```

After those runner artifacts pass, publish by pushing the already-created
annotated tag:

```bash
git push --dry-run origin refs/tags/v0.2.0-preview
git push origin refs/tags/v0.2.0-preview
```

The tag-push path automatically publishes the GitHub Release. Do not follow it
with a manual `publish=true` dispatch, because that can start duplicate
publication runs. If the automatic tag path is deliberately not used, the
safe manual publishing alternative after the remote tag exists is:

```bash
gh workflow run release.yml --ref main \
  -f source_ref=v0.2.0-preview \
  -f release_tag=v0.2.0-preview \
  -f publish=true
```

The workflow preflight resolves the selected source once and refuses manual
publication unless the tag peels to that exact source commit.

## First three Aurora 0.3 priorities

1. Implement ADR-0038 end to end: stable places, typed loans, inferred
   regions, unified exit actions, owned/view returns, and explicit
   in-loan/mutable closure capture across compiler, both backends, LSP, and
   editor.
2. Rework runtime resilience and task storage: formal kernel-waker recovery
   plus a stackless or safely copy/decommit architecture that can revisit the
   withdrawn massive-concurrency RSS claim honestly.
3. Build Array v2 on the settled view model: view-aware slicing,
   demonstrated SIMD/vectorized kernels, shape/broadcast operations, and a
   refreshed qualified NumPy benchmark protocol.

## Stop condition

Batch 6 implementation, audit, local tagging, local artifact proof, and this
report are complete. No push or publication was performed. The next actions
are the user's remote-runner verification and publish decision.
