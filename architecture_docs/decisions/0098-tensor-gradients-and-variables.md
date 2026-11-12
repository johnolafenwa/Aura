# ADR-0098: Tensor gradients are built from tensor operations, and a variable is a tensor that can be replaced

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0097 (the `tensor` module, devices, and plans)

## Context

The `tensor` module had arithmetic, reductions and `matmul`. Training and
running a neural network needs more: elements moved by index, random
numbers, gradients, weights that change, and weights on disk. The owner
asked for full support for automatic differentiation and for
transformers, for training and inference.

Three properties of ADR-0097 constrain the design. A tensor is never
changed. A GPU device runs the whole expression behind a tensor as one
plan, compiled once for each distinct plan. Every operation means the
same thing on every device.

## Decision

### Indexed operations are index maps

`slice`, `broadcast_to`, `concat` and `take` each compute an element of
the result from one element of an operand, at an index that is a
function of the result index. So they fuse into a kernel like an
elementwise step, with no buffer of their own. The reverse of a slice,
which gradients need, is one more index map that reads zero outside the
slice.

`index_add`, the reverse of `take`, is a kernel of its own: one thread
adds one element into a buffer of zeros with an atomic add. `argmax` and
`argmin` reduce one lane in one thread.

An index outside its axis is an error on the CPU device. A GPU cannot
stop the program, so there it reads as zero and adds nothing. This is
the one place where a mistake is reported on one device and not on
another.

### Random numbers depend on a seed and an index

Element `j` of a random tensor is a hash of the seed and `j`. No state
is carried from one element to the next, so a GPU makes every element in
its own thread, and the CPU device makes the same numbers. The seed is
an operand, a one-element tensor, and not part of the operation. Every
seed therefore has the same plan, and a new seed does not compile a new
kernel.

### A gradient is an expression

A float tensor computed from a watched tensor keeps the operation that
made it and its operands. `result.gradient(of)` walks those records back
from the result and builds the gradient of each operand from the
ordinary tensor operations. On a GPU the gradient is waiting nodes, and
it runs as part of whatever plan reads it, fused like any expression.
No second set of kernels exists for the backward pass.

The operations of a gradient are not themselves recorded, so there are
no second derivatives. The first request computes the gradient for every
watched tensor behind the result and keeps them on the result.

The interface is a function of values, `loss.gradient(weight)`, and not
a `backward()` call that fills a field of each weight. A tensor has no
field to fill: it is never changed.

### A variable is the one thing that changes

Training replaces weights. `Variable[T]` holds a watched tensor and can
be given another of the same dtype and shape. Its clones name one
variable, so a model and an optimizer see the same value. Tensors stay
immutable: an assignment makes the variable hold a different tensor.

Layers and optimizers are Aura source in the module. A layer is a class
whose fields are variables. An optimizer holds clones of them.

### Several tensors run as one plan

An optimizer step needs the loss, a new value for each weight, and the
optimizer's own state. These share the forward and backward passes. A
plan can have several outputs, and `evaluate(tensors)` runs a list of
tensors as one plan, so shared work runs once. The Metal device runs
them one at a time for now.

### Weights on disk are safetensors

The file format is the one PyTorch and the Hugging Face libraries read
and write: a JSON header, then little-endian elements. Loading and
saving give a `Result` with a message, as file operations do elsewhere.

## Consequences

- A program reads the loss after the optimizer step. Reading it before
  runs the forward pass once for the loss and again in the step, because
  only the outputs of a plan keep their memory.
- The plan of a training step is large. It is compiled once and kept, in
  memory and on disk.
- The Metal device runs the indexed operations, `tanh`, `sigmoid`, `erf`
  and the random tensors in host memory until its shim has them.
- A long chain of tracked tensors is freed by a chain of drops of the
  same length.

## Not decided here

- Convolution, pooling and batch normalization.
- `float16` tensors and mixed precision.
- More than one GPU, and more than one machine.
