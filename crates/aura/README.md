# aura CLI

This package is the Aura bootstrap compiler CLI. It builds the `aura` binary.
The manual's
[CLI and Tooling](https://johnolafenwa.github.io/Aura/manual/cli-and-tooling)
chapter is the full command reference.

## Build The Binary

From the repository root:

```bash
cargo build -p aura --release
```

That produces the standalone binary at:

```text
target/release/aura
```

## Use The Binary Without Cargo

After the release build completes, run the binary directly:

```bash
./target/release/aura check examples/classes/point_distance.au
./target/release/aura run examples/classes/point_distance.au
./target/release/aura run --backend direct examples/classes/point_distance.au
./target/release/aura run examples/control_flow/match_literals.au
./target/release/aura run examples/generics/box_and_wrapper.au
./target/release/aura run examples/basics/default_arguments.au
./target/release/aura run examples/collections/list_basics.au
./target/release/aura run examples/collections/list_polish.au
./target/release/aura run examples/collections/list_algorithms.au
./target/release/aura run examples/collections/slices.au
./target/release/aura run examples/collections/dict_basics.au
./target/release/aura run examples/collections/set_basics.au
./target/release/aura run examples/basics/pass_keyword.au
./target/release/aura run examples/basics/assertions.au
./target/release/aura run examples/modules/simple_import.au
./target/release/aura run examples/packages/local_path_dependencies/app/src/main.au
./target/release/aura run examples/packages/workspace/app/src/main.au
./target/release/aura run examples/traits/greeter.au
./target/release/aura run examples/traits/generic_trait_impl.au
./target/release/aura run examples/traits/generic_trait_bounds.au
./target/release/aura run examples/traits/operator_traits.au
./target/release/aura run examples/traits/ordering_traits.au
./target/release/aura run examples/traits/specialized_trait_dispatch.au
./target/release/aura run examples/basics/numbers.au
./target/release/aura run examples/numbers/numeric_casts.au
./target/release/aura run examples/numbers/numeric_builtins.au
./target/release/aura run examples/numbers/numeric_arrays.au
./target/release/aura run examples/strings/string_methods.au
./target/release/aura run examples/strings/string_parsing_and_formatting.au
./target/release/aura run examples/io/read_text_file.au
./target/release/aura run examples/io/bytes_file_io.au
./target/release/aura run examples/io/process_run.au
./target/release/aura run examples/io/process_pipes.au
./target/release/aura run examples/io/process_supervisor.au
./target/release/aura run examples/io/tcp_echo.au
./target/release/aura run examples/io/tcp_bytes.au
./target/release/aura run examples/io/udp_echo.au
./target/release/aura run examples/io/http_roundtrip.au
./target/release/aura run examples/io/websocket_roundtrip.au
./target/release/aura run examples/io/unix_tls_roundtrip.au
./target/release/aura run examples/concurrency/sleep_builtin.au
./target/release/aura run examples/concurrency/yield_now.au
./target/release/aura build -o ./target/aura-point examples/point.au
./target/release/aura build --backend direct -o ./target/aura-direct examples/basic_addition.au
./target/release/aura ast examples/classes/point_distance.au
./target/release/aura ast-json examples/classes/point_distance.au
./target/release/aura mir examples/control_flow/while_break_continue.au
./target/release/aura analyze examples/classes/point_distance.au
./target/release/aura complete --line 5 --character 11 --trigger . examples/point.au
```

The other maintained examples run the same way:

```bash
./target/release/aura run examples/basics/main_function.au
./target/release/aura run examples/basics/top_level_script.au
./target/release/aura run examples/collections/list_iteration.au
./target/release/aura run examples/collections/list_polish.au
./target/release/aura run examples/collections/list_algorithms.au
./target/release/aura run examples/collections/slices.au
./target/release/aura run examples/collections/dict_basics.au
./target/release/aura run examples/collections/set_basics.au
./target/release/aura run examples/generics/box_and_wrapper.au
./target/release/aura run examples/traits/greeter.au
./target/release/aura run examples/basics/numbers.au
./target/release/aura run examples/numbers/numeric_casts.au
./target/release/aura run examples/numbers/numeric_builtins.au
./target/release/aura run examples/strings/string_methods.au
./target/release/aura run examples/io/read_text_file.au
./target/release/aura run examples/io/bytes_file_io.au
./target/release/aura run examples/io/process_run.au
./target/release/aura run examples/io/process_pipes.au
./target/release/aura run examples/io/process_supervisor.au
./target/release/aura run examples/io/tcp_echo.au
./target/release/aura run examples/io/tcp_bytes.au
./target/release/aura run examples/io/udp_echo.au
./target/release/aura run examples/io/http_roundtrip.au
./target/release/aura run examples/io/websocket_roundtrip.au
./target/release/aura run examples/io/unix_tls_roundtrip.au
./target/release/aura run examples/concurrency/bounded_queue.au
```

## Install The Binary Somewhere On Your Path

To run `aura` without the full path, copy it into a directory on your shell
`PATH`:

```bash
mkdir -p "$HOME/.local/bin"
cp target/release/aura "$HOME/.local/bin/aura"
```

Then run:

```bash
aura run examples/classes/point_distance.au
aura help
aura --version
aura deps update
aura deps update util
```

## Command Summary

| Command | What it does |
| --- | --- |
| `aura help` | Prints CLI usage and exits successfully. |
| `aura --version` | Prints the build channel and 12-hex-digit source commit, then exits successfully. A release archive prints `aura 0.3.4-preview (0123456789ab)`. A source build prints `aura 0.3.4-dev (0123456789ab)`. |
| `aura check <file.au>` | Parses and type-checks a program. |
| `aura run [--backend mir\|direct\|auto] <file.au> [-- <program-args>...]` | Runs a program. The MIR runtime is the default. MIR is the compiler's mid-level intermediate representation. |
| `aura build -o <output> <file.au>` | Compiles a standalone native binary. |
| `aura deps update [package]` | Refreshes git dependencies for the current package or workspace and rewrites `Aura.lock`. |
| `aura new <project-path>` | Creates `Aura.toml` and `src/main.au`. It never overwrites an existing path. |
| `aura fmt [--check] [path ...]` | Normalizes line endings, trailing whitespace, and final newlines. `--check` verifies without writing. |
| `aura test [--timeout-ms N] [path ...]` | Runs package-aware Aura tests. |
| `aura lsp` | Runs the persistent JSON-lines compiler service for editor tooling. |
| `aura ast <file.au>` | Prints the parsed syntax tree. |
| `aura ast-json <file.au>` | Prints the parsed syntax tree as JSON. |
| `aura mir <file.au>` | Prints the lowered MIR for the checked program. |
| `aura analyze <file.au>` | Prints machine-readable compiler analysis as JSON. |
| `aura complete --line <n> --character <n> [--trigger .] <file.au>` | Prints machine-readable completion items as JSON. |

### Checking

- Add `--format json` for the schema-versioned structured diagnostic document.
  Human diagnostics are the default.
- You can check a nested package module directly. The CLI infers the nearest
  package root that satisfies its imports.
- Package entrypoints under `src/` resolve `Aura.toml`, local path
  dependencies, git dependencies, workspaces, and `Aura.lock`.

### Dependencies

- With no package name, `aura deps update` refreshes every branch, tag, and
  default-main git dependency.
- With a package name such as `util`, it refreshes only that dependency.

### Running

`aura run` executes the maintained user-facing surface, including:

- the `pass` and `assert` statements
- the `sleep(duration)` and `yield_now()` builtins
- explicit numeric and Duration floor division, signed computed Duration
  values, and integer `.to_float()`
- the expanded `str` utility and parsing surface, and numeric helper builtins
- `list[T]` with stable sorting and eager callable-powered map and filter,
  `dict[K, V]`, and `set[T]`
- `control.retry`, and deterministic and OS-secure randomness through `random`
- bounded `Queue[T]`, structural `Transfer` checks on task and Queue
  boundaries, and single-consumer non-repeatable task results
- scheduler-aware text and binary file I/O, the socket and networking surface,
  and the shell-free process and supervisor surface through `io`, `fs`, `net`,
  and `process`
- specialized generic trait bounds and the current operator-trait subset

Local file imports and `public` module boundaries work for file-backed
programs. When the entry file lives under a package `src/`, manifest-rooted
packages resolve sibling path dependencies, git dependencies, and workspace
members.

- Append `-- <program-args>...` to expose arguments through `sys.args()`.
- Add `--format json` to get structured output when checking or execution
  fails.

The `--backend` flag selects how the program runs:

| Backend | Behavior |
| --- | --- |
| `mir` | Executes the lowered MIR. This is the default. |
| `direct` | Builds a native binary and runs it. Build or launch failures are reported, never degraded. |
| `auto` | Prefers `direct` and degrades to the MIR runtime. Human mode prints the reason before the fallback program runs. JSON mode includes the reason in the final structured report. |

### Native Cache

Successful native builds from `aura run` are cached by content under
`AURA_CACHE_DIR`. The default is `~/.cache/aura/native`.

- Every hit verifies the entry identity, the artifact SHA-256, the
  regular-file and execute state, the size bound, and the executable shape.
  Aura then launches a private copy of those verified bytes, with no shell
  fallback.
- On the maintained Unix hosts, concurrent cold runs of the same content key
  coordinate through cross-process locks. One process builds and atomically
  publishes the entry. The others wait, then reuse the verified result.
  Established warm hits do not wait on that key's writer lock.
- Human output flushes `aura: waiting for a concurrent build...` before
  blocking and `aura: building native program...` before building a native
  program artifact.
- JSON mode buffers these notices so stderr stays exactly one JSON document.
  It reports them through `progress` on success or diagnostic `notes` on
  failure. An `auto` fallback also records its direct-to-MIR transition and
  reason in `fallback`.
- Malformed entries and executable-format or architecture failures are
  discarded and rebuilt. Temporary-directory, process-resource, and other
  environmental launch failures keep the verified entry. They follow the
  selected backend's ordinary error and fallback policy.
- The cache directory is a trust boundary. Use only a location private to the
  current OS account. On the maintained Unix hosts, Aura rejects roots owned
  by another user or writable by group or other.
- Cache keys include the native cache format `v6`, the semantic-interface
  schema `v16`, the exact linked runtime archive, and the ordered native link
  arguments, each as a separate component.
- Inherited launch leases and owner-aware staging cleanup stop an interrupted
  run's cleanup from deleting a live native child.
- Caching is optional for an installed immutable runtime layout. An empty or
  unavailable cache does not block an otherwise valid direct build, but a
  later run cannot reuse that build.

### Building

`aura build` accepts `--backend auto|direct` and `--format human|json` for
compile and build diagnostics.

| Backend | Behavior |
| --- | --- |
| `auto` | The default. Tries the direct native backend first. If direct emission is unavailable, it may fall back to a standalone embedded-MIR launcher that packages MIR with the native runtime. |
| `direct` | Forces the low-level native backend, which covers the full implemented Aura language surface. |

- Built binaries run without the original `.au` source files.
- Both backends need a host C compiler.
- A source-checkout build can refresh the runtime through Cargo. In human
  mode, it flushes `aura: waiting for a concurrent build...` before blocking
  on another process that is refreshing the shared native runtime.
- Packaged release builds use the bundled runtime and link manifest. They need
  only a host C compiler, with no Cargo or source checkout.
- File-backed and stdin-backed programs with local module imports and package
  dependencies build through this path.
- The direct build path covers scheduler-aware text and binary file I/O,
  poll-driven TCP, UDP, WebSocket, Unix-socket, and TLS socket I/O, the
  higher-level HTTP helpers, and the shell-free `process` surface, including
  supervised child processes with restart policies.

`AURA_NATIVE_KEEP_SYMBOLS=1` skips post-link stripping for profiling. The same
setting selects a separate native-cache entry for direct runs. Only the exact
value `1` enables it. To keep the runtime's own symbols, build the runtime with
`CARGO_PROFILE_RELEASE_STRIP=none`. This option cannot restore symbols already
stripped from the archive.

### Testing

`aura test` defaults to `tests/` and a 30-second per-test timeout.

- A file that declares `def test_*()` functions reports one result per
  function, labelled `path::function`.
- A file that declares none reports one result for the path.

### Editor Service

Every `aura lsp` request and response carries the compiler-owned
semantic-interface version `16`. A schema mismatch closes the connection before
analysis.

### Analysis And Completion

`aura analyze`:

- resolves local imports relative to the supplied path, for file-backed and
  stdin-backed analysis
- analyzes nested package modules directly, without false import diagnostics
- returns compiler-backed definitions that point across files for imported
  symbols

`aura complete`:

- takes zero-based `--line` and `--character` values
- expects the cursor right after `.` for member completion
- tolerates a buffer with one or more dangling member accesses, such as
  `counter.` or `helpers.math.`, including at end of file
- includes local imported modules in completions for file-backed and
  stdin-backed buffers, including imported trait methods

### Editor Queries

The CLI also answers `signature-help`, `references`, `prepare-rename`, and
`rename`. Each takes `--line` and `--character` and either a source path or
`--stdin <virtual-path>`. Rename takes `--new-name`. References optionally
takes `--include-declaration`. Results are compiler-owned JSON. A refused
rename returns `null` and makes no edits.

## Stdin Mode

The compiler-facing JSON commands read stdin for editor integration. The
ordinary `check`, `run`, and `build` commands also read stdin, and they use the
supplied path to resolve local module imports.

```bash
cat examples/classes/point_distance.au | ./target/release/aura analyze --stdin /virtual/point.au
cat examples/classes/point_distance.au | ./target/release/aura ast-json --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura complete --line 5 --character 11 --trigger . --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura build -o ./target/aura-point --stdin /virtual/point.au
cat examples/modules/simple_import.au | ./target/release/aura analyze --stdin "$(pwd)/examples/modules/simple_import.au"
cat examples/modules/simple_import.au | ./target/release/aura check --stdin "$(pwd)/examples/modules/simple_import.au"
cat examples/modules/simple_import.au | ./target/release/aura run --stdin "$(pwd)/examples/modules/simple_import.au"
./target/release/aura check examples/packages/local_path_dependencies/app/src/main.au
./target/release/aura run examples/packages/workspace/app/src/main.au
```

## Diagnostics

When a compiler-facing command fails, the default human renderer prints:

- the stable `AU####` diagnostic code and error message
- file, line, and column
- the relevant source line
- a caret under the failure location
- labeled related spans, notes, help, and machine-applicable fixes when present

`aura check --format json` emits `{"schema_version":1,"diagnostics":[...]}`.
`run` and `build` use the same structure for failures. Each diagnostic carries:

- its code, severity, and message
- primary and secondary spans
- notes, help, and edits
- `call_frames`, innermost first
- `task_ancestry`, youngest first

The frame arrays are always present, even when empty. Editor tooling consumes
the same compiler-owned fields.

At runtime, MIR and directly generated native failures keep the same typed
Aura call frames and child-task ancestry. Human output renders them as
call-chain and task notes. JSON output exposes the `call_frames` and
`task_ancestry` arrays, so tools do not need to parse prose. Built binaries
keep file, line, and caret context for arithmetic runtime failures such as
division by zero.

For `aura run --format json --backend direct`, the CLI gives the native child
two internal channels: a private trap-signal pipe and a separate bounded
diagnostic-data pipe.

- The child signals and writes one compiler-owned JSON record only when Aura
  traps. An ordinary nonzero `main` result writes neither.
- Native initialization hides both descriptors from user code and marks them
  close-on-exec.
- JSON mode can therefore tell a program that returns status `1` from an
  `AU####` runtime failure without scraping human stderr.
- A signalled but missing or malformed record is a hard post-launch failure,
  not an `auto` fallback.

Human direct runs create no private channel and keep the native renderer.

## Current Limitation

Building the Aura compiler itself uses Cargo. Build once with
`cargo build -p aura --release`, then use the resulting `aura` binary
directly.

Installed release archives are relocatable. They do not use Cargo when they
compile an Aura program.
