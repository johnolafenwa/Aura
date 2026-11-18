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

## Struct Arrays, Array Fields, Unions, And Text Callbacks

Build a release compiler and run the four FFI v2 comparisons.

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/ffi_calls/run.sh v2
```

Each implementation builds its input once. Each timed loop runs two million
iterations and checks the accumulated result. Times include loop overhead.
All three read the same nanosecond clock from the fixture C library. The
default measures wall time. Set `AURA_FFI_BENCH_CLOCK=cpu` to measure
process CPU time when sharing the host with other work.

| Case | Work in one iteration |
| --- | --- |
| `struct_array` | Pass four two-field records to C. C sums their eight signed 32-bit coordinates. |
| `array_field` | Pass a struct by value. C sums its four inline signed 32-bit integers. |
| `union_read` | Read a signed 32-bit member of one of four unions populated by C before timing. |
| `text_callback` | Call C with a named function. The callback counts the Unicode scalars in `café`. |

Aura gives C a private copy of a struct list. Rust and ctypes pass the
address of a C-layout array they already hold. In the callback, Aura owns
a checked copy of the C string. Rust borrows and validates it with
`CStr::to_str`. Python decodes the bytes passed by ctypes.

### FFI v2 results

Linux x86-64, 2026-11-18. Each row gives median nanoseconds per operation
from five runs on one pinned CPU. Aura, Rust and Python rotated through
first, second and third position. Each implementation checked its result.
The native Aura program used LLVM’s `default<O3>` pipeline and the release
runtime. The Aura compiler executable used the Rust dev profile. Rust used
`rustc -C opt-level=3`. Other workloads on the host accounted for some of
the difference between elapsed time and process CPU time.

| Case | Aura CPU | Rust CPU | ctypes CPU | Aura elapsed | Rust elapsed | ctypes elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `struct_array` | 12.89 | 6.39 | 1084.81 | 11.74 | 6.45 | 1834.55 |
| `array_field` | 8.22 | 2.75 | 936.22 | 9.11 | 3.27 | 1090.24 |
| `union_read` | 0.93 | 0.46 | 217.50 | 0.91 | 0.48 | 352.25 |
| `text_callback` | 105.73 | 26.80 | 1300.92 | 131.43 | 44.06 | 1920.14 |

CPU and elapsed samples were separate series. The text callback took
3.94 times Rust’s CPU time and 2.98 times its elapsed time.

Nine additional runs rotated two versions of the Aura program with Rust.
After checking the length and native record layout, the newer program
copied small record lists directly into the private call buffer. The
median fell from 15.74 to 11.00 ns of process CPU time, with Rust at
6.43 ns. Holding read-only callback arguments in scoped storage reduced
the text callback from 120.52 to 87.39 ns, with Rust at 25.54 ns.

Earlier paired measurements isolated the cost of the numeric union getter.
Eliding its diagnostic frame on valid input reduced the median from 8.94
to 5.62 ns across six alternating runs. Reusing the private C writeback
array for an exclusively read list reduced the median from 4.54 to
0.78 ns across eight alternating pairs. The compiler uses the ordinary
list when a later operation can escape or change its elements. Invalid
indices and malformed union bytes produce the same diagnostics and call
frames as the interpreter.
