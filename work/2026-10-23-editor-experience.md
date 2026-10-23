# Editor Experience Pass

## Goal

John asked for every gap that made the VS Code extension and language server
a poor editing experience to be fixed. The audit found six areas:

1. one type error removed hover, definitions, references, and rename for the
   whole file;
2. a syntax error emptied the outline, only the first error was reported,
   and nothing warned about unused locals;
3. a VS Code started from the Dock could not find `aura` in `~/.local/bin`
   and fell back silently to keyword-only completion;
4. completion was missing after `import `, after `case Color.`, for a member
   typed without the `.` trigger, for a half-typed local, inside f-string
   braces, and for keyword arguments;
5. there were no quick fixes, inlay hints, semantic tokens, workspace
   symbols, document highlights, or implementations;
6. the extension had no Run, Build, or Test commands.

## Work Completed

- PR #119: completion recovers the statement under the cursor, so locals
  show while a name is half typed and inside f-string braces. The grammar
  marks f-string interpolations as code. Calls offer `name=` for unbound
  parameters.
- PR #121: compiler discovery in `~/.local/bin`, `~/.cargo/bin`,
  `/opt/homebrew/bin`, and `/usr/local/bin`; the `aura.compilerPath`
  setting; **Aura: Restart Language Server**; one-time warnings when the
  compiler is missing or speaks another interface version; untitled files.
- PR #123: analysis and completion replace failing statements and check the
  rest of the file, reporting each independent error; `AU5001` unused-local
  warnings; import, `case`, and trigger-less member completion.
- PR #124: semantic tokens, inlay hints, document highlights, quick fixes,
  workspace symbols, and go-to-implementation.
- This PR: Run, Build, Test, and workspace test commands, a Run lens above
  `def main`, and a Run button in the editor title.

## Verification

- `cargo test -p aura-compiler --lib analysis`, `cargo test -p aura-core`,
  and the CLI `analyze`, `complete`, and `lsp` tests.
- Language server tests at 100% line, branch, and function coverage with
  node's built-in coverage. `c8` does not run in the cloud container.
- Extension tests, including a bundled-server protocol test for the new
  requests.

## Follow-Up

- A debugger needs debug information in native builds. The Cranelift
  backend emits none, so this waits for the LLVM backend in Batch 7 step 9,
  which can emit DWARF.
