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

## Stored Call Speed

The benchmark in `benchmarks/closures` showed that a call through a
closure kept in a class field or a list element took about 8,400
instructions. Storing boxed the closure into a runtime function value, and
every call read the box back, copied its name and signature, and went
through the runtime's boxed call.

Work completed:

- A boxed Shared or Mutable closure keeps its four words
  (`DirectCallable`), and its captures become runtime values only when
  runtime code reads them.
- A function value is shared, not copied, when a container hands it out.
- A callee that is a field of an instance kept in place, or an element of
  a list, is called where it is kept. A `for` loop over a list of Shared
  callables copies each element's words. A call through a loan of a
  Mutable element reaches the words in the list.
- The `invoke` adapter claims mutable sinks only when the caller's mask
  says it handed some over.
- Two older MIR validator faults found on the way: a nested list of
  callables whose rows differ in length, and a call through an element
  loan, `for tally in mut tallies: tally(1)`.

Instructions a call, before and after: a field call 8,400 and 76, a list
loop 8,400 and 128, a loop over Mutable elements 1,516 and 112, a Mutable
local 240 and 98. Wall times are in `benchmarks/closures/README.md`.

Verification:

- `cargo test -p aura-compiler --lib` and `cargo test -p aura-runtime
  --lib`, with new tests for the kept words, the validator cases, and the
  runtime helpers generated code names.
- The fixture suite with `run-pass/callable_stored_calls.au` and
  `run-pass/callable_stored_flows.au`, which both back ends run.
- The callable, validator, native coverage, parity, cleanup, and
  representation suites.

## Follow-up

- Step 2: local `def` statements (ADR-0105).
- Benchmarks against Rust `Box<dyn Fn>` and Python closures.
- A callable element of a list in a field cannot be called through an
  index, `self.steps[0](1)`. It reports `AU2999`. Loop over the list or
  read the list into a local first.
- A Shared callable in a tuple element cannot be called through an index,
  `pair[0]()`. It reports `AU3005`. Unpack the tuple first.
- A generic function cannot infer its type parameters from a lambda whose
  parameter types depend on them. Write the type arguments.
