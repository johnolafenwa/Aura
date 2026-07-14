# CLI And Tooling

The `aura` CLI is the product surface for checking, running, building, inspecting, and editor integration.

During repository development, commands are usually run through Cargo:

```bash
cargo run -p aura -- check examples/classes/point_distance.au
```

In an installed environment, the command shape is the same without the Cargo prefix:

```bash
aura check app.au
```

## Commands

| Command | Purpose |
| --- | --- |
| `aura check file.au` | Parse and type-check without executing. |
| `aura run file.au` | Execute through the MIR runtime. |
| `aura run file.au -- args...` | Execute with program arguments available through `sys.args()`. |
| `aura build -o path file.au` | Build a native binary. |
| `aura ast file.au` | Print the syntax tree. |
| `aura ast-json file.au` | Print syntax tree JSON. |
| `aura mir file.au` | Print lowered MIR. |
| `aura analyze file.au` | Emit diagnostics, symbols, hover data, and definition data. |
| `aura complete --line N --character C --trigger . file.au` | Emit completion items. |
| `aura deps update [name]` | Refresh all git dependencies or one named dependency. |
| `aura new path` | Create `Aurora.toml` and `src/main.au` without overwriting an existing path. |
| `aura fmt [--check] [paths...]` | Normalize Aurora source whitespace or verify formatting. |
| `aura test [--timeout-ms N] [paths...]` | Run package-aware `.au` test programs; defaults to `tests/` and a 30-second per-file timeout. |
| `aura lsp` | Run the persistent JSON-lines compiler service used by the language server. |
| `aura help` / `aura --help` | Print usage. |
| `aura version` / `aura --version` | Print version. |

## Checking

`check` is the fastest way to validate syntax, types, imports, ownership, and package resolution:

```bash
cargo run -p aura -- check examples/collections/vec_basics.au
```

Use `check` before `run` when you are editing a package or diagnosing type errors.

## Running

`run` executes a source file through the MIR runtime:

```bash
cargo run -p aura -- run examples/control_flow/while_break_continue.au
```

Runtime diagnostics include source context where possible.

## Building

```bash
cargo run -p aura -- build --backend auto -o ./target/app app.au
cargo run -p aura -- build --backend direct -o ./target/app app.au
```

`auto` is the default. It first attempts the maintained direct backend and may fall back to a native launcher that embeds serialized MIR and the MIR runtime when direct emission is unavailable. `--backend direct` forbids that fallback. Both forms are standalone executables and must implement the same checked language behavior.

An installed release archive resolves its native runtime relative to `bin/aura`, under `lib/aurora`, and needs only a host C compiler for the final link. A source-checkout binary falls back to Cargo-built runtime artifacts for contributor convenience.

## Stdin Buffers

Editor-style commands can read from stdin while using a supplied path for package roots and local imports:

```bash
cat examples/modules/simple_import.au | \
  cargo run -p aura -- analyze --stdin "$(pwd)/examples/modules/simple_import.au"
```

Stdin analysis and completion do not mutate package lockfiles.

## Analyze

`analyze` emits machine-readable data for editor tooling:

- diagnostics
- symbols
- hover information
- definition targets

The output is one JSON object with `diagnostics`, `symbols`, and `occurrences` arrays. Positions are zero-based. Diagnostics contain `code`, `line`, `start_character`, `end_character`, `message`, numeric `severity`, `secondary_spans`, `notes`, `help`, and `edits`; symbols contain `name`, `kind`, `detail`, and recursive `children`; occurrences contain `hover` and an optional `definition` range, whose `file_path` may identify another module. An edit includes its range, replacement text, and applicability.

`analyze` exits successfully even when the JSON contains source diagnostics: the request itself succeeded and the diagnostics are data. The language server prefers this compiler-backed analysis when it succeeds.

## Complete

`complete` emits completion items at a zero-based line and character position:

```bash
cargo run -p aura -- complete --line 12 --character 8 --trigger . app.au
```

Completion output is intended for tools, not humans, but it is useful when debugging the LSP.

The JSON result is an array of `{ "name": String, "kind": String, "detail": String }` objects. `line` and `character` are zero-based and `--trigger` uses its first character.

## Machine-Readable And Inspection Formats

`ast-json`, `analyze`, `complete`, and `lsp` emit JSON. The `analyze` and `complete` shapes described here are maintained tooling contracts for Aurora 0.1. `ast`, `ast-json`, and `mir` expose compiler inspection data for people and tests; their exact formatting and internal node/block shape are not a stable cross-version serialization API.

