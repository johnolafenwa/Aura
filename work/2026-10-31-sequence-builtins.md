# 2026-10-31: Sequence builtins, list operators, and slice steps

John asked for the Python gaps left after ADR-0065: `sorted`, `reversed`,
`list(...)`, slice steps, `xs + ys`, and `xs * n`, before the 0.11.0
preview. ADR-0066 specifies them.

## What changed

- `sorted(values, *, key=..., reverse=false)`, `reversed(values)`, and
  `list(values)` over a list, span, set (not `reversed`), or `Range`. They
  return new lists and leave the argument usable. `sorted` reuses the
  `list.sort` lowering on a fresh copy, so keys are computed once before
  any element moves.
- `left + right` on two lists or spans of one element type, and
  `values * n` or `n * values` with an integer count. `items += more` on a
  list extends in place instead of rebuilding the list.
- `values[start:end:step]` on lists, spans, and `str`. The parser writes
  it as a call of a hidden builtin (`%slice`, not an identifier), so every
  analysis that walks call arguments sees the endpoints and the step. MIR
  lowers it to `ListOp::SliceStep` or `StrOp::SliceStep`, backed by
  `aura_direct_vec_slice_step` and `aura_direct_string_slice_step` and by
  the same shared runtime functions in the MIR interpreter.
- Positive steps keep the one-colon slice rules. Negative steps default to
  the last position and to before the first, normalize written endpoints
  once, and trap with `AU4003` when the start is outside the value or the
  end exceeds the start. A zero step traps with `AU4003`.

## Not included

- `sorted`, `reversed`, and `list` over `str` or a dictionary. A
  dictionary is not iterated directly, so `sorted(table.keys())` is the
  form to use. `str` has no character iteration yet.
- `items += view_of_a_slice`: the checker still asks for a `view` binding
  there. `items += values[1:]` works.
- `str * n`; `text.repeat(n)` already covers it.

## Tests

- `run-pass/sequence_builtins.au` covers every form on both backends,
  including spans, keys, Unicode text, empty lists, and `+=`.
- `check-fail` fixtures cover an unorderable element, `reversed` of a set,
  the keyword-only `key`, a stepped `Array` slice, a float repetition
  count, mismatched concatenation, and redefining `sorted`.
- `run-fail` fixtures cover a zero step and both negative-step bound
  errors.
- `examples/collections/new_lists.au` is a maintained example.
