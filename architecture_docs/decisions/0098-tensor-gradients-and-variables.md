# ADR-0098: Tensor gradients are built from tensor operations, and a parameter is the one tensor that changes

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

`slice`, `expand`, `cat` and `index_select` each compute an element of
the result from one element of an operand, at an index that is a
function of the result index. So they fuse into a kernel like an
elementwise step, with no buffer of their own. The reverse of a slice,
which gradients need, is one more index map that reads zero outside the
slice.

`index_add`, the reverse of `index_select`, is a kernel of its own: one thread
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

A float tensor computed from a tensor marked with `requires_grad()` keeps the operation that
made it and its operands. `result.gradient(of)` walks those records back
from the result and builds the gradient of each operand from the
ordinary tensor operations. On a GPU the gradient is waiting nodes, and
it runs as part of whatever plan reads it, fused like any expression.
No second set of kernels exists for the backward pass.

The operations of a gradient are not themselves recorded, so there are
no second derivatives. The first request computes the gradient for every
such tensor behind the result and keeps them on the result.

### The names are PyTorch's, and the guarantees are Aura's

The owner's direction on 2026-11-12 is that the machine learning side
reads like PyTorch without losing the core of Aura. The module therefore
uses PyTorch's names: `rand`, `randn`, `cat`, `where`, `expand`,
`index_select`, `dim`, `keepdim`, `Parameter`, `forward`,
`parameters()`, `backward()`, `grad`, `zero_grad()`, `step()`, and layer
constructors such as `Linear(in_features, out_features, ...)`. A dtype
stays part of the type, and a device and a seed stay arguments.

Both forms of a gradient exist. `loss.gradient(of)` is a function of
values. `loss.backward()` adds the gradient to the `grad` of every
parameter behind the loss, and an optimizer's `step()` reads them.

### A parameter is the one thing that changes, and `mut` guards it

A `Tensor[T]` never changes. `Parameter[T]` holds a tensor and can be
given another of the same dtype and shape. The compiler keeps this rule:
the weights of a model that is not declared `mut` never change.

- `parameters()` of every layer takes `mut self`, so only a model
  declared `mut` gives its parameters to an optimizer.
- `step()`, `zero_grad()` and `Parameter.copy_` take `mut self`.
- A handle to a parameter is copied only inside the module, by
  `parameters()`. `share`, `assign` and `zero_grad` of `Parameter` are
  private, and `Parameter` has no public `clone`. Moving a parameter out
  of a model ends the use of that model.

`backward()` is the one effect the types do not show: the loss does not
name the parameters whose `grad` it fills. A gradient is working storage
and never changes what a model computes.

`step()` clears the gradients it used, because it replaces each
parameter's tensor and a gradient belongs to the tensor it was taken
for. A loop that forgets `zero_grad()` is correct. Gradients still add
up over several `backward()` calls before one `step()`.

`backward()` runs nothing. It sets the loss aside, and the `evaluate` in
`step()` runs the loss, the gradients, the new weights and the
optimizer's state as one plan.

Layers and optimizers are Aura source in the module. A layer is a class
whose fields are parameters. An optimizer holds handles to them.

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
