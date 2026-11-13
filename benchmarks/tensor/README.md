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
  MPS and CUDA sides wait for the device after each pass.

Each row prints `<name> <size> <milliseconds> <checksum> <first pass
milliseconds>`. Every row runs once before its timed passes. That first
pass carries what an expression costs once, such as compiling it for the
device, and its time is the last column.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/tensor/run.sh
```

## Workloads

`workloads.au` and `workloads.py` run eight whole programs in Aura and in
PyTorch: training a two layer network with AdamW, labelling a large
batch with a three layer network, k-means, conjugate gradient on a dense
system, training a small transformer of the nanoGPT shape, pricing an
option by Monte Carlo, training a small residual network of
convolutions and batch norms, and naming the next token with a model of
the GPT-2 shape and random weights. Each prints a result that shows the work was done,
such as the final loss. The two sides draw different random numbers, so
a result is close to the other side's, not equal to it.

```sh
bash benchmarks/tensor/run_workloads.sh          # the GPU sides
bash benchmarks/tensor/run_workloads.sh small    # sizes that finish in seconds
CPU=1 bash benchmarks/tensor/run_workloads.sh    # the CPU sides as well
```

## Results

### Whole Programs, NVIDIA A40

NVIDIA A40, CUDA 13.2, PyTorch 2.8, 2026-11-13. Milliseconds for the
timed passes of a row, the range of three runs. Other work held the
pod's processors during these runs (load average about 20), which
slows the rows that are bound by host time, PyTorch's `mlp_train` most
of all.

| Program | Aura, CUDA device | PyTorch, CUDA |
| --- | ---: | ---: |
| `mlp_train`, 500 steps | 262 to 271 | 1,162 to 1,584 |
| `mlp_infer`, 65,536 rows | 741 to 748 | 901 to 904 |
| `kmeans`, 200,000 rows | 39 | 68 |
| `conjugate_gradient`, 4,096 | 20 to 21 | 17 to 23 |
| `transformer_train`, 4 layers of 256 | 339 to 365 | 320 to 347 |
| `monte_carlo`, 10,000,000 paths | 3 to 4 | 21 to 22 |
| `cnn_train`, 50 steps | 346 to 372 | 297 to 350 |
| `gpt_infer`, 12 layers of 768 | 344 to 378 | 269 to 271 |

`gpt_infer` is the row behind. A profile of both sides shows the matrix
products level, at about 21 ms of a step each. PyTorch's other kernels
take 2.6 ms of a step and Aura's 10 ms: PyTorch adds a bias inside the
product, and runs attention and layer norm as one kernel each, where
Aura runs 306 kernels a step. The loss of `cnn_train` differs in its
fourth digit between runs on both sides: gradients that add into one
element add in an order the GPU does not fix.

### Single Expressions

### NVIDIA A40, CUDA

NVIDIA A40 with 46 GB on a rented cloud machine, driver 595.91, CUDA
13.2, PyTorch 2.8.0, 2026-11-12. Milliseconds for all the timed
repetitions of a row, then the first pass alone. The Aura column is the
third of three runs and the PyTorch column the second of two; the runs
of each side agreed to a few milliseconds. The machine's CPUs were
shared and busy (load average 13 to 18 on 96 cores), so the CPU sides
are not recorded here.

| Row | Elements | Repetitions | Aura, CUDA device | PyTorch, CUDA | Aura first pass | Aura first pass, empty kernel cache | PyTorch first pass |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `x + y` | 1,024 | 2,000 | 21 | 41 | 0 | 13 | 9 |
| `(x * y + x).sqrt()` | 1,024 | 2,000 | 33 | 69 | 0 | 15 | 17 |
| `(x * y).sum()` | 1,024 | 2,000 | 49 | 60 | 0 | 54 | 0 |
| `(x.sin() * y.cos()).abs().maximum(y.log())` | 1,024 | 2,000 | 34 | 128 | 0 | 15 | 34 |
| `x + y` | 16,777,216 | 20 | 7 | 7 | 3 | 15 | 1 |
| `(x * y + x).sqrt()` | 16,777,216 | 20 | 8 | 19 | 0 | 23 | 1 |
| `(x * y).sum()` | 16,777,216 | 20 | 6 | 10 | 1 | 94 | 0 |
| `(x.sin() * y.cos()).abs().maximum(y.log())` | 16,777,216 | 20 | 8 | 34 | 0 | 17 | 1 |
| `matmul` then `max` | 64 by 64 | 500 | 26 | 18 | 55 | 119 | 73 |
| `matmul` then `max` | 1,024 by 1,024 | 10 | 2 | 1 | 1 | 125 | 0 |

What the numbers say:

- On large tensors the CUDA device is level with PyTorch for one
  addition and two to four times faster for longer expressions. Aura
  compiles a whole expression into one kernel that passes over the data
  once, where PyTorch runs one kernel and writes one temporary tensor
  for each operation.
- One small piece of work costs 10 to 25 microseconds, about half of
  PyTorch's. The small `matmul` row is the exception: it is a cuBLAS
  call and two reduction kernels, and it is 1.4 times slower than
  PyTorch.
- The first evaluation of an expression compiles its kernel, which takes
  13 to 23 ms for an elementwise expression and more for a whole
  reduction, which has several kernels. The compiled kernels are kept on
  disk, so only the first run of a program pays this. On later runs the
  first pass costs what a timed pass costs. The 55 ms on the first
  `matmul` is cuBLAS starting, which PyTorch pays too.

### Apple M2 Pro, Metal

These times were taken before the first pass was timed on its own, so
each figure includes its row's first pass.

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

## Tracing a Run

`AURA_TENSOR_TRACE=2` makes a program on the CUDA device write one line
to standard error for each piece of GPU work: the size of its plan,
whether its kernels were already compiled, the host time since the last
traced event, the time to launch its actions, and the time waiting for
them. It also writes a line for each read into host memory and for each
operation that ran in host memory. `AURA_TENSOR_TRACE=1` adds the
slowest actions of each piece of work, and waits for the device after
every action to time it, so that run is slower. `AURA_TENSOR_TRACE=all`
lists every action in place of the slowest.
