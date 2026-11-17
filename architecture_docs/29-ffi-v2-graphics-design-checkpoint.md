# FFI v2 And raylib Design Checkpoint

Status: **Open 2026-11-16.** Work proceeds on the defaults marked below.
Sections A, C, and the AArch64 part of F are implemented and recorded in
[ADR-0106](decisions/0106-c-array-fields-and-struct-arrays.md). Section B
is implemented and recorded in [ADR-0107](decisions/0107-c-unions.md),
with one change from B3: a union is built from a member with the
associated function `Name.from_member(value)`, not `Name(member=value)`.
Section E1 is implemented in
[ADR-0108](decisions/0108-c-text-in-callbacks.md). Each default stays
open to change until the release that ships it.

This checkpoint covers two pieces of the graphics work: a maintained
raylib package, and the FFI additions that C graphics libraries need. The
`image` module and a built-in window module are separate work.

## Starting Point

Measured on main after 0.16.0, with raylib 6.0 built from source on Linux
and a virtual display:

- A raylib program written against the FFI of 0.16 opens a window, draws
  2D shapes, text, and a 3D scene through a camera, reads the frame back,
  and writes it to a PNG. It runs the same on both backends.
- Structs by value cover `Color`, `Vector2`, `Vector3`, `Rectangle`,
  `Camera2D`, `Camera3D`, `Texture`, and `Image`. A pointer field such as
  `Image.data` is declared `uint64` and passed back untouched.
- A binding generated from raylib's own API description covers 556 of
  its 600 functions.
- On AArch64, a struct argument that the convention places on the stack
  because the registers before it are taken is a build error. raylib has
  such calls: `DrawTexturePro` and `DrawModelEx` pass more than eight
  floats in structs.
- These raylib, SDL, and GLFW declarations have no spelling:

| C shape | Where it appears |
| --- | --- |
| a fixed-size array field | `Material.params`, `GLFWgamepadstate.axes`, `SDL_TextInputEvent.text` |
| a union | `SDL_Event`, and every SDL event read |
| a pointer to an array of structs | `DrawTriangleFan`, `DrawLineStrip`, `DrawSplineLinear`, `SDL_RenderDrawLines` |
| a variadic function | `TextFormat`, `TraceLog`, `SDL_Log` |
| C text passed to a callback | the GLFW error callback |

## A. Array Fields

- **A1. `name: array[T, N]` in an `extern "C"` class (default).** `T` is
  a scalar, an `extern "C"` class, or an `extern "C"` union. `N` is an
  integer literal of at least 1. The field is laid out as `T name[N]`.
- **A2. The Aura type of the field is `list[T]` (default).** The list
  must hold exactly `N` elements when the value crosses. Any other length
  fails with `AU4005` before the call. `array` names no type outside an
  `extern "C"` class or union, as `ptr` and `cstr` name none outside an
  extern signature.
- **A3. A class with an array field is not a `copy class` (default).**
  It holds a list, so it moves and clones like any class that holds one.
  A class that holds such a class is not Copy either. Classes of scalars
  stay Copy and keep their register-speed path.
- A4. A fixed-size array type in the language, with constant and dynamic
  indexing. Rejected for this work. It is a language feature with its own
  design, and a list gives the reader indexing, `len`, and iteration
  today.
- A5. A tuple of `N` elements. Rejected. A tuple has no indexing, and
  `char name[32]` would be a 32-tuple.

## B. Unions

- **B1. `extern "C" union Name:` with a block of members (default).**
  Every member starts at offset 0. The size is that of the largest
  member, rounded up to the strictest alignment. A member is a scalar, an
  `extern "C"` class, an array, or another union.
- **B2. A union value is its bytes (default).** In Aura the union is a
  class with one field, `bytes: list[uint8]`, which holds `sizeof` bytes.
  Reading a member reinterprets the leading bytes as that member's type
  and returns a new value. A member is a read-only property: it cannot
  be assigned, lent as `mut`, or viewed.
- **B3. Construction (default).** `Name()` is all zero bytes.
  `Name(member=value)` stores one member and zeroes the rest. Passing two
  members is a runtime failure. To change a union, construct a new one or
  write `bytes`.
- **B4. Crossing (default).** A union crosses by value, by `mut` pointer,
  as a field of a class, and as an array element, under the rules of a
  class. By value it is classified the way the platform classifies a C
  union.
