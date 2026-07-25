# Phase 4 `aura run` backend selector

## Goal

Add `aura run --backend mir|direct|auto` and update the forced backend-parity
matrix in the same change so its MIR leg passes `--backend mir` explicitly,
which is the recorded V4 invariant.

## Work Completed

- Added the selector with three values. `mir` executes the lowered MIR and is
  the default; `direct` builds a native binary with the direct backend and
  executes it; `auto` prefers the direct backend and degrades to the MIR
  runtime, printing the reason on standard error before the program runs.
- Made only `auto` degrade. A forced `direct` run reports a build or launch
  failure as an error, so a parity or benchmark caller can never silently
  measure the MIR runtime while believing it measured native code.
- Preserved the whole `aura run` contract on every backend: program arguments
  after `--`, `--stdin`, standard output, and the process exit code.
- Updated `backend_parity.rs` so both its MIR legs pass `--backend mir`
  explicitly rather than relying on the default.

## Decision: the default stays `mir`

The roadmap asks for `auto` as the interim default until forced-direct is
proven across every runnable maintained fixture. Measured on this workstation,
that interim default is not affordable yet:

| Backend | Hello-world `aura run` |
| --- | --- |
| `mir` | 0.012s |
| `direct` | 1.385s |

`auto` always pays the direct compile and link before it can decide, so
defaulting to it would regress every `aura run` by about two orders of
magnitude, and would multiply that cost across the 259-test CLI product suite
and again under coverage instrumentation, where the compile is far slower.

The blocker is therefore not correctness but warm-launch cost, and the
content-addressed artifact cache is its precondition. The default is recorded
in one named constant with that reasoning attached, so the cache ticket flips
one value rather than re-deriving the decision. Forced-direct correctness is
already gated independently by the full parity matrix, which runs every
run-pass and run-fail fixture through the direct backend on every CI run.

## Verification

- Unit tests pin selector parsing, the default, and that only `auto` degrades
  while `direct` reports both build and launch failures.
- A CLI product test pins identical stdout, program arguments, and exit code
  across `mir`, `direct`, `auto`, and the default, plus the usage rejection for
  an unknown backend value.
- The forced MIR/direct parity matrix passes with its MIR legs now explicit.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,388/67,014 lines (96.08141582355925%), 4,154/4,291 functions
  (96.80727103239339%), and 94,440/100,150 regions (94.29855217174239%), above
  the frozen 96.06/96.79/94.15 floors. No synthetic coverage test or exclusion
  was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Revisit the default in the artifact-cache ticket, where a warm launch should
  make `auto` affordable.
