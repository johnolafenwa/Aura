# First-Class Closures Design Checkpoint

Status: **Proceeding on the defaults below.** The owner approved the work
on 2026-11-16 and asked that it proceed on recommendations. Each default is
marked. A default the owner later changes is recorded in the
[Decision Record](#decision-record).

A capturing closure can already be stored, returned and kept in a
collection, but only after two steps of ceremony: declare a `type` alias of
a `Callable[...]` type, then call the alias around the lambda. A lambda is
one expression and its parameters cannot carry types. This checkpoint
removes the ceremony and adds closures with statement bodies.

## Starting Point

- `def(...) -> R` is a thin function value: one code pointer, Copy, no
  environment (ADR-0037, ADR-0058 Q13).
- `Callable[def(...) -> R]`, `Callable[mut def(...) -> R]` and
  `Callable[own def(...) -> R]` are owned storage for a callable with its
  environment and one of three call kinds: Shared, Mutable, Consuming
  (ADR-0058). `TaskCallable[...]` adds the proof that every capture is
  Transfer.
- A closure reaches that storage only through `Alias(value)`. A lambda, a
  function or a bound method written where a `Callable` is expected reports
  `AU2015`, "implicit erased storage".
- A lambda body is one expression. Its parameter types come from an
  expected function type. A lambda with parameters and no expected type
  reports `AU2002`.
- A capturing closure with no `Callable` destination lives only in an
  inferred local, a direct call, a compiler-known callback site or a task
  start.

## A. Storing A Closure

- **A1. A callable value packs where a `Callable` type is expected
  (default).** A lambda, a local `def`, a named function, a bound method or
  a local that holds a closure converts at every typed site: an annotated
  binding, an assignment, an argument, a field of a constructor call, an
  element of a typed list, set or dictionary literal, a `return`, and each
  arm of a conditional or `match` expression. The sites are the ones that
  already inject a union member or convert to a trait object. The rules of
  the explicit constructor apply unchanged: the contract must be admitted,
  the call kind may only weaken, a loan capture cannot be stored
  (`AU3010`), and `TaskCallable` needs Transfer captures (`AU3008`).
- A2. Keep the explicit constructor as the only way. Rejected: it is the
  ceremony this work removes, and the constructor adds no information the
  destination type does not already state.
- A3. Give `def(...) -> R` an environment, so the thin type holds
  closures. Rejected: every function value would grow from one word to
  five and stop being Copy, and a written type could no longer say whether
  a call is shared, mutable or consuming. Aura states that in the type for
  every other owned value.

This reverses one answer of ADR-0058, that erasure is always explicit.
The alias constructor stays valid. It is still the way to restrict an
already packed value to a weaker contract, and it reads well where no
destination type is in sight.

A typed destination takes a value's complete contract. Hiding a default
or a parameter name is a restriction, and the constructor makes it.

- **A4. A lambda captures a Copy parameter by value (default).** Before
  this work `def make(n: int64)` could not return `lambda value: value +
  n`, because `n` is a shared parameter. The lambda now reads a Copy
  parameter once, when it is created. A non-Copy shared parameter still
  reports `AU3002`.

A `Callable` to `Callable` flow keeps today's rule. Identical contracts
move. A restriction needs the constructor.

## B. Statement Bodies

- **B1. A `def` statement inside a function (default).** It declares a
  local closure with typed parameters, a result type and a statement body.

  ```aura fragment
  def make_counter(start: int64) -> Callable[mut def() -> int64]:
      mut count = start
      def bump() -> int64:
          count += 1
          return count
      return bump
  ```

  The local `def` follows the lambda rules for capture, call kind,
  ownership and Transfer. It captures by value when it is declared. It
  needs no expected type, because its signature is written. A local `def`
  with no captures is an ordinary function value.
- B2. A multi-line lambda, `lambda x:` followed by an indented block
  inside an expression. Rejected: an indented block inside call
  parentheses has no clean layout rule, and Python readers already expect
  a nested `def` for this.
- B3. Typed lambda parameters, `lambda (x: int64): ...`. Not taken. The
  local `def` covers the case and the lambda grammar stays as it is.

Rules of a local `def`:

- An explicit capture list follows the name: `def bump[mut count]() ->
  int64:`. The entries are those of a lambda capture list. A local `def`
  takes no type parameters, so the bracket is unambiguous.
- The name is an immutable binding in the enclosing block. A Mutable local
  `def` is callable through that binding without `mut`, because the
  binding cannot be reassigned and the declaration is the only owner.
- `return` inside the body returns from the local `def`. `break` and
  `continue` cannot cross it.
- Parameters take the same modes as a function: bare, `mut` and `own`.
  Keyword-only parameters are allowed. Defaults are not: a default belongs
  to a named module-level declaration.
- No decorators, type parameters, contracts, `view` results or generator
  bodies in this version.

## C. Recursion

- **C1. A local `def` may call itself when it is Shared or has no
  captures (default).** The call reuses the environment of the running
  call. A Mutable or Consuming local `def` that names itself reports an
  error with the fix: pass the state as a `mut` parameter.
- C2. No self reference. Rejected: a recursive helper, such as a
  depth-first search over a captured graph, is the most common reason to
  nest a `def`.

Two local `def`s cannot call each other, because each is declared before
the other exists. Use module-level functions for mutual recursion.

## D. What Stays

- Loan captures, `[values]` and `[mut values]`, stay local. A stored
  closure owns its captures. Storing a loan needs lifetime-bearing types,
  which Aura does not have.
- A thin `def(...) -> R` parameter of a user function still takes only a
  function or a capture-free lambda. A function that accepts closures
  declares a `Callable[...]` parameter, and the caller now passes the
  lambda directly.
- Packed callables stay non-Copy and non-cloneable, with no equality.
- A closure cannot be an FFI callback.

## E. Cost Targets

Measured against Rust `Box<dyn Fn>` and `Box<dyn FnMut>` and against
Python closures, in instructions a call and wall time:

- a call through a stored closure within 3x of Rust and faster than Python
- creating and dropping a stored closure within 3x of Rust
- a local `def` called directly costs what the same lambda costs

## F. Delivery

1. Implicit packing (A), with its ADR, manual, book and editor updates.
2. Local `def` (B, C), with parser, formatter, checker, both back ends,
   `aura verify`, grammar, language server, manual, book and ADR.
3. Benchmarks under `benchmarks/` and fixes for what they show.

## Decision Record

- 2026-11-16: work approved by the owner as item 3 of the missing
  features list. Defaults A1, B1 and C1 adopted without further review.
