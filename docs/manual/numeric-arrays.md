# Numeric Arrays

`Array[T]` is Aura's owned contiguous CPU numeric container. It is intended
for local preprocessing, postprocessing, evaluation, and batch-shaped numeric
work. It is smaller than a general tensor framework: shape is runtime
metadata, storage is row-major and host-only, and results own their buffers.

The only dtypes are `int32`, `int64`, `float32`, and `float64`.

```aura
def main() -> int32:
    left = Array[float64].from_list([1.0, 2.0, 3.0, 4.0], [2, 2])
    right = Array[float64].full([2, 2], 0.5)
    combined = left + right
    first_row = combined[0:1]
    print(combined.shape())
    print(combined[1, 0])
    print(first_row.sum())
    print(combined.mean())
    return 0
```

## Grammar

`Array` is a global builtin generic type rather than a module. It uses the
ordinary specialization, call, member-call, indexing, indexed-assignment, and
one-colon slice grammar:

```text
Array [ dtype ]
Array [ dtype ] . constructor ( arguments )
array [ expression { , expression } ]
array [ [ expression ] : [ expression ] ]
```

The supported `dtype` names are exactly `int32`, `int64`, `float32`, and
`float64`. Comma-separated Array indexing is distinct from a list index.
One-colon slicing selects a first-axis range. The complete syntax remains
defined by [Grammar](/manual/grammar).

There is no Array literal, dtype value, rank annotation, array-shape broadcast
syntax, view syntax, step slice, or multidimensional slice tuple.

## Typing Rules

### Constructors

The complete constructor surface is:

| Constructor | Result |
| --- | --- |
| `Array[T].zeros(shape: list[int64])` | `Array[T]` |
| `Array[T].full(shape: list[int64], value: T)` | `Array[T]` |
| `Array[T].from_list(values: list[T], shape: list[int64])` | `Array[T]` |

`T` must be one of the four maintained dtypes. Shape is a runtime
`list[int64]`, so rank and dimensions are not part of the static type.
`from_list` requires exact `list[T]`, copies its scalar elements, and leaves the
shared source list usable. Constructors never infer a different dtype from a
mixed numeric source.

### Members

| Member | Result and contract |
| --- | --- |
| `shape()` | `list[int64]`; owned shape snapshot |
| `len()` | `int64`; total element count |
| `clone()` | fresh `Array[T]` |
| `get(index: list[int64])` | `Option[T]` |
| `set(index: list[int64], value: T)` | mutable receiver; `Some(T)` replaced value or a coordinate/rank trap |
| `fill(value: T)` | mutable receiver; returns `None` |
| `map[U](f: def(T) -> U)` | `Array[U]`; `U` is one of the four dtypes |
| `sum()` | `T` |
| `min()` | `T` |
| `max()` | `T` |
| `mean()` | `float64` for every input dtype |

`map` requires a repeatable callable whose bare parameter and return type
match exactly. A consuming closure, `mut`/`own` parameter, or unsupported
result dtype is rejected. `set` and `fill` require a mutable Array place.

Direct indexing uses exactly one `int64` coordinate per runtime axis.
`array[i, j]` has type `T`; indexed assignment requires a mutable Array place
and a value of exactly `T`. `array[start:end]` returns `Array[T]`.

### Operators

`+`, `-`, and `*` accept same-dtype exact-shape Array/Array operands or one
Array and one scalar of exactly `T`, in either order. They return a fresh
`Array[T]`. `/` has those forms only for floating Arrays. Integer Array `/`
is rejected with `AU2003`, as required by ADR-0002.

There is no array-shape broadcasting or mixed promotion. An `Array[int32]` and
`Array[int64]` do not combine, and a bound scalar is never implicitly widened
or narrowed for an Array operation.

Every scalar integer type, plus `Array[int32]` and `Array[int64]`, provides
`wrapping_add`, `wrapping_sub`, `wrapping_mul`, `saturating_add`,
`saturating_sub`, and `saturating_mul`. An Array method accepts either one
same-dtype scalar or one same-shape Array. Ordinary arithmetic stays checked.

