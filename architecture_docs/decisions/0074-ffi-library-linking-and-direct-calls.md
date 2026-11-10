# ADR-0074: FFI links libraries and calls C directly

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0052 (the `Handle | None` result)

## Context

FFI v0 resolved every extern call at run time. Generated code handed a
serialized call description to one runtime function, which decoded it, looked
the symbol up with `dlsym`, built a libffi call, and boxed each argument and
result. A call of `labs` cost about 380 ns. A symbol had to be loaded into
the process already, so only the platform C library was reachable, and a
package could not wrap SQLite, BLAS, or any library of its own.

## Decision

- **A package names its libraries in the manifest.** `[ffi]` takes
  `libraries`, `library_paths`, and `frameworks`, and `[ffi.linux]` and
  `[ffi.macos]` take the same keys for one host. The settings of every
  package in the graph apply, dependencies first. They require
  `allow_ffi = true`.
- **Names are bare.** A library or framework name is ASCII letters, digits,
  `_`, `+`, `.`, and `-`, and does not start with `-` or `.`. The driver
  writes each as `-l<name>`, so a manifest cannot pass a linker option. A
  search path is joined to the manifest directory before it is passed.
- **Native code calls the symbol itself.** The emitter declares the C symbol
  and emits one call with the C types of the declaration. Narrow integer
  arguments carry `signext` or `zeroext`, and narrow results are extended
  after the call. The linker resolves the symbol, so a missing one is a link
  error.
- **Views and handles go through small runtime helpers.** A `str` or
  `list[uint8]` argument lends its own bytes for the call. A list that does
  not keep its bytes unboxed is copied for a shared view and moved to the
  unboxed form for a `mut` view. A `str` literal passes the program's
  constant bytes with no helper.
- **The interpreter keeps libffi.** It loads the manifest's libraries with
  `dlopen` before the program starts and looks symbols up at the call. MIR
  that names a library runs only through the package-checked path API,
  because loading a library runs its initializers.
- **`symbol "name"` renames.** A trailing clause on an extern function gives
  the C symbol when it differs from the Aura name. `symbol` stays an ordinary
  identifier elsewhere.

## Alternatives rejected

- **Keep run-time lookup and cache the resolved address.** It removes the
  `dlsym` but keeps a call through a pointer and the boxed marshalling, and a
  missing symbol stays a run-time failure.
- **A library name on each declaration.** One manifest list matches how the
  linker works, keeps link settings reviewable in one place next to
  `allow_ffi`, and lets a dependency carry its own.
- **Static linking settings.** Left out until a package needs one.

## Consequences

- A scalar foreign call costs under 1 ns over the C function on x86-64
  Linux, and a view argument a few nanoseconds.
- A built program fails to link when a library or symbol is missing.
  `aura run` falls back to the interpreter, which reports `AU4005`.
- Each `library_paths` directory is recorded as a run-time search path of
  the built program. Moving the program needs the library installed on the
  other machine.
- The native runtime no longer contains the call-description decoder.
