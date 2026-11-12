# Array views

Seven loops over Array views and slice assignment, written the same way
in Aura, Rust, and NumPy. `run.sh` builds and runs all three. Each line
is `<name> <size> <milliseconds> <checksum>`, and the checksums match.

| Loop | What it does |
| --- | --- |
| `row_sums` | `m[i, :].fast_sum()` for every row of a 512 by 512 matrix |
| `column_sums` | `m[:, j].fast_sum()` for every column |
| `transposed_add` | `m.transpose() + m` |
| `stencil` | `u[1:n - 1] = (u[0:n - 2] + u[2:n]) * 0.5` over a million elements |
| `row_swaps` | swaps rows through a `clone()` and two slice assignments |
| `column_scale` | `m[:, j] *= 1.0001` for every column |
| `reshape_reverse` | `reshape`, a reversed view, and the sum of every second element |

## Results

Best of three runs on the 4-core x86-64 Linux build box, 2026-11-11.
Milliseconds, lower is better. One run varies by about a fifth.

| Loop | Aura | Rust | NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: |
| `row_sums` | 32 | 27 | 123 | 1.2 |
| `column_sums` | 153 | 99 | 223 | 1.5 |
| `transposed_add` | 175 | 151 | 149 | 1.2 |
| `stencil` | 368 | 255 | 309 | 1.4 |
| `row_swaps` | 103 | 21 | 164 | 4.9 |
| `column_scale` | 249 | 126 | 153 | 2.0 |
| `reshape_reverse` | 55 | 41 | 17 | 1.3 |

## Reading the numbers

- **A view costs the same for any length.** Before views, `m[i, :]` was
  not expressible and a first-axis slice copied its rows.
- **`row_swaps` is 4.9 times Rust.** The Rust side calls
  `swap_with_slice`, which holds no row. The Aura and NumPy sides keep
  one row in a temporary, as their slice assignment needs.
- **Four loops are behind NumPy:** `transposed_add`, `stencil`,
  `column_scale`, and `reshape_reverse`. Each computes into a temporary
  Array and then copies it, or copies a strided view before a kernel
  reads it. The elementwise work of Array v2 item 2 removes those
  copies: operations walk strided views in place, a temporary operand is
  reused for the result, and `m[selection] op= value` updates in place.