## Runtime Semantics

An Array has rank at least one and owns one contiguous row-major buffer.
Dimensions are `int64`, may be zero, and may not be negative. `len()` is the
checked product of all dimensions. A zero dimension therefore makes the Array
empty while preserving its complete shape.

`zeros`, `full`, and `from_list` lay out elements in row-major order. Direct
coordinates are translated in that same order. A negative coordinate
normalizes once against its own axis. `get` returns `None` for an invalid
coordinate; method `set`, direct indexed read, and direct indexed assignment
trap. Valid `set` returns the previous scalar in `Some`.

`array[start:end]` selects complete rows along axis zero. Written endpoints
have exact type `int32`; omitted bounds and one-time negative normalization
follow the owned-slice rules. Endpoints never clamp. The fresh result shape is
`[end - start]` followed by the source's remaining dimensions. Its storage
never aliases the source.

Elementwise Array/Array operations require exactly equal shapes. Scalar forms
apply the scalar to every row-major element. Results own fresh contiguous
storage. Floating `/` uses the ordinary floating operator contract. Integer
`+`, `-`, `*`, and `sum` retain checked overflow. Wrapping operations use
fixed-width two's-complement modular arithmetic; saturating operations clamp
at the declared integer width.

`map`, reductions, `fill`, and elementwise kernels traverse row-major
storage. `sum()` of an empty Array returns the dtype's zero. `min()`, `max()`,
and `mean()` require at least one element. Floating reductions visit elements
left to right with deterministic dtype rounding and propagate NaN.
`mean()` accumulates and reports a `float64` result for every source dtype.
The contract promises no reassociation or vectorized reduction order.

## Ownership And Evaluation Order

`Array[T]` is non-Copy and cloneable. Assignment and owned argument passing
transfer the buffer; `.clone()` is the explicit full-buffer duplicate. It is
always structurally `Transfer` because its dtype is one of four Transfer
scalars, but a Task result containing an Array retains the ordinary
single-consumer observation right. Bare parameters and receivers provide
shared access. `set` and `fill` require exclusive mutable access.

Constructors evaluate arguments left to right and once. Binary operations
evaluate the left operand before the right, retain both reached Arrays for the
kernel, and consume neither shared operand. Coordinates evaluate left to
right. A direct indexed assignment captures its coordinate before evaluating
the replacement value.

Elementwise operations, `map`, and first-axis slices allocate a fresh result.
`map` invokes its repeatable callback once per element in row-major order and
moves or copies each scalar result into the output. A trap cleans up any
partial output. Shape snapshots and first-axis slices are owned copies, not
views.

## Diagnostics

`AU2001` reports an unknown Array member or constructor. `AU2002` reports an
unsupported dtype, exact argument/callback/result mismatch, or mixed dtype.
`AU2003` reports unsupported operators, including integer Array `/`, and
preserves the ordinary checked-integer guidance. `AU2004` reports invalid
argument binding. `AU2005` reserves slice steps and slice assignment with the
same owned-copy guidance as list/str slices. `AU3002` reports mutation while
shared access is active; `AU3003` reports `set`, `fill`, or indexed assignment
through an immutable place.

`AU4003` reports an out-of-range direct coordinate or invalid/reversed
first-axis slice. `get` returns `None` instead of emitting that diagnostic;
method `set` traps.

`AU4002` reports checked integer Array arithmetic overflow.

`AU4004` reports floating Array division when any divisor is zero.

`AU4005` reports shape-product/element-count overflow and allocation failure.

`AU4007` (`numeric array shape or reduction violation`) reports:

- rank-zero or negative-dimension construction
- `from_list` element-count mismatch
- exact-shape Array/Array operation mismatch
- direct coordinate-count/runtime-rank mismatch
- empty `min`, `max`, or `mean`

These failures are language behavior, not permission for a backend-specific
panic.

## Backend Support

Constructors, indexing, mutation, first-axis copies, mapping, reductions,
checked/wrapping/saturating arithmetic, scalar forms, and exact-shape
elementwise operations are implemented for MIR and direct execution. Direct
native execution uses dtype-specialized contiguous kernels. The two backends
share checked types, evaluation order, row-major results, cleanup, and exact
`AU4003`/`AU4007` behavior.

