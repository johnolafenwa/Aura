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
- The Metal module type-checks on Linux with its platform gate widened.
  The shim passes `clang -fsyntax-only` against the macOS 14.5 MPSGraph
  headers with stub system headers.
- Pending: the GPU tests and the timings on the Mac.

## Follow-up

- Record the Mac timings in `benchmarks/tensor/README.md`.
