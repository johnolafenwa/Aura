# Tensor Module

The `tensor` module has `Tensor[T]`: numbers of one dtype that live on a
device and are computed there. The device is the CPU, the Metal GPU of a
Mac, or an NVIDIA GPU through CUDA. The same
program runs on either, and gives the same integers and masks and the
same floats to within a few units in the last place.

```aura
from tensor import Device, Tensor
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
from tensor import Device, Tensor, where

def main():
    device = Device.default()
    prices = Array[float32].from_list([10.0, 12.5, 9.0, 14.0], [4])
    counts = Array[float32].from_list([3.0, 1.0, 4.0, 2.0], [4])
    x = Tensor[float32].from_array(prices, device)
    y = Tensor[float32].from_array(counts, device)
    total = (x * y).sum()
    print(total.item())
    capped = where(x.greater(x.scalar(12.0)), x.scalar(12.0), x)
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
| `sqrt()`, `exp()`, `log()`, `sin()`, `cos()`, `tanh()`, `erf()` | The function of each element. Float tensors. |
| `sigmoid()` | `1 / (1 + exp(-x))` of each element. Float tensors. |
| `gelu()` | `x * (1 + erf(x / sqrt(2))) / 2` of each element. Float tensors. |
| `softmax(dim=-1)`, `log_softmax(dim=-1)` | Each lane along `dim` as positive numbers that sum to one, and the logarithm of that, computed without overflow. |
| `less(other)`, `less_equal(other)`, `greater(other)`, `greater_equal(other)`, `equal(other)`, `not_equal(other)` | A `Tensor[bool]` of where the comparison holds. |
| `where(mask, input, other)` | A module function. Each element of `input` where `mask` holds, else of `other`. |
| `sum(dim=None, keepdim=false)`, `min(...)`, `max(...)`, `mean(...)` | Over every element, as a tensor of shape `[1]`, or along one axis, which removes that axis. With `keepdim`, the result has the tensor's rank, with length one on each reduced axis, so it broadcasts against the tensor. `mean` is for float tensors and keeps the dtype. |
| `argmax(dim=-1)`, `argmin(dim=-1)` | A `Tensor[int64]` of the place of the largest or smallest element of each lane along `dim`. The first wins a tie, and a NaN wins over every number. |
| `matmul(other)` | The matrix product, with the `Array.matmul` shape rules. |
| `reshape(shape)`, `flatten()`, `transpose()`, `permute(axes)` | The same elements in a new shape or axis order. |
| `slice(dim, start, stop, step=1)` | The entries from `start` up to, and not including, `stop` along `dim`, `step` apart. A negative bound counts from the end. |
| `index_select(dim, indices)` | For each index of a `Tensor[int64]`, the entry at that place along `dim`. The shape of `indices` takes the place of the axis. With a table of rows this is an embedding lookup. |
| `index_add(indices, size, dim=0)` | The reverse of `index_select`: a tensor with `size` entries along `dim`, where entry `k` is the sum of this tensor's entries whose index is `k`. |
| `cat(tensors, dim=0)` | A module function: the tensors of a list, one after another along `dim`. |
| `expand(shape)` | This tensor repeated to a shape that its own broadcasts to. |
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

An index given to `index_select` or `index_add` must be inside the axis. The CPU
device reports one that is not as `AU4007`. A GPU cannot stop the
program, so there such an index reads as zero and adds nothing.

## Random Tensors

| API | Result |
| --- | --- |
| `rand(shape, seed, device)` | A `Tensor[float32]` of floats spread evenly from zero up to, and not including, one. |
| `randn(shape, seed, device)` | A `Tensor[float32]` of the normal distribution with mean zero and deviation one. |
| `arange(count, device)` | A `Tensor[int64]` of the integers from zero up to, and not including, `count`. |
| `dropout(x, p, seed)` | `x` with a share `p` of its elements set to zero and the others scaled up by `1 / (1 - p)`. |

A seed always gives the same tensor, on every device, and a longer
tensor begins with the shorter one of the same seed. Use a new seed for
each tensor that must differ, such as the step number for `dropout`. A
GPU makes the numbers in its own memory.

## Gradients

`requires_grad()` gives a float tensor that gradients are taken with
respect to. A float tensor computed from one remembers how it was made.
`result.gradient(of)` is the gradient of the sum of `result` with
respect to `of`, in the shape of `of`. It is a value: nothing changes in
place.

```aura
from tensor import Device, Tensor

