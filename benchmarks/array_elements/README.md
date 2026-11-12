# Array elements

Nine loops over Array elements and the members a numeric program calls
between them, written in Aura, Rust, and NumPy. `run.sh` builds and runs
all three. Each line is `<name> <size> <milliseconds> <checksum>`, and
the checksums match to the last digit NumPy's summation order allows.

| Loop | What it does |
| --- | --- |
| `element_sum` | `acc += m[i, j]` over a 1000 by 1000 matrix |
| `element_fill` | `m[i, j] = value` for every element |
| `stencil_2d` | a four-neighbor average written with `grid[i - 1, j]` and its three neighbors, then an element copy back |
| `center_columns` | `m - m.mean(0)` |
| `mask_choose` | `m.greater(12.0)`, the mask's `count()`, and `choose` |
| `sqrt_exp` | `(m.sqrt() * 0.125).exp().sum()` |
| `batched_matmul` | 512 matrices of 32 by 32, each times one matrix |
| `lane_dot` | a dot product with `F64x4.load_array` |
| `shared_tasks` | four tasks that each sum a clone of one 2000 by 2000 Array |

The Rust side indexes a `Vec<f64>` with bounds checks. The NumPy side
writes each element loop as the whole-array call a NumPy program uses,
since a Python loop over elements measures the interpreter.

## Results

RESULTS_TABLE

## Reading the numbers

- **Element loops match Rust on reads and stay within three times on
  the stencil.** Compiled code checks each coordinate and touches the
  element in place. A loop that only reads and writes elements checks
  the Array once at its entry.
- **`element_sum` and `lane_dot` are behind NumPy** because the NumPy
  side is one kernel call: `m.sum()` and `xs @ ys`. The same calls in
  Aura are `m.fast_sum()` and `xs.dot(ys)`, which
  `benchmarks/simd` measures.
- **`sqrt_exp` is behind NumPy.** NumPy computes `exp` several elements
  at a time. Aura calls the scalar `exp` for each element, as the Rust
  side does, so every element has the bits of `math.exp`.
- **`batched_matmul` is behind NumPy's BLAS** and ahead of the plain
  Rust loops.
- **`shared_tasks` copies nothing.** Each `clone()` shares the buffer,
  so the time is four sums of 32 MB on four cores.
