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

The 7 September 2026 post-reboot Mac14,9 / Apple M2 Pro (16 GiB) run at
`50531b45797ae765ec8d885165c13042617ab567` has 11 rotating single-thread
observations per lane with excluded warmups and exact protocol validation.
All three host inventories are empty; this is contractual evidence.
Xcode CPython 3.9.6 supplies NumPy 2.0.2; Rust is pinned to 1.95.0.
Ratios below are ratios of medians per one-million-element operation.

| Workload (one million `float64` elements) | Aura median | NumPy median | Rust median | Aura / NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.205427 ms | 0.247190 ms | 0.244576 ms | 4.876520 | 4.928642 |
| Existing-array sum | 1.148677 ms | 0.168981 ms | 0.858175 ms | 6.797687 | 1.338511 |

[Raw observations](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json) have SHA-256
`58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f`.
The [Performance chapter](/manual/performance#optimization-level) records the
before comparison and controls: addition became 7.1740% slower, while sum
changed by -0.0994%. NumPy drift remained below 0.04%. These are effects of the
combined foundations changes, not an isolated Cranelift experiment.
The table covers only these exact operations; Aura's Array API is narrower
than NumPy's, and its deterministic reductions retain left-to-right order.

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
