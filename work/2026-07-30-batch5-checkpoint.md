# Batch 5 checkpoint: callables, closures, and FFI

- Date: 2026-07-30
- Entry commit: `4c9d9a2`
- Implementation checkpoint: `5b87b0b`
- Status: complete
- Stop boundary: Phase 7 and release work have not started

## Verdict

Batch 5 is complete at its authorized checkpoint. Every mandatory Batch 4
defect is closed, the Phase 6.1 through 6.4 language and library surfaces are
implemented on both maintained backends, and Phase 6.5 ends at the required
proposal-only boundary. The final repository gate is green apart from the
known global whitespace check against the unrelated user-owned
`personal/file_ops.au`; the entire Batch 5 surface and all remaining hygiene
rules pass independently.

The checkpoint does not ratify its own provisional decisions:

- ADR-0037 remains Provisional. Its complete implementation and parity matrix
  support accepting its by-value closure model at the next ruling.
- ADR-0038 remains Proposed and explicitly unimplemented. Its recommendation
  is Aurora 0.3, after an explicit ratification pass.

## B5.0 defect disposition

### B5.0-a: load-dependent join cancellation

Commit `e25387d` replaces elapsed-time cancellation with a reachability
decision over live task waits, Queue directions and handles, and nested join
dependencies. The load fixture surrounds four capacity-64 Queue producers and
four outer consumers with 12 CPU burners. All 4,000 values arrive on MIR and
direct. Separate fixtures prove true deadlock cancellation, joining-parent
exclusion, dynamic Queue handoff, and cross-join-cycle handling.

### B5.0-b: nested select payload typing

Commit `22a9073` preserves concrete types through nested generic enum patterns.
Arithmetic, comparison, f-string interpolation, typed rebinding, and
reassignment pass on MIR and direct. This completes ADR-0034's matrix.

### B5.0-c: blocking-pool watchdogs

Commit `e93e789` serializes only the two tests that mutually contend for the
bounded blocking pool, documents why, and adds the pinned-nightly Linux
ThreadSanitizer scheduler job. Repeated default-parallel runs and the full
workspace gate pass. This completes ADR-0035.

### B5.0-d: V6 baseline reconciliation

Commit `8dc509b` adds schema-4 startup/loop split measurements. The accepted
disposition records the clean whole-process baseline and treats the dirty
split only as evidence that the harness separates fixed and loop costs; it
does not claim an unproved reactor-initialization cause.

### B5.0-e: race detection

The Linux nightly ThreadSanitizer job in `e93e789` covers the scheduler unit
family and multicore/TaskGroup CLI paths. Local normal-mode sanitizer target
selection covers 214 scheduler/runtime-value tests plus the relevant CLI
families.

### B5.0-f and gate hardening

Commit `616ac71` adds human and JSON native-build wait progress, records MIR
multicore contention, and closes the residual shared-access wording. No
least-loaded admission policy landed because available evidence did not
justify one. Commits `90fe059` and `14f2b8b` make the zero-producer Queue
watchdog and coverage-profile build-lock regressions deterministic. Commits
`93d9f4f` and `af80b3f` record the accepted rulings and closed gate.

B5.0's exact settled gate passed 308 CLI tests, 1,157 compiler tests, forced
backend parity, 90 LSP tests, 13 extension tests, both coverage gates,
reference integrity, docs, audits, warning-denied Clippy, and hygiene.
Coverage was 71,457/74,328 lines (96.137391%), 4,800/4,953 functions
(96.910963%), and 104,919/111,056 regions (94.473959%).

## Phase 6 commits and gates

| Stage | Commit | Result |
| --- | --- | --- |
| 6.1 capture-free function values | `8a6dbd9` | First-class named and capture-free lambda values, structural `def(...) -> R` types, indirect calls, generic specialization, storage, and TaskGroup targets on MIR/direct |
| 6.2 callable-powered standard library | `de91f41` | Stable `Vec.sort`/`sort_by`, eager `map`/`filter`, and `control.retry` with exact failure, backoff, and cancellation semantics |
| 6.3 value-capturing closures | `e1feb04` | Expression lambdas, by-value Copy/move capture, repeatable/consuming call kinds, ownership/Transfer checks, analysis/LSP, extension, and parity |
| 6.4 manifest-gated FFI v0 | `3c8b0cd` | Manifest authorization, exact dependency reporting, C scalars/views/opaque handles, shared libffi engine, MIR/direct lowering, editor/reference surface, and maintained real process-symbol example |
| 6.5 loan/view design | `5b87b0b` | Proposed ADR-0038 only; no language, compiler, backend, editor, tutorial, or Manual implementation |

