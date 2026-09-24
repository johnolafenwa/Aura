# CLI And Tooling

The `aura` command checks, runs, builds, and inspects Aura programs. It also
serves the editor tooling. This page lists each command and flag, its output,
its exit status, and the native build cache behind `run` and `build`.

In a repository checkout, run commands through Cargo:

```bash
cargo run -p aura -- check examples/classes/point_distance.au
```

In an installed environment, drop the Cargo prefix. The command shape is the
same:

```bash
aura check app.au
```

## Commands

| Command | Purpose |
| --- | --- |
| `aura check file.au` | Parse and type-check without executing. |
| `aura run file.au` | Execute through the MIR runtime, which is the default backend. |
| `aura run --backend mir\|direct\|auto file.au` | Choose the execution backend explicitly. |
| `aura run file.au -- args...` | Execute with program arguments available through `sys.args()`. |
| `aura build -o path file.au` | Build a native binary. |
| `aura ast file.au` | Print the syntax tree. |
| `aura ast-json file.au` | Print syntax tree JSON. |
| `aura mir file.au` | Print lowered MIR. |
| `aura analyze file.au` | Emit diagnostics, symbols, hover data, and definition data. |
| `aura complete --line N --character C --trigger . file.au` | Emit completion items. |
| `aura deps update [name]` | Refresh all git dependencies or one named dependency. |
| `aura new path` | Create `Aura.toml` and `src/main.au` without overwriting an existing path. |
| `aura fmt [--check] [paths...]` | Normalize Aura source whitespace or verify formatting. |
| `aura test [-k substring] [--format json] [--timeout-ms N] [paths...]` | Discover package-aware `.au` tests, select canonical case names, and report one result per case. Defaults to `tests/` and a 30-second per-case timeout. |
| `aura upgrade` | Download and run the verified installer to replace the compiler and bundled runtime with the latest published release. |
| `aura lsp` | Run the persistent JSON-lines compiler service used by the language server. |
| `aura help` / `aura --help` | Print usage. |
| `aura version` / `aura --version` | Print the build channel and 12-hex-digit source commit. Release archives print `aura 0.3.4-preview (0123456789ab)`. Source builds print `aura 0.3.4-dev (0123456789ab)`. |

