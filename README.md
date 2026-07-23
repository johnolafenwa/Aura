# Aurora

Aurora is a systems programming language designed to have Python-like simplicity, the memory safety of Rust, and lightweight structured concurrency.

The goal is to build a systems programming language that is easy to learn and very effective for building agents and ML infrastructure.

Aurora 0.1 is an advanced technical preview, not a production release. The canonical implemented contract begins with the normative [Language Specification](docs/manual/language-specification.md), [complete grammar](docs/manual/grammar.md), and Manual; the original language proposal is historical design material. Supported hosts and pinned tools are listed in [SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md).

## Monorepo layout

This repository is intended to evolve as a monorepo for the Aurora language and its associated tools.

- `crates/`
  - Rust compiler, runtime, and CLI tooling
- `tools/`
  - editor integrations and other developer tools
- `package.json`
  - npm workspace manifest for repo-managed tools
- `examples/`
  - categorized sample Aurora programs
- `tutorials/`
  - Markdown tutorials covering the implemented language subset
- `docs/`
  - VitePress book, language proposal, and supporting documentation
- `architecture_docs/`
  - implementation-focused architecture and component deep dives for the current Aurora system
- `work/`
  - persistent task board and implementation notes

Compiler build and direct binary usage are documented in [crates/aura/README.md](crates/aura/README.md).
Compiler library testing notes live in [crates/aurora-compiler/README.md](crates/aurora-compiler/README.md).
The categorized example library is documented in [examples/README.md](examples/README.md).
The tutorial track lives in [tutorials/README.md](tutorials/README.md).
The VitePress book lives in [docs/index.md](docs/index.md) and includes the guided Learn track plus the normative language and API reference.
The repo testing strategy is documented in [docs/testing_strategy.md](docs/testing_strategy.md).
The forward-looking ML systems roadmap lives in [docs/ml_systems_support_plan.md](docs/ml_systems_support_plan.md).
The implementation architecture guide lives in [architecture_docs/README.md](architecture_docs/README.md).

Current editor tooling:

- `tools/vscode-aurora`
  - VS Code extension for Aurora syntax highlighting and LSP client integration
- `tools/aurora-language-server`
  - Aurora Language Server Protocol implementation

Current compiler workflow:

- `cargo run -p aura -- check examples/classes/point_distance.au`
  - parse and type check a program
- `cargo run -p aura -- run examples/control_flow/while_break_continue.au`
  - execute the MIR runtime
- `cargo run -p aura -- run examples/classes/methods.au`
  - execute user-defined instance and associated methods
- `cargo run -p aura -- run examples/control_flow/match_literals.au`
  - execute statement-form `match` over literal `bool`, integer, and `String` cases
- `cargo run -p aura -- run examples/enums/result_match.au`
  - execute enum construction plus exhaustive `match`
- `cargo run -p aura -- run examples/enums/result_option.au`
  - execute built-in `Result[T, E]` and `Option[T]` values with exhaustive `match`
- `cargo run -p aura -- run examples/error_handling/try_result.au`
  - execute `try expr` over `Result[T, E]`
- `cargo run -p aura -- run examples/generics/box_and_wrapper.au`
  - execute user-defined generic classes, enums, and functions
- `cargo run -p aura -- run examples/basics/default_arguments.au`
  - execute default parameter values on ordinary functions
- `cargo run -p aura -- run examples/collections/vec_basics.au`
  - execute list literals, `Vec[T]` methods, and indexed element access
- `cargo run -p aura -- run examples/collections/vec_polish.au`
  - execute negative Vec indexing, non-copy cloned reads, mutable Vec iteration, `insert(...)`, `swap(...)`, `reverse()`, `clear()`, richer Vec methods, and Vec equality
- `cargo run -p aura -- run examples/collections/map_basics.au`
  - execute `Map[K, V]` literals, `items()` / `entries()`, `extend(...)`, and the maintained map method surface
- `cargo run -p aura -- run examples/collections/set_basics.au`
  - execute `Set[T]` literals, shared-borrow set iteration, and the maintained set method surface
- `cargo run -p aura -- run examples/basics/pass_keyword.au`
  - execute the `pass` no-op statement in intentionally empty blocks
- `cargo run -p aura -- run examples/modules/simple_import.au`
  - execute local file modules with `import`, `from ... import ...`, and `public` module boundaries
- `cargo run -p aura -- run examples/packages/local_path_dependencies/app/src/main.au`
  - execute a manifest-rooted package with `src/`, a sibling path dependency, and package-local helpers
