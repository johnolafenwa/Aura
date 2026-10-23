# ADR-0065: Collection reduction builtins

- Status: Accepted
- Date: 2026-10-23
- Builds on: ADR-0041, ADR-0044, ADR-0048, and ADR-0063

## Context

Aura had `Array.sum()`, `Array.min()`, and `Array.max()`, but no way to reduce
a `list[T]` or `set[T]` without a hand-written loop. `min` and `max` took
exactly two numbers. ADR-0063 asks for each missing Python builtin to be
specified on its own before it is added. This record is that specification
for `sum`, `any`, `all`, and the collection and variadic forms of `min` and
`max`.

## Decision

### Surface

| Call | Argument | Result |
| --- | --- | --- |
| `sum(values)` | `list[T]`, `set[T]`, `Span[T]`, or `Array[T]` with numeric `T`, or a `Range` | `T`; `int64` for a `Range` |
| `min(values)`, `max(values)` | the same as `sum` | `T`; `int64` for a `Range` |
| `min(a, b, ...)`, `max(a, b, ...)` | two or more numbers of one exact type | that type |
| `any(values)`, `all(values)` | `list[bool]`, `set[bool]`, or `Span[bool]` | `bool` |

The collection parameter is named `values`. The two-number forms keep their
`left` and `right` names. Three or more numbers are positional only.

### Semantics

- `sum` adds left to right in iteration order with the checked `+` of `T`. An
  empty collection gives zero. An overflow stops the program with `AU4002`.
- `min` and `max` fold the two-number builtin left to right, so ties and
  NaN behave as `min(a, b)` and `max(a, b)` do. An empty list, set, span, or
  `Range` stops the program with `AU4001`. An empty Array keeps its `AU4007`.
- `any` stops at the first `true` element and `all` at the first `false`
  one. An empty collection gives `false` for `any` and `true` for `all`.
- The collection is read through shared access and stays usable. Set order
  is the set's iteration order.
- An `Array` argument calls the Array's own `sum()`, `min()`, or `max()`, so
  it keeps the Array's reduction order and diagnostics.

### Lowering

MIR lowers a list, set, span, or `Range` reduction to the same loop a `for`
statement over that value produces, folding each element into the result.
Both backends run that MIR, so the reduction needs no new runtime entry
point and reads unboxed list elements on the native path.

### Names

`sum`, `any`, and `all` become builtin function names. A top-level function
with one of those names is rejected with `AU2007`, as for every builtin
function. A local binding still shadows a builtin function name: calling a
local function value named `sum` or `min` calls that value.

## Consequences

- Programs with a top-level `def sum`, `def any`, or `def all` must rename
  it. Methods with those names are unaffected.
- `min(x)` with one number is now a type error that names the collection
  form rather than a missing-argument error.
- Generic iterables, key functions, `start` values, and `min`/`max` over
  `str` are not part of this decision.
