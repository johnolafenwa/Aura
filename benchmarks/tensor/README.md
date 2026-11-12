# Tensor operations on the CPU and GPU devices

The `tensor` module of ADR-0097 beside the same work in NumPy and
PyTorch: elementwise arithmetic, a reduction, elementwise maths, and
`matmul`, over `float32`.

Each row runs at two sizes. 1,024 elements show the fixed cost of one
piece of work on a device. 16,777,216 elements show the speed of the
arithmetic.

- **Aura** (`ops.au`) runs twice: on `Device.cpu()` and, with the
  argument `gpu`, on `Device.default()`. Every pass reads its result with `eval()` or `item()`,
  so the GPU does the work inside the timed loop.
- **NumPy and PyTorch** (`ops.py`) run the same expressions. The PyTorch
  MPS side calls `torch.mps.synchronize()` after each pass.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/tensor/run.sh
```

## Results

### Apple M2 Pro, Metal

Apple M2 Pro with 16 GB, macOS 26.5, 2026-11-12, with other work on the
machine (load average about 4). Milliseconds for all the repetitions of a
row. The Aura columns are from the second of two runs, which agreed to a
few percent.

| Row | Elements | Repetitions | Aura, CPU device | Aura, Metal device | NumPy | PyTorch, CPU | PyTorch, MPS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `x + y` | 1,024 | 2,000 | 2 | 294 | 1 | 2 | 297 |
| `(x * y + x).sqrt()` | 1,024 | 2,000 | 7 | 299 | 3 | 6 | 293 |
| `(x * y).sum()` | 1,024 | 2,000 | 3 | 334 | 3 | 5 | 449 |
| `(x.sin() * y.cos()).abs().maximum(y.log())` | 1,024 | 2,000 | 29 | 301 | 20 | 24 | 383 |
| `x + y` | 16,777,216 | 20 | 50 | 32 | 118 | 35 | 29 |
| `(x * y + x).sqrt()` | 16,777,216 | 20 | 390 | 31 | 332 | 82 | 60 |
| `(x * y).sum()` | 16,777,216 | 20 | 67 | 21 | 197 | 46 | 32 |
| `(x.sin() * y.cos()).abs().maximum(y.log())` | 16,777,216 | 20 | 2,976 | 35 | 2,706 | 1,059 | 108 |
| `matmul` then `max` | 64 by 64 | 500 | 7 | 92 | 2 | 2 | 123 |
| `matmul` then `max` | 1,024 by 1,024 | 10 | 135 | 10 | 12 | 11 | 13 |

MLX was not installed on the machine.

What the numbers say:

- On large tensors the Metal device is level with PyTorch on MPS for one
  addition and ahead of it for every longer expression, up to three
  times for the row of maths functions. The whole expression runs as one
  piece of GPU work, and results are written into buffers that are
  reused.
- One piece of work on the Metal device costs about 0.15 ms whatever its
  size, the same as PyTorch on MPS. Below a few tens of thousands of
  elements the CPU device is faster.
- The CPU device is the reference for what an operation means, and it is
  not tuned. It runs one operation at a time on one thread. It is level
  with NumPy on elementwise work and behind PyTorch, which uses several
  threads. Its `float32` `matmul` is about ten times slower than the
  Accelerate library that NumPy and PyTorch call.
- Float sums differ in the last digits between sides because each adds in
  a different order. The 1,024-element `sum` row differs more between
  Aura and Python only because the Aura program keeps its running total
  in `float32`.
