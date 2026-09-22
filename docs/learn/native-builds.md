# Running And Shipping

Aura has two execution paths. `aura run` uses the MIR runtime, and
`aura build` uses the native code generator. MIR is Aura's mid-level
intermediate representation. Both paths target the same language surface, and
the same test suite exercises both. They suit different moments in a project.

## `aura run`

`aura run` parses and type-checks the program, lowers it to MIR, and executes
the MIR. It starts fast and shares code paths with the rest of the tooling.
Compiler diagnostics, traces, and editor integrations therefore behave the
same way for the code you are editing.

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

The binary is self-contained. It does not need the original `.au` source to
run, and it does not call the compiler again at launch. The build still needs
the host C compiler to produce the binary.

Use `build` when:

- you want a standalone executable to ship or deploy
- you are validating native behaviour on the direct backend for a controlled
  deployment
- the program's runtime characteristics are part of what you are testing

## Backends

```bash
aura build --backend auto -o ./app app.au
aura build --backend direct -o ./app app.au
```

`auto` is the default. It first tries the direct native backend. It may fall
back to a standalone launcher that embeds checked MIR and the MIR runtime.
`direct` forbids fallback. Use it when CI must prove that direct emission
still works.

## Runtime Diagnostics

Built binaries embed source and frame metadata for runtime failures. Even a
simple failure shows its stable code, file, line, and caret:

```
error[AU4003]: list index `10` is out of bounds for length `3`
 --> app.au:5:20
  |
5 |     x: int32 = values[10]
  |                      ^
```

Arithmetic traps, list bounds errors, recursion-limit failures, and resource
cleanup paths should behave the same under `aura run` and in the native
binary. If you see a difference, report it as a bug.

The binary captures the frame data once, at the trap site. This happens
before runtime cleanup can discard the active call and task state. From those
typed records, the human output builds readable call-chain notes. For child
task failures, it also adds task-ancestry notes.

When `aura run --backend direct --format json` launches the binary, a private
bounded channel returns the same schema-version-1 diagnostic to the CLI.
Tools never need to parse the human text. The channel uses a separate trap
marker, so a missing record is not confused with `main` returning status `1`.
Its descriptors are hidden and set to close-on-exec before user code starts.

## A Checklist Before Shipping

Before a native binary goes anywhere important:

- Run `aura check` on the source.
- Run the program with `aura run` to confirm its behaviour interactively.
- Build it with `aura build` and run the binary against the same scenarios.
- For programs that do I/O or start processes, test the built executable
  against real resources, not only through `aura run`. Use files that exist,
  sockets that are open, and services that are reachable.

Reference: [CLI And Tooling](/manual/cli-and-tooling).
