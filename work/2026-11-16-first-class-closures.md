# First-Class Closures

## Goal

John approved item 3 of the missing features list on 2026-11-16: closures
are second-class. A capturing closure could be stored only through an
alias of a `Callable[...]` type called as a constructor, and a lambda is
one expression with no statement body. The design is in
`architecture_docs/29-first-class-closures-design-checkpoint.md`.

## Step 1: Packing At Typed Destinations (ADR-0104)

Work completed:

- The checker types a value written where a `Callable[...]` or
  `TaskCallable[...]` type is expected under the storage contract, and
  packs a function value, a lambda, a bound method or a closure there.
  The rules of the constructor apply unchanged. The checks moved into one
  function that the constructor and the typed destination share.
- The lowering emits the constructor's packing instruction at the same
  sites, so the validator, both back ends and `aura verify` see one
  operation. An annotated local of a storage type now has that type. It
  had the closure's own type before, which a `def` annotation still gives.
- A typed destination takes the complete contract. Hiding a default or a
  parameter name reports `AU2015` and names the constructor.
- A lambda without a capture list captures a Copy parameter by value.
- The "implicit erased storage" case of `AU2015` is gone.

Verification:

- `cargo test -p aura-compiler --lib`, the fixture suite, and the callable
  integration suites.
- New fixtures: `run-pass/callable_typed_destinations.au` and four
  `check-fail/callable_destination_*.au`.
- `examples/basics/stored_closures.au`, the Manual page and book section
  13.1, checked by `scripts/check_doc_examples.py`.

## Follow-up

- Step 2: local `def` statements (ADR-0105).
- Benchmarks against Rust `Box<dyn Fn>` and Python closures.
- A Shared callable in a tuple element cannot be called through an index,
  `pair[0]()`. It reports `AU3005`. Unpack the tuple first.
- A generic function cannot infer its type parameters from a lambda whose
  parameter types depend on them. Write the type arguments.
