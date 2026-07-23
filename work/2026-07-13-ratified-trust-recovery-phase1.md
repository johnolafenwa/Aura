# Ratified Trust-Recovery Phase 1

## Goal

Implement the first eight tickets from the ratified Aurora trust-recovery roadmap using failing-first regressions, forced-backend parity checks, and synchronized documentation updates.

## Work completed

- Recorded accepted ADRs for D1-D13 and linked the decision index from the architecture guide.
- Added the forced-MIR/forced-direct runtime-fixture harness and fixed MIR `io.write` streaming exposed by it.
- Implemented symmetric contextual `None`, unit equality, and the rejected-`is` migration diagnostic.
- Contained non-copy borrowed-result calls while preserving copy materialization; migrated maintained examples and teaching material.
- Replaced ambiguous dotted semantic places with typed roots and field projections; closed mutable-match field/ancestor invalidation holes while preserving sibling writes.
- Made direct call depth, error capture, cancellation fallback, cleanup stacks, cleanup IDs, drain state, and primary diagnostics task-local. Pure-Rust/MIR task exits still unwind live Rust frames; direct-generated tasks use a documented forced-exit boundary that externalizes and drains scheduler-owned cleanup state before resetting generated stacks whose Cranelift frames cannot be safely unwound on macOS arm64.
- Moved DNS resolution plus TCP, UDP, Unix, TLS, HTTP, and WebSocket connection setup into the bounded blocking service, with one absolute timeout budget and cancellation-safe late-result disposal.
- Removed the environment-based `sys.args()` transport. MIR runs receive explicit immutable argv inherited by child tasks; built programs read real host argv.
- Corrected runtime documentation for single-threaded cooperative execution, starvation, fixed stacks, linear readiness polling, and single-observer resource-bearing task results; removed unsupported zero-copy/parallel claims.
- Ran an independent integration review and fixed four cross-cutting defects it exposed: nested contextual-`None` MIR/direct divergence, a borrowed-return escape through operator traits, projected iteration borrows over-freezing sibling fields, and borrowed-return containment losing diagnostic priority behind expected-type unification.

## Phase 1.5 readiness audit

- The current maintained corpus contains 330 fixture `.au` files, 115 examples, 21 tutorials, and 29 manual pages.
- D2 affects 7 fixture files, 4 examples, 5 tutorials, and 8 manual pages.
- D3 has 25 definite fixture files / 33 default-driven binding sites and 296 broad literal-candidate fixture files; examples contain 8 definite files / 14 sites and 108 broad candidates; 4 tutorials and 6 manual pages are relevant.
- D4 affects 2 fixture files, 2 examples, 3 tutorials, and 8 manual pages.
- D5 affects 5 fixture files, 2 examples, 4 tutorials, and 6 manual pages.
- D6 affects 64 unique fixture files, 30 unique examples, 9 tutorials, and 12 manual pages.
- `own` is cleanly reservable: there are no exact language identifiers, compiler/LSP identifiers, or existing keyword entries using it.
- No D1-D13 decision has been proved unworkable, but Phase 1.5 should not begin until the D2 conversion spelling, the D4 negative-indexing roadmap contradiction and method scope, and the D6 default-argument/generic/builtin ownership rules are ratified precisely.

## Verification

- Focused compiler fixture, semantic, MIR, native-codegen, runtime, direct-runtime, and CLI regressions pass for tickets 1-7.
- The forced product test sustains 1,000 simultaneously suspended tasks on both MIR and direct execution.
- The forced backend-parity harness passes every runtime fixture through explicit MIR and direct execution with fallback disabled.
- Exact full `npm run ci` passes, including compiler coverage at 96.02% lines / 96.90% functions / 93.96% regions, LSP coverage at 100%, reference integrity, docs build, dependency audit, warning-free Clippy, and hygiene.

## Follow-up

- Stop before ticket 9 as ordered. Ratify the Phase 1.5 ambiguities recorded above, then implement int64/uint64 direct-backend unboxing before the D3 default flip.