- B5. Members as assignable fields. Deferred. It needs a place that
  aliases part of another place, which Aura's ownership rules do not
  have.
- B6. Mapping a union to an Aura enum. Rejected. A C union has no tag.
  Where the tag lives is the library's convention, and the binding
  author reads it and picks the member.

A `bool` member read from bytes other than 0 or 1 fails with `AU4001`,
as a `bool` field of a struct does.

## C. Arrays Of Structs

- **C1. `ptr[Name]` and `mut ptr[Name]` (default).** The argument is a
  `list[Name]`, where `Name` is an `extern "C"` class or union. The
  elements cross as a C array of `struct name`. No length crosses.
- **C2. The call converts the elements (default).** Aura keeps a class
  value as machine words, not in the C layout, so the call writes a C
  array before it and, for `mut`, reads it back after. The cost is linear
  in the list. A buffer of floats that must cross with no copy is an
  `Array[float32]`, which already passes its own storage.
- C3. Keeping `list[Name]` in the C layout so it crosses with no copy.
  Deferred. It changes the storage of every list of such a class, and
  the measured cost of C2 decides whether it is worth it.

## D. Pointers Inside Structs

- **D1. A pointer field stays `uint64` (default).** raylib passes
  `Image`, `Texture`, `Font`, and `Model` back by value, so the address
  is never followed. Code that must follow one reads through the `mem`
  module.
- D2. Opaque handle fields and typed pointer fields. Deferred. A handle
  is not Copy and has a `drop` function, and a struct that holds one
  needs rules for who releases it.

## E. Other Gaps

- **E1. `cstr` parameters in callbacks (default, after A to C).** The
  text is copied into a `str` for the call. This covers the GLFW and
  SDL log and error callbacks.
- E2. Variadic functions. Deferred. `TextFormat`, `TraceLog`, and
  `SDL_Log` format text, which an f-string already does. No drawing,
  window, or input function of the three libraries is variadic.
- E3. Closures as callbacks. Not in this work. First-class closures are
  being designed separately, and a C callback needs a `void *` slot the
  library hands back.

## F. Layout And Passing

`c_layout.rs` owns the layout of arrays and unions, as it owns structs.

- An array field has the alignment of its element and `N` times its size.
- A union has the strictest alignment and the largest size of its
  members.
- System V classifies each eightbyte from every scalar that overlaps it,
  across union members and array elements. Mixed integer and float is
  integer class.
- AArch64 treats an aggregate as a homogeneous float aggregate when every
  scalar in it is the same float type and it holds at most four of them,
  counted by size, so a union of `float` and `float[2]` is two floats.
- Windows passes by size, as it does a struct.

- **F1. AArch64 struct arguments that overflow the registers (default).**
  The generated call passes such a struct as one aggregate argument, and
  LLVM places it on the stack as the convention requires. The build
  error of FFI v1 goes away.

Classes of scalars keep the generated-code path of FFI v1. A class with
an array or a union, a union itself, and a struct array cross through one
shared routine in the runtime that both backends call, so they agree by
construction.

## G. The raylib Package

- **G1. `examples/packages/raylib` (default).** One binding module,
  generated from raylib's API description by a script in the package,
  and programs that use it: a brick game with input, a 3D scene with a
  camera, and an image written to a file.
- **G2. Tested where raylib is installed (default).** The test skips
  when `cc -lraylib` does not link, as the SQLite example does. It runs
  the image program with no window, and the window programs for a fixed
  number of frames when a display is present. A Linux CI job installs
  raylib and a virtual display and runs them.
- **G3. The system's raylib (default).** The package does not vendor or
  build raylib. The page says how to install it on each platform.
- G4. Publishing the binding as a package. Deferred with the registry.

## H. Order Of Work

1. The raylib package on the FFI of 0.16, with its test and CI job.
2. Array fields, unions, and struct arrays: layout, checker, both
   backends, acceptance tests against a C fixture library.
3. The raylib binding gains the functions step 2 unlocks. An SDL2 event
   loop and a GLFW key callback are acceptance tests where those
   libraries are installed.
4. `cstr` in callbacks.
5. Manual, book, ADRs, editor grammar, benchmarks against Rust and
   Python `ctypes`.
