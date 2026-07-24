# Phase 3 Retrying Network Worker

Date: 2026-07-24

## Goal

Close the final Phase 3 application gate with a maintained worker that composes
Aurora's existing HTTP, randomness, `Duration`, structured-concurrency, and
resource-cleanup surfaces into a bounded retry policy. This ticket adds no new
language semantics or runtime API.

## Work Completed

- Added `examples/agents/retrying_network_worker.au` with an ephemeral loopback
  HTTP server and a structured worker task.
- Kept retry classification explicit and application-owned: only `503` retries;
  the recovered path returns `200`, a subsequent `429` is terminal, and the
  exhausted path returns its final `503`.
- Seeded `random.Rng(42)` and combined its deterministic jitter with exponential
  `Duration` backoff.
- Placed the final-attempt guard before the RNG draw, retry log, and
  `sleep(...)`, preventing an unobservable final delay or an unused random draw.
- Applied explicit five-second deadlines to listener acceptance, HTTP requests,
  and task results.
- Scoped the `TaskGroup`, HTTP listener, exchanges, and responses so every
  resource has deterministic cleanup.
- Registered the exact maintained example path in the root/example indexes,
  concurrency and networking tutorials, executable reference map, and reference
  integrity gate.

## Verification

- The focused CLI product regression
  `retrying_network_worker_runs_with_computed_backoff_on_both_backends` passes
  through both the MIR and forced-direct backends.
- The exact observable trace pins three scenarios and seven real loopback
  requests: recovery after one retry, terminal rate limiting after one retry,
  and exhaustion after three attempts.
- Lightweight reference-integrity and diff checks pass for the documented
  surface.
- `npm run coverage:compiler:check` passes all 257 instrumented CLI tests, 795
  compiler library tests, and supporting suites at 60,904/63,399 lines
  (96.06460669726651%), 3,976/4,099 functions (96.99926811417419%), and
  88,875/94,275 regions (94.27207637231504%), above the frozen
  96.06/96.79/94.15 floors. No synthetic-coverage test, exclusion, or
  coverage-only production restructuring was added.
- The exact-tree full `npm run ci` decision gate passes.

## Follow-Up

Commit this retry-worker ticket, then continue to Phase 3.5 in the authorized
Batch 2 order.
