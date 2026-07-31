# ADR-0041: Contiguous numeric arrays and explicit integer arithmetic modes

- Status: Accepted
- Date: 2026-07-31
- Roadmap decision: Batch 6, Phase 7.3
- Builds on: ADR-0002, ADR-0016, ADR-0022, and ADR-0040
- Distinct from: ADR-0038

## Context

Aurora can express numeric work with scalars and `Vec[T]`, but a Vec does not
promise a dense dtype-specific layout, multidimensional shape, or a native
elementwise/reduction kernel. That forces ordinary preprocessing,
postprocessing, evaluation, and batch-shaping work across a Python process
boundary even when no accelerator, autograd system, or general tensor
framework is needed.

Phase 7.3 needs one deliberately small host-side array contract. It must keep
dtype and shape errors loud, retain Aurora's checked integer arithmetic by
default, avoid hidden broadcasting or mixed promotion, and stop short of the
place/view model accepted for Aurora 0.3 under ADR-0038.

## Decision

### Type, dtype, shape, and storage

`Array[T]` is a global builtin move type. `T` is exactly one of `int32`,
`int64`, `float32`, or `float64`. No other specialization exists.

Every Array has rank at least one and owns one contiguous row-major buffer.
Its shape is a `Vec[int64]`. Dimensions may be zero; dimensions may not be
negative; and the checked dimension product must fit the implementation's
maintained element-count and allocation limits.

Shape is runtime metadata. Aurora 0.2 has no shape values in its static type
identity: two values with type `Array[float64]` may have different runtime
shapes.

### Constructors

The complete constructor surface is:

```aurora
Array[T].zeros(shape: Vec[int64]) -> Array[T]
Array[T].full(shape: Vec[int64], value: T) -> Array[T]
Array[T].from_vec(values: Vec[T], shape: Vec[int64]) -> Array[T]
```

`zeros` and `full` allocate exactly the shape's element count.
`from_vec` requires the shape product to equal `values.len()` and copies those
scalar values into the Array's row-major order. Its shared source Vec remains
usable. A rank-zero shape, negative dimension, or count mismatch reports
`AU4007`. Shape-product/element-count overflow and allocation failure report
`AU4005`.

### Members

The complete member surface is:

```aurora
shape() -> Vec[int64]
len() -> int64
clone() -> Array[T]
get(index: Vec[int32]) -> Option[T]
set(index: Vec[int32], value: T) -> Option[T]
fill(value: T) -> None
map[U](f: def(T) -> U) -> Array[U]
sum() -> T
min() -> T
max() -> T
mean() -> float64
```

`set` and `fill` require a mutable receiver. `set` returns the replaced scalar
in `Some` for a valid coordinate and traps on an invalid coordinate or rank.
`get` returns `Some` or `None` without trapping. `shape()` returns an owned shape
snapshot. `map` is eager and row-major, requires a repeatable exact
`def(T) -> U` callback, and restricts `U` to the same four Array dtypes.

`sum`, `min`, and `max` preserve `T`. `mean` returns `float64` for every
dtype. Integer `sum` has the dtype's ordinary checked arithmetic behavior.
Floating reductions visit row-major elements left to right with deterministic
dtype rounding and propagate NaN. `mean` accumulates in `float64`; no
reassociation or vectorized reduction order is promised. `min`, `max`, and
`mean` report `AU4007` on an empty Array; `sum` returns the dtype's zero.

### Indexing and first-axis slices

`array[i, j, ...]` uses one exact `int32` coordinate per axis. Direct indexed
read and assignment require the coordinate count to equal the runtime rank.
Negative coordinates normalize once against their own axis. An out-of-range
coordinate reports `AU4003`; a direct coordinate-count/rank mismatch reports
`AU4007`. `get` converts invalid coordinates into `None`; method `set`, direct
indexed read, and direct indexed assignment trap.

`array[start:end]` copies a half-open range along the first axis into a fresh
owned contiguous Array. Written endpoints have exact type `int32`; omitted
and one-time-negative-normalized bounds follow ADR-0040 without clamping. The
result shape replaces the first dimension with `end - start` and retains all
later dimensions. Invalid or reversed endpoints report `AU4003`.

Array slice steps, slice assignment, and views are unavailable. A slice never
aliases its source and is not an assignable place.

