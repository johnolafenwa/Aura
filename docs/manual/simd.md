# SIMD Module

The `simd` module has values that hold two, four, or eight floats and do
arithmetic on all of them at once. Native code keeps such a value in vector
registers, so one `+` adds every lane with one or two instructions.

```aura
import simd
```

Use it when a loop over a list of floats does not turn into vector
instructions on its own. The usual case is a float sum, which Aura adds in
source order: with lane values you choose the order yourself, and it is the
same on every machine.

## Lane Classes

| Class | Lanes | Element type |
| --- | --- | --- |
| `simd.F64x2` | 2 | `float64` |
| `simd.F64x4` | 4 | `float64` |
| `simd.F32x4` | 4 | `float32` |
| `simd.F32x8` | 8 | `float32` |

Each class is an ordinary Copy class whose public fields `l0`, `l1`, and so
on are the lanes. You can construct one by naming its fields, read a lane
with `value.l2`, pass it to a function, return it, and store it in a class
field. Assigning one copies it.

There are no integer lane classes. Integer arithmetic in Aura reports
overflow, and a check in each lane would cost more than the lanes save.

## Public API

`V` stands for the lane class and `E` for its element type.

| API | Signature | Result |
| --- | --- | --- |
| `V.splat` | `splat(value: E) -> V` | Every lane holds `value`. |
| `V.load` | `load(values: list[E], at: int64) -> V` | The elements of `values` from index `at`, one for each lane. |
| `store` | `store(self, values: mut list[E], at: int64)` | Writes the lanes over the elements of `values` from index `at`. |
| `+`, `-`, `*`, `/` | `V` with `V` | The operation in each lane. |
| unary `-` | `V` | Each lane negated. |
| `min` | `min(self, other: V) -> V` | In each lane, `self` where it is less than `other`, else `other`. |
| `max` | `max(self, other: V) -> V` | In each lane, `self` where it is greater than `other`, else `other`. |
| `abs` | `abs(self) -> V` | The absolute value of each lane. |
| `sqrt` | `sqrt(self) -> V` | The square root of each lane. |
| `mul_add` | `mul_add(self, factor: V, addend: V) -> V` | `self * factor + addend` in each lane. |
| `sum` | `sum(self) -> E` | The sum of the lanes, added in pairs. |
| `to_list` | `to_list(self) -> list[E]` | The lanes as a new list, `l0` first. |

Both operands of an operator must be the same lane class. To scale by one
number, make it a lane value first with `splat`.

## Example

```aura
from simd import F64x4

def dot(xs: list[float64], ys: list[float64]) -> float64:
    mut acc = F64x4.splat(0.0)
    for i in range(0, len(xs) - 3, 4):
        acc = acc + F64x4.load(xs, i) * F64x4.load(ys, i)
    mut total = acc.sum()
    for i in range(len(xs) - len(xs) % 4, len(xs)):
        total += xs[i] * ys[i]
    return total

def main():
    xs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    ys = [0.5, 0.25, 2.0, 1.0, 3.0, 0.5]
    print(dot(xs, ys))
```

```text
29.0
```

The first loop takes four elements at a time and stops while four are still
left. The second loop finishes the last one to three elements.
`examples/numbers/simd_lanes.au` also clamps a `float32` list with
`F32x8`.

## Exact Results

Every operation is defined lane by lane in terms of the scalar operation, so
a result never depends on the CPU, on `--target-cpu`, or on whether the
program runs under `aura run` or as a native binary.

- **`sum`** adds neighbors first: `(l0 + l1) + (l2 + l3)` for four lanes,
  and `((l0 + l1) + (l2 + l3)) + ((l4 + l5) + (l6 + l7))` for eight.
- **`mul_add`** rounds after the product and again after the sum. It is not
  a fused multiply-add.
- **`min` and `max`** return `other` when the comparison is false. So a NaN
  in `self` gives `other`, and a NaN in `other` gives the NaN.
- **`sqrt`** of a negative lane is NaN.
- **`/`** by a lane that holds zero traps with `AU4004`, as scalar float
  division does.
- **`float32` lanes** round each result to `float32`, as scalar `float32`
  arithmetic does.

A loop that keeps a lane value as its running sum adds in a different order
than a loop that keeps one float. The two can differ in their last bits.

## Loads And Stores

`load` and `store` read and write one element for each lane, at `at`,
`at + 1`, and so on. Each index follows the list index rules: it must be
inside the list, or the program traps with `AU4003`, and a negative index
counts from the end.

```aura
from simd import F32x4

def main():
    mut out: list[float32] = [0.0, 0.0, 0.0, 0.0, 0.0]
    F32x4.splat(2.5).store(out, 1)
    print(out)
```

```text
[0.0, 2.5, 2.5, 2.5, 2.5]
```

A `store` at index 2 of that list would trap, because the fourth lane has no
element to go to.

When the list is a local variable or a parameter and the arguments are
positional, the compiler treats the call as the element reads or writes it
stands for. A [counted loop](/manual/performance#counted-loops-over-number-lists)
then checks the range of every load and store once, before the loop starts,
and a trap reports the line of the call. Any other call, such as
`F64x4.load(self.samples, i)`, runs the method and checks each element.
Bind the list to a local first to get the fast form.

A value read from a list and written back to the same list needs no
temporary:

```aura
from simd import F64x4

def clamp(xs: mut list[float64], low: float64, high: float64):
    lo = F64x4.splat(low)
    hi = F64x4.splat(high)
    for i in range(0, len(xs) - 3, 4):
        F64x4.load(xs, i).max(lo).min(hi).store(xs, i)

def main():
    mut xs = [-5.0, 0.5, 12.0, 3.0, 99.0]
    clamp(xs, 0.0, 10.0)
    print(xs)
```

```text
[0.0, 0.5, 10.0, 3.0, 99.0]
```

## Performance

A native build copies each lane method into its caller and keeps the lanes
in registers. On a baseline x86-64 build an `F64x4` is two 128-bit
registers. With `--target-cpu x86-64-v3` or `native` on a machine with AVX2
it is one 256-bit register. On Apple Silicon it is two 128-bit registers.
The results are the same in every case.

On `benchmarks/simd`, a dot product over 1,024 `float64` elements with
`F64x4` runs about three times as fast as the scalar loop, which has to add
in source order.

These limits apply:

- **A program that starts tasks** checks whether to yield once in each loop
  iteration. The lane arithmetic inside an iteration is still vector code.
- **`Array` elements** are not loaded directly. Use the `Array` kernels, or
  copy into a list.
- **`aura run` on the interpreter backend** computes each lane as a
  separate float. The results are the same and there is no speedup.
