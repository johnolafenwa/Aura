# Iterator bound and float64 gaps

Date: 2026-11-10. Asked for by John: fix the three gaps the evaluation of
the work since 0.13.0-preview left open.

## Goal

1. `list[T]` implements `IntoIterator[T]`.
2. Enum and supertrait bounds can name a trait with associated types.
3. A false float64 claim in `aura verify` fails fast instead of running to
   the time limit.

## Work completed

### `list[T]` as `IntoIterator[T]`

- `std/iter.au` has `ListIter[T]` and `impl[T] IntoIterator[T] for list[T]`.
  `into_iter` reverses the list once and `next` pops, so every item moves
  out without a copy.
- Loops written directly over a list, set, `Span`, `Range`, or `Queue`
  keep the builtin loop. Before, `enumerate`, `zip`, and comprehensions
  switched to `into_iter()` when the type had an implementation, which
  consumed the collection.
- A second `impl IntoIterator[...] for list[...]` is a duplicate-impl
  error.

Making a list satisfy the bound exposed two older bugs, both fixed:

- A generic implementation or bound from another module rejected the
  caller's own classes. `trait_bound_identity` keyed the bound's arguments
  as types of the implementation's module, so `Generator[Box]` read as
  `Iterator[iter.Box]`. The arguments are now keyed in the checking
  module. An imported trait or implementation has its own types qualified
  already.
- A call through a module name, `util.keep(items)`, did not infer a type
  parameter that only a bound names. The local's type stayed `list[T]`
  and the build stopped with `AU4001`. The module-call path in `mir.rs`
  now runs the same bound inference as a bare call.

## Verification

- `cargo test -p aura-compiler --lib`: 1939 passed. The one failure,
  `package_io_helpers_cover_local_error_paths`, fails only when the tests
  run as root.
- `cargo test -p aura-compiler --test fixtures`.
- New fixtures: `run-pass/into_iterator_list.au`,
  `run-pass/into_iterator_list_imported.au`,
  `check-fail/into_iterator_list_duplicate.au`. Both run on the MIR and
  the native backend.

## Follow-up

- Items 2 and 3 of the goal.
- `set[T]`, `dict[K, V]`, and `Range` do not implement `IntoIterator`.
- A comprehension over an `IntoIterator` type parameter binds borrowed
  items, so `[x for x in source]` cannot move non-copy items out
  (`AU3002`). `source.into_iter().collect()` does.
