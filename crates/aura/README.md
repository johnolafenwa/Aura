# aura CLI

This package contains the Aurora bootstrap compiler CLI.

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
./target/release/aura run examples/control_flow/match_literals.au
./target/release/aura run examples/generics/box_and_wrapper.au
./target/release/aura run examples/basics/default_arguments.au
./target/release/aura run examples/collections/vec_basics.au
./target/release/aura run examples/collections/vec_polish.au
./target/release/aura run examples/collections/map_basics.au
./target/release/aura run examples/collections/set_basics.au
./target/release/aura run examples/basics/pass_keyword.au
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
./target/release/aura build -o ./target/aurora-point examples/point.au
./target/release/aura build --backend direct -o ./target/aurora-direct examples/basic_addition.au
./target/release/aura ast examples/classes/point_distance.au
./target/release/aura ast-json examples/classes/point_distance.au
./target/release/aura mir examples/control_flow/while_break_continue.au
./target/release/aura analyze examples/classes/point_distance.au
./target/release/aura complete --line 5 --character 11 --trigger . examples/point.au
```

You can do the same with the other current examples:

```bash
./target/release/aura run examples/basics/main_function.au
./target/release/aura run examples/basics/top_level_script.au
./target/release/aura run examples/collections/vec_iteration.au
./target/release/aura run examples/collections/vec_polish.au
./target/release/aura run examples/collections/map_basics.au
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

If you want to use `aura` without typing the full path, copy it into a directory on your shell `PATH`.

Example:

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

- `aura help`
  - print CLI usage and exit successfully
- `aura --version`
  - print the current CLI version and exit successfully
- `aura check <file.au>`
  - parse and type check a program
  - add `--format json` for the schema-versioned structured diagnostic document; human diagnostics remain the default
  - nested package modules can now be checked directly, with the CLI inferring the nearest package root that satisfies their imports
  - package entrypoints under `src/` now also resolve `Aurora.toml`, local path dependencies, git dependencies, workspaces, and `Aurora.lock`
- `aura deps update [package]`
  - refresh git dependencies for the current package or workspace and rewrite `Aurora.lock`
  - with no package name, all branch/tag/default-main git dependencies are refreshed
  - with a package name such as `util`, only that dependency is refreshed
- `aura run <file.au>`
  - run a program through the MIR runtime
  - this now includes the maintained `pass` statement and `sleep(duration)` builtin
  - the maintained user-facing surface now also includes explicit floor division, divisor-sign remainder, integer `.to_float()`, the expanded `String` utility and parsing surface, numeric helper builtins, `Vec[T]`, `Map[K, V]`, `Set[T]`, bounded `Queue[T]`, scheduler-aware text/binary file I/O plus the maintained socket/networking and shell-free process/supervisor surface through `io`, `fs`, `net`, and `process`, specialized generic trait bounds, and the current operator-trait subset
  - local file imports and `public` module boundaries now work for file-backed programs
  - manifest-rooted packages now also resolve sibling path dependencies, git dependencies, and workspace members when the entry file lives under a package `src/`
  - append `-- <program-args>...` to expose arguments through `sys.args()`
  - add `--format json` to select structured output when checking or execution fails
- `aura new <project-path>`
  - create `Aurora.toml` and `src/main.au`; existing paths are never overwritten
- `aura fmt [--check] [path ...]`
  - normalize line endings/trailing whitespace/final newlines, or verify without writing
- `aura test [--timeout-ms N] [path ...]`
  - run package-aware Aurora test programs; defaults to `tests/` and a 30-second per-file timeout
- `aura lsp`
  - run the persistent JSON-lines compiler service for editor tooling
- `aura build -o <output> <file.au>`
  - compile a standalone native binary for a program
  - this accepts `--backend auto|direct`
  - this also accepts `--format human|json` for compile and build diagnostics
  - `auto` is the default; it first tries the direct native backend and may fall back to a standalone embedded-MIR launcher when direct emission is unavailable
  - `direct` forces the new low-level native backend for the full currently implemented Aurora language surface
  - source-checkout builds can refresh the runtime through Cargo; packaged release builds use the bundled runtime and require only a host C compiler
  - file-backed and stdin-backed programs with local module imports and package dependencies now build correctly through this path
  - the maintained direct build path now also covers builtin scheduler-aware text/binary file I/O, poll-driven TCP/UDP/WebSocket/Unix/TLS socket I/O, higher-level HTTP helpers, and the shell-free `process` surface including supervised child processes with restart policies
- `aura ast <file.au>`
  - print the parsed syntax tree
- `aura ast-json <file.au>`
  - print the parsed syntax tree as JSON
- `aura mir <file.au>`
  - print the lowered MIR for the checked program
- `aura analyze <file.au>`
  - print machine-readable compiler analysis as JSON
  - file-backed and stdin-backed analysis now resolve local imports relative to the supplied path
  - nested package modules can now be analyzed directly without false import diagnostics
  - compiler-backed definitions now point across files for imported symbols instead of stopping at the importing file
- `aura complete --line <n> --character <n> [--trigger .] <file.au>`
  - print machine-readable completion items as JSON
  - `--line` and `--character` are zero-based
  - member completion expects the cursor to be positioned just after `.`
  - the CLI now tolerates the common incomplete-editor state where the buffer currently contains one or more dangling member accesses such as `counter.` or `helpers.math.`, including at EOF
  - local imported modules now participate in compiler-backed completions for both file-backed and stdin-backed buffers, including imported trait methods
- built binaries now preserve file, line, and caret context for arithmetic runtime failures such as division by zero

## Stdin Mode

Compiler-facing JSON commands still use stdin for editor integration, and the ordinary `check`, `run`, and `build` commands now honor the supplied stdin path when resolving local module imports.

Examples:

```bash
cat examples/classes/point_distance.au | ./target/release/aura analyze --stdin /virtual/point.au
cat examples/classes/point_distance.au | ./target/release/aura ast-json --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura complete --line 5 --character 11 --trigger . --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura build -o ./target/aurora-point --stdin /virtual/point.au
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
`run` and `build` use the same structure for failures. Each diagnostic carries
its code, severity, message, primary and secondary spans, notes, help, and
edits; editor tooling consumes the same compiler-owned fields.

## Current Limitation

Building the Aurora compiler itself still uses Cargo. Installed release archives are relocatable and do not use Cargo when compiling an Aurora program.

The supported build path today is:

1. build once with `cargo build -p aura --release`
2. use the resulting `aura` binary directly after that

The current `aura build` matrix is:

1. `--backend auto` is the default
2. `--backend direct` uses the true direct native backend for the full currently implemented Aurora language surface
3. built binaries no longer depend on the original `.au` source files at runtime
4. both backend paths need a host C compiler; installed archives load the bundled runtime and link manifest without Cargo or the source checkout

The maintained execution architecture is now:

1. `aura run` executes through the MIR runtime
2. `aura build --backend direct` requires direct native emission; `--backend auto` may instead package MIR with the native runtime when direct emission is unavailable
3. both execution paths now cover the maintained Aurora language surface, including builtin text/binary file I/O, shell-free subprocess helpers, plus TCP, UDP, HTTP, WebSocket, Unix-socket, and TLS networking
