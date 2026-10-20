# Batch 7 Step 6: Integer Array Kernels

## Goal

Vectorize the `int32` and `int64` Array kernels while keeping each result
and each diagnostic the same as an element-by-element loop, as step 6 of
the [Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md)
plans with decision D1.

## Work Completed

- `ArrayInteger` gives `i32` and `i64` their overflowing, wrapping, and
  saturating `add`, `sub`, and `mul`. The kernels no longer convert each
  element to an `IntegerValue`.
- `integer_array_kernel` picks the operation and mode once, outside the
  loop. Wrapping and saturating modes map the elements in one pass.
- Checked mode works in blocks of 256 elements. It computes a whole block
  into a stack buffer and ORs a sign mask per element: `(a ^ r) & (b ^ r)`
  for addition and `(a ^ b) & (a ^ r)` for subtraction. LLVM vectorizes
  this loop, where it does not vectorize the carry flag. The block is then
  copied to the result. A block that
  overflowed is scanned again for its first failing element, so `AU4002`
  names the same flat index as before.
- `sum()` folds each block's minimum, maximum, and wrapping sum in one
  pass. When the largest magnitude times the block length cannot move the
  running sum out of range, the block's sum is exact and is added at once.
  Any other block is added element by element to find the first prefix
  that overflows.
- Integer division on a non-empty Array keeps its `AU4001` error, and an
  empty one keeps returning an empty result.

## Verification

- New tests compare every operator, mode, and operand order with an exact
  `i128` element-by-element reference. The inputs mix small values with
  both extremes, at lengths 0, 7, 255, 256, 1,000, and 4,099. They pass
  on the old kernels and on the new ones.
- A new test compares `sum()` with an exact prefix-sum reference, including
  sums that overflow and then return into range.
- The existing Array fixtures and runtime tests pass.
- Each program below repeats one operation 100 times on
  one-million-element arrays. Each time is the fastest whole-process run on
  the Linux container, of three runs before and five after. Times include
  about 45 ms of input setup.

| Operation | Before | After |
| --- | ---: | ---: |
| `int64` `left + right` | 3,724 ms | 138 ms |
| `int32` `sum()` | 3,974 ms | 47 ms |
| `int64` `sum()` | 4,080 ms | 100 ms |
| `int32` `wrapping_mul` | 2,935 ms | 58 ms |
| `int32` `max()`, unchanged | 68 ms | 52 ms |

## Follow-Up

- Checked multiplication keeps `overflowing_mul`, which does not vectorize
  for `int64` on baseline x86-64.
- `int64` `sum()` does not vectorize on baseline x86-64. SSE2 has no 64-bit
  compare for the block minimum and maximum. It still runs about 0.5 ns per
  element.
- Floating kernels keep their existing code.
