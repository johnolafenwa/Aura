# Numeric Arrays

`Array[T]` is Aura's owned, contiguous numeric container for the CPU. Use it
for local preprocessing, postprocessing, evaluation, and batch-shaped numeric
work. It is smaller than a general tensor framework:

- Shape is runtime metadata.
- Storage is row-major and lives on the host.
- Every result owns its buffer.

The only dtypes, or element types, are `int32`, `int64`, `float32`, and
`float64`.

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

`Array` is a global builtin generic type, not a module. It uses the ordinary
grammar for specialization, calls, member calls, indexing, indexed assignment,
and one-colon slices:

```text
Array [ dtype ]
Array [ dtype ] . constructor ( arguments )
array [ expression { , expression } ]
array [ [ expression ] : [ expression ] ]
```

The supported `dtype` names are exactly `int32`, `int64`, `float32`, and
`float64`. Comma-separated Array indexing is different from list indexing. A
one-colon slice selects a range along the first axis. [Grammar](/manual/grammar)
defines the complete syntax.

Aura has no Array literal, dtype value, rank annotation, array-shape
broadcast syntax, view syntax, step slice, or multidimensional slice tuple.

## Typing Rules

### Constructors

These are all the constructors:

| Constructor | Result |
| --- | --- |
| `Array[T].zeros(shape: list[int64])` | `Array[T]` |
| `Array[T].full(shape: list[int64], value: T)` | `Array[T]` |
| `Array[T].from_list(values: list[T], shape: list[int64])` | `Array[T]` |

`T` must be one of the four dtypes. Shape is a runtime `list[int64]`, so rank
and dimensions are not part of the static type.

`from_list` requires exactly `list[T]`. It copies the scalar elements, and the
shared source list stays usable. Constructors never infer a different dtype
from a mixed numeric source.

### Members

| Member | Result and contract |
| --- | --- |
| `shape()` | `list[int64]`; owned shape snapshot |
| `len()` | `int64`; total element count |
| `clone()` | fresh `Array[T]` |
| `get(index: list[int64])` | `T \| None`; `None` for an out-of-bounds coordinate or rank mismatch |
| `set(index: list[int64], value: T)` | mutable receiver; `T \| None` holding the replaced value, or a coordinate/rank trap rather than `None` |
| `fill(value: T)` | mutable receiver; returns `None` |
| `map[U](f: def(T) -> U)` | `Array[U]`; `U` is one of the four dtypes |
| `sum()` | `T` |
| `min()` | `T` |
| `max()` | `T` |
| `mean()` | `float64` for every input dtype |

`map` requires a repeatable callable whose bare parameter type and return
type match exactly. The checker rejects a consuming closure, a `mut` or `own`
parameter, and an unsupported result dtype. `set` and `fill` require a mutable
Array place.

### Indexing And Slicing

Direct indexing takes exactly one `int64` coordinate per runtime axis.
`array[i, j]` has type `T`. Indexed assignment requires a mutable Array place
and a value of exactly `T`. `array[start:end]` returns `Array[T]`.

### Operators

`+`, `-`, and `*` accept either of these operand pairs and return a fresh
`Array[T]`:

- two Arrays with the same dtype and exactly the same shape
- one Array and one scalar of exactly `T`, in either order

`/` has the same forms for floating Arrays only. Integer Array `/` is rejected
with `AU2003`.

There is no array-shape broadcasting and no mixed promotion. An
`Array[int32]` and an `Array[int64]` do not combine. A bound scalar is never
implicitly widened or narrowed for an Array operation.

### Wrapping And Saturating Arithmetic

Every scalar integer type, plus `Array[int32]` and `Array[int64]`, provides
`wrapping_add`, `wrapping_sub`, `wrapping_mul`, `saturating_add`,
`saturating_sub`, and `saturating_mul`. The Array methods accept one scalar of
the same dtype or one Array of the same shape. Ordinary arithmetic stays
checked.

## Runtime Semantics

### Shape And Layout