`aura lsp` is a persistent JSON-lines compiler service. Each input line is an object with an optional `id`, `method`, `path`, and `source`. Supported requests are:

```json
{"id":1,"method":"analyze","path":"/absolute/app.au","source":"print(1)\n"}
{"id":2,"method":"complete","path":"/absolute/app.au","source":"value.\n","line":0,"character":6,"trigger":"."}
```

Each response is one line containing the same `id` plus either `result` or an `error` string. Paths give the virtual source a package/import context; ranges and completion positions are zero-based.

## Output And Exit Status

| Outcome | Exit status and streams |
| --- | --- |
| help/version | `0`; result on stdout |
| malformed command usage | `2`; usage on stderr |
| `check` success | `0`; exactly `ok` plus a newline on stdout |
| compile, build, or runtime failure | `1`; rendered diagnostic on stderr |
| `run` with `main() -> None` | `0` |
| `run` with `main() -> int32` | the returned integer requested as the host process status |
| successful `analyze` containing source diagnostics | `0`; JSON on stdout |
| `test` with any failed program | `1`; summary on stdout and diagnostics on stderr |

A broken stdout pipe is intentional clean termination and exits `0`; this lets commands compose with consumers such as `head` without printing a secondary failure.

## VS Code And LSP

The VS Code extension keeps one persistent `aura lsp` process for diagnostics, symbols, hover, go-to-definition, and completions. Requests are debounced, cancellable, version-guarded, and invalidated by dependency. If the compiler process cannot start, a small lexical recovery layer provides declarations and top-level completion; it intentionally does not duplicate compiler semantics.

Compiler-backed method hover and completion details include the receiver
contract. They render shared receivers canonically as `self`, consuming
receivers as `own self`, and mutable receivers as `borrow mut self`. A source
declaration written with the explicit shared synonym `borrow self` therefore
appears as `self` in these signatures.

Ordinary parameter signatures preserve `own`, `borrow`, and `borrow mut`
spelling, and built-in hover/completion detail exposes retained-value contracts
such as `Vec.push(value: own T)`. Class field and enum payload completion detail
also renders their implicit constructor ownership as `own`.

Useful repo commands:

```bash
npm run check:lsp
npm run test:lsp
npm run check:extension
npm run test:extension
```

## Documentation Site

The VitePress book is served with:

```bash
npm run docs:dev
```

Build it with:

```bash
npm run docs:build
```

Validate the normative reference structure and navigation with:

```bash
npm run check:reference
```

GitHub Pages builds use the same command with `VITEPRESS_BASE=/Aurora/` so project-page asset URLs are rooted correctly.

## Repository Gates

The local repo gate is:

```bash
npm run ci
```

That gate checks Rust formatting, Rust tests, native/MIR parity, LSP tests and coverage, VS Code extension tests, compiler coverage, reference integrity, docs build, npm and RustSec audits, Clippy with warnings treated as errors, and repository hygiene.

GitHub Actions runs the repo gate on Linux and macOS. The release workflow publishes `v*` tag builds as GitHub Release assets, including platform CLI archives, the packaged VS Code extension, and a static docs archive.

## Grammar

The command line is a tooling protocol, not part of Aurora source grammar. Its maintained invocation forms are the command forms in the table above and the usage text printed by `aura help`. The single-source compiler commands use either one `.au` path or their documented `--stdin <virtual-path>` form; the virtual path supplies module and package context while standard input supplies the source text. `fmt` and `test` instead accept their documented path lists. `aura run` alone accepts program arguments after `--`. `--format human|json` is accepted by `check`, `run`, and `build` and does not change source-language parsing.

Aurora source accepted by these commands is governed by the [Grammar](/manual/grammar), not by this page. Command names, options, output formats, and exit statuses are case-sensitive.

## Typing Rules

`check`, `run`, and `build` use the same package resolver, parser, static checker, and ownership checker. A program that fails those stages is not executed or emitted. `analyze` exposes the same semantic model in a recoverable editor-oriented report, and `complete` queries completion at a zero-based source position. Inspection commands expose intermediate compiler data but do not define additional source types.

For `check`, `run`, and `build`, JSON diagnostic mode has schema version `1` and contains a `diagnostics` array. The current compile pipeline stops at its first failure, so a failed invocation contains exactly one diagnostic and a successful `check` contains none; tools must not treat that cap as proof that the rest of an invalid source file has no errors. Each diagnostic carries its stable code, severity, message, optional primary span, secondary spans, notes, help, and machine-applicable edits. The `analyze` and persistent-service representations carry the same semantic diagnostic information in their documented editor-coordinate shapes.

