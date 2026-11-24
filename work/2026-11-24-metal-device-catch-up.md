# Metal device catch-up

Date: 2026-11-24

## Goal

The Metal device ran the newer tensor operations on the CPU and ran one
tensor's work at a time. Bring it level with the CUDA device: every
operation CUDA runs on the GPU runs on the GPU on Metal, and tensors
evaluated together run as one MPSGraph. Then time the workload programs
of `benchmarks/tensor` on an Apple M2 Pro.

## Work Completed

- `metal_shim.m` builds MPSGraph steps for `tanh`, `sigmoid`, `erf`,
  slices, the reverse of a slice, `expand`, `cat`, `index_select`,
  `index_add`, `argmax`, `argmin`, and random tensors.
- A slice reads its start from a tensor, as on CUDA, so slices that
  differ only in their start share one compiled graph. It is a gather
  over places computed in the graph.
- `index_select` reads zero for an index outside the axis, as on CUDA.
  `index_add` adds nothing for one.
- `argmax` and `argmin` are built from a reduction and a first-match
  search, because the library's own arg reductions do not promise
  Aura's rules: the first of equal elements wins, and a NaN wins.
- Random tensors compute the CPU kernel's 64-bit hash in `int64`
  arithmetic with masked shifts. The first use makes 96 floats for each
  of three seeds and compares them bit for bit with the CPU kernel. A
  system that differs, or lacks the bitwise operations of macOS 13,
  keeps random tensors on the CPU kernel.
- A plan with several outputs runs as one graph. The Metal device no
  longer evaluates each tensor of an optimizer step on its own, which
  computed the forward and backward pass once for each weight.
- A variable assigned in the middle of a step waits for the next run on
  Metal too.
- `AURA_TENSOR_TRACE=1` prints one line for each Metal run with its
  steps, leaves, outputs, and times.

## Verification

- `cargo test -p aura-runtime --lib tensor` passes on Linux.
- `cargo clippy -p aura-runtime --all-targets -- -D warnings` passes.
- On an Apple M2 Pro with macOS 26.5.2, `cargo test -p aura-runtime
  --lib tensor` passes with every GPU test run on the Metal device, and
  the eight small workloads of `benchmarks/tensor/workloads.au` give the
  CPU device's results.
- One defect was found there: MPSGraph keeps an `int64` constant as a
  double, which rounded the four multipliers of the random hash. A word
  wider than 53 bits is fed from a buffer on each run.
- With `AURA_TENSOR_TRACE=1`, the workloads show two kinds of work still
  in host memory on Metal: the sum of an `int64` tensor, which is
  checked for overflow, and a cast from `float32` to `int64`, which is
  checked for range. Both are by design.

## Follow-up

- Record the Mac timings in `benchmarks/tensor/README.md`. A first run
  was taken on battery in Low Power Mode and is not recorded.
- That run shows `cnn_train` and `monte_carlo` well behind PyTorch on
  MPS. A training step of `cnn_train` is one graph of 812 steps that
  runs in about 45 ms, and its convolution is built from an index
  select. `monte_carlo` spends its time in the 64-bit hash of the normal
  floats.
