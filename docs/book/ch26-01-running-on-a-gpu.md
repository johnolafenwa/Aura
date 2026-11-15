# Running on a GPU

## What you need

On a Mac you need nothing. `Device.metal()` uses the GPU built into the
machine, through libraries that are part of macOS.

On Linux with an NVIDIA card, you need NVIDIA's driver and two
libraries from the CUDA toolkit: NVRTC, which compiles code for the card,
and cuBLAS, which multiplies matrices. Aura looks for them where the
system loader looks, and under `CUDA_HOME`, `CUDA_PATH`, and
`/usr/local/cuda`. It works with CUDA 11, 12, or 13 and uses the newest
it finds. It is tested with CUDA 13. You do not need the toolkit to
install Aura or to build a program, only to run one on the card.

On Windows there is no GPU device yet. Programs run on the CPU device
there.

If something is missing, nothing breaks. `Device.cuda()` and
`Device.metal()` return `None`, and `Device.default()` returns the CPU
device. Listing 21-4 is the program to run when you want to know what
Aura found.

## The one line

We have written every program with the device in one binding at the top
of `main`, passed to everything that makes a tensor. This is where that
pays off. To run any program in these chapters on a GPU, change

```aura fragment
device = Device.cpu()
```

to

```aura fragment
device = Device.default()
```

and nothing else. Layers create their weights on the device they are
given. `load_float32` reads a file straight to the device. Operations on
tensors that live on a device run on that device and leave their results
there. The numbers come back to ordinary memory only when the program
asks for them, with `item()` or `to_array()`.

Two rules keep this simple. Both operands of an operation must be on the
same device, and mixing them is an `AU4007` error, not a silent copy.
When you do want to move a tensor, `x.to(device)` makes a copy on the
other device.

There is one restriction to know about on a Mac. Metal has no 64-bit
floats, so a `Tensor[float64]` cannot be made on the Metal device. We
have used `float32` throughout, which is the usual choice in machine
learning anyway.

The answers do not depend on the device. Integers and masks are
identical. Floats agree to within the last digit or two, because a GPU
adds a long list of numbers in a different order than a CPU does.

## A GPU waits

On the CPU device, `x * y` multiplies when that line runs. On a GPU
device it does not. The operation returns a tensor that knows its shape
and its dtype, and holds a note of how to compute itself. The work
happens when the program first needs actual numbers: at `item()`,
`to_array()`, `to(device)`, or an explicit `eval()`.

The reason is cost. Every trip to a GPU has a fixed price, from a few
tens of microseconds to a fifth of a millisecond depending on the GPU,
whatever the size of the work. If each operator made
its own trip, an expression like `(x * y + x).sqrt()` would pay three
times and write two temporary tensors. By waiting, Aura sees the whole
expression, compiles it into a single piece of GPU code, and passes over
the data once. A training step is the extreme case: the forward pass, the
gradients, and the optimizer's update all wait, and run together.

For your programs this has two consequences.

The first is that where you read matters. In Chapter 22 we said to read `loss.item()`
after `optimizer.step()`, and promised a reason. This is it. `step()`
runs the forward pass, the gradients, and the update as one piece of
work. A `loss.item()` placed before `step()` would force the forward pass
to run on its own first, and then `step()` would run it again as part of
the whole. The answer would be the same, and the loop slower.

The second is that each read is a trip. A loop that calls `item()` on every pass makes
the GPU stop and hand over a number on every pass. Reading the loss every
hundred steps, as our training loops do, is the habit to keep.

The first time a program evaluates a particular expression on a GPU, Aura
compiles it, which takes ten to twenty milliseconds on CUDA. It keeps the
result, in memory and on disk, so the second pass of a loop does not pay
it and neither does the next run of the program.

## Small work belongs on the CPU

A GPU is not faster at everything. The fixed price of a trip means that
small tensors are quicker on the CPU, where there is no trip to make.
Here is a program that times the same expression at two sizes on whatever
device `Device.default()` picks.

*Filename: src/main.au*

```aura
import sys
import tensor
from tensor import Device, Tensor

def timed(device: Device, size: int64, passes: int64) -> int64:
    x = tensor.rand([size], 1, device)
    y = tensor.rand([size], 2, device)
    warm = (x * y).sum().item()
    start = sys.monotonic_time_ms()
    mut total = 0.0
    for round in range(passes):
        total += (x * y).sum().item() as float64
    return sys.monotonic_time_ms() - start

def main():
    device = Device.default()
    print(f"device: {device.name()}")
    print(f"1,024 elements, 2,000 passes: {timed(device, 1024, 2000)} ms")
    print(f"16,777,216 elements, 20 passes: {timed(device, 16777216, 20)} ms")
```

<span class="caption">Listing 26-1: Timing one expression on a small and a large tensor</span>

On the machine we have used all along, which has no GPU, the default
device is the CPU. Your times will differ.

```console
$ aura run --backend direct src/main.au
device: cpu
1,024 elements, 2,000 passes: 9 ms
16,777,216 elements, 20 passes: 1041 ms
```

The first call in `timed`, before the clock starts, is a warm-up: it pays
for anything that happens once, such as compiling the expression for a
GPU.

The Aura repository records the same expression at the same sizes on two
GPUs, in `benchmarks/tensor`. On an NVIDIA A40, the 2,000 small passes
took 49 ms and the 20 large passes took 6 ms. On the Metal GPU of an
Apple M2 Pro they took 334 ms and 21 ms, against 3 ms and 67 ms for the
CPU device of the same Mac.

Read the two rows separately. On 16 million elements the GPUs are far
ahead: the A40 does in 6 ms what took our CPU about a second. On 1,024
elements the order is reversed. Each pass costs the GPU its trip, 25
microseconds on the A40 and about 170 on the Mac, and the CPU finishes
the whole pass in a fraction of that.

So the rule of thumb is about size. Below a few tens of thousands of
elements per operation, use the CPU device. The models of Chapters 22 and
23 are in that range, and they would run slower on a GPU. GPT-2, whose
every matrix product involves millions of numbers, is far past it.

## How fast is it?

The repository's benchmarks include eight whole programs, each written
once in Aura and once in PyTorch, run on the same NVIDIA A40. These are
times in milliseconds from the release these chapters were written for,
and the full tables with their conditions are in
`benchmarks/tensor/README.md`.

| Program | Aura, CUDA | PyTorch, CUDA |
| --- | ---: | ---: |
| Training a two-layer network, 500 steps | 251 to 259 | 838 to 1,567 |
| Labelling 65,536 rows with a three-layer network | 739 to 746 | 904 to 906 |
| k-means on 200,000 rows | 38 to 39 | 68 |
| Training a transformer, 4 blocks of width 256 | 318 to 334 | 323 to 387 |
| Training a small convolutional network, 50 steps | 319 to 339 | 290 to 343 |
| Next-token prediction, GPT-2's shape, 10 steps | 274 to 277 | 270 to 272 |

On a GPU, Aura is level with PyTorch on the transformer and
convolutional rows and ahead where a program is made of many small steps
or long elementwise expressions, which compile to one pass over the
data.

The last row is a model of the shape of Chapter 25's: twelve blocks of
width 768. Each step reads eight windows of 256 tokens and takes about 28
ms. Our CPU took most of a second for one window of a few dozen tokens.

The CPU device is a different story, and you have felt it in the last two
chapters. It is the reference for what every operation means. It runs one
operation at a time, and its matrix product is several times slower than
the tuned libraries PyTorch calls. It is the right place to learn, to
test, and to run small models. For training anything the size of Chapter
24's transformer or larger, use a GPU if you have one.
