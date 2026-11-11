# ADR-0077: FFI passes C structs by value and by pointer

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0074 (direct calls), ADR-0075 (out-parameters), ADR-0076
  (array pointers)

## Context

After ADR-0076 a foreign function can take scalars, text, bytes, arrays of
scalars, and opaque handles. Many C interfaces also pass small structs: a
point, a color, a time value, a result pair such as `div_t`. Some take them
by value and some through a pointer the function fills. Neither form could
be declared, so a binding needed a C adapter that split the struct into
scalar arguments.

Passing a struct by value is the part of the C calling convention with the
most rules. Where each byte travels depends on the size of the struct, the
types of its fields, the target, and the registers the earlier arguments
took.

## Decision

- **`extern "C" class Name:` declares a C struct.** The block lists fields
  only. A field is a fixed-width scalar or another `extern "C"` class, held
  inline. There are no methods, invariants, field defaults, or type
  parameters, and at least one field.
- **In Aura code it is a `copy class`.** It is built, read, assigned, and
  stored as any copy class is. It has a C layout only while it crosses.
- **`value: Name` and `-> Name` cross by value.** `value: mut Name` passes
  the address of a C copy, and the argument holds the copy's fields when
  the call returns. `mut Name` is also the spelling for a
  `const struct name *`. `own Name` is `AU3004`.
- **One layout module serves both backends.** `c_layout` in the runtime
  crate computes field offsets, size, and alignment with the natural
  alignment rule, and classifies how a struct is passed.
- **System V x86-64.** A struct over 16 bytes travels in memory: the caller
  copies an argument to the stack, and passes the address of the result's
  storage. A smaller struct travels as one register per 8 bytes, a vector
  register when every scalar in those bytes is a float and a general
  register otherwise. When the registers that are left do not hold all of
  an argument, the whole argument goes to the stack.
- **AArch64.** A struct of one to four floats of one type travels in that
  many floating-point registers. Any other struct up to 16 bytes travels in
  one or two general registers. A larger struct travels as the address of a
  caller copy, and a larger result through the result address register.
- **A built program emits the convention itself.** It writes the fields
  into a zeroed stack slot in the C layout and passes the slot's words as
  register arguments, or its address with the `byval` or `sret` marker the
  code generator needs. Results come back the same way in reverse.
- **The interpreter uses libffi struct types** built from the same layout.
- **A `bool` field must come back as 0 or 1.** Another byte is `AU4001`, as
  it is for every other `bool` C writes.

## Alternatives rejected

- **Pass every struct by pointer.** Functions that take or return a struct
  by value could not be declared, and those are the ones an adapter is
  most awkward for.
- **Give every class a C layout.** An ordinary class holds strings, lists,
  and other values with no C form, and its layout is the compiler's to
  change.
- **A separate `struct` keyword.** A C struct in Aura code is a copy class
  in every respect, so it is declared as one, marked `extern "C"` as
  opaque handles are.
- **A read-only pointer form.** `value: Name` already means by value, and a
  second spelling for `const struct name *` would be a new parameter mode
  for one case. `mut Name` covers it at the cost of asking for a mutable
  place.
- **Call through libffi in built programs.** It handles every struct, but
  builds each call at run time, which costs many times what the call does.
  Emitting the convention costs a store and a load for each field.

## Consequences

- Struct-passing C functions are declared without an adapter.
- The compiler carries calling convention rules for two targets. They are
  tested against a C compiler's code for 13 struct shapes on the build
  host. Only x86-64 can be tested in a cloud container, so the AArch64
  rules are checked by unit tests of the classifier and by macOS CI.
- On AArch64, a struct argument that the convention would move to the stack
  because the registers ran out is a build error. The stack layout there
  differs between Linux and macOS, and separate arguments cannot spell it.
  Declaring the parameter earlier, or as `mut Name`, avoids it.
- Unions, bit-fields, array fields, pointer fields, packed structs, and
  `ptr[Name]` arrays of structs have no spelling.
