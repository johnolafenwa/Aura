# Batch 6: Phase 7, final self-audit, and release preparation

- Date: 2026-07-30
- Status: active
- Entry commit: `8131ebe`
- Stop boundary: final report; do not push commits or tags and do not publish
  archives

## Goal

Close the four Batch 6 entry findings, implement Python-shaped
comprehensions and owned slices, add the first contiguous numeric array
surface with native kernels, then conduct a fresh-eyes product and claims
audit. Prepare and locally verify the 0.2.0 technical-preview release without
publishing it.

## Accepted entry rulings

- Batch 5 is accepted.
- ADR-0037 is Accepted.
- ADR-0038 is Accepted as a design; implementation targets Aurora 0.3 and is
  not authorized in the 0.2 cycle. Its ten ratification questions are all
  answered yes as recommended.
- Compiler coverage floors remain frozen at 96.18% lines, 96.97% functions,
  and 94.62% regions until the one-time final downward-truncated re-ratchet.

## Current stage

B6.0 is active in strict-order entry closure:

- B6.0-b is implemented with dedicated `AU2008`. Named function values,
  capture-free closures, and capturing closures uniformly reject `==` and
  `!=` before backend selection, with exact semantic, fixture, registry, and
  MIR/direct diagnostic-parity tests. The first complete gate found two stale
  FFI closure assertions that still expected generic opaque-handle equality;
  both now explicitly pin `AU2008` precedence while direct opaque-handle
  comparisons retain `AU2003`.
- B6.0-c is implemented. ADR-0037 is recorded as Accepted; ADR-0038 has the
  exact accepted-design-only status and all ten yes answers. The retry and Vec
  callback pages now describe repeatable capturing closures, and the closure
  page pins the enclosing bare-parameter restriction and Copy workaround.
- B6.0-d is implemented through a shared owned-process-group guard. Every
  scalable-runtime workload and the legacy direct-loop helper launches in a
  new session, verifies `SIGTERM` then `SIGKILL` cleanup, reaps the leader and
  streams, and turns silent cleanup failure into a failed benchmark. The
  54-test Python harness covers success, validation failure, timeout,
  interruption, TERM-resistant descendants, and ineffective kill detection.
- B6.0-a is complete. The baseline Mac rebooted at
  `2026-07-30 23:02:25 +0100`; a clean detached schema-4 run at
  `18654158d22b2227149369e7911af04aafcbeecb` measured whole-process V6
  medians of `36.691666 ms` (`int32`) and `14.837417 ms` (`int64`), within
  `1.99%` / `1.12%` of and slightly faster than the accepted reactor-era pair.
  The slower dirty pair therefore does not reproduce after reboot and is
  attributed to dirty/load/thermal host state rather than a HEAD regression.
  All maintained runtime gates pass; only the already-withdrawn 100,000-task
  RSS claim remains red. The contractual raw report SHA-256 is
  `134efcc894742ed73b16e07f1e31845c83d19930d5894b4dc39f01533a9be2fd`,
  and no benchmark workload survived the process-group cleanup.
- Integrated verification passes the 54-test benchmark harness, Python
  compilation, shell parsing, Rust formatting, the focused B6.0-b
  semantic/fixture/registry/forced-parity tests, the complete executable
  reference gate (9 integrity tests, 59 migration tests, and all 683
  manifests), the VitePress build, and scoped whitespace checks. The full
  repository CI gate is now the only remaining B6.0 checkpoint requirement.
- Phase 7 must not start until B6.0-a and the combined gate are complete.

## Authorized sequence

1. Close B6.0-a through B6.0-d and commit the gated entry result.
2. Implement and gate list, set, and map comprehensions.
3. Implement and gate owned Vec and scalar-indexed String slicing.
4. Implement and gate contiguous arrays, scalar and array integer
   wrapping/saturating operations, native kernels, and measured post-reboot
   NumPy comparisons.
5. Run the independent 30-program fresh-eyes corpus on MIR and direct.
6. Produce the consolidated post-reboot benchmark story and claims audit.
7. Align positioning, README, changelog, reference version, and release
   guidance.
8. Build and install-test supported archives, create only the local
   `v0.2.0-preview` tag, run the final gates and coverage re-ratchet, and write
   the final report.

## Standing verification

Each language or tooling behavior change begins with a failing regression.
Every Phase 7 stage lands its compiler, backend, LSP, examples, tutorials, and
normative reference surface together. Coverage-closing tests must pin
observable behavior, diagnostics, or parity; no synthetic execution-only
tests or unjustified exclusions are permitted.

The unrelated modified `personal/file_ops.au` and untracked
`architecture_docs/decisions/0022-implicit-shared-capability-syntax.md` remain
outside Batch 6.

## Follow-up

Record each B6.0 disposition, per-stage commit and gate, the post-reboot
hardware provenance, all measured results, the fresh-eyes corpus, every
autonomous/provisional decision and ADR, release archive hashes, installed
verification, exact user publish commands, and the first three Aurora 0.3
priorities in the final report.
