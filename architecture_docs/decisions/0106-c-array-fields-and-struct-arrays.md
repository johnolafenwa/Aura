# ADR-0106: C array fields, pointers to struct arrays, and structs on the stack

- Status: Accepted
- Date: 2026-11-16
- Builds on: ADR-0076 (`ptr[T]` array pointers), ADR-0077 (C structs)
- Design checkpoint: `architecture_docs/29-ffi-v2-graphics-design-checkpoint.md`

## Context

A binding of raylib generated from the library's own API description
reached 556 of its 600 functions on the FFI of 0.16. Most of the rest
needed one of three things:

- A struct with a fixed-size array field. `Material` holds `float
  params[4]`, and every function that takes a material was out of reach.
  GLFW's gamepad state and SDL's text events have the same shape.
- A pointer to the first of a run of structs. `DrawLineStrip`,
  `DrawTriangleFan`, and the spline functions take `const Vector2 *`.
- On AArch64, a struct argument that the convention places on the stack
  because the registers before it are taken was a build error.
  `DrawModelEx` and `DrawTexturePro` pass more than eight floats in
  structs.

## Decision

- **`name: array[T, N]` in an `extern "C"` class** is laid out as
  `T name[N]`. `T` is a scalar, an `extern "C"` class, or another array.
  `N` is an integer literal from 1 to 1048576.
- **The Aura type of the field is `list[T]`.** The list must hold exactly
  `N` elements when the value crosses, or the call fails with `AU4005`.
  `array` names a type only in such a field, as `ptr` and `cstr` name one
  only in an extern signature. The parser rewrites the field's type and
  records the lengths on the class.
- **A class with an array field is not Copy**, and neither is an
  `extern "C"` class that holds one. It holds a list and moves like any
  class that does. Classes of scalars stay Copy.
- **`ptr[Name]` and `mut ptr[Name]`** take a `list[Name]`, where `Name`
  is an `extern "C"` class, and pass a C array of the structs. The call
  writes the array before the foreign function runs and, for `mut`, reads
  it back after. An empty list passes a null pointer.
- **Conversion uses the declared C layout.** The interpreter and native
  runtime pack and unpack values with `c_values.rs` and `c_layout.rs`.
  Native code passes plain structs in registers or memory according to the
  host ABI. For a read-only struct pointer it can copy native list storage
  when each scalar offset, width and stride matches C. For a small struct
  holding a single scalar array, it checks the field's representation and
  length before copying its bytes. A union's byte field uses the same check.
  Other values use the runtime conversion and its diagnostics.
- **Layout.** An array has the alignment of its element and `N` times its
  size. System V classifies each eightbyte from every scalar that
  overlaps it. AArch64 counts the scalars of an array toward the four a
  homogeneous float aggregate may hold.
- **A struct that does not fit the registers that are left goes to the
  stack whole.** On AArch64 a built program passes it as one array
  argument, which LLVM keeps together and places as the convention
  requires, and no later argument takes a register of the kind the struct
  needed. System V keeps its by-value stack copy.
- **A struct that needs only one kind of register is not moved by
  arguments of the other kind that went to the stack.** Before this
  decision, a struct of integers that followed nine floats was passed on
  the stack on both conventions, where C passes it in registers.

## Alternatives rejected

- **A fixed-size array type in the language.** It is a language feature
  with its own design: indexing by a constant, bounds, generic lengths. A
  list gives indexing, `len`, and iteration today, and the length is
  checked where it matters, at the boundary.
- **A tuple of `N` elements.** A tuple has no indexing, and `char name[32]`
  would be a 32-tuple.
- **Keeping `list[Name]` in the C layout so it crosses with no copy.** It
  changes the storage of every list of such a class. The measured cost of
  the copy decides whether it is worth it, and a buffer of floats already
  crosses with no copy as an `Array[float32]`.
- **Passing an overflowing AArch64 struct with `byval`.** Darwin packs
  stack arguments at their natural alignment, and `byval` aligns to eight
  bytes, so a pair of floats after one float would land four bytes late.
  It also leaves later arguments free to use the registers the struct
  could not.

## Consequences

- The raylib binding covers 587 of 600 functions. What is left is
  variadic functions and callbacks that take C text or `void *`.
- A struct with an array field costs a runtime call and a copy of each
  element on every crossing, where a class of scalars costs a store and a
  load per field.
- A `ptr[Name]` call costs time linear in the list.
- `ClassDecl` records the array lengths of an `extern "C"` class in
  `c_arrays`, and `CFieldKind` has an `Array` case, which tools that read
  the AST or MIR as JSON see.
- Unions, C text in callbacks, variadic functions, and closures as
  callbacks stay open. The design checkpoint lists them.
