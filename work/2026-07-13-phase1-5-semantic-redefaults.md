# Phase 1.5 Semantic Re-defaults

## Session

- Started: 2026-07-13 13:58:22 BST.
- Paused: 2026-07-13 15:16:57 BST after 1h 18m 35s.
- Stop rule: complete Phase 1.5 and report, stop immediately on a ratified-rule gate failure or unforeseen ambiguity, or reach 12 continuous hours.

## Goal

Preserve accepted Phase 1 in logical commits, complete verification cleanup tickets V1-V5, unbox direct-backend int64/uint64 with measured parity, and execute the breaking Phase 1.5 migration in the ratified D3, D2, D4, D5, D6 order with a full gate and separate commit for every decision.

## Work completed

- Recorded the substantial-work session before beginning commit surgery or implementation.
- Preserved accepted Phase 1 as four logical commits: forced backend harness, semantic recovery, runtime hardening, and documentation/ADRs.
- Completed V1 by restoring an exact check-fail pin for mutable binding of a shared borrowed value without tripping D9 borrowed-return containment first.
- Completed V2 with a debug teardown assertion and failing-first regression that forbids suspended direct-generated tasks from reaching scheduler destruction with forced-exit cleanup still attached.
- Completed V3 with symmetric focused diagnostics for comparisons between `None` and non-optional values.
- Completed V4 by making Phase 4's real `aura run --backend mir` selector and same-change parity-harness update a durable architecture requirement.
- Completed V5 by documenting that timed-out/cancelled blocking-pool jobs occupy one of the fixed 2-8 workers until completion, plus the Phase 5 configurability and saturation/recovery test requirement.
- Captured the ticket 9 pre-change release-binary benchmark on a Mac14,9 Apple M2 Pro with 16 GiB RAM, arm64 macOS 15.7.8 (24G806): over 25 alternating, output-validated runs after five warmups, int32 median was 32.001 ms (MAD 1.135 ms, p95 37.527 ms) and boxed int64 median was 4223.969 ms (MAD 32.941 ms, p95 4274.286 ms), a 131.996x gap.
- Extended the direct scalar ABI to int64 and uint64 across contextual literals, function/task thunks, arithmetic, signed/unsigned comparisons and division, checked overflow, printing, typed boxing/unboxing, and checked unboxed numeric casts. Added exact MIR/direct fixtures for arithmetic, casts, signed upper/lower/division/multiplication/unary-negation overflow, unsigned upper/underflow/multiplication overflow, and cross-signedness cast failures.
- Captured the ticket 9 post-change release-binary benchmark on the same machine and protocol: int32 median was 30.677 ms (MAD 0.071 ms, p95 30.764 ms) and int64 median was 15.806 ms (MAD 0.881 ms, p95 16.755 ms), for an int64/int32 ratio of 0.515x. This passes the required <=1.5x gate.
- Strengthened the ticket 9 acceptance matrix with full-range uint64 task-thunk round trips, the signed `MIN % -1` edge, unsigned unary-negation underflow, wide-to-int32 success and failure, exact and inexact uint64-to-float64 casts, coverage-build FFI calls for every new unboxed helper, and a direct/MIR int64 boxed-boundary overflow diagnostic regression.

## Verification

- V1-V5 checkpoint: 486 compiler library tests and all six fixture suites pass under the repository's 32 MiB serialized test-thread contract; focused failing-first regressions pass; reference, docs build, formatting, and diff checks pass.
- Ticket 9 focused direct-codegen/runtime tests, all six fixture suites, release builds, output validation, the 25-run benchmark gate, the 499-test serialized compiler library suite, the coverage-library FFI test, Clippy with warnings denied, formatting/diff checks, and the full forced-MIR/direct runtime-fixture matrix pass against the completed ticket 9 tree. The expanded parity matrix passed in 217.57 seconds.

## Follow-up

- Resolve four D2 semantics before implementation: whether integer `to_float()` preserves the exact checked semantics of `as float64` or explicitly rounds; whether `//` and floor `%` are integer-only or also apply to floats; whether `//` is builtin-only or gains a `FloorDiv` trait; and whether `//=` exists. The current ratifications do not determine these choices, so the Phase 1.5 migration is paused rather than improvised.
- Phase 2 reference work remains explicitly out of scope until Phase 1.5 sign-off.
