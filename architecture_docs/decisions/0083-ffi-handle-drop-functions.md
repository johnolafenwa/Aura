# ADR-0083: An opaque handle names the C function that releases it

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0052 (opaque handles), ADR-0075 (handle out-parameters)

## Context

An opaque handle is released by passing it to an extern function that
takes it as `own`. Nothing made a program do so. A handle that went out of
scope, or sat in a list or a class instance that was dropped, leaked the C
object behind it.

A wrapper class could not close the gap. A method with `mut self` cannot
move a handle out of a field to pass it to the closing function, so a
class that held a database could not offer `close()` for a `with` block.
A binding of a real library, such as SQLite, leaked every object an error
path skipped.

## Decision

- **`extern "C" opaque class Name drop function`.** The clause names an
  extern function of the same module. It takes the handle as its one
  parameter, declared `own`, and returns a scalar or nothing. Another shape
  is `AU2005`. `drop` is a word only in this clause.
- **The function runs when the last owner of a handle goes away.** A
  handle value has one shared core that holds the address and the
  function. The core calls the function when it is dropped, with the
  address, if the program still owns the object.
- **An `own` argument gives the object up.** Just before a foreign call,
  each handle passed to an `own` parameter has its core cleared. The
  explicit close stays legal and the function runs once.
- **Out-parameters compare addresses.** After a call with a
  `mut Handle | None` argument, an unchanged address keeps the handle the
  place held, with its core. A changed address makes a new handle, and the
  core of the old one is cleared, because the C function replaced it.
- **Built programs pass the function's address** to the runtime when they
  make a handle. The interpreter looks the symbol up among the loaded
  libraries at that point, and a missing one is `AU4005`.
- **Release order among handles dropped together is not specified.**

## Alternatives rejected

- **A general destructor hook for classes.** It is a larger language
  feature: it needs rules for moves out of fields, for failure inside a
  destructor, and for generic code. A C function that takes one pointer
  needs none of them, and it is the case FFI has.
- **`with` only.** A context manager covers a lexical scope. It does not
  cover a handle in a list or in a returned value, and it does not help
  the wrapper class that cannot move its field.
- **Reference-counted handles that can be cloned.** Handles stay
  non-cloneable. The shared core exists because the interpreter and the
  runtime copy values internally, not as a source feature.
- **Release the object an out-parameter replaced.** C functions differ:
  some free the old object and some overwrite the pointer. Releasing it
  would be a double free for the first kind. Giving it up leaks for the
  second kind, which is what happened before this decision.

## Consequences

- A binding declares its closing function once, and error paths release
  what they hold.
- Every handle of a type with the clause is released, however it was made.
  A C function that returns a pointer its library keeps must return an
  opaque class without the clause.
- A handle still held when the process ends may not be released.
- Two C objects with an order between them need an explicit release, a
  narrower scope, or a function that tolerates either order.
- A handle value is one allocation larger than the address it wraps.
