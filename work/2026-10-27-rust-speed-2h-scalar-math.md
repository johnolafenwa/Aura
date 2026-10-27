# Rust Speed Step 2h Part 5: Math Built-ins Compute In Words

## Goal

Step 2h of Phase 2 of the Rust-speed plan moves ownership off boxes. The
step plan (`rust-speed-plan/phase-2/step-2h-plan.md` in the project
files) lists two causes of boxes that parts 1 to 4 did not touch: the
scalar math built-ins box their arguments and results, and runtime
helpers that return nothing allocate a unit box that generated code
releases at once. The census named `min`, `max`, and `abs` as every box
in `image_filters`, and `sqrt` as most of `nbody` and `raytracer`. The
element-path probe behind element loans was 107,369 of the unit boxes in
`sql_engine` and 28,086 in `json_roundtrip`.

## The Old Convention

`abs`, `min`, `max`, `sqrt`, and `round` called runtime helpers that take
and return boxes (`aura_direct_min` and the others). A call on two
`int64` locals boxed both, the helper compared the values, boxed the
result, and generated code unboxed it into the target. The element-path
probe (`aura_direct_element_path_probe`, and its `_promote` form) and the
element-path store returned a boxed `Value::Unit`, which generated code
released.

## Changes

- `native_codegen_math.rs`: when each argument is a scalar local or a
  numeric literal, the call computes in words.
  - `min` and `max` compare and select. A tie keeps the first argument
    (`a <= b` for `min`, `a >= b` for `max`) and a NaN picks the second,
    as the helpers did. `uint64` compares unsigned; every other integer
    compares signed, since a word holds a narrow integer's exact value.
    A literal first argument gives an `int64` or `float64` result, as the
    helper's result did. Arguments the fast path does not take, such as a
    narrow local against a literal out of its range, keep the helper.
  - `abs` negates a negative integer, keeps an unsigned one, and clears
    a float's sign bit. The smallest value of a signed kind calls
    `aura_direct_fail_narrow_overflow`, which reports the same
    "integer value `128` does not fit in `int8`" trap the helper's
    result did when unboxed.
  - `sqrt` of a float calls the existing word helper
    `aura_direct_sqrt_f64`, and `round` of a float calls the new
    `aura_direct_round_f64`, which shares `round_float_to_int64` with the
    boxed form. `round` of an integer keeps the value. A `float32` result
    is rounded to `float32`, as unboxing the helper's result did.
- The element-path probe, its promoting form, and the element-path store
  return nothing, and generated code releases nothing after them.

## Verification

- New fixture `run-pass/scalar_math_unboxed.au`: every integer width,
  `uint64` above the `int64` range, literals first and second, a clamp
  helper, ties on `0.0` and `-0.0` that show which argument came back,
  `abs` of `-0.0`, `sqrt` and `round` with ties to even, `float32`
  results, and a loop shaped like `image_filters`. Its native build
  references none of the boxed helpers.
- New fixture `run-fail/int8_abs_minimum_overflow.au`: `abs` of `-128` as
  `int8` stops with the same message on both backends. The existing
  `int64_abs_minimum_overflow` and `round_float_out_of_int64_range`
  fixtures cover the wide cases.
- A code generation test checks that scalar math calls the word helpers
  and the overflow failure, and not the boxed helpers. A runtime test
  covers `aura_direct_round_f64`, including its trap.
- Runtime tests of the element-path helpers no longer expect a unit box.
- Runtime, compiler, and CLI tests, backend parity,
  `check:cross-language`, `check:operation-probes`, `cargo fmt --check`,
  hygiene, and clippy.

## Measurements

Callgrind instruction counts and `AURA_RUNTIME_STATS=1` box counts, the
30 cross-language programs at the check size, Linux container, release
builds. Before is part 4.

| Program | Boxes before | Boxes after | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `image_filters` | 31,101 | 33 | 18.4M | 1.3M |
| `nbody` | 20,092 | 52 | 14.4M | 7.3M |
| `raytracer` | 38,758 | 4,109 | 36.6M | 24.0M |
| `astar_pathfinding` | 8,782 | 34 | 13.3M | 8.9M |
| `spreadsheet` | 15,766 | 8,858 | 22.2M | 18.5M |
| `order_book` | 3,879 | 2,020 | 11.1M | 10.0M |
| `sort_records` | 28,289 | 22,283 | 44.6M | 41.3M |
| `interpreter` | 40,369 | 25,219 | 86.8M | 83.2M |
| `mini_browser` | 455,802 | 412,184 | 580.7M | 560.7M |
| `sql_engine` | 683,934 | 552,398 | 1,412.2M | 1,375.5M |
| All 30 programs | 1,886,045 | 1,523,899 | | |

The suite showed `pubsub_broker` 12.3% and `kv_server` 2.2% higher, but
direct callgrind runs give 80.1M before and 78.5M after for
`pubsub_broker`, and between 46.6M and 47.2M for `kv_server` on either
build: their tasks and sockets vary between runs. No other program runs
more instructions.

Retains are unchanged at 87,876 across the 30 programs, 91.7% below 2g.

## What Is Left For The Gate

- Leak, failure, and cancellation cleanup tests (step plan part 5).
- `str` values read out of collections (`json_roundtrip`, `docdb`,
  `csv_aggregate`, `word_freq`) and boxed `int64` values in `lru_cache`
  are most of the retains left.
