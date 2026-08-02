# ADR-0043: Unified `int64` index domain

- Status: Accepted
- Date: 2026-08-02
- Roadmap decision: Batch S1, Aura 0.3 Python surface
- Builds on: ADR-0003, ADR-0029, ADR-0030, ADR-0039, ADR-0040, and ADR-0041

## Context

Collection lengths, enumeration positions, ranges, slices, and numeric Array
coordinates describe the same family of positions. A single domain lets values
flow directly between these operations, keeps large collections representable,
and gives MIR and direct execution one backend-independent contract.

Aura still requires exact static types in ordinary assignments, arguments, and
operators. Indexing needs one deliberately narrow exception so fixed-width
small integers remain convenient without creating general implicit numeric
conversion.

## Decision

### Canonical position type

The canonical type is `int64` for:

- direct list indices and indexed assignment
- list index-taking methods, including `get`, `set`, `swap`, `insert`, and
  indexed `pop`
- present slice start and end expressions for lists, strings, and Arrays
- every `range` bound and every value yielded by Range iteration
- the position produced by `enumerate`
- scalar and tuple Array coordinates
- every element of an Array coordinate container
- source-collection indices carried by `WaitAny`, `WaitAll`, and
  `SelectOutcome` variants
- collection lengths, capacity arguments, and positions returned by search
  operations

This makes the following ordinary code exact and cast-free:

```aura
last = values[len(values) - 1]

for index in range(len(values)):
    print(values[index])

for index, value in enumerate(values):
    print(values[index])
```

`range` is half-open and yields `int64` values. Negative list indices, slice
endpoints, and Array coordinates keep their operation-specific normalization
rules; changing the integer domain does not change those rules.

### Scoped lossless widening

An index-domain expression may have one of these fixed-width source types:

```text
int8, int16, int32, int64, uint8, uint16, uint32
```

The compiler converts the six narrower types to `int64` because every
value in each complete declared domain is representable. `int64` is already
exact. The conversion is inserted only at the index-domain position and is
explicit in MIR.

`intsize` and `uintsize` do not receive implicit widening. Their domains vary
by target, so accepting them would make otherwise identical source type-check
differently across targets. Wider fixed-width integers, floating-point values,
booleans, and all non-integer types are rejected with `AU2002`.

This rule does not apply to ordinary binding initialization, assignment,
return values, operators, generic inference, collection elements, or user
function parameters. Those positions continue to require exact types or an
explicit cast.

### Array coordinate containers

Array shapes and coordinate containers have the exact type `list[int64]`.
Scalar and tuple coordinate components use the scoped widening rule above.
A `list[int32]` is not converted to `list[int64]`: container conversion would
be a different language feature and could allocate or duplicate elements.

### Runtime and backend contract

MIR stores public positions and collection traversal counters as `int64`.
Losslessly narrower source values cross an explicit MIR cast before the
operation. The direct backend uses its `int64` scalar lane for range, index,
slice, iteration, and Array-coordinate paths. Both backends normalize and
validate positions with wide intermediates before converting a validated
offset to a host allocation index.

An invalid runtime position remains a typed runtime failure under the
operation's existing diagnostic code. Values outside the static `int64`
domain are rejected before execution.

## Consequences

- Lengths, ranges, enumeration positions, and indices compose without casts.
- The largest statically representable position is independent of `int32`.
- Small fixed-width integer variables remain usable at position sites without
  enabling implicit numeric conversion elsewhere.
- Pointer-sized source types have target-stable rejection.
- Array coordinate storage and both execution backends share one signed
  64-bit contract.
- Practical collection and Array size remains bounded by address space,
  allocation limits, element size, and available memory.

## Verification

The maintained test surface pins:

- all three cast-free examples above under MIR and direct execution
- fixed-width lossless widening at indices, slices, ranges, swaps, and Array
  scalar or tuple coordinates
- exact `list[int64]` Array coordinate containers
- exact `int64` source positions in `WaitAny`, `WaitAll`, and `SelectOutcome`
  payloads
- rejection of pointer-sized, wider, unsigned-64, floating-point, boolean,
  and non-integer positions
- rejection of implicit `int32` to `int64` conversion outside index-domain
  positions
- `i64::MIN` and `i64::MAX` normalization and failure behavior
- byte-equivalent MIR/direct observable behavior

The V6 direct integer-loop benchmark is rerun at the completed migration
checkpoint, with whole-process and startup-split results recorded against the
maintained baseline.
