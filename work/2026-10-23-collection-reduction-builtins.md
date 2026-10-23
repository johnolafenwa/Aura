# Collection Reduction Builtins

## Goal

John asked why Aura has no `sum` for lists, then approved adding the
reduction builtins Python programmers reach for. ADR-0065 is the
specification.

## Design

- `sum`, `any`, and `all` are new builtin functions. `min` and `max` gain a
  one-argument collection form and a variadic form for three or more
  numbers.
- Accepted collections: `list[T]`, `set[T]`, `Span[T]`, and `Range`.
  `sum`, `min`, and `max` also accept `Array[T]` and call the Array's own
  reduction.
- MIR lowers a list, set, span, or Range reduction to the loop a `for`
  statement over the value produces (`lower_collection_reduction`). Both
  backends run that MIR, so there is no new runtime op or runtime helper.
  `sum` uses `Binary Add`, which the checks pass gives its overflow trap.
  `min` and `max` compare with `<=` or `>=` like the scalar helper. `any`
  and `all` leave the loop through the same path as `break`.
- An empty list, set, span, or Range for `min` or `max` ends in
  `AssertFail` with the message ``min(...)` of an empty collection``,
  which reports `AU4001` at the call.
- Variadic `min`/`max` evaluates every argument left to right, then folds
  the two-argument runtime call.
- The checker's builtin branch now skips names bound as locals, so a local
  function value named `sum` or `min` is called as that value. MIR already
  routed such calls through `CallTarget::Value`.

## Tests

- `run-pass/collection_reductions.au`: every form, including spans, sets,
  ranges, floats, `uint8`, Arrays, empty collections, and comprehensions.
- `run-pass/builtin_names_shadowed_by_locals.au`: local `sum` function
  value and local `min` integer.
- `run-fail/collection_min_empty.au`, `run-fail/collection_sum_overflow.au`.
- `check-fail/collection_sum_requires_numbers.au`,
  `check-fail/collection_any_requires_bool.au`,
  `check-fail/min_max_variadic_exact_type.au`,
  `check-fail/min_max_single_number.au`.
- Checked by hand that `aura run` and `aura build` print the same output
  for each run fixture.
