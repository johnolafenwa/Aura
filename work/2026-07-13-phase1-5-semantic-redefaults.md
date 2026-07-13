# Phase 1.5 Semantic Re-defaults

## Session

- Started: 2026-07-13 13:58:22 BST.
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

## Verification

- V1-V5 checkpoint: 486 compiler library tests and all six fixture suites pass under the repository's 32 MiB serialized test-thread contract; focused failing-first regressions pass; reference, docs build, formatting, and diff checks pass.

## Follow-up

- Phase 2 reference work remains explicitly out of scope until Phase 1.5 sign-off.