def main():
    device = Device.default()
    x = Tensor[float32].from_array(Array[float32].from_list([1.0, 2.0, 3.0], [3]), device).requires_grad()
    y = (x * x + x * 2.0).sum()
    print(y.gradient(x).to_array())
```

```text
Array[float32](shape=[3], values=[4.0, 6.0, 8.0])
```

| API | Result |
| --- | --- |
| `requires_grad()` | This tensor as one that gradients are taken with respect to. Float tensors only. |
| `detach()` | This tensor as a constant. No gradient passes through it. |
| `gradient(of)` | The gradient with respect to `of`. A tensor the result does not depend on has a gradient of zeros. |
| `backward()` | Adds the gradient to the `grad` of every `Parameter` behind this tensor. See the next section. |

The first `gradient` or `backward` call on a result computes the
gradients for every tensor behind it, and later calls on the same result
take them. A gradient is built from the ordinary operations, so on a GPU
it waits and runs with the rest of the expression. Every float operation
has a gradient except `matmul` with a rank-one operand. Reshape the
vector to a matrix first. Masks and integer tensors have none. Where
several elements tie for a `min` or `max`, they share the gradient
equally. A gradient is not itself tracked, so there are no second
derivatives.

## Parameters, Layers, and Optimizers

The names here follow PyTorch: `Parameter`, `forward`, `parameters`,
`backward`, `grad`, `zero_grad`, and `step`. A training loop reads the
way it does there:

```aura
import tensor
from tensor import Device, Linear, SGD, Tensor

def main():
    device = Device.default()
    # Learn y = 2 * a - 3 * b + 1 from four points.
    inputs = Tensor[float32].from_array(Array[float32].from_list([1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, -1.0], [4, 2]), device)
    wanted = Tensor[float32].from_array(Array[float32].from_list([3.0, -2.0, 0.0, 8.0], [4, 1]), device)
    mut model = Linear(2, 1, 7, device)
    mut optimizer = SGD(model.parameters(), lr=0.1)
    for step in range(500):
        error = model.forward(inputs) - wanted
        loss = (error * error).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    print(model.weight.value().to_array())
