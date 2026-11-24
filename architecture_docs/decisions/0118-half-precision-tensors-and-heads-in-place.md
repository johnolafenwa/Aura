# ADR-0118 Half-precision tensors and attention heads read in place

Status: Accepted.

## Context

The tensor module of ADR-0097 had `float32` and `float64` float tensors.
Trained transformers are commonly run in half precision, which halves
their memory and is faster on GPUs with half-precision hardware. The
`gpt_infer` workload was also about two percent behind PyTorch on an
NVIDIA A40, and the measured difference was three copies in each layer
that put the heads of the queries, keys and values beside each other.

## Decision

### `float16` is a tensor dtype only

`tensor.float16` is a class with no members. It names the element type
of a `Tensor[float16]`. Aura has no `float16` number and no
`Array[float16]`: the `Elements` implementation of `Tensor[float16]`
takes and gives `float32` numbers and arrays. No compiler change and no
change to `Array` is needed.

In the runtime a plan slot's dtype is a code. Code 5 is `float16`. A
`float16` tensor in host memory is a `float32` `Array` whose elements
are all values a half can hold, with a flag on the node. Every host
result of a `float16` operation is rounded to the nearest half, ties to
even. A `float32` holds 24 significant bits and a half 11, so rounding
the `float32` result of `+`, `-`, `*`, `/` or `sqrt` gives the IEEE half
result exactly.

An operation whose float operands are not all `float16` or all another
dtype is an error. There is no silent widening.

### The CUDA device stores halves

The generated kernel source declares `aura_h`, sixteen bits that convert
to and from `float` implicitly. A kernel reads halves, computes each
step in `float`, and rounds each step by storing it to an `aura_h`
value, which keeps results equal to the host's. Reductions and lane
kernels accumulate in `float` and round once when they store. The
conversions are the PTX `cvt` instructions. A software conversion is
used when the kernels are compiled for the CPU in the tests, and on the
GPU when `AURA_TENSOR_SOFT_HALF=1` is set.

Matrix products of halves call `cublasHgemm`, `cublasHgemmBatched` and
`cublasHgemmStridedBatched`. `index_add` on halves runs in host memory.

The Metal device keeps `float16` tensors in host memory until its
kernels learn the dtype.

### Products read heads in place

An attention splits `[batch, length, heads * d]` rows with a reshape to
`[batch, length, heads, d]` and a permute to `[batch, heads, length,
d]`. When every reader of that permute is a matrix product, directly or
through a transpose of the last two axes, the generator does not write
it. The product action carries the row stride of each such operand, the
width of the rows, and the offsets of each head, and the device calls
the pointer-table batched product with that stride as the leading
dimension. Operands that are being built are started early at the rows
under the split.

`AURA_TENSOR_HEADS_IN_PLACE=0` turns this off.

The softmax of the scores, with its scale and mask, was already one
kernel. A single kernel for the whole attention was not written: the
two products are cuBLAS calls at PyTorch's speed, and a hand-written
product would have to match them.

## Consequences

- A trained transformer runs in half precision through the generic
  functions `linear`, `layer_norm` and `scaled_dot_product_attention`.
  The layers and optimizers stay `float32`, so training in half
  precision and mixed precision training are not covered.
- `save` writes `F16` and `load_float16` reads any float width.
- A `float16` tensor in host memory takes four bytes an element. Only
  device memory is halved.
- The pointer-table product needs the table written by a small kernel
  before each call, as the spread products of merged heads already do.
- Heads that anything other than a product reads are still copied.
