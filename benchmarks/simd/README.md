# Array kernels

The Array kernels of ADR-0085 and ADR-0086 beside the same work in Rust,
in NumPy, and in plain Python: the left-to-right `sum`, the reordered
`fast_sum`, `dot`, `min` and `max`, elementwise `+`, and `matmul`, over
`float64` elements.

Each kernel runs at two sizes. 1,024 elements fit in the first-level
cache, so the time is the kernel and the cost of calling it. One million
elements do not fit in any cache, so the time is mostly memory.

- **Aura** (`kernels.au`) runs twice: as built, and with
  `AURA_ARRAY_KERNELS=baseline`, which turns the 256-bit copies off.
- **Rust** (`kernels.rs`) writes each kernel the way a Rust program writes
  it first: iterator sums and plain loops over slices. Its `fast_sum` is
  eight partial sums written by hand, because the standard library has no
  reordered sum. It is built twice: for the generic CPU, and with
  `-C target-cpu=native`.
- **NumPy** (`kernels.py`) calls `sum`, `dot`, `min`, `max`, `+`, and `@`.
- **Python** (`kernels.py python`) runs the same kernels as loops over
  lists, at one hundredth of the repetitions.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/simd/run.sh
```

## Results

Linux x86-64 cloud container with AVX2 and AVX-512, four cores shared
with other work, 2026-11-11. Milliseconds for all the repetitions of a
row. The Python column is its measured time multiplied by 100.

| Kernel | Elements | Repetitions | Aura | Aura, baseline kernels | Rust | Rust, native | NumPy | Python |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `sum` | 1,024 | 400,000 | 336 | 338 | 305 | 275 | 528 | 1,800 |
| `fast_sum` | 1,024 | 400,000 | 56 | 67 | 55 | 47 | | |
| `dot` | 1,024 | 400,000 | 120 | 88 | 297 | 289 | 299 | 19,000 |
| `min` and `max` | 1,024 | 400,000 | 196 | 326 | 718 | 190 | 1,084 | 7,500 |
| `+` | 1,024 | 400,000 | 228 | 213 | 161 | 103 | 662 | 14,300 |
| `sum` | 1,000,000 | 300 | 218 | 218 | 218 | 222 | 113 | 2,500 |
| `fast_sum` | 1,000,000 | 300 | 87 | 87 | 88 | 93 | | |
| `dot` | 1,000,000 | 300 | 175 | 185 | 230 | 256 | 58 | 21,000 |
| `min` and `max` | 1,000,000 | 300 | 219 | 242 | 494 | 246 | 157 | 10,000 |
| `+` | 1,000,000 | 300 | 267 | 334 | 297 | 297 | 374 | 24,500 |
| `matmul` | 64 by 64 | 2,000 | 108 | 111 | 150 | 145 | 26 | 42,400 |
| `matmul` | 512 by 512 | 4 | 100 | 116 | 232 | 177 | 10 | |

Every side prints the same checksum for every row but one: NumPy's
512 by 512 product differs in the last digit, because its library adds in
another order and fuses each multiply with its add.

What the numbers say:

- `fast_sum` runs four to six times faster than `sum` on a small Array,
  and matches eight hand-written partial sums in Rust. NumPy's `sum`
  reorders too, which is why it beats a left-to-right sum on one million
  elements. `fast_sum` is ahead of it there.
- `dot` is ahead of the Rust iterator form, which adds from left to
  right, and ahead of NumPy on a small Array.
- NumPy calls a tuned linear algebra library for `dot` and `@`. It uses
  fused multiply-add, 512-bit registers, and several threads, so it is
  ahead on a large `dot` and on `matmul`. Aura's `matmul` keeps the
  plain three-loop order and its bits, and is ahead of the same loop in
  Rust.
- The host is shared, so a row moves by a fifth between runs. The
  baseline column is within that of the 256-bit column on most rows: at
  1,024 elements the call costs as much as the kernel, and at one million
  the memory does. `min` and `max` show the width most clearly.
- Plain Python is 5 to 400 times slower than Aura on every row.
