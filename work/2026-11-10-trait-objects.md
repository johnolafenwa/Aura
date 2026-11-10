# Trait objects

Date: 2026-11-10. Asked for by John: "trait objects (dynamic dispatch), so
a list can hold an open set of types", bug free, within range of Rust,
faster than Python, and complete. The design he approved is in
`architecture_docs/27-trait-objects-design-checkpoint.md`.

## Step 1: the type, conversion, and calls

What works, on both backends:

- `dyn Trait` in every type position except a dictionary key and a set
  element. `dyn Reader[int64]` for a generic trait.
- A value converts where the trait object type is expected: a typed
  binding, an assignment, an argument, a `return`, a field, a payload, an
  element of a typed collection literal, a member of a union such as
  `dyn Tool | None`.
- A method call runs the implementation of the held type. Default
  methods, supertrait methods, `mut self` and `own self` receivers.
- `dyn Tool` converts to `dyn Named` for `trait Tool: Named:`, and
  satisfies `T: Tool`.
- Traits and implementations from other modules, by name, by namespace,
  and through an import alias.

What is refused, each with its own message:

- `AU2050`: a bare trait name as a type, `dyn` before a non-trait, a trait
  with associated types, a wrong number of trait arguments, an `impl` for
  a trait object type.
- `AU2051`: a call to a method with no `self`, with type parameters, or
  with `Self` in its signature; a trait object for a type parameter whose
  bound promises such a method; a `mut` argument that is not already the
  trait object type.
- `AU2008` for `==`, a dictionary key, a set element. `AU3007` for
  `clone()` and `list.copy()`. `AU3005` for `value = items[i]`.
  `AU3008` at a task boundary.

## How it is built

- The checker and MIR before specialization use
  `Type::Named("dyn", [Named(trait, args)])`. The trait is named by its
  canonical name: bare for a trait of the program's own module,
  `module.Name` with the module's full path otherwise, so an import alias
  does not change the type.
- A conversion is recorded in the union-injection table and lowered as a
  move into a place of the trait object type. There is no new MIR
  instruction.
- After specialization, `mir_trait_objects::erase_trait_objects` replaces
  each trait object type with a type parameter named `dyn Trait[args]`.
  Every later pass and both backends then treat the value as the erased
  generic value they already support: one handle to a box that holds the
  value with its type.
- A trait object is never clone-safe. A method receiver that selects an
  element, `tools[i].run()`, and an argument to a shared or `mut`
  parameter borrow the element in place. The checker marks those index
  expressions before it types the enclosing expression, because the
  overlap and loan checks type a receiver on their own first. An owned
  read of the same expression is still refused.

## Checked

- 20 run-pass fixtures and 24 check-fail fixtures named `trait_object_*`.
  Each run-pass program prints the same on the interpreter and as a
  native binary, and the native binaries are clean under valgrind.
- `cargo test -p aura-compiler --lib`: 1,951 pass. The one failure,
  `package_io_helpers_cover_local_error_paths`, expects an unwritable
  directory and fails when the tests run as root, as they do in the
  cloud container; it does not touch this work.

## Known gaps, for the next steps

- `value as dyn Trait`, `dyn Iterator[T]` in `for`, `match` type arms,
  and `trait Tool: Transfer:` are not built yet.
- A call still tries the trait's implementations one by one against the
  box's type, and a `for` over a list copies each element that holds a
  heap value. Steps 5 and 6 remove both, with probes against Rust and
  Python.
- A lambda whose body calls a method on a list element it captured,
  `lambda: items[0].area()`, fails with `AU4001` (loan flow). This is
  older than trait objects: it happens with `list[Label]` too.