An Array has rank one or more and owns one contiguous row-major buffer.
Dimensions are `int64`. They can be zero but not negative. `len()` is the
checked product of all dimensions, so a zero dimension makes the Array empty
while keeping its full shape.

`zeros`, `full`, and `from_list` lay out elements in row-major order, and
direct coordinates map to storage in the same order.

### Coordinates

A negative coordinate is normalized once against its own axis. For an invalid
coordinate:

- `get` returns `None`.
- Method `set`, direct indexed reads, and direct indexed assignment trap.

A valid `set` returns the previous scalar as its `T | None` result.

### First-Axis Slices

`array[start:end]` selects complete rows along axis zero. Written endpoints
have exact type `int32`. Omitted bounds and one-time negative normalization
follow the owned-slice rules for lists. Endpoints never clamp. The result has
shape `[end - start]` followed by the source's remaining dimensions. It is
fresh storage that never aliases the source.

### Arithmetic

- Array/Array operations require exactly equal shapes.
- Scalar forms apply the scalar to every element in row-major order.
- Results own fresh contiguous storage.
- Floating `/` follows the ordinary floating operator rules.
- Integer `+`, `-`, `*`, and `sum` keep checked overflow.
- Wrapping operations use fixed-width two's-complement modular arithmetic.
- Saturating operations clamp at the declared integer width.

### Reductions

`map`, reductions, `fill`, and elementwise kernels walk storage in row-major
order.

- `sum()` of an empty Array returns the dtype's zero.
- `min()`, `max()`, and `mean()` require at least one element.
- Floating reductions visit elements from left to right with deterministic
  dtype rounding, and they propagate NaN.
- `mean()` accumulates and returns a `float64` result for every source
  dtype.

The contract promises no reassociation and no vectorized reduction order.

## Ownership And Evaluation Order

`Array[T]` is non-Copy and cloneable. Assignment and owned argument passing
move the buffer. `.clone()` is the explicit way to duplicate the whole buffer.

An Array is always structurally `Transfer`, because every dtype is a
`Transfer` scalar. A Task result that holds an Array still has the ordinary
single-consumer observation right. Bare parameters and receivers give shared
access. `set` and `fill` require exclusive mutable access.

Evaluation order:

- Constructors evaluate their arguments once, from left to right.
- A binary operation evaluates the left operand, then the right. It keeps both
  Arrays for the kernel and consumes neither shared operand.
- Coordinates evaluate from left to right.
- A direct indexed assignment captures its coordinate before it evaluates the
  new value.

Elementwise operations, `map`, and first-axis slices allocate a fresh result.
`map` calls its repeatable callback once per element in row-major order, and
moves or copies each scalar result into the output. If a trap occurs, any
partial output is cleaned up. Shape snapshots and first-axis slices are owned
copies, not views.

## Diagnostics

Compile-time codes:

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown Array member or constructor. |
| `AU2002` | Unsupported dtype, mismatched argument, callback, or result type, or mixed dtypes. |
| `AU2003` | Unsupported operator, including integer Array `/`. The message keeps the ordinary checked-integer guidance. |
| `AU2004` | Invalid argument binding. |
| `AU2005` | Slice steps and slice assignment, which are reserved. The message gives the same owned-copy guidance as for list and str slices. |
| `AU3002` | Mutation while shared access is active. |
| `AU3003` | `set`, `fill`, or indexed assignment through an immutable place. |

Runtime codes:

| Code | Cause |
| --- | --- |
| `AU4002` | Checked integer Array arithmetic overflows. |
| `AU4003` | A direct coordinate is out of range, or a first-axis slice is invalid or reversed. `get` returns `None` instead, but method `set` traps. |
| `AU4004` | Floating Array division with any zero divisor. |
| `AU4005` | Shape product or element count overflows, or allocation fails. |
| `AU4007` | Numeric array shape or reduction violation. See below. |

`AU4007` (`numeric array shape or reduction violation`) covers:

- construction with rank zero or a negative dimension
- a `from_list` element count that does not match the shape
- an Array/Array operation whose shapes differ
- a direct coordinate count that does not match the runtime rank
- `min`, `max`, or `mean` on an empty Array

