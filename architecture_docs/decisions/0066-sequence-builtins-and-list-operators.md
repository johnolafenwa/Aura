# ADR-0066: Sequence builtins, list operators, and slice steps

- Status: Accepted
- Date: 2026-10-31
- Builds on: ADR-0040, ADR-0061, ADR-0063, and ADR-0065

## Context

After ADR-0065, the Python gaps users met most often in list code were
`sorted`, `reversed`, `list(...)`, `xs + ys`, `xs * n`, and slice steps such
as `xs[::2]` and `text[::-1]`. ADR-0040 reserved the second slice colon, and
ADR-0063 asks for each missing builtin to be specified before it is added.
This record specifies all six. John asked for them on 2026-10-31, ahead of
the 0.11.0 preview.

## Decision

### Surface

| Form | Operands | Result |
| --- | --- | --- |
| `sorted(values, *, key=..., reverse=false)` | `list[T]`, `Span[T]`, `set[T]`, or `Range` | `list[T]` in stable sorted order |
| `reversed(values)` | `list[T]`, `Span[T]`, or `Range` | `list[T]`, last element first |
| `list(values)` | `list[T]`, `Span[T]`, `set[T]`, or `Range` | `list[T]` in iteration order |
| `left + right` | two `list[T]` or `Span[T]` values of one `T` | `list[T]` |
| `values * n`, `n * values` | a `list[T]` or `Span[T]` and an integer `n` | `list[T]` |
| `values[start:end:step]` | `list[T]`, `Span[T]`, or `str` | `list[T]`, or `str` |

A `Range` has `int64` elements. `key` and `reverse` are keyword-only, as in
Python. The repetition count and the step use the `int64` index domain, so a
losslessly narrower integer widens there.

### Semantics

- Every form returns a fresh owned value and reads its operands through
  shared access, so they stay usable. Elements are copied as a one-colon
  slice copies them: Copy elements bitwise, clone-safe elements by clone. An
  element type that cannot be duplicated, such as a resource or a
  non-cloneable closure, is rejected with `AU3007`.
- `sorted` orders exactly as `list.sort` does, with the same `key` and
  `reverse` rules and the same `AU2002` for an unorderable type.
- `values * n` with `n <= 0` is an empty list. Operands evaluate left to
  right.
- `items += more` on a list extends `items` in place with copies of the
  elements of `more`. It is not `items = items + more`, which would copy
  `items` on every step of a loop.
- A stepped slice with a positive step takes the one-colon slice
  `values[start:end]`, with its defaults, normalization, and `AU4003`
  checks, and keeps every `step`-th position from its start.
- With a negative step, an omitted `start` is the last position and an
  omitted `end` lies before the first, so `values[::-1]` reverses. Written
  endpoints normalize once, as in a one-colon slice. The start must be a
  position in the value and the end must not exceed the start; otherwise
  the slice stops with `AU4003`. Equal endpoints give an empty result.
- A zero step stops with `AU4003`.
- A `str` step counts Unicode scalar values, as `str` slicing does.
- An empty step, as in `values[::]` or `values[1:3:]`, is a one-colon slice.

Endpoints are not clamped. This keeps the ADR-0040 rule that a slice either
selects what it names or stops, rather than Python's silent clamping.

### Lowering

- The parser writes a stepped slice as a call of a hidden builtin whose name
  is not an identifier, with omitted endpoints marked. The checker and MIR
  treat it like any other builtin call, so every analysis that walks call
  arguments sees the endpoints and the step.
- MIR lowers a stepped slice of a list or `str` to one runtime operation
  (`ListOp::SliceStep`, `StrOp::SliceStep`) that both backends call. A span
  is first copied to a list.
- `sorted`, `reversed`, `list`, `+`, `*`, and list `+=` lower to the
  operations a hand-written copy, `sort`, `reverse`, `extend`, or `for`
  loop lowers to, so they add no other runtime entry points.

### Names

`sorted`, `reversed`, and `list` become builtin function names. A top-level
function with one of those names is rejected with `AU2007`. A local binding
still shadows a builtin function name.

## Consequences

- Programs with a top-level `def sorted`, `def reversed`, or `def list` must
  rename it.
- `values[a:b:c]` no longer reports `AU2005`. Slice assignment still does.
- `sorted`, `reversed`, and `list` do not take a `str` or a dictionary. A
  dictionary is not iterated directly, so `sorted(table.keys())`
  sorts its keys. Character iteration of `str` is a separate decision.
- `reversed` returns a list, not a lazy iterator, because Aura has no
  general iterator protocol yet.
- `Array` keeps axis-zero one-colon slices only; a stepped Array slice is
  `AU2003`.
