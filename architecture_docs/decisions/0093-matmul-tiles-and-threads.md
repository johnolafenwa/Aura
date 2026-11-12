# ADR-0093: A float `matmul` runs in register tiles and on several threads

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0086 (`matmul`), ADR-0085 (kernel dispatch)

## Context

ADR-0086 fixed what `matmul` computes: each result element starts at zero
and adds its products in ascending order of the inner index, with each
product rounded before it is added. Its kernel walked the right operand a
row at a time and updated a whole row of results in memory.

NumPy ran the same products four to ten times faster. Three things
separated the two:

- **The running sums lived in memory.** Every step loaded a piece of the
  result row, added to it, and stored it back.
- **One thread did the work.** NumPy's library uses every core for a
  large product.
- **NumPy fuses each multiply with its add.** That is one instruction
  where the exact form needs two, and it rounds once where the exact form
  rounds twice, so its digits differ from the three-loop program's.

The first two can change without changing a result.

## Decision

- **With AVX2, a float product of two matrices runs in tiles.** A tile
  is two rows by sixteen `float64` columns of the result, or thirty-two
  `float32` columns. Its running sums stay in registers for a block of
  128 inner indexes. The kernel first copies that block of the right
  operand's columns into a packed buffer so the inner loop reads it in
  order. Columns left after the last whole tile use the row kernel.
- **The order of additions for each element does not change.** A tile
  loads the element's running sum, adds the block's products in ascending
  inner order, and stores it. The results are the bits of ADR-0086.
- **A product of at least 2^21 multiplications is split by rows across
  threads.** Each thread owns a run of whole rows of the result, the
  calling thread takes the first run, and the threads end before the call
  returns. Each thread gets at least 2^21 multiplications.
- **`AURA_ARRAY_THREADS`** sets the most threads one product uses. The
  default is the number of CPUs the process may run on, capped at 16. A
  value that is not a positive integer is ignored.
- **The entry point keeps its signature**: two slices, the output slice,
  and `(rows, inner, columns)`. Integer products, vector operands, the
  x86-64 baseline, and arm64 keep the row kernel on one thread per
  product, except that threads apply at every level.

## Alternatives rejected

- **A fused product.** A variant of the same tiles with fused
  multiply-add ran 1.25 to 1.35 times faster in a prototype. It would
  give different digits on a CPU without the instruction, or run a slow
  software form there. That gain does not pay for a second set of
  results.
- **512-bit tiles.** They measured no faster than the 256-bit tiles on
  the test host.
- **Tiles at the x86-64 baseline.** Sixteen registers of two lanes hold
  too small a tile. The row kernel was as fast.
- **A thread pool.** Starting threads costs tens of microseconds, and
  the threshold keeps a product that small on the calling thread.
- **Using the task scheduler's workers.** A product can run before any
  task exists and inside one, and the kernel must not yield.

## Consequences

- In `benchmarks/simd`, 2,000 products of 64 by 64 matrices went from
  108 ms to 41 ms, and four products of 512 by 512 from 100 ms to 13 ms
  on four shared cores. On one thread the second is 42 ms.
- NumPy takes 16 ms and 7 ms for the same rows, and 18 ms for the second
  on one thread. The factor of two to two and a half that remains on one
  thread is the fused instruction and the packing of a small product.
- At the x86-64 baseline the rows are 82 ms and 27 ms. Only the threads
  apply there.
- A program that runs many tasks and multiplies large matrices in each
  can ask for more threads than it has cores. `AURA_ARRAY_THREADS=1`
  turns the splitting off.
- NEON tiles are not measured and not built.