```

### What Can Change

A `Tensor[T]` never changes. A `Parameter[T]` is the one tensor type
that does: it holds a float tensor that training replaces. The compiler
keeps this rule:

> The weights of a model that is not declared `mut` never change.

It holds because of three things. `parameters()` takes `mut self`, so
only a model declared `mut` can give its parameters to an optimizer.
`step()` and `zero_grad()` take `mut self`, so the optimizer is declared
`mut` too. And a program cannot copy a `Parameter`: moving one out of a
model ends the use of that model. So `model = Linear(...)` without `mut`
is a model you can pass around and run, and no code can train it.

`loss.backward()` is the one effect the types do not show. It adds to
the `grad` of each parameter behind `loss`. A gradient is working
storage: it never changes what `forward` computes.

| API | Result |
| --- | --- |
| `Parameter[T](value: Tensor[T])` | A parameter that starts as a float tensor. |
| `value(self) -> Tensor[T]` | The tensor the parameter holds now. A result computed from it has a gradient with respect to it. |
| `grad` | A property: the gradients `backward()` has added since the parameter was last replaced. Zeros when there are none. |
| `copy_(mut self, value: Tensor[T])` | Replaces the tensor the parameter holds, for example with loaded weights, and clears `grad`. |
| `evaluate(tensors)` | A module function. Runs the work of a list of tensors as one piece, so an expression several of them share is computed once. |

### Gradients Add Up, and a Step Uses Them

Each `backward()` adds to `grad`, as in PyTorch, so a loop can call it
for several batches before one `step()`. `step()` clears the gradients
it used. A loop that forgets `zero_grad()` therefore still trains
correctly, which differs from PyTorch, where the old gradients would
stay. `zero_grad()` is for dropping gradients that no step will use.

`backward()` does not run anything on a GPU. `step()` runs the loss,
every gradient, and every new weight as one piece of GPU work. Read the
loss with `item()` after `step()`. Reading it between `backward()` and
`step()` is correct and slower: the model runs once for the loss and
again inside the step.

### Layers

Each layer is a class whose fields are parameters. Its constructor makes
fresh weights from a seed, `forward` runs it, and `parameters()` lists
its parameters for an optimizer. A model is a class of your own whose
fields are layers, with the same two methods.

| Class or function | What it computes |
| --- | --- |
| `Linear(in_features, out_features, seed, device)` | `x.matmul(weight) + bias` over the last axis. `weight` has shape `[in_features, out_features]`. |
| `Embedding(num_embeddings, embedding_dim, seed, device)` | A row of `weight` for each index of a `Tensor[int64]`. |
| `LayerNorm(normalized_shape, device)` | Each lane along the last axis moved to mean zero and deviation one, then scaled by `weight` and moved by `bias`. |
| `MultiheadAttention(embed_dim, num_heads, seed, device)` | Self-attention with several heads over a `[batch, length, width]` tensor. `forward(x, is_causal=false)`. |
| `MixtureOfExperts(width, hidden, experts, active, seed, device)` | Several two layer networks and a gate. `forward(x)` weights the outputs of each row's `active` highest scoring experts by the softmax of their scores. `weights(x)` gives those weights. Every expert runs on every row. |
| `one_hot(indices, num_classes)` | A mask with one more axis, of length `num_classes`, that holds at the place each index names. |
| `top_indices(x, count)` | The places of the `count` largest elements of each lane along the last axis, largest first. |
| `top_mask(x, count)` | A mask in the shape of `x` that holds at the `count` largest elements of each lane along the last axis. |
| `scaled_dot_product_attention(query, key, value, is_causal=false)` | Attention over the last two axes. With `is_causal`, row `i` attends to rows `0` to `i` only. |
| `cross_entropy(logits, targets)` | The mean over the rows of `logits` of the negative logarithm of the softmax entry the row's target names. |
| `SGD(params, lr)` | `step()` moves each parameter against its gradient. |
| `AdamW(params, lr=0.001, weight_decay=0.01)` | `step()` takes one Adam step with weight decay. |

Both optimizers also have `minimize(loss)`, which is `loss.backward()`
followed by `step()`.

### Differences From PyTorch

- A tensor's dtype is part of its type: `Tensor[float32]`.
- The device and the random seed are arguments. There is no default
  device, global seed, or gradient mode to set.
- A layer's weight has shape `[in_features, out_features]`, and
  `forward` is called by name.
- `dim` and `keepdim` are spelled as in PyTorch. Aura's `Array` uses
  `axis` for the same thing.
- `step()` clears the gradients it used.

## Saving and Loading

Tensors are saved in the safetensors file format, which PyTorch and the
Hugging Face libraries read and write.

| API | Result |
| --- | --- |
| `save(path, names, tensors) -> Result[int64, str]` | Writes a list of tensors of one dtype under a list of names. Gives the file's length in bytes. |
| `weight_names(path) -> Result[list[str], str]` | The names of the tensors in a file, in the file's order. |
| `load_float32(path, name, device) -> Result[Tensor[float32], str]` | One float tensor of a file. A tensor stored as `float16`, `bfloat16`, or `float64` is converted. |
| `load_int64(path, name, device) -> Result[Tensor[int64], str]` | One integer tensor of a file. |

The error is a message that names the file and what is wrong with it. A
tensor the file stores with no axes loads with shape `[1]`.

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
- `where`, `reshape`, `flatten`, `transpose`, `permute`
- on CUDA: `tanh`, `sigmoid`, `erf`, `slice`, `index_select`, `index_add`,
  `cat`, `expand`, `argmax`, `argmin`, and the random tensors.
  On Metal these run on the CPU for now.
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
| Integer arithmetic, comparisons, `where`, shape changes | Equal. |
| `+`, `-`, `*`, `/`, `sqrt`, `abs`, `minimum`, `maximum`, `min`, `max` on `float32` | Equal. |
| `sum`, `mean`, `matmul` | Within a few units in the last place. The GPU adds in a different order. |
| `exp`, `log`, `sin`, `cos`, `pow`, `tanh`, `sigmoid`, `erf`, `randn` | Within a few units in the last place. |
| `slice`, `index_select`, `cat`, `expand`, `argmax`, `argmin`, `rand` | Equal. |
| `index_add` | Within a few units in the last place. The GPU adds in a different order. |

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

- No integer division. `Array` has none either.
- No convolution or pooling yet.
- No `float16` tensors. A `float16` file loads as `float32`.
- One GPU. A tensor does not span devices or machines.
- No way yet to write your own GPU operation. That is the planned kernel
  layer, which will work on these same tensors without copying them.