- `cargo run -p aura -- run examples/packages/workspace/app/src/main.au`
  - execute a workspace member package with a workspace-root `Aurora.toml`
- `cargo run -p aura -- run examples/traits/greeter.au`
  - execute trait declarations, `impl Trait for Type`, and bounded generic calls
- `cargo run -p aura -- run examples/traits/generic_trait_impl.au`
  - execute generic trait declarations and generic impl headers
- `cargo run -p aura -- run examples/traits/generic_trait_bounds.au`
  - execute specialized generic trait bounds such as `T: Mapper[int32]`
- `cargo run -p aura -- run examples/traits/operator_traits.au`
  - execute operator traits through `+` and unary `-`
- `cargo run -p aura -- run examples/traits/ordering_traits.au`
  - execute ordering traits through `<`, `<=`, `>`, and `>=`
- `cargo run -p aura -- run examples/traits/specialized_trait_dispatch.au`
  - execute bounded dispatch across specialized generic trait impls
- `cargo run -p aura -- run examples/basics/numbers.au`
  - execute floor division, divisor-sign remainder, and rounded integer `.to_float()` conversion
- `cargo run -p aura -- run examples/concurrency/duration_arithmetic.au`
  - execute signed Duration constructors, runtime scaling, floor division,
    comparison, and floating unit conversion
- `cargo run -p aura -- run examples/randomness/deterministic_rng.au`
  - execute the stable seeded random stream, unbiased integer mapping, and
    deterministic in-place shuffle
- `cargo run -p aura -- run examples/json/dynamic_values.au`
  - parse recursive JSON into typed variants, inspect an exact accessor, and
    emit deterministic compact and pretty JSON
- `cargo run -p aura -- run examples/numbers/numeric_casts.au`
  - execute explicit numeric casts with `expr as Type`
- `cargo run -p aura -- run examples/numbers/numeric_builtins.au`
  - execute the maintained builtin numeric helper surface `abs(...)`, `min(...)`, `max(...)`, `sqrt(...)`, and `float64.sqrt()`
- `cargo run -p aura -- run examples/strings/string_methods.au`
  - execute single-quoted strings, Unicode-scalar `len()`, UTF-8 `byte_len()`, and the maintained `String` method surface including `split`, `replace`, case conversion, and prefix/suffix stripping
- `cargo run -p aura -- run examples/strings/string_parsing_and_formatting.au`
  - execute parsing builtins, scalar/boolean `.to_string()`, and `String.join(...)`
- `cargo run -p aura -- run examples/io/read_text_file.au`
  - execute the maintained builtin file I/O surface through `fs.exists(...)`, `fs.read_to_string(...)`, and `io.write(...)`
- `cargo run -p aura -- run examples/io/bytes_file_io.au`
  - execute binary file helpers plus `fs.File.read_bytes()` / `write_bytes(...)`
- `cargo run -p aura -- run examples/io/process_run.au`
  - execute shell-free subprocess helpers through `process.run(..., group=true)`, UTF-8/raw captured stdio, and `process.Completed.check()`
- `cargo run -p aura -- run examples/io/process_pipes.au`
  - execute `process.start(..., group=true)`, interactive `process.Pipe` I/O, and `process.Child.wait_ok(...)`
- `cargo run -p aura -- run examples/io/process_supervisor.au`
  - execute `process.supervisor()`, named child restart policies, backoff, and group-aware supervised shutdown
- `cargo run -p aura -- run examples/io/tcp_echo.au`
  - execute the maintained builtin TCP networking surface through `net.listen(...)`, `net.connect(...)`, and `TcpStream` / `TcpListener`
- `cargo run -p aura -- run examples/io/tcp_bytes.au`
  - execute timeout-aware TCP byte I/O through `connect_timeout(...)`, `read_exact(...)`, `read_bytes(...)`, and `write_bytes(...)`
- `cargo run -p aura -- run examples/io/udp_echo.au`
  - execute UDP binding, datagram receive/send, and `net.UdpDatagram`
- `cargo run -p aura -- run examples/io/http_roundtrip.au`
  - execute the maintained HTTP listener/request helpers on the shared evented runtime scheduler
- `cargo run -p aura -- run examples/io/websocket_roundtrip.au`
  - execute timeout-aware WebSocket listener/connect helpers on the nonblocking socket runtime
- `cargo run -p aura -- run examples/io/unix_tls_roundtrip.au`
  - execute the Unix-socket and TLS surface on Unix hosts using bundled PEM assets
- `cargo run -p aura -- run examples/agents/control_plane_foundations.au`
  - execute typed JSON/TOML metadata, path helpers, counters, and structured log/trace events
