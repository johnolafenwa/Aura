# FFI call overhead

What one call from Aura into C costs, next to the same call from Rust and
from Python. The C functions in `native/ffi_calls.c` do almost nothing, so
the time measured is the cost of crossing the boundary.

| Case | C signature | What crosses |
| --- | --- | --- |
| `add` | `int64_t fc_add(int64_t, int64_t)` | two 64-bit integers |
| `narrow` | `int32_t fc_narrow(int32_t, uint8_t)` | narrow integers, plus a `%` in the loop |
| `scale` | `double fc_scale(double, float)` | floats |
| `six` | `int64_t fc_six(int64_t x 6)` | six integers |
| `text` | `uint64_t fc_text(const uint8_t *, uint64_t)` | a `str` view |
| `bytes` | `uint64_t fc_bytes(const uint8_t *, uint64_t)` | a `list[uint8]` view |
| `fill` | `void fc_fill(uint8_t *, uint64_t)` | a `mut list[uint8]` view |

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/ffi_calls/run.sh
```

The script builds the C library, then the Aura package in this directory,
`ffi_calls.rs`, and runs `ffi_calls.py` with ctypes and, when installed,
cffi. Each prints `<case> <nanoseconds per call>`. Aura and Rust make 50
million calls a case and Python 2 million.

## Results

Linux x86-64 cloud container, 2026-11-10, nanoseconds per call:

| Case | Aura | Rust | Python ctypes | Python cffi |
| --- | --- | --- | --- | --- |
| `add` | 0.5 | 1.0 | 389 | 181 |
| `narrow` | 4.2 | 3.1 | 349 | 186 |
| `scale` | 0.8 | 1.0 | 343 | 150 |
| `six` | 1.3 | 1.3 | 772 | 363 |
| `text` | 2.8 | 1.0 | 327 | 189 |
| `bytes` | 1.1 | 1.1 | 345 | 198 |
| `fill` | 1.9 | 1.0 | 296 | 173 |

The container is shared, so a number moves by a few tenths of a nanosecond
between runs. A `str` argument asks the runtime for its bytes, which is the
1.8 ns over Rust in `text`.
