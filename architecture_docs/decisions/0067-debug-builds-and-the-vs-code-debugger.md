# ADR-0067: Debug builds and the VS Code debugger

- Status: Accepted
- Date: 2026-10-31
- Builds on: ADR-0064

## Context

John asked for a VS Code debugger on 2026-10-31. Aura native executables
carried no debug information: the LLVM backend wrote no DWARF, the build ran
the `O3` pipeline, and the link stripped the result. MIR instructions carried
no statement positions, so the backend had no line table to write.

## Decision

### `aura build --debug`

`aura build --debug -o out file.au` writes a debug build:

- Lowering puts a `SourceLocation { line, column }` instruction before each
  statement's instructions. It also puts a `SourceName { local, name }`
  instruction at the start of the entry block for each compiler-made local
  that holds a source variable, such as a `for` target or a rebound name.
  Only a debug build lowers these. Every pass, the interpreter, and the
  emitter's semantics ignore them. With them removed, a debug module is the
  module a normal build lowers. A test checks this for every run-pass
  fixture.
- The LLVM backend runs no pass pipeline. It writes code at
  `OptimizationLevel::None` in one codegen unit. Each Aura function gets a
  subprogram, and each instruction gets the location of the statement it
  came from. Each source variable held in one word gets a variable at its
  stack slot:
  - A scalar is a typedef of its Aura name, such as `int64` or `bool`.
  - A runtime value is the record `AuraValue`, holding the value's address,
    under a typedef of its Aura name, such as `list[str]`.
  - A plain class value is a record of its one-word fields, one fragment
    per field.
- The link keeps every section and skips `strip`. It forces in the runtime's
  `aura_debug_render`, which nothing else calls. On macOS, `dsymutil` writes
  `out.dSYM` before the objects are removed.

`aura_debug_render(value)` returns the text `print` writes, with a `str`
quoted. It never runs the program's `Display` code, and it answers instead of
failing. A null word reads `<unset>`.

### The extension

The extension contributes the `aura` debug type. A launch saves the file,
runs `aura build --debug` into the extension's storage directory, and starts
LLVM's `lldb-dap` on the result. It finds `lldb-dap` from `aura.lldbDapPath`,
then from `xcrun -f lldb-dap` on macOS, then from `lldb-dap`,
`lldb-dap-<version>`, or `lldb-vscode` on `PATH`. The launch passes
`debug/aura_lldb.py`, an lldb formatter that shows each `AuraValue` through
`aura_debug_render`. **Debug File**, a Debug lens above `def main`, and F5
with no `launch.json` all start a session for the open file.

## Consequences

- Breakpoints, stepping by statement, the call stack, scalars, class
  fields, and runtime values work in VS Code and in plain lldb.
- A union, a callable, and a class with a field of more than one word are
  not shown yet. A value the ownership pass has already released after its
  last use reads `<unset>` or null.
- lldb evaluates C expressions, not Aura ones.
- A normal build is unchanged. Its MIR, objects, and link are the same as
  before.

## Alternatives considered

- **A span on every MIR instruction.** This would touch every pass that
  builds instructions, and every serialized module would carry spans. The
  marker instructions cost nothing outside a debug build.
- **Keeping the objects on macOS for lldb's debug map.** The objects live in
  a temporary directory, and a moved binary would lose them. `dsymutil` makes
  a bundle that travels with the binary.
- **A debug adapter written in TypeScript.** `lldb-dap` already speaks the
  protocol and ships with Xcode and LLVM.