- `cargo run -p aura -- run app.au -- --model small`
  - pass program arguments exposed through `sys.args()`
- `cargo run -p aura -- new agent-app`
  - create a manifest-rooted project without overwriting existing files
- `cargo run -p aura -- fmt --check agent-app`
  - verify Aurora source normalization
- `cargo run -p aura -- test agent-app/tests`
  - run package-aware Aurora test programs
- `cargo run -p aura -- run examples/resources/with_resource.au`
  - execute deterministic scoped cleanup with `with`
- `cargo run -p aura -- run examples/concurrency/task_group_start.au`
  - execute the maintained queue/task concurrency surface
- `cargo run -p aura -- run examples/concurrency/bounded_queue.au`
  - execute bounded queues with `Queue[T](capacity=...)` on the shared scheduler
- `cargo run -p aura -- run examples/concurrency/sleep_builtin.au`
  - execute `sleep(duration)` delays in the MIR-backed runtime path
- `cargo run -p aura -- build -o ./target/aurora-point examples/point.au`
  - compile a standalone native binary through the default auto backend
- `cargo run -p aura -- build --backend direct -o ./target/aurora-direct ./examples/basic_addition.au`
  - force the direct native backend for the full currently implemented Aurora language surface
- `cargo run -p aura -- ast examples/classes/point_distance.au`
  - print the parsed syntax tree
- `cargo run -p aura -- ast-json examples/classes/point_distance.au`
  - print the parsed syntax tree as machine-readable JSON
- `cargo run -p aura -- mir examples/control_flow/while_break_continue.au`
  - print the lowered MIR for the checked program
- `cargo run -p aura -- analyze examples/classes/point_distance.au`
  - print machine-readable compiler analysis for diagnostics, symbols, hover, and definition
- `cargo run -p aura -- check --format json examples/classes/point_distance.au`
  - emit the stable, schema-versioned compiler diagnostic document used by CLI tooling; `run` and `build` accept the same diagnostic format
- `cargo run -p aura -- help`
  - print CLI usage and exit successfully
- `cargo run -p aura -- --version`
  - print the current CLI version and exit successfully
- `cat examples/modules/simple_import.au | cargo run -p aura -- analyze --stdin "$(pwd)/examples/modules/simple_import.au"`
  - analyze an editor-style buffer while still resolving local imports relative to the supplied path
- `cargo run -p aura -- complete --line 5 --character 11 --trigger . examples/point.au`
  - print machine-readable completion items at a source position
  - `--line` and `--character` are zero-based
  - member completion expects the cursor positioned just after `.`
  - the CLI tolerates the common incomplete-editor state where the current buffer contains one or more dangling member accesses such as `counter.` or `helpers.math.`, including when they appear at EOF
  - stdin-backed completion now also resolves local imported modules relative to the supplied file path, including imported trait methods
- `cat examples/modules/simple_import.au | cargo run -p aura -- run --stdin "$(pwd)/examples/modules/simple_import.au"`
  - execute an editor-style buffer while still resolving local imports relative to the supplied path
- `cargo run -p aura -- check examples/packages/local_path_dependencies/app/src/main.au`
  - type-check a package entrypoint using `Aurora.toml`, `src/`, local path dependencies, and git dependencies
- `cargo run -p aura -- deps update`
  - refresh all branch/tag/default-main git dependencies for the current package or workspace and rewrite `Aurora.lock`
- `cargo run -p aura -- deps update util`
  - refresh only the `util` git dependency for the current package or workspace
- `cat examples/modules/simple_import.au | cargo run -p aura -- check --stdin "$(pwd)/examples/modules/simple_import.au"`
  - type-check an editor-style buffer while still resolving local imports relative to the supplied path
- `npm run coverage:compiler`
  - measure current Rust compiler coverage with `cargo-llvm-cov`, using the full Rust workspace test surface while reporting compiler production files
- `npm run coverage:compiler:check`
  - enforce the current compiler coverage floor
- `npm run test:rust`
  - run the Rust test suite with one test thread and a larger test stack so direct-backend CLI binaries do not contend with each other and deep parser-limit regressions do not overflow the host test harness
- `npm run coverage:lsp:check`
  - enforce the current LSP coverage floor
- `npm run check:format`
  - verify Rust formatting
- `npm run check:clippy`
  - run the Rust lint gate with warnings treated as errors
- `npm run check:audit`
  - run npm and RustSec vulnerability gates
- `npm run check:hygiene`
  - reject whitespace errors, tracked generated executables, editor metadata, and scratch evaluation corpora