MIR is Aura's mid-level intermediate representation. The editor query
commands `signature-help`, `references`, `prepare-rename`, and `rename` are
described in [Signature Help, References, And Rename](#signature-help-references-and-rename).

## Checking

`check` is the fastest way to validate syntax, types, imports, ownership, and
package resolution:

```bash
cargo run -p aura -- check examples/collections/list_basics.au
```

Use `check` before `run` when you edit a package or diagnose type errors.

## Running

`run` executes a source file through the MIR runtime:

```bash
cargo run -p aura -- run examples/control_flow/while_break_continue.au
```

Runtime diagnostics include source context where possible.

### Task Workers

By default, task execution uses the available parallelism that the host
reports. The provisional `AURA_WORKERS` environment override accepts a
positive integer, including a count larger than the host's available-core
count.

- `AURA_WORKERS=4 aura run app.au` selects four pinned task workers.
- `AURA_WORKERS=1` keeps single-worker cooperative execution through the same
  pinned-worker architecture.

MIR runs, forced-direct runs, and standalone direct binaries use the same
override. Empty, zero, signed, whitespace-padded, nonnumeric, and overflowing
values stop execution with `AU4006` and identify the raw invalid value.
Checking, analysis, completion, and formatting do not start the task runtime.

### Blocking Operations

Blocking host operations use a separate process-wide pool with two settings:

| Variable | Value | When absent |
| --- | --- | --- |
| `AURA_BLOCKING_WORKERS` | A positive integer. The runtime uses exactly that blocking worker count, without clamping. | The runtime uses available host parallelism, falls back to `4`, and clamps that derived default to `2..=8`. |
| `AURA_BLOCKING_QUEUE_CAPACITY` | A positive integer that bounds accepted jobs waiting in the pool's FIFO queue. Running jobs and callers waiting for admission do not use this capacity. | The pending queue is unbounded. |

MIR execution, direct execution, and launched standalone native binaries
validate both settings before any user code runs. Empty, zero, signed,
whitespace-padded, non-decimal, and overflowing values stop execution with
`AU4006`. The diagnostic names the setting and renders the supplied value. A
non-Unicode value is displayed lossily.

The pool follows these rules:

- The first runtime preflight reads both settings once. The configuration is
  then fixed for the life of the process.
- A valid preflight creates no blocking worker threads. The first submission
  creates the complete configured set. Production reuses it until the process
  exits, and Aura has no shutdown or join surface for it.
- A full bounded queue parks a lightweight task through the scheduler.
- A timeout or cancellation before queue insertion prevents submission. Once a
  job is inserted, its host work cannot be retracted, and any late result is
  discarded.
- The bound limits the accepted pending backlog, not running work or admission
  waiters. So it cannot guarantee progress for unrelated blocking I/O while
  all workers are stuck.

## Building

```bash
cargo run -p aura -- build --backend auto -o ./target/app app.au
cargo run -p aura -- build --backend direct -o ./target/app app.au
```

`--backend auto` is the default. It first tries the maintained direct backend.
When direct emission is unavailable, it may fall back to a native launcher that
embeds serialized MIR and the MIR runtime. `--backend direct` forbids that
fallback. Both forms produce standalone executables, and both must implement
the same checked language behavior.

An installed release archive finds its native runtime relative to `bin/aura`,
under `lib/aura`. It needs only a host C compiler for the final link. A binary
built from a source checkout falls back to Cargo-built runtime artifacts, for
contributor convenience.

### Keeping Native Symbols For Profiling

Set `AURA_NATIVE_KEEP_SYMBOLS=1` to skip the post-link `strip` step. This works
when you build a user executable or run with the direct backend. For example,
this command keeps the available symbols:

`AURA_NATIVE_KEEP_SYMBOLS=1 aura build --backend direct -o fib fib30.au`

The option follows these rules:

- Only the exact value `1` enables it. An absent or different value keeps the
  normal stripping policy.
- It is part of the native cache key, so profiling executables and normally
  stripped executables use separate cache entries.
- It applies to both direct binaries and generated MIR launchers.
- It changes symbol retention, not language behavior or source diagnostics.

It does not restore symbols already removed from the runtime archive. To
profile a source build, build the release compiler and runtime with
`CARGO_PROFILE_RELEASE_STRIP=none`, and keep native symbols during the user
build.

## Native Executable Profile

Workspace release builds of Aura 0.3.4 use `opt-level=3`, fat LTO, one codegen
unit, `debug=false`, and `strip="symbols"`. Panic unwinding stays enabled for
the runtime's `extern "C-unwind"` boundaries. This profile reduces shipped
compiler metadata. It does not strip the runtime archive's linkable symbols or
change the default MIR execution mode.

Direct builds and embedded-MIR launcher builds both request unused-section
removal: `-Wl,-dead_strip` on macOS and `-Wl,--gc-sections` on Linux. After a
successful link, Aura runs `strip -S -x` on the user executable. This removes
debug and local symbols but keeps globals for process-global FFI.

- The host C toolchain must supply `strip`. Missing or failing stripping is a
  build error.
- Aura's embedded source metadata keeps typed call frames, task ancestry, and
  source-located traps. Native debugger symbol information is reduced.
- No separate debug companion artifact is emitted.
- The runtime archive stays intact.

[Performance](performance.md#executable-size) records the executable-size
measurements and the current limits of these optimizations.

## Stdin Buffers

Editor-style commands can read source from stdin. The supplied path still sets
the package root and local imports:

```bash
cat examples/modules/simple_import.au | \
  cargo run -p aura -- analyze --stdin "$(pwd)/examples/modules/simple_import.au"
```

Stdin analysis and completion do not change package lockfiles.

## Analyze

`analyze` emits machine-readable data for editor tooling:

- diagnostics
- symbols
- hover information
- definition targets

The output is one JSON object with `diagnostics`, `symbols`, and `occurrences`
arrays. Positions are zero-based.

| Array | Entry fields |
| --- | --- |
| `diagnostics` | `code`, `line`, `start_character`, `end_character`, `message`, numeric `severity`, `secondary_spans`, `notes`, `help`, `edits`, and the always-present `call_frames` and `task_ancestry` arrays. |
| `symbols` | `name`, `kind`, `detail`, and recursive `children`. |
| `occurrences` | `hover` and an optional `definition` range, whose `file_path` may identify another module. |

Analysis frame spans use zero-based coordinates and an optional `file_path`.
An edit includes its range, replacement text, and applicability.

`analyze` exits successfully even when the JSON contains source diagnostics.
The request itself succeeded, and the diagnostics are data. The language
server prefers this compiler-backed analysis when it succeeds.

## Complete

`complete` emits completion items at a zero-based line and character position:

```bash
cargo run -p aura -- complete --line 12 --character 8 --trigger . app.au
```

The result is a JSON array of `{ "name": str, "kind": str, "detail": str }`
objects. `line` and `character` are zero-based. `--trigger` uses its first
character. The output is meant for tools, not people, but it helps when
debugging the LSP.

## Signature Help, References, And Rename

The compiler owns these queries. The language server advertises signature
help, references, and rename with prepare support. It forwards the current
source and discards cancelled or stale responses. Rename edits include the
document version.

```sh
aura signature-help --line 5 --character 22 --stdin /project/main.au
aura references --line 4 --character 18 --include-declaration --stdin /project/main.au
aura prepare-rename --line 5 --character 7 --stdin /project/main.au
aura rename --line 5 --character 7 --new-name answer --stdin /project/main.au
```

Each command reads source from stdin and returns JSON. They also accept a
source file in place of `--stdin <virtual-path>`. They do not change source or
package lockfiles.

| Command | Result | When refused or unresolved |
| --- | --- | --- |
| `signature-help` | The complete rendered contract, parameter labels, and active parameter. This includes stored `Callable`/`TaskCallable` values, keyword-only slots, and `= ...` defaults. | `null` |
| `references` | Occurrence ranges. | An empty array. |
| `prepare-rename`, `rename` | The selected range and edits, after the compiler rechecks that binding is preserved. | `null` |

Rename refuses builtins, keywords, imported-package definitions, and changes
that would collide or change binding. No lexical rename fallback is used.

## Machine-Readable And Inspection Formats

`ast-json`, `analyze`, `complete`, and `lsp` emit JSON.

- The `analyze` and `complete` shapes on this page are maintained tooling
  contracts for Aura 0.3.
- `ast`, `ast-json`, and `mir` expose compiler inspection data for people and
  tests. Their exact formatting and internal node and block shape are not a
  stable cross-version serialization API.

`aura lsp` is a persistent JSON-lines compiler service. Every request requires
`semantic_interface_version: 16` and the string fields `method`, `path`, and
`source`. `id` is optional and is echoed in the response. The supported
requests are:

```json
{"id":1,"semantic_interface_version":17,"method":"analyze","path":"/absolute/app.au","source":"print(1)\n"}
{"id":2,"semantic_interface_version":17,"method":"complete","path":"/absolute/app.au","source":"value.\n","line":0,"character":6,"trigger":"."}
```

Each response is one line. It contains the same `id`,
`semantic_interface_version: 16`, and either `result` or an `error` string.

- The path gives the virtual source its package and import context.
- Ranges and completion positions are zero-based.
- A missing or different semantic interface version is an incompatible
  request and returns a schema-mismatch error.

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
| completed human `test` run | `0` when every selected case passes; `1` otherwise; case output and summary on stdout, failures on stderr |
| completed JSON `test` run | `0` when every selected case passes; `1` otherwise; exactly one schema-version-1 document on stdout and no human progress lines |

A broken stdout pipe is a deliberate clean termination and exits `0`. This
lets commands feed consumers such as `head` without printing a secondary
failure.

## Testing

`aura test` finds test cases, runs each one, and reports one result per case.

**Inputs.** With no paths, it recursively reads `.au` files under `tests/`.
Given files or directories, it uses those inputs. Files are visited in
normalized path order.

**Discovery.** Within a file, module functions are discovered in declaration
order.

- A parameterless `test_*` function returning `None` is one case. Its
  canonical name is `path::test_name`.
- A module `test_*` function with parameters is an invalid test declaration.
- Class, trait, and implementation methods are not discovered.
- A file that declares no module `test_*` function is one case named `path`.
  It is entered through `main()` or its top-level statements.

**Filtering.** `-k substring` performs a literal, case-sensitive substring
match over the complete canonical case name. Selection happens after parameter
registrations are expanded, so the substring may select a bracketed case
label.

- `-k` may appear once, and its value must be non-empty.
- A valid filter that selects no cases succeeds with `0 passed; 0 failed`.
- A missing, empty, or repeated filter value is a usage error and exits with
  status 2.

### Setup And Teardown

A file may declare ordinary parameterless `setup()` and `teardown()` module
functions returning `None`. For each selected case, the runner calls setup,
then the case if setup succeeded, then teardown.

- Teardown runs after an attempted setup, even when setup traps.
- Teardown runs after a case trap or a non-zero file-level `main()` result.
- Teardown does not run during discovery.
- A hook with parameters, a non-`None` result, or a name that collides with a
  non-function declaration is a check-time test failure.

Setup, case, and teardown are isolated entries into one module that is already
checked and lowered. The runner does not re-read or re-check the source
between them. Aura values and module-runtime state do not pass between phases
or cases. External effects, such as file writes, stay observable.

The first failure is primary:

- When teardown also fails, human output prints `teardown also failed for ...`
  after the primary failure. JSON stores the teardown failure in the case
  record's `secondary` object with `stage: "teardown"`.
- If setup succeeds and only teardown fails, the teardown failure is primary.

The timeout covers the complete lifecycle. A timed-out worker cannot be
forcibly stopped, so teardown is not promised after a timeout.

### Parameterized Registration

A parameterized `test_*` function is parameterless and returns
`list[(str, def() -> None)]`. The runner calls it once during discovery and
expands its list in order. The canonical case name is
`path::test_name[label]`.

Each tuple holds:

- a non-empty label, unique within that registration
- a named, capture-free, repeatable, parameterless function returning `None`

The required `def() -> None` element type excludes capturing closures. It
keeps every expanded case independently invocable.

Registration finishes before filtering, and it never runs a returned case. It
never calls setup or teardown either. These are each one discovery failure for
the registration, and none of its cases run:

- a registration trap or timeout
- an invalid returned value
- an empty or duplicate label
- an invalid case function

An empty registration contributes no cases.

Registration stdout is captured once. Human mode writes all registration
stdout before the case results. JSON mode records non-empty registration
stdout in the top-level `discovery` array. Its entries contain `name`, `file`,
and `stdout`, in registration order.

### Test Output Contract

**Human mode.** Each case writes its captured stdout. A passing case then
writes `ok <canonical-name>`. A failed case writes `FAILED <canonical-name>`
to stderr, followed by its ordinary source diagnostic or a runner reason such
as a timeout. Standard output ends with `<passed> passed; <failed> failed`. Test
records and the summary keep canonical discovery order.

**JSON mode.** `aura test --format json` writes exactly one JSON document to
stdout and no human progress lines. The top-level object has:

- integer `schema_version: 1`
- a `summary` object with integer `selected`, `passed`, and `failed` counts
- an ordered `tests` array

Every test record contains `name`, `file`, `outcome` (`passed` or `failed`),
and a non-negative integer `duration_ms` that covers its complete lifecycle.
Non-empty captured output appears as `stdout`.

A failed record has exactly one primary failure form:

- A trapped test contains `diagnostic`, which uses the existing structured
  diagnostic schema, including optional assertion operand records.
- A runner failure contains `reason`.

A second teardown failure appears as `secondary`, with `stage` plus either
`diagnostic` or `reason`. Invalid command usage still goes to stderr and exits
2.

Assertions run normally in every mode. `aura test` has no option to strip
assertions.

## VS Code And LSP

The VS Code extension keeps one persistent `aura lsp` process. It provides
diagnostics, symbols, hover, go-to-definition, and completions through the
Language Server Protocol (LSP). Requests are debounced, cancellable,
version-guarded, and invalidated by dependency.

If the compiler process cannot start, a small lexical recovery layer provides
declarations and top-level completion. It does not duplicate compiler
semantics.

Compiler-backed method hover and completion details include the receiver
contract:

- a shared receiver renders as `self`
- a consuming receiver renders as `own self`
- a mutable receiver renders as `mut self`

Ordinary parameter signatures keep their bare, `own`, and `mut` spelling.
Built-in hover and completion detail shows retained-value contracts such as
`list.append(value: own T)`. Class field and enum payload completion detail
renders their implicit constructor ownership as `own`.

Useful repository commands:

```bash
npm run check:lsp
npm run test:lsp
npm run check:extension
npm run test:extension
```

## Documentation Site

Serve the VitePress book with:

```bash
npm run docs:dev
```

Build it with:

```bash
npm run docs:build
```

GitHub Pages builds use the same command with `VITEPRESS_BASE=/Aura/`, so
project-page asset URLs are rooted correctly.

## Repository Gates

The local repository gate is:

```bash
npm run ci
```

The gate checks:

- Rust formatting and Rust tests
- native/MIR parity
- LSP tests and coverage
- VS Code extension tests
- compiler coverage
- the docs build
- npm and RustSec audits
- Clippy, with warnings treated as errors
- repository hygiene

GitHub Actions runs the repository gate on Linux and macOS. The release
workflow publishes `v*` tag builds as GitHub Release assets. These include the
platform CLI archives, the packaged VS Code extension, and a static docs
archive.

## Grammar

The command line is a tooling protocol, not part of Aura source grammar. Its
maintained invocation forms are the command forms in the table above and the
usage text that `aura help` prints.

- The single-source compiler commands take either one `.au` path or their
  documented `--stdin <virtual-path>` form. The virtual path supplies module
  and package context, and standard input supplies the source text.
- `fmt` and `test` take their documented path lists instead.
- Only `aura run` accepts program arguments, after `--`.
- `check`, `run`, and `build` accept `--format human|json`. It does not change
  how source is parsed.

The [Grammar](/manual/grammar) governs the Aura source these commands accept,
not this page. Command names, options, output formats, and exit statuses are
case-sensitive.

## Typing Rules

`check`, `run`, and `build` use the same package resolver, parser, static
checker, and ownership checker. A program that fails those stages is not
executed or emitted. `analyze` exposes the same semantic model in a
recoverable, editor-oriented report. `complete` queries completion at a
zero-based source position. Inspection commands expose intermediate compiler
data but do not define additional source types.

For `check`, `run`, and `build`, JSON diagnostic mode has schema version `1`
and contains a `diagnostics` array. The compile pipeline stops at its first
failure. So a failed invocation contains exactly one diagnostic, and a
successful `check` contains none. Tools must not treat that cap as proof that
the rest of an invalid source file has no errors.

Each diagnostic carries its stable code, severity, message, optional primary
span, secondary spans, notes, help, machine-applicable edits, `call_frames`,
and `task_ancestry`.

- The frame arrays are always present.
- Call frames are ordered innermost first. Task ancestry is ordered youngest
  first.
- Every public schema-version-1 frame span has a required `path` and its own
  coordinates. Multi-file failures do not rely on the primary span's path.

The `analyze` and persistent-service representations carry the same semantic
diagnostic information in their zero-based editor-coordinate shapes. There,
`file_path` is optional only for source-only analysis.

## Runtime Semantics

`check` runs no program code. `run` executes checked MIR and forwards the
arguments after `--` to `sys.args()`. `build` emits a standalone host
executable. With `auto`, it tries the direct backend and may use the
MIR-launcher fallback. With `direct`, failure to emit directly is an error.
Both built forms must keep the checked language semantics.

Human-format `check` success writes exactly `ok` followed by a newline.
JSON-format success writes a schema-version-1 object with an empty diagnostic
array. [Output And Exit Status](#output-and-exit-status) gives the complete
stream and status rules.

## Ownership And Evaluation Order

The choice of CLI command or output format does not change Aura ownership,
borrowing, cleanup, or evaluation order. `run`, a directly built program, and
a MIR-launcher build must observe the same left-to-right source evaluation and
the same resource cleanup rules.

Tool-side writes are explicit:

- `fmt` without `--check`, `deps update`, and successful lockfile-producing
  package commands may write files.
- `analyze --stdin` and `complete --stdin` do not write a lockfile.

Source received through `--stdin` is not kept after the command or service
request. Its virtual path still matters for imports, module identity, and
diagnostic locations.

## Diagnostics

Compiler-backed commands can report any code in the append-only registry.
[Diagnostics](/manual/diagnostics) defines the structured schema and each
code's precise rules.

| Code | Meaning |
| --- | --- |
| `AU1001` | invalid lexical input |
| `AU1002` | invalid f-string delimiter |
| `AU1101` | invalid syntax |
| `AU2001` | name-resolution failure |
| `AU2002` | type mismatch |
| `AU2003` | unsupported operator |
| `AU2004` | argument-binding failure |
| `AU2005` | unsupported syntax or feature |
| `AU2006` | builtin method collision |
| `AU2007` | builtin function redefinition |
| `AU2008` | equality unavailable |
| `AU2010` | union member or context failure, including an extern union other than the `Handle \| None` result |
| `AU2011` | ambiguous union injection |
| `AU2012` | cyclic type alias |
| `AU2013` | union pattern coverage failure |
| `AU2014` | invalidated narrowing |
| `AU2015` | callable contract mismatch |
| `AU2999` | general compile-time rejection without a narrower code |
| `AU3001` | use of a moved value |
| `AU3002` | borrow violation |
| `AU3003` | mutability violation |
| `AU3004` | invalid ownership mode |
| `AU3005` | non-copy indexed read |
| `AU3006` | non-copy indexed compound assignment |
| `AU3007` | non-cloneable state duplication |
| `AU3008` | non-transferable task or Queue boundary |
| `AU3009` | single-consumer task-result duplication |
| `AU3010` | view escape or returned-view provenance failure |
| `AU3011` | collection mutation under a live element view |
| `AU4001` | general runtime trap |
| `AU4002` | arithmetic overflow or underflow |
| `AU4003` | bounds or lookup violation |
| `AU4004` | zero divisor |
| `AU4005` | resource, allocation, or I/O failure |
| `AU4006` | invalid runtime configuration |
| `AU4007` | numeric Array shape or reduction violation |

Human diagnostics render as `error[AU####]`, with source context when a span
is available. After the ordinary notes come readable call-chain and task-entry
notes, built from the typed frame arrays. These generated lines are not
repeated in the structured `notes` field.

`--format json` emits the schema-version-1 report on standard error for a
failing `check`, `run`, or `build`.

Some failures are CLI errors, not Aura-language diagnostics: usage errors,
missing command-line operands, and host failures that stop the tool itself
from starting. They print usage or a tool error and have no `AU####` code.

## Backend Support

All maintained execution routes share the parser, checker, package resolver,
diagnostic model, analysis engine, and MIR lowering.

| Backend | Behavior |
| --- | --- |
| `aura run --backend mir` | The default. Executes the lowered MIR. |
| `aura run --backend direct` | Builds a native binary with the direct backend and executes it. A build or launch failure is an error. A forced `direct` run never degrades, so a parity or benchmark caller cannot silently measure the other backend. |
| `aura run --backend auto` | Prefers the direct backend. Falls back to the MIR runtime only when direct building or launching is unavailable. |

Once a direct child runs under `auto`, its outcome is final and never triggers
MIR fallback. This covers an Aura trap, signal termination, a wait failure, and
a diagnostic-protocol failure. When a fallback happens, human mode prints the
reason on standard error before the MIR program runs. JSON mode includes it in
the final structured report after execution.

Every backend observes the same program arguments, standard output, exit code,
and complete runtime diagnostic, including typed call frames and task
ancestry.

`aura run` defaults to `mir` for the interactive edit-run path. `aura build`
defaults to `auto` for artifact production. `aura build --backend direct` uses
native direct emission, and `--backend auto` may select the checked
MIR-launcher fallback. Maintained measurements put a cold miss at about 1.3
seconds. A stripped native hello-world executable is 1,586,968 bytes; the
[Performance](/manual/performance#executable-size) chapter has the size table. Each cache hit reads, hashes, and privately materializes the
artifact. Workloads dominated by programs seen once, including CI, still pay
the cold path on every program.

### Direct Runs In JSON Mode

For `--format json` on maintained Unix hosts, the CLI supplies a private
trap-signal pipe and a separate diagnostic-data pipe bounded to 1,048,576
bytes.

- A native child that traps signals the trap and writes exactly one
  EOF-delimited, compiler-owned diagnostic JSON record. It suppresses human
  stderr only after that write succeeds.
- Native initialization owns both descriptors, marks them close-on-exec, and
  removes their internal environment entries before user code runs. An
  Aura-started subprocess cannot observe them or delay EOF.
- No signal or record is written for a normal `main` result, including status
  `1`. The CLI does not infer a trap from a process status or parse human text.
- A trap signal without one valid record is a hard host execution failure.

Human direct runs create no private protocol. The child renders its complete
human diagnostic.

### Native Cache

The native path is content-addressed. A successful direct build atomically
publishes three things into the cache: its binary, that artifact's SHA-256,
and a key-bound unique entry identity. The cache key covers:

- native cache format `v5`
- compiler-owned semantic-interface schema version `16`
- this compiler's version
- the host target
- the backend
- the exact linked runtime archive content
- its ordered native link arguments
- the complete lowered program, which already includes the entry source and
  every resolved dependency source

The format and semantic identities are independent key fields. Changing
compiler-owned type or ownership metadata invalidates artifacts even if the
native container format is still readable. Cache artifacts above 512 MiB are
not kept, and the just-built program still runs.

A later run with the same inputs requires a regular directory and bounded
regular sidecars. It verifies the entry identity, digest, artifact size,
execute permission, and platform-native executable shape before it uses the
entry. It then launches a private copy of exactly those verified bytes through
a native execution path with no shell fallback. Replacing the shared cache
pathname after verification cannot substitute different bytes.

These make the entry a cache miss: missing or mismatched metadata, truncation,
a non-regular member, a lost execute permission, or an executable-format or
architecture rejection. Aura then quarantines and removes that exact entry and
rebuilds before running.

Environmental launch failures are not evidence that verified cache bytes are
corrupt. These include a temporary-directory failure, a process-resource
failure, and a `noexec` mount. Aura keeps the entry, and reports the failure or
falls back according to the selected backend.

### Concurrent Builds

On maintained Unix hosts, processes coordinate cache setup with two kinds of
lock:

- A short runtime-identity lock protects source-checkout runtime discovery.
- A separate writer lock for each content key protects the
  miss/recheck/build/publish sequence.

So N concurrent cold runs of the same program perform one build. After it is
published, the other N-1 processes recheck and use the verified entry.
Existing verified hits take the optimistic read path and do not wait for a
writer that holds that key. Locks are released before the linked output runs.
Atomic publication and invalidation still ensure that readers never observe a
partial entry, and that a stale invalidator cannot delete a replacement
published for the same key.

### Progress Notices

In human mode, a native `run` flushes these exact lines:

- `aura: waiting for a concurrent build...` before it blocks on another
  builder
- `aura: building native program...` before it starts building a native
  program artifact

A source-checkout `aura build` flushes the same exact wait line before it
blocks on another process that is refreshing the shared runtime. The reporter
prints each notice at most once per invocation.

JSON `run` mode provisionally puts the one-document stderr contract ahead of
immediate progress. It buffers the same exact strings. It emits them in a
successful report's `progress` array or in a failed diagnostic's `notes`. A
JSON build failure also keeps a buffered wait notice in the diagnostic's
`notes`.

A successful `auto` run fallback also carries
`fallback: {"from":"direct","to":"mir","reason":"..."}`. A failed MIR fallback
keeps the direct failure and progress as diagnostic notes. Tools should not
expect real-time progress in JSON mode until a structured streaming contract
is ratified.

### Cache Directory

`AURA_CACHE_DIR` selects the cache directory. The default is
`~/.cache/aura/native`.

The directory is a trust boundary. Its colocated SHA-256 detects corruption,
but it does not authenticate bytes written by a hostile account. The root must
be private to the current OS user, and every writer with access to it must be
trusted. On the maintained Unix hosts, Aura rejects a root that another user
owns or that group or other can write. It creates or tightens accepted cache
directories to mode `0700`.

- Private launch copies are removed after the child exits.
- Each launch carries an inherited exclusive lease. Later cleanup keeps the
  directory while either the `aura` parent or the native child still uses it.
- Interrupted cache-publication, memo, and quarantine stages are collected
  only after their encoded 24-hour grace period, and only once their owner
  process is gone.
- An installed immutable runtime can still perform a direct build when
  caching is disabled or unavailable. No cache lock is needed merely to build,
  and the uncached artifact is not kept.

### Language Server Compatibility

The language server delegates semantic analysis and completion to the
persistent compiler service. Every JSON-lines request and response identifies
semantic-interface schema version `16`. A missing or different identity closes
the incompatible service. It also invalidates all document analysis before the
lexical recovery path is used, so cached function-type or ownership metadata
cannot cross compiler versions. The lexical fallback is recovery-only. It is
not a second language implementation.

### Backend Parity

Backend parity is a release gate. A construct accepted by one maintained
execution backend must have the same observable result or complete diagnostic
in the other. This includes frame records and their source paths, subject only
to the platform limits documented below. The parity harness performs no
MIR-specific frame-note masking.

## Limits And Implementation-Defined Behavior

- Native linking requires a supported host C compiler and the installed Aura
  runtime layout described above.
- `ast`, `ast-json`, and `mir` are inspection formats, not stable
  serialization APIs.
- The formatter normalizes the maintained whitespace surface. It is not a
  configurable style engine.
- `aura test` discovers tests by the `test_` name prefix, not by annotation.
- A timed-out test worker cannot be forcibly stopped inside the CLI process.

Filesystem path interpretation, process exit-code width, executable format,
linker selection, and the availability of Unix-only APIs follow the maintained
host platform. [Current Limits](/manual/current-limits) collects the package
graph, source-size, recursion, runtime, and backend limits.

## Status

The commands and contracts this page documents as maintained are implemented
in Aura 0.3. CLI, compiler, LSP, extension, backend-parity, and
repository-gate tests cover them. `analyze`, `complete`, and diagnostic schema
version `1` are maintained tooling contracts. Internal AST and MIR layouts are
deliberately unstable.

Aura 0.3 has no package registry, no publishing and installation workflow, no
Windows support, no configurable formatter, and no annotation-based test
discovery. Its maintained execution engines are the MIR runtime and the direct
native backend.

Design record:
[ADR-0031: CLI backend defaults](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0031-cli-backend-defaults.md).
