# raylib And FFI v2

Date: 2026-11-16

## Goal

Give Aura a first route to graphics: a maintained raylib package, and the
FFI additions that raylib, SDL, and GLFW need. The design is in
`architecture_docs/29-ffi-v2-graphics-design-checkpoint.md`.

## Plan

1. A raylib package on the FFI of 0.16, with tests and a CI step.
2. Array fields, unions, and arrays of structs in the checker, the
   runtime, and both backends.
3. The raylib binding gains what step 2 unlocks. SDL2 and GLFW acceptance
   tests.
4. `cstr` parameters in callbacks.
5. Manual, book, ADRs, editor grammar, and benchmarks.

## Step 1: the raylib package

- `examples/packages/raylib/tools/generate_binding.py` reads raylib's
  `raylib_api.json` and writes `src/raylib.au`: 29 structs, every enum
  value as an `int32` constant, the named colors as `Color` constants, and
  556 of raylib 6.0's 600 functions. The end of the module lists the 44
  it leaves out with the reason for each.
- raylib 6.0's API file has a description with unescaped quotation marks.
  The script escapes descriptions before it parses the file.
- `src/main.au` is a brick game, `src/scene.au` a 3D scene with a camera
  and a model built from a mesh, and `src/picture.au` draws into an image
  and writes a PNG with no window.
- `RAYLIB_DEMO_FRAMES` stops a window program after that many frames, and
  `RAYLIB_DEMO_SHOT` names a PNG it writes first. The tests use both.
- `crates/aura/tests/ffi_acceptance.rs` runs the three programs on both
  backends. The tests skip when `cc -lraylib` does not link, and the
  window programs also need `DISPLAY` on Linux or
  `AURA_RAYLIB_WINDOW_TESTS` elsewhere.
- `scripts/install-raylib.sh` builds raylib 6.0 on Linux and starts Xvfb
  under GitHub Actions, and installs Homebrew's raylib on macOS. The
  `verify` and `platform-tests` jobs run it.

What the 44 unbound functions need:

| Need | Functions |
| --- | --- |
| a struct with a fixed-size array field | 16 |
| a pointer to an array of structs | 16 |
| a callback raylib calls with a `va_list`, C text, or on its audio thread | 10 |
| a variadic parameter list | 2 |

Found on the way:

- On AArch64, a struct argument that goes to the stack because the float
  registers before it are taken is a build error. `DrawModelEx` and
  `DrawTexturePro` are such calls, so the scene uses `DrawModel` until
  step 2 removes the limit.

## Verification

- The three programs on both backends on x86-64 Linux under Xvfb, with
  raylib 6.0 built on its SDL platform layer. The saved frames were
  looked at.
- `cargo test -p aura --test ffi_acceptance maintained_raylib`.
- `python3 scripts/check_doc_examples.py`.

Not run: macOS, Windows, arm64, and raylib on its default GLFW platform
layer, which the CI step builds.

## Step 2: Array Fields And Arrays Of Structs

ADR-0106 records the decisions. What changed:

- `name: array[T, N]` in an `extern "C"` class. The parser records the
  lengths in `ClassDecl.c_arrays` and types the field `list[T]`. A class
  with such a field, or one that holds such a class, is not `Copy`.
- `ptr[Name]` and `mut ptr[Name]` for an `extern "C"` class. The call
  takes a `list[Name]`, and an empty list passes a null pointer.
- `crates/aura-runtime/src/c_values.rs` converts between Aura values and C
  bytes for any layout. The interpreter always uses it. Built programs
  keep their inline code for structs of plain scalars and call it through
  `aura_direct_ffi_pack_struct` and its siblings for everything else.
- On AArch64, a struct that does not fit the registers left is one LLVM
  array argument, which LLVM places on the stack as AAPCS64 requires.
- `CStruct::argument` no longer sends a struct to the stack because the
  registers of a kind it does not use have run out. Before, a built
  program passed `struct { int a, b; }` wrongly after nine `double`
  arguments. The interpreter was right because libffi places arguments
  itself.
