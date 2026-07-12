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

The output is one JSON object with `diagnostics`, `symbols`, and `occurrences` arrays. Positions are zero-based. Diagnostics contain `line`, `start_character`, `end_character`, `message`, and numeric `severity`; symbols additionally contain `name`, `kind`, `detail`, and recursive `children`; occurrences contain `hover` and an optional `definition` range, whose `file_path` may identify another module.

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
