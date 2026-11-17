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

## Follow-Up

Steps 2 to 5.