These traps are defined language behavior. A backend must not panic in their
place.

## Backend Support

MIR and direct execution both implement:

- constructors, indexing, and mutation
- first-axis copies
- `map` and reductions
- checked, wrapping, and saturating arithmetic
- scalar forms and exact-shape elementwise operations

Direct native execution uses contiguous kernels specialized per dtype. Both
backends share checked types, evaluation order, row-major results, cleanup,
and exact `AU4003` and `AU4007` behavior.

The compiler's analysis and the language server expose the same constructors,
member signatures, result types, hover, definitions, completions, and
diagnostics. The bundled VS Code extension uses that compiler-owned surface.

## Limits And Implementation-Defined Behavior

Aura 0.3 Arrays are CPU-only, contiguous, row-major, and have rank one or
more. They do not have:

- array-shape broadcasting or mixed promotion
- views, reshape, or transpose
- matrix multiplication
- equality or ordering
- multidimensional slicing, step slices, or slice assignment
- autograd, device placement, or distributed storage
- aliasing of foreign buffers

Shape metadata is dynamic, and the checker does not prove that shapes are
compatible. Allocation is limited by host memory and the element-count checks.
Floating arithmetic follows the host IEEE-754 rules. This API is narrower than
NumPy's.

## Measured Performance

These measurements were taken on a quiet host, not after a reboot. They have
not yet been repeated under the full post-reboot protocol.

Both runs are contractual under the benchmark runner's qualification rules:

- clean detached sources
- 11 rotating pairs, with warmups excluded
- exact protocol and checksum validation
- three empty host inventories
- successful input and hash rechecks

No override was used.

| Setting | Value |
| --- | --- |
| Host | Mac14,9 / Apple M2 Pro / 16 GiB |
| Python | Xcode CPython 3.9.6 |
| NumPy | 2.0.2 |
| Rust | 1.95.0 |
| Threads | single-threaded |
| Unit | time per operation; ratios are ratios of medians |

"Before" is merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`. "After" is
implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`. Each clean
checkout ran its own unchanged runner in the same session, after the full
local gates. Later documentation edits do not change compiler or runtime
sources.

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

Addition meets the 1.5x Rust target at 1.006524x and takes 79.9591% less Aura
time. Its after Aura/NumPy ratio is 1.006974.

The sum ratio against NumPy is 6.682308. Most of that gap comes from the
deterministic reduction-order policy. Under the same left-to-right order, Aura
stays within 1.34x of Rust.

Addition allocates a fresh owned result, and sum reuses its input. These
workloads do not rank Aura or the Array API in general.

The [before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json)
data, [after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json)
data, [comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
keep every observation and the identities of every source and binary.

## Kernel Implementation

The shared runtime that MIR and direct execution use picks each floating-point
operation before entering its slice loop. Release arm64 disassembly contains
`fadd.4s` and `fadd.2d`, plus the matching `fsub`, `fmul`, and `fdiv` vector
forms, including scalar broadcasts. Division checks for the first zero
divisor after the fallible output allocation. Integer arithmetic and
sequential reductions keep their existing implementations.

Both runtime paths match 1,008 frozen output-bit and diagnostic cases,
recorded before the kernels were written, in debug and optimized release
builds. The cases cover:

- all four element types
- empty and vector-boundary lengths, and 1,000,001 elements
- NaN payloads, infinities, and signed zero
- arithmetic modes and first-trap indices

The kernels do not reassociate, do not approximate division, and add no new
numeric policy.

## Status

Contiguous numeric Arrays and explicit scalar and Array integer arithmetic
modes are part of Aura 0.3. The contract is exactly the surface on this page.
It does not include tensor placement, views, shape transformations, or
distributed execution.

Design records:
[ADR-0041: Contiguous numeric arrays and explicit integer arithmetic modes](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0041-contiguous-numeric-arrays.md)
and [ADR-0002: Integer division and modulo](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0002-integer-division-and-modulo.md).
