# Aura Language Server

This package is the Aura Language Server Protocol (LSP) implementation. It is
a thin JavaScript transport. Semantic analysis comes from the Aura compiler.

The server provides:

- completion items
- member completion after `.`
- document symbols
- hover
- go-to-definition
- document diagnostics
- signature help, references, and rename

Compiler-backed analysis supplies all of these. It also covers:

- contextual lambda parameter scope, captured-name navigation, callable hover,
  and closure ownership diagnostics
- progressively scoped comprehension targets, including hover, exact
  go-to-definition, nested-clause completion, and owned result-type inference
- incomplete comprehension clauses and filters, which keep exact `AU1101`
  diagnostics, broad recovery completions, and safe empty hover responses
- owned list and str slices, with compiler-owned result types, exact endpoint
  diagnostics, retained-source ownership analysis, and hover and navigation
  for names inside base and endpoint expressions
- incomplete or reserved slice forms, which keep the compiler's exact `AU2005`
  step and assignment guidance with no JavaScript-side reinterpretation
- global numeric `Array[T]` constructors, members, multidimensional indexing,
  first-axis slices, operator result types, and exact compiler-owned dtype and
  shape diagnostics
- extern C and opaque-handle symbols, hover, definitions, completions, and
  package-authorization diagnostics

## Compiler Service

The server starts one persistent compiler service:

- `aura lsp`

Requests and responses are newline-delimited JSON. Each carries the
compiler-owned `semantic_interface_version: 16`. Every request must include
that exact field:

```json
{"id":1,"semantic_interface_version":16,"method":"analyze","path":"/absolute/app.au","source":"print(1)\n"}
```

This identity is separate from the numeric schema version of the public
diagnostic document. If the compiler reports a missing or different semantic
identity, the transport:

- rejects and disposes that compiler
- invalidates all cached document analysis
- uses lexical recovery for the failed request

Responses are bounded to 16 MiB. With a matching compiler, the server:

- caches analysis per document version
- debounces changes
- cancels obsolete completion work
- guards asynchronous responses by document version
- invalidates only changed documents and their dependents

## Diagnostics

Compiler diagnostics keep their stable `AU####` code, related source spans,
notes, help, and machine-applicable edits through the LSP mapping. The bridge
does not classify or recreate semantic diagnostics on its own.

`Diagnostic.data` also carries the compiler-owned `call_frames` and
`task_ancestry` arrays. Compiler responses always include both.

- Frame spans use zero-based `line`, `start_character`, and `end_character`
  coordinates.
- Each frame keeps its optional `file_path`.
- The bridge does not parse human backtrace notes or reconstruct paths or
  ancestry.
- Compile-time diagnostics normally carry empty frame arrays. The populated
  shape is available to editor workflows that present runtime diagnostics.

A failed structured assertion may also carry an optional `assertion_operands`
array in `Diagnostic.data`. Each operand keeps the compiler-owned `label`,
`type`, rendered `value`, and `truncated` flag. The field is absent when the
diagnostic has no captured assertion operands. The bridge does not infer or
re-render values.

## Lexical Recovery

If the compiler process cannot start, a lexical recovery layer provides only:

- recovered top-level declarations, extern C functions, opaque handles, and
  nested method declarations
- top-level keywords, builtins, and recovered declaration completions
- same-file hover and definition for recovered declarations

Recovery has no semantic diagnostics or member inference by design. Compiler
recovery normally handles incomplete buffers. The JavaScript layer is a
recovery layer, not a second Aura type system.

## Development

From the repo root:

- `npm ci`
- `cargo build -p aura`
- `npm run check:lsp`
- `npm run test:lsp`
- `npm run coverage:lsp`

The build command provides `target/debug/aura` for work in this checkout. To
install the compiler-owned server binary on `PATH` for any Aura workspace, run:

```bash
cargo install --path crates/aura --locked --force
```

That installs the `aura` executable. The editor launches its `aura lsp`
subcommand over stdio, so you never run `aura lsp` yourself.

The VS Code extension bundles this package's JavaScript transport, which
starts `aura lsp`. After you install or build `aura`, rebuild, package, and
force-install the transport so VS Code does not keep an older local VSIX:

```bash
npm run package:extension
code --install-extension tools/vscode-aura/aura-language.vsix --force
```

Then run **Developer: Reload Window**. For a workspace outside this
repository, put `aura` on `PATH` or set `AURA_LSP_AURA_PATH` to its absolute
path before you launch VS Code.

## Architecture

| File | Role |
| --- | --- |
| `src/server.js` | LSP transport and request handlers |
| `src/compiler_bridge.js` | Owns the persistent compiler process and the machine-readable request lifecycle |
| `src/recovery.js` | Lexical recovery for when the compiler is unavailable, and nothing else |

Two rules shape the design:

- Diagnostics and navigation come from compiler-owned analysis.
- Recovery stays lexical, so semantic behavior has exactly one implementation.

## Signature help, references, and rename

The compiler owns these queries. The language server advertises signature
help, references, and rename with prepare support. It forwards the current
source and discards cancelled or stale responses. Rename edits include the
document version. The compiler refuses to rename builtins, keywords, and
imported-package definitions, and refuses any change that would collide or
change binding. There is no lexical rename fallback.

```sh
aura signature-help --line 5 --character 22 --stdin /project/main.au
aura references --line 4 --character 18 --include-declaration --stdin /project/main.au
aura prepare-rename --line 5 --character 7 --stdin /project/main.au
aura rename --line 5 --character 7 --new-name answer --stdin /project/main.au
```

Each command reads source from stdin and returns JSON.

| Query | Result |
| --- | --- |
| Signature help | The complete rendered contract, parameter labels, and the active parameter. This covers stored `Callable` and `TaskCallable` values, keyword-only slots, and `= ...` defaults. |
| References | Occurrence ranges. An unresolved query returns an empty array. |
| Rename | The selected range and edits, after the compiler rechecks that bindings are preserved. |

A refused signature-help or rename query returns `null`. These commands also
accept a source file in place of `--stdin <virtual-path>`. They never change
source files or package lockfiles.
