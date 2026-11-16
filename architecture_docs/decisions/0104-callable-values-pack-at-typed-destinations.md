# ADR-0104: A callable value packs where owned callable storage is expected

- Status: Accepted
- Date: 2026-11-16
- Amends: ADR-0058 (answer C1, explicit erasure), ADR-0037 (capture of a
  shared parameter)
- Design checkpoint: [First-class closures](../29-first-class-closures-design-checkpoint.md)

## Context

ADR-0058 gave Aura owned callable storage. `Callable[def(...) -> R]` holds
a function, a closure with its captures, or a bound method, with one of
three call kinds. A value reached that storage only through a declared
alias called as a constructor:

```aura
type Handler = Callable[def() -> int64]

def make(base: int64) -> Handler:
    offset = base + 1
    return Handler(lambda: offset)
```

A lambda written where the storage type was expected reported `AU2015`,
"implicit erased storage". So did a function name and a bound method.
Storing a closure in a field, a list or a result always took an alias and
a constructor call, and the constructor repeated what the destination type
already said.

A lambda also could not capture a parameter of the enclosing function
unless the parameter was `own`, even when its type was Copy. A function
that returns a closure over an `int64` argument had to copy the argument
into a local first.

## Decision

### Packing at a typed destination

A function value, a lambda, a bound method, or a local that holds a
closure packs where a `Callable[...]` or `TaskCallable[...]` type is
expected. The sites are the ones that inject a union member and convert to
a trait object:

- an annotated binding and an assignment to a typed place
- an argument for a parameter of the storage type
- a field in a constructor call and a field assignment
- an element of a typed list, set, dictionary or tuple literal, and of a
  comprehension with an expected type
- a `return` in a function with that result type
- each arm of a conditional or `match` expression with that expected type
- a member of a union, such as `Callable[def() -> int64] | None`

```aura
def make(base: int64) -> Callable[def() -> int64]:
    return lambda: base + 1
```

The storage contract is the lambda's typing context, so its parameter
types, names and result come from the destination. The rules of the
constructor apply unchanged:

- the contract must be admitted (`AU2002`, `AU2015`)
- the call kind may only weaken (`AU2015`)
- a loan capture cannot be stored (`AU3010`)
- `TaskCallable` needs every capture to be Transfer (`AU3008`)

A value that is packed already keeps the ADR-0058 rule. An identical
contract moves. A restriction to a weaker contract needs the constructor,
which stays valid everywhere.

### Capturing a Copy parameter

A lambda without a capture list captures a Copy value by value wherever
the name is reached, including a shared or `mut` parameter of the
enclosing function. The value is read once, when the closure is created.
A non-Copy shared parameter still reports `AU3002`, because the closure
cannot own a value its creator does not own.

## Consequences

- No alias and no constructor call are needed to store a closure. The
  destination type is the only place the storage type is written.
- `AU2015` no longer reports "implicit erased storage".
- The lowering packs with the instruction the constructor uses, so the
  validator, both back ends and `aura verify` see one operation.
- A thin `def(...) -> R` type is unchanged. It is one code pointer and
  holds no captures. A function that accepts closures declares a
  `Callable[...]` parameter.

## Alternatives

- **Keep the constructor as the only way.** Rejected. The constructor adds
  no information, and the ceremony is what made closures second-class.
- **Give `def(...) -> R` an environment.** Rejected. Every function value
  would grow from one word to five and stop being Copy, and a written type
  would no longer state whether a call is shared, mutable or consuming.

## Tests

- `crates/aura-compiler/tests/fixtures/run-pass/callable_typed_destinations.au`
- `crates/aura-compiler/tests/fixtures/check-fail/callable_destination_*.au`
- `crates/aura-compiler/src/sema_tests.rs` and `mir_tests.rs`, tests named
  `callable_destination_*`
