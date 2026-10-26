# Rust Speed Step 2b: Specialized Generic Functions

## Goal

Step 2b of Phase 2 of the Rust-speed plan John approved on 2026-10-26
(decision 3): compile each generic user function once for each list of
concrete type arguments its calls use, so later steps can give the copies'
values typed layouts. The design for all of Phase 2 is in
`architecture_docs/24-typed-layouts-design-note.md`.

## Changes

- `mir_specialize.rs` runs in `mir::lower`, after `type_param_calls` and
  before runtime ops are resolved. It follows calls from every function,
  binds each generic callee's type parameters from the call's argument
  and destination types (keyword arguments by name, integer literals as
  `int64`, `T | None` against a union's other member), and copies the
  callee and the lambdas and default helpers it owns with the types
  substituted. A copy is named `function[arg, ...]`.
- A call keeps the generic original when a type argument is not concrete,
  an argument's type is not exactly its parameter's, a type argument nests
  more than 32 deep, a union would merge members, or the module has 4,096
  copies.
- `mir_methods.rs` resolves member calls and `with` exits that run a
  generic method to the copy for their receiver type, through the same
  binding, so resolution and validation agree.
- A `None` test on a value a copy made concrete becomes a constant or a
  union tag test.
- `public_function_name` drops bracketed type argument lists, so call
  chains name the source function on both backends. The native emitter
  knows a copied method's receiver kind from its function, and the
  initializer construction check reads the public name.
- `lower_implicit` does not specialize, so `aura run` is unchanged.

## Verification

- Unit tests in `mir_specialize_tests.rs` cover copies and their types,
  keyword arguments, lambdas, defaults, methods, `T | None`, union merges,
  the depth limit, and that the specialized module validates, interprets,
  and compiles.
- New fixtures: `run-pass/generic_functions_specialized.au` (functions,
  methods, `mut self`, a generic `with` manager, lambdas, defaults,
  keyword arguments, polymorphic recursion) and
  `run-fail/generic_copy_trap_names_its_function.au`.
- A returned view through a bounded trait method (`h.inner().right` with
  `h: T`) now types its intermediate loan from the trait method, so a copy
  checks the loan against the concrete type.
- Tests that exercise erased generic calls build their module with
  `mir::lower_unspecialized`, or call the erased original on purpose.
- Every run-pass fixture still emits: 96 of the 672 existing objects change (programs
  that call generic user or prelude functions), none fails.
- Backend parity (every runtime fixture on both backends) passes.
- `aura-compiler` lib tests pass except
  `package_io_helpers_cover_local_error_paths`, which fails only when the
  tests run as root.

## Measurements

Linux container, release builds of the 2a branch (before) and this change
(after).

- `hello_world`: 986,736 bytes before, 986,648 after. Cold build 0.38 s
  before, 0.34 s after (median of 5).
- Reference agent (`examples/agents/tool_runner`): cold build 1.02 s
  before, 0.93 s after (median of 5), within the 1.5 s gate. Binary
  1,913,008 bytes before, 1,873,544 after.
- Cross-language suite at the check size under callgrind: 27 programs
  within 0.1%. None of the 30 defines a generic function. The three
  threaded servers (`http_api`, `kv_server`, `pubsub_broker`) moved by up
  to 15% in one run, but their generated code is identical before and
  after (only addresses differ). Three more runs of each overlap:
  `http_api` 31.4M to 32.4M before and 31.1M to 32.5M after, `kv_server`
  83.9M to 85.6M and 82.4M to 84.5M, `pubsub_broker` 147.7M to 151.2M and
  148.9M to 150.7M.

