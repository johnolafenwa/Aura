# V6: direct `int32` loops against `int64`

## Goal

Diagnose why the direct backend's `int32` ten-million iteration loop is
markedly slower than the same loop at `int64`, fix the cause if it is
contained, and keep both measurements in the benchmark baseline either way.

## Reproduction

The first attempt to reproduce was wrong and is worth recording: summing
`0..10_000_000` into an `int32` accumulator overflows `int32` and traps, so it
measures a trap rather than a loop. The workload has to be a counter loop,
where both widths do the same arithmetic and neither overflows.

With that corrected, the inversion reproduced immediately, best of seven:

| Width | Before |
| --- | --- |
| `int32` | 0.0697s |
| `int64` | 0.0115s |

## Diagnosis

The direct backend represents every integer as a 64-bit runtime value. For
`int64`, an arithmetic operation lowers to Cranelift's overflow-producing form,
so the overflow flag falls out of the single native instruction and costs one
conditional branch.

For a narrow width there is no such flag, so the backend performed the
arithmetic in 64 bits and then range-checked the result. That check was a
two-sided signed comparison: materialize `i32::MIN`, materialize `i32::MAX`,
compare below, compare above, `or` the two results, branch. Five instructions
plus a branch, on the result of every `int32` operation, against `int64`'s one
instruction plus a branch.

That is the whole inversion. It is not a code-generation quality problem in the
loop, and it is not specific to loops; a loop simply executes the check ten
million times.

## Fix

The range check is now one biased unsigned comparison. A 64-bit value is an
`int32` exactly when adding `-i32::MIN` lands it inside the unsigned 32-bit
range, so the check is one `iadd_imm` and one `icmp_imm` plus the same branch.

| Width | Before | After |
| --- | --- | --- |
| `int32` | 0.0697s | 0.0327s |
| `int64` | 0.0115s | 0.0111s |
| `int32` / `int64` | 6.05x | 2.95x |

`int64` is unchanged, as expected: the change touches only the narrow-width
check.

## What still separates the two widths

The residual roughly 3x is the branch itself. `int64` gets its overflow test as
a side effect of the arithmetic instruction, while a narrow width must compute
a separate predicate and branch on it, and that branch splits the loop body
into extra blocks on every operation.

Closing the rest would mean performing narrow arithmetic in its own width and
using the overflow-producing form at that width, which changes how integers are
represented throughout the backend rather than how one check is written. That
is a representation change, not a contained fix, so it is recorded here rather
than attempted inside this ticket.

## Verification

- The exact `int32` boundary behavior is pinned: both extremes remain
  representable, and one step past either end traps with `AU4002` and the same
  message, so the cheaper check kept the same range.
- A `run-fail` fixture pins the same overflow at the boundary through the
  fixture oracle, which the forced MIR/direct parity matrix runs on both
  backends.
- The benchmark is runnable as `npm run bench:direct-integer-loops`, and both
  widths are recorded in `benchmarks/direct_integer_loops/README.md`.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,409/67,039 lines (96.07691045510822%), 4,158/4,295 functions
  (96.81024447031432%), and 94,472/100,184 regions (94.29849077697038%), above the frozen
  96.06/96.79/94.15 floors. No synthetic coverage test or exclusion was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Narrow-width arithmetic in its own width is the remaining V6 lead, and it is
  a backend representation change rather than a local fix.