The per-stage compiler coverage results were:

| Stage | Lines | Functions | Regions |
| --- | ---: | ---: | ---: |
| B5.0 | 96.137391% | 96.910963% | 94.473959% |
| 6.1 | 96.15% | 97.04% | 94.53% |
| 6.2 | 96.14% | 96.97% | 94.55% |
| 6.3 | 96.133899% | 96.923367% | 94.542331% |
| 6.4 canonical commit | 96.180377% | 96.970796% | 94.628512% |
| Final checkpoint | 96.181573% | 96.970796% | 94.629319% |

Every coverage-closing test pins an observable semantic outcome, diagnostic,
runtime result, manifest policy, or backend-parity contract. No synthetic
coverage test, production coverage-only edit, or coverage exclusion was
added.

## Final checkpoint verification

The clean implementation replay passed:

- formatting and all 49 scalable-runtime harness tests
- 320 CLI tests, including both B5.0-c watchdogs under default parallelism
- 1,385 compiler-library tests
- 6 retry, 4 FFI acceptance, and 2 closure acceptance tests
- every remaining Rust unit, fixture, and integration target
- the forced MIR/direct parity matrix
- 97 language-server tests and 15 extension tests
- compiler and language-server coverage gates
- executable reference integrity and the documentation build
- npm audit with zero vulnerabilities
- RustSec audit with only the allowed `rustls-pemfile` unmaintained warning
- Clippy with warnings denied

Language-server coverage remains 100%: 937/937 lines, 49/49 functions, and
251/251 branches.

The final compiler totals are:

- 80,453/83,647 lines: 96.18157256088085%
- 5,346/5,513 functions: 96.97079629965536%
- 117,329/123,988 regions: 94.62931896635159%

The one-time Batch 5 re-ratchet therefore sets the downward-truncated floors
to 96.18% lines, 96.97% functions, and 94.62% regions.

Global `check:hygiene` reaches only pre-existing trailing whitespace in the
unrelated user-owned `personal/file_ops.au`. Excluding that file and the
untracked user-owned ADR-0022 draft, `git diff --check`, historical-commit
checks, artifact checks, executable checks, and scheduler-safety hygiene all
pass. Neither user-owned file is part of Batch 5.

## Maintained reference inventory

The normative reference is synchronized through:

- `docs/manual/functions.md` for structural function types and function values
- `docs/manual/closures.md` for lambda syntax, capture, callability, storage,
  ownership, Transfer, and maintained limits
- `docs/manual/collections.md` and `docs/manual/control-plane.md` for the new
  Vec algorithms and retry contract
- `docs/manual/ffi.md` for ABI v0, manifest authorization, marshalling,
  ownership, error containment, and exclusions
- the grammar, expression, type, static-semantics, execution-model,
  ownership, concurrency, package, diagnostics, API-index, status, limits,
  and conformance pages for cross-cutting rules

The teaching and executable surface includes:

- `tutorials/03-functions.md` and `examples/basics/function_values.au`
- `examples/modules/function_values.au`
- `examples/collections/vec_algorithms.au`
- `examples/agents/retry_with_backoff.au`
- `examples/basics/closures.au`
- `docs/learn/ffi.md`, `tutorials/26-ffi.md`, and the maintained
  `examples/packages/ffi_getpid` package

Reference integrity re-executes the maintained Aurora blocks and checks their
source hashes; these are not documentation-only claims.

## Provisional decisions and Batch 6 boundary

ADR-0037 should move to Accepted at the next authoritative ruling. Its
implemented decisions are expression-only lambdas; by-value Copy or move
capture; repeatable read-only and consuming call kinds; explicit rejection of
shared/mutable capability capture; structural Transfer only when every capture
is Transfer; and no closure-environment erasure into arbitrary written
`def(...) -> R` storage.

ADR-0038 should remain outside Batch 6 implementation unless explicitly
ratified and rescheduled. It proposes explicit place-based shared/mutable
views, one-origin returned-view contracts, inferred last-use regions, typed
MIR loans, stable backend storage, direct write-through, unified cleanup, and
exhaustive explicit in-loan lambda capture lists. The recommended target is
Aurora 0.3 because the stable-place and unified-exit work is a backend
correctness project rather than a surface-only addition.

The previously recorded direct-backend match-arm binding-slot isolation issue
remains a valid candidate for Batch 6 defect closure if it has not already
been superseded by the next batch authority. It should be reproduced against
this checkpoint before assignment.

Phase 7 and release work have not started.
