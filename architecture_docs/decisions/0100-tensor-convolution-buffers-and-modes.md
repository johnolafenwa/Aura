# ADR-0100: Convolution from indexed reads and a product, and buffers and modes of a module

- Status: Accepted
- Date: 2026-11-13

## Context

A residual network needs convolution, pooling and batch norm. The
`tensor` module had none of them. A batch norm also needs two things a
`Module` (ADR-0098, ADR-0099) did not have: tensors that a model keeps
and saves but no optimizer changes, and a mode that differs between
training and evaluation.

A device adds an operation in three places: the CPU kernels, the CUDA
code generator and the Metal device, each with a gradient.

## Decision

Convolution and pooling are Aura source in `std/tensor.au`, built from
operations every device already has.

- `windows` reads every window of a `[batch, channels, height, width]`
  tensor with one `index_select` over the flattened image. An image with
  padding gets one more element after its last, and every index outside
  the image names it.
- `conv2d` is those windows times the weight as one matrix product.
  `max_pool2d` and `avg_pool2d` reduce the windows along their last
  axis. The gradients are the gradients of `index_select`, `matmul`,
  `max` and `mean`.
- `BatchNorm2d` is elementwise arithmetic and means.

A `Collector` gathers two more things. `buffer(name, param)` gathers a
tensor that `state_dict()` saves and `load_state_dict()` loads, and that
`parameters()` and `named_parameters()` leave out. `mode(flag)` gathers
a flag, a one element tensor on the CPU device. `Module.train()` and
`Module.eval()` set every flag of a model to one or zero. Both take
`mut self`.

`BatchNorm2d.forward` in training mode assigns its running statistics
through `self`, which is not `mut`.

## Consequences

- The new layers run on the CPU, Metal and CUDA devices with no new
  device code, and a convolution's product runs in cuBLAS.
- A convolution holds a copy of the image for each kernel element, nine
  for a three by three kernel. A device operation that calls cuDNN or
  MPSGraph would hold less and may run faster. It can replace `conv2d`
  later without a change to programs.
- The window indices are built on the host for each call.
- Reading a mode flag is a read of host memory, so it does not wait for
  the GPU.
- Running statistics change through a model that is not `mut`. The rule
  that weights change only through a `mut` binding still holds for
  parameters.
- Kernels are square, with one stride and one padding. Dilation, groups
  and other dimensions are not covered.
