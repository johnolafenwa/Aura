# Tensor Module

The `tensor` module has `Tensor[T]`: numbers of one dtype that live on a
device and are computed there. The device is the CPU, the Metal GPU of a
Mac, or an NVIDIA GPU through CUDA. The same
program runs on either, and gives the same integers and masks and the
same floats to within a few units in the last place.

```aura
from tensor import Device, Tensor, choose
```

Use an `Array` for numeric work on the CPU. Use a tensor when the work is
large enough to be worth a GPU: arithmetic over millions of elements, or a
long expression over big operands. Small work is faster on the CPU,
because each run on a GPU has a fixed cost of about a fifth of a
millisecond.

This release has the CPU device on every platform, the Metal device on a
Mac, and the CUDA device on Linux. See [Devices](#devices) for what each
needs.

## Example

```aura
from tensor import Device, Tensor, choose

def main():
    device = Device.default()
    prices = Array[float32].from_list([10.0, 12.5, 9.0, 14.0], [4])
    counts = Array[float32].from_list([3.0, 1.0, 4.0, 2.0], [4])
    x = Tensor[float32].from_array(prices, device)
    y = Tensor[float32].from_array(counts, device)
    total = (x * y).sum()
    print(total.item())
    capped = choose(x.greater(x.scalar(12.0)), x.scalar(12.0), x)
    print(capped.to_array())
```

```text
106.5
Array[float32](shape=[4], values=[10.0, 12.0, 9.0, 12.0])
```

## Devices

`Device` is a Copy class that names where a tensor's memory is.

| API | Result |
| --- | --- |
| `Device.cpu()` | The CPU device. Every machine has it. |
| `Device.metal()` | The Metal GPU of this Mac as `Device \| None`. `None` on other systems and on a Mac whose GPU cannot be used, such as a virtual machine. |
| `Device.cuda()` | The NVIDIA GPU of this machine as `Device \| None`. `None` when there is no NVIDIA driver or the CUDA libraries are missing. |
| `Device.default()` | The GPU when this machine has one Aura can use, else the CPU. |
| `name(self) -> str` | `"cpu"`, `"metal"`, or `"cuda"`. |
| `is_gpu(self) -> bool` | Whether the device is a GPU. |

A tensor stays on the device it was made on. Both operands of an operation
must be on one device. `to(device)` copies a tensor to another device.

The CUDA device needs an NVIDIA driver and two libraries of the CUDA
toolkit at run time: NVRTC, which compiles kernels, and cuBLAS. Aura
looks for them in the loader's path and under `CUDA_HOME`, `CUDA_PATH`,
and `/usr/local/cuda`. Building Aura or an Aura program needs no CUDA
toolkit. The device uses the first GPU.

There is no Windows GPU device yet. The CPU device runs every program
there.

## Dtypes

`T` is `float32`, `int32`, `int64`, or `bool` on every device, and also
`float64` on the CPU and CUDA devices. A mask, the result of a
comparison, is a `Tensor[bool]`.

Metal has no `float64`. Making a `Tensor[float64]` on the Metal device, or
converting one to `float64` there, is error `AU4007`. A tensor never
narrows a `float64` silently. Convert the `Array` first, with `values as Array[float32]`.

Float operations on a tensor follow IEEE 754 on every device: an
overflow gives an infinity, division by zero gives an infinity or a NaN,
and a value outside a function's domain, such as `log` of a negative
number, gives a NaN. The program does not
stop. `Array` division and the `Array` math members stop the program in those
cases.

Integer arithmetic on a tensor wraps on every device. A GPU cannot stop a
program on overflow, and a result must not depend on the device. This is
different from an `Array`, whose integer arithmetic is checked.

## Making and Reading Tensors

These members name the dtype, so they are called on `Tensor[T]` with the
dtype written: `Tensor[float32].from_array(values, device)`. `E` is the
element type.

| API | Signature | Result |
| --- | --- | --- |
| `Tensor[E].from_array` | `from_array(values: Array[E], device: Device) -> Tensor[E]` | A tensor holding a copy of the Array's elements. |
| `Tensor[E].full` | `full(shape: list[int64], value: E, device: Device) -> Tensor[E]` | Every element is `value`. |
| `Tensor[E].zeros` | `zeros(shape: list[int64], device: Device) -> Tensor[E]` | Every element is zero, or `false`. |
| `to_array` | `to_array(self) -> Array[E]` | An Array holding a copy of the elements. |
| `item` | `item(self) -> E` | The element of a tensor that has exactly one. |
| `scalar` | `scalar(self, value: E) -> Tensor[E]` | A one-element tensor on this tensor's device, to use as an operand. |

Moving between an `Array` and a tensor is always written. No operation
takes one where it expects the other.

## Operations

A tensor is never changed. Each operation gives a new tensor. Shapes
follow the [`Array` rules](/manual/numeric-arrays): two shapes broadcast
when each pair of dimensions, counted from the last, is equal or has a
one.

| API | Result |
| --- | --- |
| `a + b`, `a - b`, `a * b` | The operation on each pair of elements. |
| `a / b` | Division, for float tensors. |
| `-a` | Each element negated. |
| `minimum(other)`, `maximum(other)` | The smaller or larger of each pair. A NaN operand gives NaN. |
| `pow(exponent)` | Each element raised to the matching exponent. Float tensors. |
| `powf(exponent)` | Each element raised to one `float64` exponent. Float tensors. |
| `relu()` | Each element, or zero where it is negative. |
| `clamp(low, high)` | Each element kept between two `float64` bounds. Float tensors. |
| `abs()` | The absolute value of each element. |
| `sqrt()`, `exp()`, `log()`, `sin()`, `cos()` | The function of each element. Float tensors. |
| `less(other)`, `less_equal(other)`, `greater(other)`, `greater_equal(other)`, `equal(other)`, `not_equal(other)` | A `Tensor[bool]` of where the comparison holds. |
| `choose(mask, when_true, when_false)` | A module function. Each element of `when_true` where `mask` holds, else of `when_false`. |
| `sum(axis=None)`, `min(axis=None)`, `max(axis=None)`, `mean(axis=None)` | Over every element, as a tensor of shape `[1]`, or along one axis, which removes that axis. `mean` is for float tensors and keeps the dtype. |
| `matmul(other)` | The matrix product, with the `Array.matmul` shape rules. |
| `reshape(shape)`, `flatten()`, `transpose()`, `permute(axes)` | The same elements in a new shape or axis order. |
| `to_int32()`, `to_int64()`, `to_float32()`, `to_float64()` | Each element converted. A float that does not fit the integer type is an error. |
| `to(device)` | This tensor on another device. |
| `shape()`, `len()`, `device()` | The shape as `list[int64]`, the element count, and the device. |
| `clone()` | Another handle to the same memory. Nothing is copied. |
| `eval()` | Runs the work the tensor is waiting on. |

An operator takes two tensors of the same dtype, or a tensor and a
number on either side: `x * 2.0`, `1 - x`, `x / n`. The number becomes
one element of the tensor's dtype, so `x * 2.0` on a `Tensor[float32]`
stays `float32`. An integer tensor takes integers only: a float with it
is error `AU4007`, because its fraction would be lost unseen. The
elementwise members take tensors. For a number there, use `relu()`,
`clamp(low, high)`, `powf(exponent)`, or make a tensor with `scalar`:
`x.greater(x.scalar(12.0))`.

A mistake in shapes or dtypes is reported by the call that makes it, on
every device, with the message the same `Array` operation gives.

## When the Work Runs

The CPU device runs each operation when it is called.

A GPU device waits. An operation gives a tensor that knows its shape and
dtype but has not been computed. The whole expression behind a tensor
runs as one piece of GPU work when the program reads it: `to_array()`,
`item()`, `to(device)`, or `eval()`. This is why a long expression costs
one trip to the GPU and not one for each operator.

```aura fragment
z = (x * y + x).sqrt()    # nothing has run on the GPU
z.eval()                  # the three operations run together
```

Aura compiles each distinct expression for the GPU once and keeps it. A
loop that evaluates the same expression over tensors of the same shapes
pays that cost on its first pass only.

A chain of more than 128 waiting operations runs when it reaches that
length, so a loop that never reads a result cannot grow without bound.

The results do not depend on when the work runs.

## What Runs on the GPU

On a GPU device these run on the GPU:

- `+`, `-`, `*`, `minimum`, `maximum`, negation, and comparisons, for
  float and integer tensors
- `/`, `pow`, `abs`, `sqrt`, `exp`, `log`, `sin`, `cos`, the four
  reductions, and `matmul` of operands of rank two or more, for float
  tensors
- `choose`, `reshape`, `flatten`, `transpose`, `permute`
- conversions from an integer dtype to a float dtype, and from `int32` to
  `int64`

A float tensor is `float32` on Metal and `float32` or `float64` on CUDA.

Every other operation on a GPU tensor runs on the CPU and gives a tensor
on the same device: integer reductions and `abs`, which are checked for
overflow, conversions from a float to an integer, which are checked for
range, integer `matmul`, and `matmul` with a rank-one operand. So every
operation exists on every device and means the same thing. An empty
tensor is always computed on the CPU.

The two GPU devices do the work differently. Metal hands the expression
to Apple's MPSGraph. CUDA has no library of elementwise operations, so
Aura writes the expression as one CUDA kernel, compiles it the first time
the expression is evaluated, and keeps the compiled kernel. A chain of
elementwise operations is a single pass over the data with no temporary
tensors in between. `matmul` is a cuBLAS call. The first evaluation of
each distinct expression on CUDA therefore takes 10 to 20 milliseconds,
and later ones take none of that. Compiled kernels are also kept on disk,
in `cuda-kernels` under `~/.cache/aura` (or under `AURA_CACHE_DIR` when
it is set), so a later run of the program does not compile them again.
The directory can be deleted at any time.

## Results Across Devices

| Operation | GPU result compared with the CPU device |
| --- | --- |
| Integer arithmetic, comparisons, `choose`, shape changes | Equal. |
| `+`, `-`, `*`, `/`, `sqrt`, `abs`, `minimum`, `maximum`, `min`, `max` on `float32` | Equal. |
| `sum`, `mean`, `matmul` | Within a few units in the last place. The GPU adds in a different order. |
| `exp`, `log`, `sin`, `cos`, `pow` | Within a few units in the last place. |

The test suite checks these on a machine with a GPU. The hosted CI
machines have no usable GPU, so the GPU checks run on a developer Mac and
on an NVIDIA machine and report that they were skipped elsewhere. The
generated CUDA kernels are also compiled for the CPU and checked on every
CI run.

## Errors

| Code | When |
| --- | --- |
| `AU4007` | Shapes that do not broadcast, operands of different dtypes or devices, an axis outside the rank, `item()` on a tensor with more than one element, `float64` on the Metal device, a device this machine does not have, or a GPU failure. |

## Limits

- No indexing or slicing of a tensor. Convert with `to_array()`.
- No integer division. `Array` has none either.
- No convolution, pooling, or other neural network layers yet.
- No way yet to write your own GPU operation. That is the planned kernel
  layer, which will work on these same tensors without copying them.