- `npm run docs:dev`
  - start the VitePress Aurora book locally
- `npm run docs:build`
  - build the VitePress Aurora book
- `npm run check:reference`
  - verify that the normative language-reference pages, navigation, and core conformance statements stay present
- `npm run ci`
  - run the current repo-quality gate locally, including formatting, serialized Rust tests, Node tests, coverage floors, docs build, audit, Clippy warnings-as-errors, and diff hygiene

GitHub Actions:

- `.github/workflows/ci.yml`
  - runs the repo gate on Linux and macOS
- `.github/workflows/docs.yml`
  - builds the VitePress book and deploys it to GitHub Pages from `main`
- `.github/workflows/release.yml`
  - builds Linux and macOS CLI archives, packages the VS Code extension and docs, and publishes them to GitHub Releases for `v*` tags or manual release runs

Current `build` status:

- `aura build` now accepts `--backend auto|direct`
- `aura build` defaults to `auto`
- `auto` first tries the direct native backend and may fall back to a standalone embedded-MIR launcher when direct emission is unavailable
- `direct` now performs true low-level native code generation for the full currently implemented Aurora language surface
- the built binary no longer reparses source or compiles a generated Rust runner at build time
- the built binary no longer depends on the original `.au` source files at runtime
- built binaries now render arithmetic runtime failures with file, line, and caret context from embedded source
- release archives include the Aurora native runtime and do not require Cargo or a source checkout; `aura build` still requires a host C compiler
- manifest-aware commands now resolve local path dependencies, git dependencies, and workspace members when the entry file lives under a package with `Aurora.toml`
- git dependencies support `git = "..."` with `rev`, `tag`, or `branch`, and default to `branch = "main"` when no selector is provided
- the current package-system milestone writes a local `Aurora.lock` at the package root or workspace root, pinning resolved git revisions and recording relative paths for local path dependencies
- both maintained execution paths now cover the builtin `io`, `fs`, `net`, and `process` module surface for scheduler-aware text/binary file I/O, poll-driven TCP/UDP/WebSocket/Unix/TLS socket I/O, higher-level HTTP helpers, shell-free subprocess execution with captured pipes, and supervised child processes with restart policy support

Current `run` status:

- `aura run` now executes programs through the MIR runtime for the current implemented Aurora surface
- queues, task groups, wait helpers, `try`, `with`, scheduler-aware file I/O, the maintained poll-driven socket networking surface, and the shell-free `process` module now run through the same MIR-backed public execution path
- the maintained execution architecture is now the MIR runtime for `run` plus native direct codegen for `build`

## VS Code install

The extension has two server pieces:

- the JavaScript LSP transport bundled inside the VSIX
- the compiler-owned semantic service started as `aura lsp`

Build both pieces before installing from this checkout. In particular, do not
reuse an existing `tools/vscode-aurora/aurora-language.vsix` after the language
server changes; that ignored local artifact may contain an older server bundle.

Install the current server and extension:

1. Run `npm ci` from the repo root.
2. Build the repo-local compiler service with `cargo build -p aura`. To install
   the actual `aura lsp` server binary on your `PATH` for every Aurora
   workspace, also run:

   ```bash
   cargo install --path crates/aura --locked --force
   ```

   This installs the `aura` executable (normally under `~/.cargo/bin`); the
   extension starts its `aura lsp` subcommand automatically. There is no second
   semantic-server executable to install.
3. Build and package the current JavaScript LSP transport with
   `npm run package:extension`.
4. Install that newly generated package:

   ```bash
   code --install-extension tools/vscode-aurora/aurora-language.vsix --force
   ```

   If the `code` shell command is unavailable, use **Extensions → … → Install
   from VSIX…** and select the same file.
5. Run **Developer: Reload Window** in VS Code, then reopen an `.au` file.

The language server keeps one persistent `aura lsp` compiler service for
diagnostics, document symbols, hover, go-to-definition, and completions. In
this repository it discovers `target/debug/aura` or `target/release/aura`.
For an Aurora workspace elsewhere, put `aura` on `PATH` or launch VS Code with
`AURORA_LSP_AURA_PATH` set to the absolute compiler path:

```bash
AURORA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aurora-project
```

Compiler diagnostics retain their stable `AU####` code, related spans, notes,
help, and edits through the LSP bridge instead of being reimplemented in
JavaScript. If the compiler process is unavailable, a small lexical recovery
layer provides basic declarations and top-level completions.

Full extension install and packaging steps are documented in [tools/vscode-aurora/INSTALL.md](tools/vscode-aurora/INSTALL.md).
