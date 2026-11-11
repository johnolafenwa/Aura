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
| `cstr` | `uint64_t fc_cstr(const char *)` | a `cstr` argument made from a held string |
| `name` | `const char *fc_name(void)` | a `cstr` result copied into a string |
| `out` | `int32_t fc_out(int64_t, int64_t *)` | a `mut int64` out-parameter |
| `floats` | `double fc_floats(const double *, int64_t)` | a `ptr[float64]` over a list of 8 |
| `bump` | `void fc_bump(double *, int64_t)` | a `mut ptr[float64]` over the same list |
| `vec2` | `Vec2 fc_vec2_add(Vec2, Vec2)` | two 16-byte structs of floats in registers, and one back |
| `triple` | `Triple fc_triple_next(Triple)` | a 24-byte struct in memory, and one back |
| `move` | `void fc_vec2_move(Vec2 *, double)` | a `mut Vec2` struct pointer |
| `callback` | `int64_t fc_apply(int64_t (*)(int64_t, int32_t), int64_t)` | a C function that calls back once |

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
| `cstr` | 22 | 18 | 179 | 126 |
| `name` | 38 | 20 | 155 | 248 |
| `out` | 1.9 | 2.0 | 359 | 158 |
| `floats` | 2.5 | 4.6 | 370 | 229 |
| `bump` | 2.5 | 2.4 | 353 | 172 |
| `vec2` | 13.9 | 14.1 | 410 | 297 |
| `triple` | 13.6 | 10.0 | 303 | 201 |
| `move` | 9.4 | 3.1 | 439 | 172 |
| `callback` | 6.8 | 2.5 | 513 | 446 |

The container is shared, so a number moves by a few tenths of a nanosecond
between runs. A `str` argument asks the runtime for its bytes, which is the
1.8 ns over Rust in `text`.

In `cstr` each side makes a NUL-terminated copy of a string it holds: Aura
in its runtime, Rust with `CString::new`. In `name` each side copies the
result into an owned string: Rust with `CStr::to_str` and `to_owned`. Both
cases are one allocation and one copy, and Aura's extra time in `name` is
the box of the new `str`.

In `floats` and `bump` every side passes the address of storage it already
holds: Aura the list's own elements, Rust a `Vec<f64>`, and Python an array
made once before the loop.

The struct cases were measured on 2026-11-11. Each call feeds its result to
the next, so `vec2` and `triple` time the round trip of a struct through
memory on both sides. In `move`, Rust passes the address of the struct it
holds. Aura writes the fields to a C copy, passes its address, and reads
the fields back, which is the 6 ns over Rust.

In `callback` the C function calls the function it is handed once. Rust
passes an `extern "C" fn`, and Python a `CFUNCTYPE` or `ffi.callback`
wrapper of a lambda. Aura's 4 ns over Rust is the frame its entry packs and
the guard that keeps a failure from unwinding through C.