Compiler analysis and the language server expose the same constructors,
member signatures, result types, hover, definitions, completions, and
diagnostics. The bundled extension uses that compiler-owned semantic surface.

## Limits And Implementation-Defined Behavior

Aura 0.3 Arrays are CPU-only, contiguous, row-major, and rank-at-least-one.
They have no array-shape broadcasting, mixed promotion, views, reshape,
transpose, matrix multiplication, equality, ordering, multidimensional slicing, step
slices, slice assignment, autograd, device placement, distributed storage, or
foreign-buffer aliasing.

Shape metadata is dynamic; the checker does not prove shape compatibility.
Allocation is limited by host memory and the maintained element-count checks.
Floating arithmetic follows the existing host IEEE-754 contract. This surface
is narrower than NumPy's API.

## Measured Performance

The item 7 measurements are labeled **quiet host, not post-reboot; contractual re-measure scheduled with the 0.3.4 release session.**
Both runs are contractual under the runner's qualification rules: clean detached
sources, 11 rotating pairs, excluded warmups, exact protocol/checksum validation,
three empty host inventories and successful input/hash rechecks. No override
was used. The host is Mac14,9 / Apple M2 Pro / 16 GiB, with Xcode CPython 3.9.6,
NumPy 2.0.2 and Rust 1.95.0. Measurements are single-threaded, per operation;
ratios are ratios of medians.

Before is merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`;
after is implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`.
Each clean checkout used its own unchanged runner in the same measurement
session, after the full local gates. Later publication edits do not change
compiler/runtime sources.

| Workload (one million `float64` elements) | Aura before | Aura after | Change in time | Before Aura / Rust | After Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.244818 ms | 0.249473 ms | -79.9591% | 5.039359 | 1.006524 |
| Existing-array sum | 1.149271 ms | 1.149844 ms | +0.0498% | 1.338990 | 1.339087 |

| Control | Before median | After median | Drift |
| --- | ---: | ---: | ---: |
| NumPy add | 0.253398 ms | 0.247745 ms | -2.2310% |
| Rust add | 0.247019 ms | 0.247856 ms | +0.3387% |
| NumPy sum | 0.173397 ms | 0.172073 ms | -0.7639% |
| Rust sum | 0.858312 ms | 0.858677 ms | +0.0425% |

Addition meets the task's 1.5x Rust target at 1.006524x, with 79.9591% less
Aura time; its after Aura/NumPy ratio is 1.006974. The sum ratio is 6.682308
against NumPy. That gap is chiefly the deterministic reduction-order policy:
Aura remains within 1.34x of Rust under the same left-to-right order.
Addition allocates a fresh owned result; sum reuses its input. These exact
workloads do not establish a general language or Array-API performance ranking.

[Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json),
[after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json),
[comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
retain all observations and source/binary identities.

## Kernel implementation

The shared runtime used by MIR and direct execution selects each floating-point
operation before its slice loop. Release arm64 disassembly contains `fadd.4s`
and `fadd.2d`, plus the corresponding `fsub`, `fmul` and `fdiv` vector forms,
including scalar broadcasts. Division validates the first zero divisor after
fallible output allocation. Integer arithmetic and sequential reductions retain
their original implementations.

Both runtime paths match 1,008 frozen pre-change output-bit and diagnostic cases
in debug and optimized release builds. The corpus covers all four element types,
empty and vector-boundary lengths, 1,000,001 elements, NaN payloads, infinities,
signed zero, arithmetic modes and first-trap indices. The rewrite does not introduce
reassociation, approximate division or a new numeric policy.

## Status

Contiguous numeric Arrays and explicit scalar/Array integer arithmetic modes
are Accepted for Aura 0.3 under
`architecture_docs/decisions/0041-contiguous-numeric-arrays.md`.
The maintained contract is the exact surface on this page and contains no
broader tensor placement, views, shape transformations, or distributed
execution.
