# Array kernels, list loops, and lane values

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
| `matmul` | 64 by 64 | 2,000 | 41 | 82 | 150 | 145 | 16 | 42,400 |
| `matmul` | 512 by 512 | 4 | 13 | 27 | 232 | 177 | 7 | |

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
  ahead on a large `dot` and on `matmul`.
- Aura's `matmul` keeps the plain three-loop order and its bits. With
  AVX2 it runs in register tiles, and a large product uses every core
  (ADR-0093). It is 3.5 times faster than the same loop in Rust at 64 by
  64 and 13 times faster at 512 by 512. NumPy is still 2.6 times faster
  at 64 by 64 and about twice as fast at 512 by 512. With one thread on
  each side (`AURA_ARRAY_THREADS=1`, `OPENBLAS_NUM_THREADS=1`) the 512 by
  512 row is 42 against 18. That factor is the fused multiply-add, which
  is one instruction where the exact product and sum are two.
- The host is shared, so a row moves by a fifth between runs. The
  baseline column is within that of the 256-bit column on most rows: at
  1,024 elements the call costs as much as the kernel, and at one million
  the memory does. `min` and `max` show the width most clearly.
- Plain Python is 5 to 400 times slower than Aura on every row.

## List loops

`loops.au`, `loops.rs`, and `loops.py` run the same five indexed loops
over `list[float64]`, a Rust `Vec<f64>` behind slices, and a Python list:
`scale` multiplies in place, `saxpy` adds a multiple of one list to
another, `blur` averages each element with its neighbors, `clamp` writes
back the elements outside a range, and `dot` adds products from left to
right. `run.sh` runs them after the kernels. Aura and Rust are each
built for the generic CPU and for the host CPU.

| Loop | Elements | Repetitions | Aura | Aura, native | Rust | Rust, native | Python |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `scale` | 1,024 | 400,000 | 58 | 62 | 62 | 82 | 22,800 |
| `saxpy` | 1,024 | 400,000 | 129 | 86 | 96 | 81 | 20,900 |
| `blur` | 1,024 | 400,000 | 297 | 305 | 291 | 291 | 45,700 |
| `clamp` | 1,024 | 400,000 | 355 | 89 | 349 | 98 | 21,100 |
| `dot` | 1,024 | 400,000 | 283 | 293 | 284 | 297 | 22,900 |
| `scale` | 1,000,000 | 300 | 124 | 101 | 97 | 108 | 11,200 |
| `saxpy` | 1,000,000 | 300 | 194 | 196 | 191 | 199 | 14,300 |
| `blur` | 1,000,000 | 300 | 231 | 221 | 238 | 229 | 27,800 |
| `clamp` | 1,000,000 | 300 | 264 | 103 | 263 | 123 | 12,400 |
| `dot` | 1,000,000 | 300 | 229 | 242 | 230 | 262 | 10,600 |

Aura and Rust print the same checksum on every row. Python runs one
hundredth of the repetitions, so its checksums differ and its column is
the measured time multiplied by 100.

- Aura is within the run-to-run spread of Rust on every row, and 40 to
  400 times faster than Python.
- `scale` and `saxpy` compile to packed multiplies and adds in both
  languages. `clamp` needs a masked store, which the generic x86-64 CPU
  lacks, so it is four times faster in a build for the host CPU.
- `blur` and `dot` are scalar in both languages. `blur` divides, and
  `dot` adds in source order, which no compiler may reorder.

## Lane values

`lanes.au`, `lanes.rs`, and `lanes.py` run six kernels written with the
lane values of the `simd` module (ADR-0087): a dot product, `saxpy`, the
length of each pair of coordinates (`norms`), a clamp, the absolute
difference of two `float32` lists, and a `float32` sum. The Rust side
uses lane structs with one field for each lane and the same order of
operations, which is how stable Rust writes them without vendor
intrinsics. Python has no lane values, so it runs the plain loops.

Milliseconds for 1,024 elements, 400,000 times:

| Kernel | Lane class | Aura | Aura, native | Rust | Rust, native | Python |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `dot` | `F64x4` | 80 | 66 | 145 | 132 | 13,100 |
| `saxpy` | `F64x4` | 94 | 91 | 215 | 62 | 15,000 |
| `norms` | `F64x4` | 442 | 444 | 434 | 441 | 45,500 |
| `clamp` | `F64x4` | 84 | 56 | 151 | 67 | 29,600 |
| `absdiff` | `F32x8` | 49 | 46 | 262 | 32 | 17,000 |
| `sum8` | `F32x8` | 95 | 84 | 93 | 77 | 6,400 |

Aura and Rust print the same checksum on every row. Python runs one
hundredth of the repetitions and adds in source order, so its checksums
differ and its column is the measured time multiplied by 100.

- On a generic build Aura is at or ahead of Rust on every row. Rust keeps
  a bounds check on each load and store. Aura checks the range of every
  lane once, where the loop starts.
- On a build for the host CPU, which has 512-bit registers, Rust is
  about one and a half times as fast on `saxpy` and `absdiff`. Its loop
  vectorizer turns the struct loop into 512-bit instructions and unrolls
  it four times. Aura keeps the width the program wrote. About a fifth
  of Aura's time on those rows is the cost of entering the function,
  which is the same for any list length.
- `dot` with `F64x4` takes a third of the time of the scalar `dot` in
  the list loop table above, which has to add in source order.
- `norms` is bound by the square root, which costs the same in every
  column.
- Python is 60 to 300 times slower than Aura.

## Integer lane values

`int_lanes.au`, `int_lanes.rs`, and `int_lanes.py` run two kernels over
`int64` lists with `I64x4` (ADR-0092), and the same kernels as plain
loops: `add` writes the sum of two lists to a third, and `total` sums
one list. Every addition is checked in the Aura and Rust forms. The Rust lane struct calls `checked_add` on each lane.

Milliseconds for 1,024 elements, 400,000 times, best of five:

| Kernel | Aura lanes | Aura lanes, native | Aura plain loop | Rust lanes | Rust lanes, native | Rust plain loop | Python |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `add` | 174 | 100 | 209 | 155 | 155 | 304 | 17,900 |
| `total` | 129 | 79 | 155 | 114 | 103 | 166 | 10,500 |

- On a build for the host CPU, which has AVX2, Aura tests the four lanes
  of a sum with one branch and keeps them in one register. The lanes
  take half the time of the plain loop and two thirds of Rust's.
- On a generic build each lane keeps its own branch. The lanes are 1.2
  times as fast as the plain loop, and Rust's lane struct is about a
  tenth ahead.
- Python runs one hundredth of the repetitions, and its column is the
  measured time multiplied by 100.
