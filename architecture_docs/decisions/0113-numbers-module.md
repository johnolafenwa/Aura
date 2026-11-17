# ADR-0113. Numeric Library Types

## Context

A fixed-width integer cannot hold every factorial or sequence term. Binary
floating point cannot represent many decimal prices exactly. Numerical code
also needs a complex value which carries its components through ordinary
arithmetic and function calls.

## Decision

Expose `BigInt`, `Decimal`, and `Complex` through the `numbers` module.
Use explicit conversions. Integer literals retain their usual width and
binary arithmetic requires operands of the same numeric type.

A `BigInt` stores an `int64` or a list of unsigned 64-bit magnitude words.
A `Decimal` stores a signed coefficient and decimal exponent. Strip
trailing decimal zeros after each operation and give zero exponent zero.
A scalar coefficient and exponent fit in a small value. A larger
coefficient shares the integer magnitude representation. The list records
its sign and exponent so a returned magnitude becomes the result's
storage. These fields are private. Arithmetic results are independent and
may move between tasks under the usual ownership rules.

Native locals and calls carry the storage tag and active payload directly.
Only the large member owns a list reference. A number stored inside
another language union uses the ordinary nominal class at that boundary.
Both execution engines use the same arbitrary-precision kernels. Native
code directly calls the borrowed-limb addition entry and the scalar
modular exponentiation entry.

Addition, subtraction, and multiplication of decimals preserve the exact
result. Precision and rounding belong to individual operations. Division
through the operator uses 28 significant digits and round-to-nearest with
ties to even. An explicit division takes both the requested precision and
rounding mode. Complex values use two binary64 components and the ordinary
Copy class layout.

## Consequences

A workload can pay for larger coefficients only when they are present.
Generic storage and conversion into containers can still require boxing.
The six arithmetic workloads under `benchmarks/numbers` compare native
Aura with Rust and Python and check each result.

[Numbers Module](../../docs/manual/numbers.md)