- The binding generator emits array fields and struct pointers, and the
  binding covers 587 of 600 functions. The 13 left are 2 variadic
  functions, 10 that take callbacks, and one with a `Rectangle **`
  parameter.
- The picture draws a hill with `ImageDrawTriangleFan`, the game draws the
  ball's trail with `DrawLineStrip`, and the scene uses `DrawModelEx` and
  `DrawMesh` with `LoadMaterialDefault`, whose `Material` has a
  `float params[4]` field.

Verification for step 2:

- `cargo test -p aura-runtime --lib c_values c_layout native_foreign_calls`.
- `cargo test -p aura-compiler --lib`.
- `cargo test -p aura --test ffi_acceptance`: the fixture library tests
  compare both backends for array fields, nested arrays, struct arrays
  written back, wrong lengths, and structs after exhausted registers.
- The three raylib programs on both backends under Xvfb.

Not run: any arm64 or macOS machine. The generated AArch64 assembly was
read against AAPCS64. Speed of the runtime conversion path is not yet
measured; a list of structs is converted element by element on every
call.

## Step 3: Unions

ADR-0107 records the decisions. What changed:

- `extern "C" union Name:` parses into a class that holds
  `bytes: list[uint8]`, a property and a `from_` function for each member,
  and the private declarations those call. See
  `Parser::parse_extern_union`.
- `ffi_struct_layout` lays a union out from the fields of its
  `__Name_members` class, and the checker sets the length of the `bytes`
  default once layouts are known.
- An extern function with the symbol `aura.reinterpret` reads the C bytes
  of its one struct argument as its struct result. The interpreter does it
  in `ffi::reinterpret` and built programs in
  `compile_reinterpret`.
- Exported extern functions carry module-qualified names in their C
  layouts, so a value read from C in an importing module has the class
  name the importer knows.
- The editor analysis hides the generated declarations and lists a
  union's members in the outline. The language server's recovery outline
  and the VS Code grammar know `extern "C" union`.

Verification for step 3:

- `cargo test -p aura-compiler --lib` and `cargo test -p aura-runtime
  --lib`. Three compiler tests fail on this machine with or without the
  change: one needs a non-root user, and two float proofs need a newer Z3
  than the 4.8.12 installed here.
- `cargo test -p aura --test ffi_acceptance`: unions by value, by pointer,
  as fields, in arrays, and through `ptr[Name]` against the fixture
  library, the failure cases, and an SDL event loop.
- `node --test ./test/*.test.js` in `tools/aura-language-server`.

Not run: any arm64 or macOS machine. Unions of floats take a different
path there, which the fixture's `UHfa` union exercises in CI.

## Step 4: Text In Callbacks

The foreign declaration uses `cstr` for a NUL-terminated C string. The
named Aura function takes a `str`. Both backends copy and validate each
argument. The C fixture checks empty and multibyte strings, two text
arguments in one call, repeated callbacks, and the two `AU4005` failure
cases. The GLFW example exercises the error callback and passes its
window handle to the key callback as a `uint64`.

ADR-0108, the manual, chapter 18, and the changelog describe the rule.
The documentation generator is current. All 1,100 documentation blocks
passed the example checker on x86-64 Linux (666 check-pass, 195
check-fail and 239 fragments).

## Native Conversion And Cleanup

A callback that trapped while holding two C strings left two native boxes
allocated. The regression checks the native allocation counters. The entry
now frees each converted argument after the guarded body exits, including
when conversion of a later argument or the Aura function traps. The C fixture
finishes cleanup before Aura reports the trap. With two text arguments,
the native regression reports 11 boxes allocated and 11 freed.

A four-record array passed to C 64 times incurred 256 conversions from
native records into Aura values. Generated code checks the C and Aura layouts
and gives C a private copy of matching record storage. Other lists retain
the field-by-field conversion. Small fixed scalar arrays can be copied
directly from a typed class field after checking its storage and length.

The three-language benchmark is in
[`benchmarks/ffi_calls`](../benchmarks/ffi_calls/README.md#struct-arrays-array-fields-unions-and-text-callbacks).
The Linux host links raylib 6.0, GLFW, SDL2 and SQLite under Xvfb.
The full host suites and cross-platform CI are in progress.
