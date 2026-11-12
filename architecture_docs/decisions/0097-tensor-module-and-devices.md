# ADR-0097: The `tensor` module computes on a device, and a GPU runs an expression as one plan

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0041 (numeric Arrays), ADR-0088 (Array views), ADR-0089
  (broadcasting, reductions, comparisons), ADR-0090 (batched `matmul`),
  ADR-0087 (the `simd` module, for a builtin module written partly in
  Aura)

## Context

Aura's numeric work ran only on the CPU. The goal is a GPU kernel layer
in the manner of OpenAI's Triton language: kernels written in Aura and
compiled for a GPU. The owner chose the order. First a layer of tensor
operations that call the libraries each platform already has. Then the
kernel compiler, on the same tensors.

The first layer has to leave room for the second. A kernel must be able
to read and write a tensor's memory without a copy, and must see the
result of the library operations that came before it.

A probe on an Apple M2 Pro measured what Metal's MPSGraph allows:

- A graph reads an `MTLBuffer` the program allocated, and writes its
  result into one, with no copy either way. A compute kernel compiled at
  run time reads the same buffer.
- Each run costs 150 to 250 microseconds whatever the size. Building a
  graph costs about 3.5 milliseconds.
- `float32`, `int32` and `int64` arithmetic work. `float64` does not
  exist. Integer and `bool` `matmul`, and anything on `float64`, kill the
  process. MPSGraph reports a mistake by aborting, never by an error.
- Adding 16 million `float32` takes 1.4 ms against about 5 ms on the CPU.
  A 1024 by 1024 `matmul` is no faster than the CPU.

## Decision

### A separate type with a device

`Tensor[T]` is a new type in a builtin module `tensor`, separate from
`Array[T]`. A tensor has a device, which is a property of the value at
run time and not of its type: `Device.cpu()`, `Device.metal()`. Moving
between an Array and a tensor is always written, with `from_array` and
`to_array`. Both operands of an operation are on one device.

A tensor is immutable. Each operation gives a new tensor. This is what
lets a device put off the work.

The surface is ordinary Aura. `Tensor[T]` is a generic class written in
`std/tensor.au`. Operators come from trait impls, members follow the
`Array` names, and a mistake is an Aura diagnostic. The class holds one
field of the opaque host type `tensor.Node`, a reference-counted handle
that the runtime frees when the last tensor naming it goes. Members that
name the dtype in their signature, `from_array`, `to_array`, `item`,
`full`, `zeros` and `scalar`, come from one impl of the trait
`Elements[A, E]` for each dtype, because a type parameter cannot be the
element type of an `Array`.

### One meaning on every device

The Array kernels define every operation. The CPU device runs them. A
GPU result is checked against them.

- Shapes and dtypes are checked on the host at the call, with the Array
  rules and the Array messages.
- Integer arithmetic wraps on every device, because a GPU cannot trap
  and a result must not depend on the device.
- A GPU has no `float64`, and a tensor never narrows one. `float64` on a
  GPU device is `AU4007`.
- An operation the GPU library lacks, or gives a different meaning, runs
  through the Array kernels in host memory and gives a tensor on the same
  device. These are the checked ones: integer reductions and `abs`, a
  float to integer conversion, integer `matmul`, and `matmul` with a
  rank-one operand. So every operation exists on every device, and the
  host never asks MPSGraph for something that would abort.

### A GPU runs an expression as one plan

On a GPU device an operation the library has gives a waiting node: a
dtype, a shape, the operation, and its operand nodes. Nothing runs.

Reading a tensor (`to_array`, `item`, `to`, `eval`) collects the waiting
nodes behind it into a plan. A plan is a list of slots, each a leaf or a
step over earlier slots, and names no memory. Two runs of one expression
over tensors of the same shapes therefore have equal plans. The Metal
device compiles a plan to one MPSGraph, keeps it in a table keyed by the
plan, and runs it with each leaf reading its buffer and the result
written into a new buffer.

A chain of more than 128 waiting nodes runs when it is made.

The CPU device runs each operation when it is called. A test switch
makes it wait instead, so the plan code runs in CI on every platform.

### Memory a kernel can share

A tensor on the Metal device is an `MTLBuffer` in shared storage that
Aura allocates and owns. MPSGraph is handed the buffer and never
allocates a result itself. The later kernel layer will bind the same
buffer to a compute kernel. A kernel launch will first run the plan its
operands wait on, which is the same step as a read.

### Loading

The Objective-C side is one file, compiled by the runtime's build script
on macOS. It names no framework class at link time. Metal and
MetalPerformanceShadersGraph are opened with `dlopen` the first time a
program asks for the device, so a program that does not use tensors
links and loads neither. The only link-time addition is the Objective-C
runtime, which every macOS process already holds.

## Consequences

- A program written against `Tensor` runs unchanged on a machine with no
  GPU.
- A GPU result can differ from the CPU in the last places of a `sum`,
  `mean`, `matmul`, `exp`, `log`, `sin`, `cos` or `pow`. Everything else
  is equal.
- The hosted CI machines have no usable Metal device. The Metal tests
  report that they were skipped there and run on a developer Mac. The
  plan code is covered everywhere through the CPU device.
- An intermediate tensor that the program keeps and reads after reading
  a result made from it is computed twice. `eval()` on it first avoids
  that.
- Small work is slower on a GPU than on the CPU because of the fixed
  cost of a run.

## Not decided here

- The kernel layer: its syntax, and compiling Aura to Metal and CUDA.
- A CUDA device. cuBLAS and cuDNN have no general elementwise
  operations, so a complete CUDA device needs generated kernels and
  comes after the kernel layer. It also needs NVIDIA hardware to test.
- A Windows device.
- Neural network operations: convolution, pooling, normalization.
- `tensor * 2.0`. Two impls of one operator trait for one type are not
  told apart at run time yet.
