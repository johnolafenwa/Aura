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

Best of three runs on the 4-core x86-64 Linux build box, 2026-11-12.
Milliseconds, lower is better. One run varies by about a fifth.

| Loop | Aura | Rust | NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: |
| `element_sum` | 73 | 65 | 28 | 1.1 |
| `element_fill` | 207 | 125 | 532 | 1.7 |
| `stencil_2d` | 191 | 64 | 217 | 3.0 |
| `center_columns` | 94 | 130 | 134 | 0.7 |
| `mask_choose` | 126 | 144 | 108 | 0.9 |
| `sqrt_exp` | 195 | 233 | 48 | 0.8 |
| `batched_matmul` | 30 | 66 | 10 | 0.5 |
| `lane_dot` | 154 | 54 | 22 | 2.9 |
| `shared_tasks` | 248 | 154 | 208 | 1.6 |

## Reading the numbers

- **Element loops match Rust on reads.** Compiled code checks each
  coordinate and touches the element in place. A loop that only reads
  and writes elements checks the Array once at its entry.
- **`stencil_2d` is three times Rust** and ahead of NumPy's slice form.
  Each access still checks two coordinates. The Rust loop checks one
  flat index and its compiler removes most of those checks. Proving the
  coordinates of a counted loop in range at the loop's entry, as list
  loops do, is the next step.
- **`lane_dot` is 2.9 times a Rust loop over chunks,** the same ratio as
  lanes loaded from a list.
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
  so the time is four sums of 32 MB. One sum on the main task takes
  1.2 ms, and the four tasks finish in 2.5 ms, so they do not all run at
  once. The same holds for tasks that touch no Array, which makes it a
  scheduler cost and not a transport cost.