## Runtime Semantics

`check` performs no program execution. `run` executes checked MIR and forwards arguments after `--` to `sys.args()`. `build` emits a standalone host executable: `auto` tries the direct backend and may use the MIR-launcher fallback, while `direct` makes inability to emit directly an error. Both built forms must preserve the checked language semantics.

Human-format `check` success writes exactly `ok` followed by a newline. JSON-format success writes a schema-version-1 object with an empty diagnostic array. `analyze` returning source diagnostics is a successful tooling request and therefore exits `0`; malformed CLI usage exits `2`; compile, build, and runtime failures exit `1`. A successful `main() -> int32` requests that integer as the process status. The complete stream and status rules are in the table above.

## Ownership And Evaluation Order

Selecting a CLI command or output format does not alter Aurora ownership, borrowing, cleanup, or evaluation order. `run`, a directly built program, and a MIR-launcher build must observe the same left-to-right source evaluation and the same resource cleanup rules.

Tool-side mutations are explicit: `fmt` without `--check`, `deps update`, and successful lockfile-producing package commands may write files; `analyze --stdin` and `complete --stdin` do not write a lockfile. Source received through `--stdin` is not retained after the command or service request, but its virtual path remains semantically significant for imports, module identity, and diagnostic locations.

## Diagnostics

Compiler-backed commands can surface the complete append-only registry. `AU1001` means invalid lexical input; `AU1002` means an invalid f-string delimiter; and `AU1101` means invalid syntax. `AU2001` means name-resolution failure; `AU2002` means type mismatch; `AU2003` means unsupported operator; `AU2004` means argument-binding failure; `AU2005` means focused migration guidance; `AU2006` means a builtin handle method collision; and `AU2999` means a general compile-time rejection without a narrower code. `AU3001` means use of a moved value; `AU3002` means a borrow violation; `AU3003` means a mutability violation; `AU3004` means an invalid ownership mode; `AU3005` means a non-copy indexed read; and `AU3006` means a non-copy indexed compound assignment. `AU4001` means a general runtime trap; `AU4002` means arithmetic overflow or underflow; `AU4003` means a bounds or lookup violation; `AU4004` means a zero divisor; and `AU4005` means a trapping resource or I/O failure. The structured schema is defined in [Diagnostics](/manual/diagnostics).

Human diagnostics render as `error[AU####]` with source context when a span is available. `--format json` emits the schema-version-1 report on standard error for a failing `check`, `run`, or `build`. Usage errors, missing command-line operands, and host failures that prevent the tool itself from starting are CLI errors rather than Aurora-language diagnostics; they print usage or a tool error and have no `AU####` code.

## Backend Support

The parser, checker, package resolver, diagnostic model, analysis engine, and MIR lowering are shared by all maintained execution routes. `aura run` uses the MIR runtime. `aura build --backend direct` uses native direct emission, and `--backend auto` may select the checked MIR-launcher fallback. The language server delegates semantic analysis and completion to the persistent compiler service; its lexical fallback is recovery-only and is not a second language implementation.

Backend parity is a release gate. A construct accepted by one maintained execution backend must have the same observable result or diagnostic in the other, subject only to the platform limits documented below.

## Limits And Implementation-Defined Behavior

Native linking requires a supported host C compiler and the installed Aurora runtime layout described above. `ast`, `ast-json`, and `mir` are inspection formats, not stable serialization APIs. The formatter currently normalizes the maintained whitespace surface; it is not a configurable style engine. `aura test` runs `.au` programs as test units rather than discovering functions by annotation, and a timed-out worker cannot be forcibly stopped inside the CLI process.

Filesystem path interpretation, process exit-code width, executable format, linker selection, and availability of Unix-only APIs follow the maintained host platform. Package graph, source-size, recursion, runtime, and backend limits are collected in [Current Limits](/manual/current-limits).

## Status

The commands and contracts documented as maintained on this page are implemented in Aurora 0.1 and covered by CLI, compiler, LSP, extension, backend-parity, and repository-gate tests. `analyze`, `complete`, and diagnostic schema version `1` are maintained tooling contracts; internal AST and MIR layouts are intentionally unstable.

A package registry, publishing and installation workflow, Windows support, a
configurable formatter, and annotation-based test discovery are unavailable.
They are future work and are not part of this normative reference. Aurora has
no second legacy execution engine alongside the maintained MIR runtime and
direct native backend.
