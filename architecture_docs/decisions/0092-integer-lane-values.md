# ADR-0092: The `simd` module has `int64` lanes with checked arithmetic

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0087 (lane values), ADR-0085 (target CPU)

## Context

ADR-0087 left integer lanes out. Integer arithmetic in Aura reports
overflow, a check after each lane is a branch after each lane, and LLVM
does not join lanes across branches. The lanes then cost what they save.

Two measurements changed the answer for `int64`:

- **The check does not need a branch for each lane.** A signed sum
  overflows exactly when both operands differ in sign from the result.
  That is the sign of `(a ^ sum) & (b ^ sum)`, a word LLVM computes on
  every lane at once. One test of the words joined by `or` covers the
  whole operation.
- **It pays where four lanes fit a register.** On a build for AVX2 an
  `I64x4` sum of 1,024 elements takes under half the time of the checked
  scalar loop. On the x86-64 baseline two lanes fit, the joined check
  costs more than a branch for each lane, and the plain lane code already
  runs at the speed of the scalar loop.

## Decision

### The classes

- **`I64x2` and `I64x4`** are public Copy classes in `std/simd.au` with
  `int64` fields `l0`, `l1`, and so on.
- **Each has** `splat`, `load`, `store`, `min`, `max`, `abs`, `sum`, and
  `to_list`, and the operators `+`, `-`, `*`, and unary `-`.
- **There is no `/`, `sqrt`, or `mul_add`.**
- **Loads and stores lower as element accesses**, as the float classes'
  do, so a counted loop checks the range of every lane once.

### Semantics

- **Every operation is the checked scalar operation in each lane.** A
  sum, difference, product, or negation outside `int64`, and `abs` of the
  least `int64`, stops the program with `AU4002`.
- **The report is the scalar one.** It names the value that does not
  fit and the lane's position in `<std>/simd.au`. When several lanes
  overflow, the first lane is reported.
- **`sum` adds in pairs**, and each of its sums is checked.
- **The interpreter runs the methods as written**, so both backends give
  the same values and the same reports.

### Native code

- **A lane method that only adds, subtracts, or multiplies and builds
  its result defers its checks.** Such a method is one block with no
  call, so nothing it does can be seen before it returns. Native code
  computes the wrapped result and the overflow word of each sum and
  difference, joins the words, and tests them once before the return.
- **The rare side of that test repeats the checks in source order** and
  reports the first that failed, with the operands the scalar check
  would report.
- **This form is used when the target CPU has AVX2**: `--target-cpu
  x86-64-v3`, `x86-64-v4`, or `native` on such a machine. Every other
  target keeps a branch after each lane. The values and the reports are
  the same in both forms.
- **A product keeps its branch.** LLVM has the overflow flag of a 64-bit
  product only for one lane at a time, and no CPU before AVX-512
  multiplies 64-bit lanes.

## Alternatives rejected

- **Wrapping lanes.** They would be three to four times faster than the
  scalar loop on every CPU, and they would be the one place where `+` on
  an `int64` silently wraps.
- **An overflow flag checked after the loop.** A loop that stores its
  results would have written wrong values before the program stopped.
- **`int32` lanes.** Aura holds an `int32` in a 64-bit word and checks
  its range after each operation. Eight `int32` lanes then need two
  256-bit registers and sixteen range checks. Measured, `I32x8` products
  ran between 0.8 and 1.1 times the speed of the scalar loop on every
  target. They can follow when narrow integers are computed at their own
  width.
- **The joined check on every target.** On the x86-64 baseline it made an
  `I64x4` sum twice as slow as the branches: the compare of 64-bit lanes
  needs SSE4.2, and the rare side's use of each lane keeps LLVM from
  using vector registers at all.
- **The joined check on AArch64.** It is not measured there yet, so
  AArch64 keeps the branches.

## Consequences

- On `benchmarks/simd`, `I64x4` on an AVX2 build adds two lists in half
  the time of the scalar loop and sums one in under half. On a baseline
  build the lanes are 1.2 times as fast as the scalar loop.
- **`min` and `max` do not get faster below AVX-512**, which is the first
  x86-64 level with a 64-bit lane maximum.
- A build for another target CPU changes speed only.
- The native cache already keys on the target CPU, so both forms of a
  lane method never share a cache entry.
