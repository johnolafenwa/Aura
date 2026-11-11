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

## Step 2: `as dyn`, iterators, and conversion sources

- `value as dyn Trait` names the conversion where nothing supplies the
  type: `tools = [Echo() as dyn Tool, Counter()]`. It converts exactly as
  the implicit form does in the same place: it moves the value when it is
  bound, stored, or passed to an `own` parameter, and it borrows the value
  as a `self` receiver or a shared argument. A cast is a new value, so it
  is refused as a `mut` receiver or `mut` argument (`AU2051`) and as a
  `view` source (`AU3004`).
- `dyn Iterator[T]`, and a trait object of a trait that extends
  `Iterator[T]`, is a `for` source, in `for`, `for ... in mut`,
  `for ... in own`, comprehensions, `enumerate`, and `zip`. It satisfies
  an `Iterator[T]` bound.
- A value of a union type converts when every member implements the
  trait. The trait object holds the union's active member, not the union:
  lowering branches on the tag and moves that member's payload in. A
  value of a bounded type parameter converts too, including when the
  parameter is itself a trait object.
- The parser reads `as dyn Name` as a cast. `with group() as dyn:` still
  binds a name called `dyn`.

## The exact held type

Step 1 lowered a conversion as a plain move, and each backend then worked
out the held type from the value. That is not exact, and it showed as
three bugs in step 1:

- A `float32` and a `float64` that both implement the trait ran each
  other's method: a float value does not say its width.
- On `aura run`, `Cell[float32]` ran the method of `Cell[float64]`, and a
  call on a trait object holding a value of a generic enum failed with
  "type `Maybe` has no MIR implementation": an interpreter value does not
  carry its type arguments.

A conversion is now its own MIR rvalue, `TraitObject { value, held }`,
with the static type of the value. The interpreter wraps the value as
`Value::TraitObject { held, value }` and dispatches on `held`. The native
runtime uses that wrapper for a float only, because every other boxed
value already carries its exact type; a `mut self` call on a held float
puts the wrapper back. A value known only by a type parameter or a union
gives its active member at run time, so a trait object never holds a
union, also when a generic function converts its `T`.

Also fixed here: an element of a list of trait objects can be passed to a
`mut` parameter, `widen(shapes[0])`.

## Step 3: `match` type arms

- `match` on a trait object takes type arms spelled like a union's:
  `case Square as square:`. The checkpoint wrote `case Echo(echo):`; that
  spelling is already a class pattern over fields, so the union spelling
  is used and the checkpoint records the change.
- An arm names one type that implements the trait and matches exactly
  that type. The match needs a final `case _:` or a name. `AU2013`
  reports a missing catch-all, an arm whose type does not implement the
  trait, an arm that names a trait object, a second arm for one type, and
  an arm after the catch-all.
- A bare `match` gives a shared binding and locks the scrutinee for the
  arm. `match mut` gives a mutable binding and writes it back at the end
  of the arm and on `return`, `break`, and `continue`. `match own` moves
  the value out.
- Match statements and match expressions, guards, a scrutinee that is a
  field, a list element, or a dictionary entry, and a type arm inside an
  enum payload pattern all work.
- Lowering: `TraitObjectHolds { value, ty }` tests the held type and
  `TraitObjectTake { value, ty }` copies or moves the held value out.
  `match mut` reuses the write-back path of `match mut` on a union.

## Step 4: `trait Job: Transfer:`

- `Transfer` is a marker trait in the prelude. A module names it without
  an import, as it names `Ord`. It has no methods and no implementations.
- Wherever a bound or a supertrait names the marker, the checker asks the
  structural Transfer property of the type (`transfer_failure`) instead
  of looking for an `impl`.
- `trait Job: Transfer:` admits only Transfer types. `impl Job for Dice:`
  reports `AU3008` when `Dice` holds a `random.Rng`, and names the field.
  A generic type states the condition: `impl[T: Transfer] Job for Box[T]:`.
- So `dyn Job` is Transfer, and so is the trait object of a trait that
  extends `Job`. It is a task argument, a task result (`Task.result()`
  moves it out), and a `Queue` payload, alone or inside a list, a
  dictionary, a tuple, a union, or a class.
- `T: Transfer`, and `T: Job`, make a type parameter Transfer inside a
  generic function, and each call proves the bound for its type argument.
  An unbounded type parameter is refused as before.
- `dyn Transfer` holds any Transfer value. It has no methods, so a type
  arm is how the value is used.
- `impl Transfer for T:` reports `AU3008`. A trait the program itself
  names `Transfer` is an ordinary trait and is not the marker.
- A prelude trait named only after `dyn` is now found. Before, the
  on-demand prelude import looked only for "unknown trait" errors.
- No backend change: a trait object crosses a worker boundary as its box
  does.

## How it is built

- The checker and MIR before specialization use
  `Type::Named("dyn", [Named(trait, args)])`. The trait is named by its
  canonical name: bare for a trait of the program's own module,
  `module.Name` with the module's full path otherwise, so an import alias
  does not change the type.
- A conversion is recorded in the union-injection table and lowered as
  the rvalue `TraitObject { value, held }` into a place of the trait
  object type. An upcast, `dyn Tool` to `dyn Named`, is a plain move.
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

- 37 run-pass fixtures and 56 check-fail fixtures named `trait_object_*`.
  Each run-pass program prints the same on the interpreter and as a
  native binary, and the native binaries are clean under valgrind.
- Unit tests in `aura-runtime` for the two new runtime entry points.
- `package_io_helpers_cover_local_error_paths` expects an unwritable
  directory and fails when the tests run as root, as they do in the
  cloud container; it does not touch this work.

## Known gaps, for the next steps

- An explicit type argument that names the enclosing function's own type
  parameter, `apply(run_one[T], job)` or `group.start(run_one[T], job)`,
  fails at run time with "type `T` has no MIR implementation". This is
  older than trait objects and needs no `Transfer`. Leaving the type
  argument to inference, `group.start(run_one, job)`, works.
- A call still tries the trait's implementations one by one against the
  box's type, a shared `match` copies the held value into its binding,
  and a `for` over a list copies each element that holds a heap value.
  Steps 5 and 6 remove these, with probes against Rust and Python.
- `aura verify` does not model the three new rvalues yet (step 7).
- A lambda whose body calls a method on a list element it captured,
  `lambda: items[0].area()`, fails with `AU4001` (loan flow). This is
  older than trait objects: it happens with `list[Label]` too.
- In a shared `match` on a union or an enum, binding an arm's value to a
  new name, `case Square as square:` then `other = square`, fails with
  `AU4001`. This is older than trait objects too, and the same code on a
  trait object works.
