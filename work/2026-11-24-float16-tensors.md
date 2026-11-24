# Half-precision tensors and attention heads read in place

ADR-0118 describes the design. The Tensor Module page of the Manual
shows the public API, under Half Precision.

## What changed

- `Tensor[float16]`, `to_float16()`, `load_float16`, and `save` of
  halves as `F16`. `tensor.linear`, `tensor.layer_norm` and
  `scaled_dot_product_attention` take float tensors of any dtype.
- The CUDA kernel generator knows a sixteen bit float type. Reductions
  and lane kernels add in `float`. Products of halves are cuBLAS half
  products.
- On CUDA a product reads the heads of an attention from the rows the
  heads were split from.
- `benchmarks/tensor/gpt2.au` and `gpt2.py` run GPT-2's real weights in
  `float32` and `float16`. `benchmarks/tensor/nanogpt.au` and
  `nanogpt.py` train a character model on Tiny Shakespeare.

## Verification

- Runtime tests: the conversion between a half and a `float32` over all
  65,536 halves, rounding at ties, overflow and subnormals; every
  planned expression with `float16` leaves, at once and as a plan; the
  generated kernels compiled for the CPU against the CPU device; saving
  and loading; gradients; the actions of an attention with heads read
  in place, and the cases where a split is still written.
- Compiler test: a program over `Tensor[float16]` from Aura source.
- GPT-2 in Aura and in PyTorch name the same tokens on the CPU, in
  `float32` and in `float16`.

## Not covered

- Metal keeps `float16` tensors in host memory.
- Layers and optimizers are `float32`. There is no mixed precision
  training.
- No single kernel for a whole attention.
