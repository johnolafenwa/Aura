# ADR-0084: Foreign contracts are checked when the program runs, and `aura run` loads libraries privately

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0074 (FFI links libraries and calls C directly)
- Source: the 2026-11-10 security and bug review, findings HIGH-01 and MED-05

## Context

An `extern "C"` function may state `requires` and `ensures` clauses.
`aura verify` proved the `requires` at each call and assumed the `ensures`,
and nothing checked either when the program ran. The verifier cannot read C,
so a wrong `ensures` was taken as true, and `aura build
--remove-proved-checks` then removed bounds checks that the wrong clause
appeared to prove. Every other contract in the language is checked when the
program runs, so the foreign one was the only clause a run could silently
break.

`aura run` on the interpreter loaded each `[ffi]` library with
`RTLD_GLOBAL` and found symbols through the whole process. A foreign
declaration could therefore name a function of the `aura` program itself,
including the runtime's and LLVM's, and a library loaded for one program
stayed visible by name to the next in the same process. The loaded set was
keyed by library name alone, and versioned file names were tried in text
order, so `libfoo.so.10` sorted before `libfoo.so.2`.

## Decision

- **A foreign contract is checked at each call.** A call to an `extern "C"`
  function with a contract goes through a generated function,
  `name::__extern_contract`, that checks each `requires`, calls the C
  function, and checks each `ensures`. A clause that fails stops the program
  with `AU4009`, as any contract does. A function with no contract is called
  directly, as before, and costs nothing more.
- **`--remove-proved-checks` keeps these checks.** A proof may rely on a
  foreign `ensures` only because the run checks it.
- **`aura verify` is unchanged.** It still proves the `requires` and assumes
  the `ensures`. The lowering for verification calls the C function directly.
- **`aura run` loads each library privately.** Libraries load with
  `RTLD_LOCAL`. A symbol is looked up in the loaded libraries, in the order
  they were loaded, and then in the process. A symbol that resolves into the
  `aura` program or into LLVM is refused as not found.
- **A library is loaded once for each name and search path list**, and
  versioned file names are tried from the highest version down.

## Consequences

- A contract on a foreign function costs its clauses at every call. Leave the
  contract off a function called in a hot loop, or keep its clauses cheap.
- A library that depends on symbols another library exports globally,
  without naming that library as a dependency, loads in a built program and
  fails under `aura run`. Link the dependency properly, or build the program.
- Lookup is across all loaded libraries, not only those of the declaring
  package. Two libraries that export the same name resolve to the one loaded
  first.

## Alternatives considered

- **Keep foreign contracts trusted and document it.** That leaves a clause
  that removes checks with nothing behind it.
- **Check only the `ensures`.** A `requires` states what the C function
  needs to be safe to call, so a call that breaks it is the more dangerous
  one.
- **Keep `RTLD_GLOBAL` and filter symbol names.** A name filter cannot tell a
  library's symbol from the host's when they share a name.
