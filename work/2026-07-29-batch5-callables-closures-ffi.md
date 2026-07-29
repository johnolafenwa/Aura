# Batch 5: defect closure, callables, closures, and FFI

- Date: 2026-07-29
- Status: active
- Entry commit: `4c9d9a2`
- Stop boundary: checkpoint after Phase 6.5; do not begin Phase 7 or release work

## Goal

Close every mandatory Batch 4 follow-up before expanding Aurora's semantic
surface. Then implement Phase 6 in the ratified sequence: capture-free
function values, callable-powered collection and retry APIs, value-capturing
expression lambdas, manifest-gated FFI v0, and a proposal-only loan/view ADR.

## Entry decisions

- Batch 4 is accepted, including its 100,000-task RSS escape-hatch
  disposition.
- ADR-0032, ADR-0033, and ADR-0036 are Accepted.
- ADR-0034 is Accepted after B5.0-b closed its nested generic select payload
  matrix and passed independent audit.
- ADR-0035 is Accepted after B5.0-c serialized only the two mutually
  contending watchdogs and passed repeated default-parallel runs.
- The compiler floors remain frozen at 96.13% lines, 96.90% functions, and
  94.46% regions until the one-time checkpoint re-ratchet.

## B5.0 disposition

B5.0 is implementation-complete and committed as five isolated changes:

- `e25387d` replaces elapsed-time TaskGroup join cancellation with a
  reachability search over live task waits, queue directions, dynamically
  acquired queue handles, and nested join dependencies. The artificial-load
  regression runs 12 CPU burners around four producers, four consumers, and a
  capacity-64 queue; all 4,000 values arrive on MIR and direct. Separate
  regressions pin a true deadlock, the joining-parent exclusion, dynamic queue
  handoff, and a cross-join cycle. A post-gate correction keeps queue-iteration
  producer lifetime separate from general queue-handle reachability; all six
  queue-iteration CLI regressions pass.
- `22a9073` preserves concrete nested `SelectOutcome` payload types through MIR
  pattern lowering. Arithmetic, comparison, f-string interpolation, typed
  rebinding, and reassignment pass on MIR and direct; independent audit also
  covered task, deadline, non-copy, and nested generic payloads.
- `e93e789` narrowly serializes only the two mutually contending blocking-pool
  watchdogs and adds a pinned-nightly Linux ThreadSanitizer workflow. The local
  normal-mode sanitizer selection runs all 214 runtime-value scheduler tests
  plus the multicore and TaskGroup join CLI families; actual TSan execution is
  intentionally Linux-only.
- `8dc509b` advances the scalable-runtime report to schema 4 and adds paired
  startup/loop workloads. The accepted alternate disposition records the clean
  whole-process baseline and treats the available dirty split only as evidence
  that the tool can separate fixed and loop work, not as proof of causality.
- `616ac71` is the required single B5.0-f commit: human and JSON `aura build`
  wait reporting, the MIR multicore contention limit, and the final two
  descriptive shared-access labels. No least-loaded admission change landed
  because the available measurement did not establish a sound scheduling
  policy.

ADR-0032, ADR-0033, ADR-0034, ADR-0035, and ADR-0036 are Accepted. ADR-0034's
nested-payload completion matrix and ADR-0035's default-parallel watchdog
condition are now satisfied.

## B5.0 verification

- `RUST_MIN_STACK=33554432 cargo test`: green, including 308 CLI tests and
  1,157 compiler-library tests.
- Queue-iteration CLI regression family: 6/6; the role-separation unit,
  reachability units, and both originally failing direct cases pass.
- Blocking-pool watchdogs: three prior default-parallel repetitions plus the
  combined workspace gate pass.
- Benchmark-runner suite: 49/49.
- Reference integrity: 9 Python checks, 59 migration checks, all 683 maintained
  manifest files, and 116 verified Aurora manual blocks pass.
- `npm run docs:build`, `npx --yes github-actionlint`,
  `cargo fmt --all -- --check`, and warning-denied Clippy pass.
- Human and JSON build-wait regressions pass after the final reporter lifetime
  cleanup.

The exact clean-tree `npm run ci`, frozen-floor coverage check, forced-backend
matrix, LSP/extension suites, audits, and hygiene remain the final B5.0 gate.
Phase 6 has not started.

## Verification policy

Each behavior change starts with a failing regression and receives focused
MIR/direct or workflow validation before the combined B5.0 gate. Phase 6 does
not start until all B5.0 items and their conditional ADR dispositions are
green. Each Phase 6 stage then receives its own focused tests, reference
updates, coverage check, full CI gate, and commit family.

## Follow-up

At checkpoint, record B5.0 evidence, per-stage commits, callable/closure/FFI
reference inventory, suite counts, parity, coverage and the one-time
re-ratchet, provisional decisions, and any work moved into or out of Batch 6.
