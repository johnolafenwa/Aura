# Trait Objects Design Checkpoint

Status: **Ratified 2026-11-10.** The owner approved every default below.
See the [Ratification Record](#ratification-record).

A trait object lets one list hold values of an open set of types. This
checkpoint settles its spelling, where it is allowed, how a value becomes
one, which methods it can call, how it is owned, and how it is stored.

## Starting Point

- Traits are nominal and generic, with supertraits, default methods, and
  associated types. Every trait call is resolved at compile time, except
  that a union value dispatches on its active member.
- A list of mixed types is a list of a union, `list[Book | Coffee]`. The
  set of types is closed: a module that adds a type must edit the union.
  Unions do not widen either, so `A | B` is not assignable to
  `A | B | None`.
- A generic function is compiled once per concrete type on the direct
  backend. A value of an unspecialized type parameter already exists there
  as an erased value: one handle to a box that holds the value with its
  type. A call through such a value tries the trait's implementations
  against the box's type.

Measured on `main` before this work, for 30,000 shapes of three classes
and `area()` summed twenty times: Rust `Box<dyn Shape>` 10 instructions a
call, Python 61 ns a call, an Aura union list 2,398 instructions a call.
A static call on a `list` of a class that holds a `str` costs 1,710
instructions, because the loop copies each element.

## A. Spelling

- **A1. `dyn Trait` (default).** `tools: list[dyn Tool]`. The word marks
  the cost and keeps a trait name from being read as a type. `dyn` is
  contextual: it is special only before a trait name in a type, and it is
  reserved as a type name.
- A2. The bare trait name, `list[Tool]`. Rejected: a bound `T: Tool` and a
  type `Tool` would look alike and mean different things, and a bare trait
  name as a type is today's most likely beginner mistake. It now reports
  `AU2050` with the two fixes.

`dyn` takes one trait, with its type arguments: `dyn Reader[int64]`. A
trait with associated types cannot follow `dyn` (`AU2050`).

## B. Where It Is Allowed

A trait object type is valid wherever a type is, including a generic
argument (`Box[dyn Tool]`) and a union member (`dyn Tool | None`). Two
positions are refused: a dictionary key and a set element, because a trait
object does not define equality.

`impl Other for dyn Tool` is rejected (`AU2050`). A trait object answers
for its own trait and supertraits by calling the value it holds; a second
implementation for the object itself would give one call two answers.

## C. Conversion

- **C1. Implicit at typed sites, plus `value as dyn Trait` (default).** A
  value converts where a trait object type is expected and its type
  implements the trait. The sites are the ones that inject a union member
  today: a typed binding, an assignment to a typed place, an argument, a
  `return`, a constructor field, a payload, an element of a typed
  collection literal. `value as dyn Trait` names the conversion where no
  site supplies the type.
- C2. Explicit only. Rejected: `tools.append(Echo() as dyn Tool)` at every
  site is noise the union rule already avoids.

The conversion applies to one value. `list[Echo]` does not convert to
`list[dyn Tool]`. A trait object converts to a trait object of a
supertrait, and to nothing else. It does not convert back to the type it
holds; section F is the way back.

A `mut` parameter of a trait object type takes a place that already has
exactly that type (`AU2051`), because the callee may store a value of
another type there.

## D. Which Methods Are Callable

- **D1. Per method (default).** A method is callable on a trait object
  when one call can serve every implementing type: it takes a receiver, it
  has no type parameters of its own, and `Self` appears nowhere else in
  its signature. The trait may declare other methods; calling one on a
  trait object reports `AU2051`.
- D2. Per trait: a trait with one such method cannot be a trait object.
  Rejected: adding a `Self` method to a trait would break every
  `dyn Trait` user.

## E. Bounds And Iteration

`dyn Tool` satisfies `T: Tool`, and a bound on a supertrait, when every
method the bound promises is callable on a trait object. A generic
function is then compiled for `T = dyn Tool` like any other type.

`dyn Iterator[T]` is a `for` source.

## F. Getting The Type Back

- **F1. `match` type arms with a required catch-all (default).**
  `case Echo(echo):` selects on the held type. The set of types is open,
  so the match needs `case _`.
- F2. A downcast method returning `T | None`. Rejected: it adds a
  reflective call where `match` already reads naturally.

## G. Ownership

A trait object is an owned value. It moves and is never copied.

- No `clone()`, and no `copy()` on a collection of trait objects. The type
  behind the object is not known where it is used, so nothing says a copy
  is safe. A trait that wants copies declares a method that returns one.
- No `==`.
- `value = items[i]` is refused (`AU3005`), as it is for any element that
  cannot be cloned. A method call on an element and a shared or `mut`
  argument borrow the element in place.

## H. Tasks

- **H1. `trait Tool: Transfer:` (default).** A trait that requires
  `Transfer` admits only Transfer implementing types, so its trait object
  is Transfer and may cross a task boundary. Any other trait object is
  not Transfer (`AU3008`).
- H2. `dyn Tool + Transfer`. Rejected: a second axis on every use for a
  property that belongs to the trait.

## I. Representation

- **I1. One handle to the value's own box (default).** The box holds the
  value and its type. This is the representation of an erased
  type-parameter value, so lists, dictionaries, fields, and unions store
  it with the layouts they have. Dispatch reads the type from the box.
- I2. A Rust-style two-word pointer with a method table. Rejected: it
  touches every container layout and the task boundary, for a saving that
  step 6 can reach with a type number in the box header.
- I3. Lowering `dyn Trait` to the union of every implementing type in the
  program. Rejected: unions do not nest, widen, or narrow, separate
  modules would disagree on the set, and the largest member would size
  every element.

Performance bar: a call on a trait object within 3x of Rust's
`Box<dyn Trait>` call on the probe above, and faster than Python.

## Delivery Plan

1. Parser, checker, conversion, calls, ownership rules, both backends.
2. Generic traits, bounds, upcasts, `as dyn`, `dyn Iterator[T]`.
3. `match` type arms.
4. `trait Tool: Transfer:`.
5. Lend collection elements to method receivers instead of copying them.
6. Dispatch on a type number, operation probes, a cross-language program.
7. `aura verify`, the language server, the manual, the book, and the ADR.

## Diagnostics

| Code | Meaning |
| --- | --- |
| `AU2050` | Invalid trait object type. |
| `AU2051` | Invalid trait object use. |

Existing codes cover the rest: `AU2008` for equality, `AU3005` for reading
an element out, `AU3007` for `clone` and `copy`, `AU3008` at a task
boundary.

## Progress

- **Step 1 (2026-11-10).** `dyn Trait` parses, checks, and runs on both
  backends. In the checker and in MIR before specialization, the type is
  `Type::Named("dyn", [Named(trait, args)])`, where the trait is named by
  its canonical name: bare for a trait of the program's own module,
  `module.Name` otherwise. After specialization
  `mir_trait_objects::erase_trait_objects` replaces each trait object type
  with the type parameter `dyn Trait[args]`, so every later pass and both
  backends treat it as the erased value of section I. A conversion is
  recorded in the union-injection table and lowered as a move into a place
  of the trait object type. Supertrait calls, upcasts, and bounds already
  work in this step.

## Ratification Record

- 2026-11-10 09:22 UTC: the owner chose "Approve all defaults" on the
  decision card for this checkpoint: the `dyn Trait` spelling (A1),
  `match` type arms (F1), `trait Tool: Transfer:` for tasks (H1), and the
  speed bar of 3x Rust `dyn` and faster than Python. The remaining
  defaults (C1, D1, I1) were approved with them.
