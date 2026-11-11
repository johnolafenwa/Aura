# ADR-0081: Trait objects are `dyn Trait`

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0052 (unions), ADR-0033 (structural Transfer)
- Design checkpoint: `architecture_docs/27-trait-objects-design-checkpoint.md`

## Context

A list could mix types only through a union, which names every member. A
program that takes plugins, handlers, or strategies cannot list the types
that will implement its trait, so the union had to be edited for each new
one, with every function that takes the list. A type parameter does not
help: it is one type at each call.

## Decision

- **`dyn Trait` is a type.** It is the type of a value of any type that
  implements the trait. It is valid wherever a type is, except as a
  dictionary key or a set element. A bare trait name stays not a type
  (`AU2050`). A trait with associated types cannot be a trait object.
- **A value converts where the type is already written.** An annotated
  binding, an assignment to a typed place, an argument, a `return`, a field,
  a payload, and an element of a typed collection literal convert a value
  whose type implements the trait. `value as dyn Trait` names the conversion
  where nothing else gives the type. The conversion moves the value. A
  `list[T]` does not convert to a `list[dyn Trait]`.
- **A method is callable when one call can serve every type.** It takes a
  receiver, declares no type parameters, and names `Self` nowhere else. The
  rule is per method, so a trait may declare other methods and still be a
  trait object. Supertrait methods are callable, and a trait object converts
  to a trait object of a supertrait.
- **A trait object is an owned value without `==` or `clone()`.** It moves,
  lends, and stores as a class instance does. The trait says nothing about
  comparing or copying the types behind it.
- **`match` selects on the held type.** A type arm is `case Type as name:`,
  the spelling of a union type arm, and matches exactly that type. The match
  needs a final catch-all, because the set of types is open. `match mut`
  writes the binding back and `match own` moves the value out.
- **`trait Job: Transfer:` lets a trait object cross tasks.** `Transfer` is a
  prelude marker trait answered by the structural Transfer property. A trait
  that extends it admits only Transfer types (`AU3008` at the `impl`), so
  `dyn Job` is a task argument, a task result, and a `Queue` payload.
- **The representation adds no wrapper.** The checker spells the type
  `Named("dyn", [trait])`, and MIR erases it to an opaque parameter type. A
  conversion is the rvalue `TraitObject { value, held }`, which records the
  static type the value had. The interpreter wraps the value with that type.
  Native code keeps the value's own box, which already carries its exact
  type, and wraps only a float.
- **A call dispatches on the held type.** MIR lists the candidate
  implementations. Native code compares the class code in the box header and
  falls back to the general type tests for a generic class, an enum, and a
  scalar. A shared `for` loop over `list[dyn Trait]` lends each element, and
  a method the dispatch may pick keeps its shared receiver with the caller.
- **`aura verify` follows what it can see.** A call on a trait object relies
  on the trait method's contract. A trait object the function made from a
  value of a known type keeps that type and value, so a call on it uses the
  implementation's own contract and a type arm on it is decided.

## Alternatives rejected

- **A bare trait name as the type**, as in `list[Tool]`. It hides that the
  value is chosen at run time and gives up `==`, `clone()`, and fields, and
  it would make `def f(tool: Tool)` and `def f[T: Tool](tool: T)` look alike
  while behaving differently.
- **A whole-trait object-safety rule.** One generic method would bar a trait
  from ever being a trait object. The per-method rule keeps the trait usable
  and reports the method at the call.
- **A vtable pointer beside the value.** Every boxed value already carries
  its type, and a second word would change the layout of every place a trait
  object is stored. The class code comparison costs 34 instructions a call
  against 12 for Rust's `Box<dyn Trait>`, inside the 3x bar.
- **Downcasting with a method**, such as `tool.as[Echo]()`. `match` already
  selects on a held type for unions, and the required catch-all states the
  open set at the place it matters.
- **`case Echo(echo):` as the type arm.** That is already a class pattern
  over fields.
- **Implicit conversion of collections.** Converting a `list[Echo]` to a
  `list[dyn Tool]` would copy or rebox every element behind an assignment.

## Consequences

- A call on a `list[dyn Trait]` element in a shared loop costs 34
  instructions on x86-64 Linux, against 12 for Rust and about 4,600 before
  the dispatch work. The mixed probe runs in 12 ns a call against 60 ns for
  Python.
- A `mut self` call on a list element still takes the element out and
  stores it back, as it does for a list of class instances.
- A receiver that is a generic class, an enum, or a scalar uses the slower
  type tests.
- A shared `match` on a trait object copies the held value for the arm.
- The verifier treats a trait object it did not see made, or one merged
  from paths that hold different types, as holding any type.
