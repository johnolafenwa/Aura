# ADR-0089: Arrays broadcast, reduce along an axis, compare, and convert

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0041 (numeric Arrays), ADR-0086 (Array products and
  reordered sums), ADR-0088 (Array views)
- Amends: ADR-0041, which requires two Array operands to have one shape,
  limits the dtypes to four numeric types, and lists broadcasting, axis
  reductions, comparisons, and conversion as non-goals

## Context

Two Arrays in one operation needed exactly the same shape. Subtracting a
row of column means from a matrix took a loop. A reduction gave one
number for the whole Array, so a sum per row took a loop over views. An
Array could not be compared, converted to another dtype, or passed
through a math function without `map`, which calls a closure per
element.

ADR-0088 made views cheap. An operation on a view still copied the view
first, and every arithmetic result was a new buffer, so
`u[1:-1] = (u[:-2] + u[2:]) * 0.5` made three passes where one would do.

## Decision

### Broadcasting

- **Two Array operands of one dtype broadcast.** Their shapes are
  aligned at the last axis. Two dimensions match when they are equal or
  one of them is one. A missing leading dimension counts as one. The
  result has the larger dimension along each axis.
- **Shapes that do not broadcast are `AU4007`**, as unequal shapes were.
- **Broadcasting never converts.** Both operands have one dtype, and a
  scalar operand has that dtype, as before.
- **The integer mode members broadcast the same way:** `wrapping_add`,
  `saturating_sub`, and the rest.
- **A fault reports the row-major index of the result element** it
  belongs to, as the same operation on copied operands would.

### Operations work where the elements lie

- **A strided or broadcast operand is read in place.** No operand is
  copied into row-major order first.
- **`a[selection] op= value` updates the selected elements.** It means
  `a[selection] = a[selection] op value` with the selection evaluated
  once, and it gives that result. Float elements are updated where they
  lie. Nothing is written when the operation faults.
- **The result of nested arithmetic reuses a buffer.** In
  `(a + b) * 0.5` the sum is a temporary nothing else can read, so the
  product is computed into the sum's buffer.

None of this is observable except in time and memory.

### Axis reductions

`sum`, `min`, `max`, `mean`, `fast_sum`, and `fast_mean` accept one
`int64` axis.

- **The result is an Array with that axis removed.** A rank-1 Array
  gives an Array of shape `[1]`.
- **A negative axis counts from the last.** An axis outside the rank is
  `AU4007`.
- **Each element of the result is the reduction of one lane:**
  `m.sum(0)[j]` is `m[:, j].sum()`, with the same bits and the same
  faults.
- **The dtype follows the reduction without an axis.** `sum`, `min`,
  `max`, and `fast_sum` keep the dtype. `mean` and `fast_mean` give
  `Array[float64]`.
- **An empty lane follows the reduction without an axis.** Its `sum` is
  zero, and its `min`, `max`, or `mean` is `AU4007`.

### Elementwise members

| Member | On | Result |
| --- | --- | --- |
| `abs()` | every numeric dtype | The magnitude of each element. An integer minimum is `AU4002`. |
| `minimum(other)`, `maximum(other)` | every numeric dtype | The smaller or larger of each pair. `other` is an Array that broadcasts or one element. A float pair with a NaN gives NaN, and `-0.0` is below `0.0`, as `min` and `max` order them. |
| `sqrt()` | `float64` | The square root. A finite negative element is `AU4001`. |
| `exp()`, `log()`, `log2()`, `log10()`, `sin()`, `cos()`, `tan()` | `float64` | The `math` function of each element, with that function's domain and overflow rules. |
| `pow(exponent: float64)` | `float64` | `math.pow` of each element and `exponent`. |
| `floor()`, `ceil()`, `trunc()` | `float64` | `Array[int64]`, as the `math` functions return `int64`. An element that does not fit is `AU4002`. |

- **A member gives what `map` with the scalar function gives.** The
  diagnostic code is the scalar function's, and the message names the
  flat index of the first failing element.
- **The math members take `float64` only**, as the `math` module does.
  A `float32` Array is converted first.

### Conversion

- **`array as Array[T]` converts each element as the scalar cast
  does.** An integer must fit the target. An integer to a float must be
  exact. A float to an integer must be finite and in range, and is
  truncated toward zero. The first element that fails reports the
  scalar cast's diagnostic with its flat index.
- **`to_float()` on an integer Array gives `Array[float64]`**, rounding
  to nearest as the integer member does.
- **A cast to the Array's own dtype shares its buffer.**

### Comparisons and masks

- **`bool` is an Array dtype.** `Array[bool]` has the constructors,
  selections, shape members, slice assignment, `clone`, `to_list`, and
  `map` of any Array. It has no arithmetic, numeric reductions,
  products, or math members.
- **Comparisons are members:** `less`, `less_equal`, `greater`,
  `greater_equal`, `equal`, and `not_equal` take an Array that
  broadcasts or one element, and give `Array[bool]`. A float comparison
  with a NaN is `false`, except `not_equal`, which is `true`.
- **The comparison operators stay unavailable on Arrays.** `a == b`
  between lists is one `bool`. An elementwise `==` would give the same
  spelling a second meaning.
- **A mask has `any()`, `all()`, `count()`, `logical_and(other)`,
  `logical_or(other)`, and `logical_not()`.** `count()` is the number of
  `true` elements as `int64`.
- **`mask.choose(when_true, when_false)`** gives an Array that takes
  each element from `when_true` where the mask is `true` and from
  `when_false` elsewhere. Each argument is an Array that broadcasts with
  the mask or one element, and both have one dtype.

## Alternatives rejected

- **Elementwise `==` and `<` operators.** They read well in NumPy and
  make `if a == b:` an error or a surprise. A list comparison in Aura
  is one `bool`, and an Array is a collection too.
- **Masks as `Array[int32]`.** No new dtype, and every mask would be
  four bytes per element with nothing stopping arithmetic on it.
- **Type promotion while broadcasting.** `int32 + float64` giving
  `float64` hides a conversion that can lose precision. Aura converts
  nowhere else without a cast.
- **Math members on `float32`.** The `math` module is `float64` only.
  A second set of `float32` functions would need its own exceptional
  value table.
- **An axis reduction that keeps the axis with length one.** It saves a
  `reshape` before broadcasting against the source and makes the common
  case, a vector of sums, carry an axis nobody wants. `reshape` is free.
- **A fused expression evaluator.** Compiling `(a + b) * 0.5` into one
  loop removes every temporary. Reusing the temporary's buffer removes
  the allocation and half the memory traffic with no new compiler
  stage.

## Consequences

- A program that relied on `AU4007` for two shapes that happen to
  broadcast now gets a result.
- `Array[bool]` adds a fifth storage kind to the runtime. Every
  operation that cannot take a mask rejects it in the checker.
- An in-place float update that faults in the general form is detected
  before any element is written. An integer update always uses the
  general form.
- The reuse of a temporary's buffer depends on the compiler seeing that
  the temporary has one reader. A temporary bound to a name is never
  reused.