### Elementwise arithmetic

The binary operators `+`, `-`, and `*` support:

- two Arrays with the same `T` and exactly equal runtime shapes
- `Array[T]` with a scalar of exactly `T`
- a scalar of exactly `T` with `Array[T]`

The result is a fresh `Array[T]`. There is no broadcasting and no mixed-dtype
promotion. Array/Array shape mismatch reports `AU4007`.

`/` has the same forms only for `float32` and `float64`. Integer Array
division is rejected statically with `AU2003`, preserving ADR-0002's rule that
integer `/` is not silently redefined. This decision adds no Array `//`, `%`,
power, matrix multiplication, comparison, or equality operator.

### Wrapping and saturating integer arithmetic

Every scalar integer type and integer `Array[T]` provides:

```aurora
wrapping_add(rhs)
wrapping_sub(rhs)
wrapping_mul(rhs)
saturating_add(rhs)
saturating_sub(rhs)
saturating_mul(rhs)
```

For a scalar receiver, `rhs` has the same scalar integer type and the result
has that type. For `Array[int32]` or `Array[int64]`, `rhs` is either a
same-shape Array with the same `T` or one scalar of exactly `T`; the result is
a fresh Array. Array shape mismatch reports `AU4007`.

Ordinary scalar and Array `+`, `-`, and `*` remain checked. Wrapping uses
two's-complement modular arithmetic at the declared width. Saturating clamps
to that width's minimum or maximum. No implicit arithmetic-mode change occurs.

### Evaluation, ownership, and backend kernels

`Array[T]` is non-Copy, explicitly cloneable, and always structurally
`Transfer` because all four dtypes are Transfer. A Task containing an Array
still has the ordinary single-consumer result observation right.

Constructors and binary operations evaluate arguments left to right and once.
Binary Array operations retain both Array operands through the kernel and
return independent owned storage. Index coordinates evaluate left to right.
`map` visits row-major elements left to right. A callback trap or runtime
failure cleans up the partial result.

MIR and direct execution share the same observable contract. Direct native
execution uses dtype-specialized contiguous kernels; MIR remains the checked
development path. Kernel implementation details do not weaken the checked
shape, coordinate, arithmetic, cleanup, or diagnostic rules.

## Non-goals

Aurora 0.2 does not add broadcasting, mixed promotion, views, transpose,
reshape, matrix multiplication, multidimensional slicing, step slices,
autograd, accelerator placement, distributed arrays, foreign-buffer aliasing,
or NumPy API compatibility.

The maintained post-reboot NumPy comparison measures only one-million-element
`float64` addition and sum on one named host. It is release evidence, not a
portable performance guarantee or an acceptance threshold.

## Consequences

Aurora gains a small self-contained CPU numeric layer with explicit dtype,
shape, ownership, arithmetic-mode, and failure contracts. Common local data
work no longer requires a subprocess boundary. The intentionally narrow
surface leaves shape transformations, aliasing, promotion, accelerators, and
broader tensor semantics for later decisions.

The absence of broadcasting and views makes some programs more explicit and
may allocate more than NumPy. That cost is accepted for a first sound owned
surface.

## Completion tests

- semantic tests for the four dtype specializations, rejected dtypes,
  constructor/result/member signatures, exact scalar forms, callback typing,
  integer division, and no mixed promotion
- runtime tests for rank, zero dimensions, row-major layout, count/shape
  failures, indexing, mutable updates, first-axis copies, reductions, empty
  reductions, deterministic floating order/NaN propagation,
  checked/wrapping/saturating boundaries, cleanup, and both
  backends
- MIR/direct parity tests for all public operations and `AU4003`/`AU4007`
  diagnostics
- compiler analysis, completion, hover, definition, language-server, and
  bundled-editor protocol coverage
- a maintained example and source-hash-pinned executable Manual block
- a tested post-reboot benchmark harness retaining raw Aurora/NumPy samples,
  checksums, order, environment, host/commit/input hashes, and a derived
  summary without a performance gate

## Ratification

Batch 6 authorizes this decision as the binding Aurora 0.2 contiguous numeric
Array and explicit integer arithmetic-mode contract. The compiler, both
backends, reference, diagnostics, editor surface, examples, benchmark
protocol, and work evidence land together.
