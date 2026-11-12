# GPU tensors: the `tensor` module and its devices

Date: 2026-11-12

## Goal

Tensors that compute on a GPU, checked against PyTorch on real machine
learning work, for training and inference: automatic differentiation,
what transformers and mixture-of-experts models need, convolution,
mixed precision, optimisers, and weights on disk. The first step is a
layer over the vendors' libraries. A kernel layer in Aura syntax comes
later, on the same tensors (ADR-0097).

## Work completed

- The `tensor` module: `Tensor[T]` and `Device`, with elementwise
  arithmetic and maths, comparisons, `where`, reductions, `matmul`,
  shape operations, casts, and moves between devices.
- Three devices. The CPU device runs the Array kernels and defines what
  each operation means. The Metal device builds one MPSGraph for each
  distinct expression. The CUDA device writes each expression as CUDA C,
  compiles it with NVRTC, keeps the compiled kernel in memory and on
  disk, and calls cuBLAS for `matmul`.
- Operators take a number on either side: `x * 2.0`, `1 - x`.
- A program carries the Aura-source part of a builtin module only when
  it imports that module. Before, a program that imported `json` carried
  the function names and frame descriptors of every such module, about
  27 KB for `tensor` alone.
- The type checker lets a generic implementation such as
  `impl[T] Scale[T] for float64` satisfy the bound `Scale[int32]`.

## Verification

- Apple M2 Pro, Metal: every tensor test passes, with 50 expressions
  compared with the Array kernels on the device.
- NVIDIA A40, CUDA 13.2: every tensor test passes, with 56 expressions
  compared on the device.
- `benchmarks/tensor/README.md` has the times beside PyTorch on both
  machines. On large tensors both devices are level with PyTorch for one
  addition and ahead of it for longer expressions.
- Hosted CI has no usable GPU. The plan path runs there on the CPU
  device, and the generated CUDA source runs there compiled as C++.

## Second pass: gradients and layers

- Indexed operations, the functions `tanh`, `sigmoid` and `erf`, random
  tensors made on the device, automatic differentiation, variables,
  plans with several outputs, safetensors files, and the transformer
  layers with SGD and AdamW (ADR-0098).
- Verification: 41 gradient rules agree with central differences, run at
  once and as plans. The generated CUDA source for every new operation
  agrees with the CPU kernels when compiled as C++. A one-block
  transformer written in Aura trains to the right answers on the CPU
  device, interpreted and as a native build. Not yet run on the A40 or
  on the Mac.

## Follow-up

- Run the new kernels on the A40. Give the Metal shim the new
  operations.
- Batched `matmul` as one cuBLAS call. It is one call for each matrix
  pair now, which is slow for attention heads.
- Top-k.
- Convolution, pooling, batch norm.
- `float16`.
- Whole programs beside PyTorch: MLP training, large-batch inference,
  k-means, conjugate gradient, Monte Carlo pricing, GPT-2 inference,
  nanoGPT training, a mixture-of-experts transformer, ResNet-18 and a
  CIFAR-10 CNN.
- Compiler gaps found on the way. Two impls of one trait for one type
  that differ only in the trait's arguments type-check, but compiled
  code does not tell them apart. A method of an imported class cannot
  bound its own type parameter by a trait the importing module did not
  import. A method of a generic class that names another generic class
  of its module fails to check in a program that does not import that
  other class.

## PyTorch names and the `mut` rule (2026-11-12, evening)

- The owner asked for the machine learning side to read like PyTorch
  without losing the core of Aura. The module now uses PyTorch's names,
  with `loss.backward()`, `grad`, `zero_grad()` and `step()`.
- Rule kept by the compiler: the weights of a model not declared `mut`
  never change. Nine ways around it are tested and rejected.
- A40 run of commit 767ffa0: all 33 tensor tests pass. The workload
  programs show Aura CUDA 5 times faster than PyTorch CUDA on Monte
  Carlo, level on inference, and 8 to 180 times slower on conjugate
  gradient, transformer training, MLP training and k-means. The slow
  rows are open work.
