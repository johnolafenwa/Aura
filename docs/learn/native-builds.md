# Running And Shipping

Aurora has two execution paths: the MIR runtime behind `aura run`, and the native code generator behind `aura build`. They target the same language surface and are exercised by the same test suite, but they fit slightly different moments in a project.

## `aura run`

`aura run` parses, type-checks, lowers the program to Aurora's mid-level intermediate representation, and executes that representation. It is fast to start and shares code paths with the rest of the tooling, so compiler diagnostics, traces, and editor integrations behave the same as the code you are editing.

Use `run` for:

- iterating quickly while writing a program
- examples, smoke tests, and scratch files
- anything that lives in a test runner or script

## `aura build`

`aura build` compiles the program to a standalone native binary:

```bash
aura build -o ./app examples/basics/main_function.au
./app
```

The resulting binary is self-contained: it does not need the original `.au` source to run, and it does not re-invoke the compiler at launch. The build pipeline still needs the host C compiler to produce the artifact.

Use `build` when:

- you want a standalone executable you can ship or deploy
- you are validating native behaviour for a controlled deployment on the direct backend
- a program's runtime characteristics are part of what you are testing

## Backends

```bash
aura build --backend auto -o ./app app.au
aura build --backend direct -o ./app app.au
```

`auto` is the default. It first tries the direct native backend and may fall back to a standalone launcher that embeds checked MIR and the MIR runtime. Selecting `direct` explicitly forbids fallback and is useful when CI must prove direct emission remains available.

## Runtime Diagnostics

Built binaries embed source and frame metadata for runtime failures. A simple
failure at minimum renders its stable code, file, line, and caret:

```
error[AU4003]: vector index `10` is out of bounds for length `3`
 --> app.au:5:20
  |
5 |     x: int32 = values[10]
  |                      ^
```

Arithmetic traps, vector bounds errors, recursion-limit failures, and resource cleanup paths are expected to behave identically between `aura run` and the native binary. If you observe a difference, it is a bug worth reporting.

The frame data is captured once at the trap site, before runtime cleanup can
discard the active call/task state. Human output synthesizes readable
call-chain and, for child failures, task-ancestry notes from those typed
records. When `aura run
--backend direct --format json` launches the binary, a private bounded channel
returns the same schema-version-1 diagnostic to the CLI; tools never need to
parse the human text. The internal transport uses a separate trap marker so a
missing record is not confused with `main` returning status `1`; its
descriptors are hidden and close-on-exec before user code starts.

## A Checklist Before Shipping

Before a native binary goes anywhere important:

- Run `aura check` on the source.
- Run the program through `aura run` to confirm behaviour interactively.
- Build with `aura build` and run the binary against the same scenarios.
- For programs that do I/O or start processes, run them against the real resources — files that exist, sockets that are open, services that are reachable — in the built executable, not only through `aura run`.

Reference: [CLI And Tooling](/manual/cli-and-tooling).
