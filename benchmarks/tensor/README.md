# Tensor operations on the CPU and Metal devices

The `tensor` module of ADR-0097 beside the same work in NumPy and
PyTorch: elementwise arithmetic, a reduction, elementwise maths, and
`matmul`, over `float32`.

Each row runs at two sizes. 1,024 elements show the fixed cost of one
piece of work on a device. 16,777,216 elements show the speed of the
arithmetic.

- **Aura** (`ops.au`) runs twice: on `Device.cpu()` and on
  `Device.metal()`. Every pass reads its result with `eval()` or `item()`,
  so the GPU does the work inside the timed loop.
- **NumPy and PyTorch** (`ops.py`) run the same expressions. The PyTorch
  MPS side calls `torch.mps.synchronize()` after each pass.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/tensor/run.sh
```

## Results

RESULTS_PLACEHOLDER
