# ADR-0086: Arrays gain `dot`, `matmul`, and reductions in a fixed vectorized order

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0041 (numeric Arrays), ADR-0085 (kernel dispatch)
- Amends: ADR-0041, which lists matrix multiplication as a non-goal and
  leaves the result of `min` and `max` over zeros of both signs to the host

## Context

ADR-0041 promises that a float `sum` and `mean` add from left to right.
That order is a chain: each addition waits for the one before it, so a
CPU with eight-wide registers still adds one element at a time. NumPy
sums the same Array about seven times faster because it adds in another
order.

The two most common numeric kernels, the inner product and the matrix
product, are not there at all. A program writes them as loops over
`array[i, j]`, which run a bounds check and a runtime call per element.

A faster order must not cost reproducibility. A sum that depends on the
width of the machine's registers prints different digits on two machines,
and Aura's benchmarks and tests compare output exactly.

## Decision

### Reordered reductions

- **`fast_sum() -> T` and `fast_mean() -> float64`** are new members
  beside `sum` and `mean`, which keep their left-to-right order.
- **One fixed order on every machine.** For a float Array of `n`
  elements, let `w` be the largest multiple of 32 that is at most `n`.
  Element `i < w` is added to partial sum `i % 32`, in index order, each
  partial sum starting at zero. The partial sums are then added in
  halves: sum `s` takes sum `s + 16`, then `s + 8`, `s + 4`, `s + 2`, and
  `s + 1`. Elements `w` to `n - 1` are added to that total in index
  order.
- **Rounding.** `fast_sum` adds in the element type. `fast_mean` widens
  each element to `float64`, adds in that order, and divides by the
  count, as `mean` does in its own order.
- **NaN and infinity** follow the additions. A NaN result is the one
  canonical NaN.
- **Integer Arrays.** `fast_sum` is `sum`: checked, exact, and `AU4002`
  on overflow. `fast_mean` adds the elements as `float64` values in the
  reordered order.
- **Empty Arrays.** `fast_sum` is zero and `fast_mean` is `AU4007`, as
  for `sum` and `mean`.

### `dot`

- **`dot(other: Array[T]) -> T`** takes an Array of the same dtype and
  the same shape, of any rank, and returns the sum of the products of
  matching elements. A different shape is `AU4007`, and a different dtype
  is `AU2002`.
- **A float `dot`** rounds each product in the element type, then adds
  the products in the reordered order above.
- **An integer `dot`** is checked. The first product, or the first
  running sum in index order, that leaves the element type's range is
  `AU4002` with its flat index.

### `matmul`

- **`matmul(other: Array[T]) -> Array[T]`** multiplies shapes `[n, k]`
  and `[k, m]` into `[n, m]`, `[n, k]` and `[k]` into `[n]`, and `[k]`
  and `[k, m]` into `[m]`. Any other pair of shapes is `AU4007`.
- **Each result element starts at zero and adds its `k` products in
  ascending order of the inner index.** Each product is rounded before it
  is added. The kernel walks the right operand a row at a time, so the
  additions for one element stay in order while a whole row of results
  advances together. `matmul` therefore needs no reordering.
- **An integer `matmul`** is checked, and a product or running sum out
  of range is `AU4002` with the flat index of a result element that
  overflowed.

### `min` and `max`

- **A float `min` treats negative zero as below positive zero, and
  `max` treats positive zero as above.** ADR-0041 left that pair to the
  host. Every other result is unchanged, including NaN for an Array that
  holds a NaN. The kernels now compare many elements at once.

## Alternatives rejected

- **Change `sum` itself.** Programs already depend on its order, and a
  left-to-right sum is the one a reader can check by hand.
- **A `reorder=true` argument.** A second member is easier to find in
  completion, and a call site says which order it runs in.
- **Let the register width pick the order.** It is the fastest choice
  on each machine and gives each machine its own digits.
- **Pairwise summation, as NumPy does.** Its error bound is a little
  better. Its order depends on a block size chosen for speed, and the 32
  partial sums vectorize as well without one.
- **A reordered `matmul`.** The row-at-a-time order is already
  vectorized, so reordering would buy nothing.
- **A strict left-to-right `dot`.** It would run at the speed of `sum`,
  and a caller who wants that order can write the loop.
- **Broadcasting `matmul` over leading dimensions.** Arrays have no
  broadcasting anywhere else.

## Consequences

- On one million `float64` elements `fast_sum` runs about four times
  faster than `sum`, and both stay available.
- `dot` and `matmul` give the same bits on every CPU and in both
  backends, which share the kernels.
- `fast_sum` and `sum` can differ in the last digits, and the
  documentation says so where a reader picks between them.
- The matrix product is no longer a non-goal of the Array surface.
  Transpose, reshape, and views still are.
