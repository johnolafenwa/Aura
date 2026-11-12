# ADR-0090: `matmul` multiplies stacks of matrices

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0086 (Array products and reordered sums), ADR-0088
  (Array views), ADR-0089 (broadcasting)
- Amends: ADR-0086, which limits `matmul` to operands of rank 1 and 2

## Context

`matmul` accepted a matrix by a matrix, a matrix by a vector, and a
vector by a matrix. A program with a stack of matrices, such as one
weight matrix per layer or one transform per sample, wrote a loop over
the first axis, took a view of each matrix, called `matmul`, and
assigned the product into a result. Each turn of that loop paid for two
views, a result buffer, and a slice assignment.

## Decision

The last two dimensions of a `matmul` operand are its matrix. The
dimensions before them are its batch.

| Left | Right | Result |
| --- | --- | --- |
| `[.., n, k]` | `[.., k, m]` | `[.., n, m]` |
| `[.., n, k]` | `[k]` | `[.., n]` |
| `[k]` | `[.., k, m]` | `[.., m]` |

The batch dimensions of the two operands broadcast by the rule of
ADR-0089. They are aligned from the right, a missing dimension counts as
one, and a dimension of one stretches. Any other difference is `AU4007`.
The result has the broadcast batch dimensions followed by the matrix
dimensions of the product.

A rank-1 operand is always one vector. It has no batch dimensions, so one
vector serves every batch position. Two rank-1 operands are still
`AU4007`, because that product is `dot`.

Each batch position is one product of ADR-0086. It uses the same kernel
and adds in the same order, so a batched product gives the bits of the
loop it replaces. An integer product is checked. `AU4002` reports the
flat index of the failing element in the whole result.

An empty batch gives an empty result with the product's shape.

## Consequences

- `stack.matmul(matrix)` replaces a loop of views and slice assignments
  with one call and one result buffer.
- The static type is unchanged. Rank is not part of an Array type, so
  the shape check stays at run time.
- A view operand is read in its logical order, as for every other Array
  operation.
- The kernels are those of ADR-0086 and ADR-0092. A stack of small
  matrices runs on the calling thread, because each product decides its
  own thread count from its own size.

## Alternatives considered

- **A separate `batch_matmul` member.** It would keep `matmul` strict
  about rank. Every array library a reader may know uses one product
  for both, and the rank-2 rule is the batch rule with no batch.
- **Treating a rank-1 operand as a batch of scalars.** A vector on
  either side is one vector in the three existing forms. Keeping that
  meaning keeps every existing program's result.
- **Running batch positions on several threads.** It changes no bits,
  and is left to the kernel work, which already owns the thread policy.
