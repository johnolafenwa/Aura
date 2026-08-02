# Aura

Aura is a compiled, statically typed systems language for ML infrastructure and
reliable agents. It combines Python-like readability with Rust-style safety:
deterministic ownership, structured concurrency, typed failure, native
executables, and no garbage collector.

Install the Aura 0.2 technical preview on Linux x64, macOS x64, or macOS arm64:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

Aura aims to democratize systems programming for teams building model-serving
infrastructure, agent runtimes, data and evaluation workers, tool executors,
and the control planes around them. Shared access, exclusive mutation,
ownership transfer, resource cleanup, and child-task lifetime are checked by
the language.

Read [Why Aura](docs/positioning.md) for the project direction. The canonical
implemented contract begins with the normative
[Language Specification](docs/manual/language-specification.md),
[complete grammar](docs/manual/grammar.md), and Manual. Current measurements
and the optimization roadmap live in the
[Performance chapter](docs/manual/performance.md). Supported hosts and pinned
tools are listed in [SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md).

## Monorepo layout

This repository is intended to evolve as a monorepo for the Aura language and its associated tools.

- `crates/`
  - Rust compiler, runtime, and CLI tooling
- `tools/`
  - editor integrations and other developer tools
- `package.json`
  - npm workspace manifest for repo-managed tools
- `examples/`
  - categorized sample Aura programs
- `tutorials/`
  - Markdown tutorials covering the implemented language subset
- `docs/`
  - VitePress book, language proposal, and supporting documentation
- `architecture_docs/`
  - implementation-focused architecture and component deep dives for the current Aura system
- `work/`
  - persistent task board and implementation notes

Compiler build and direct binary usage are documented in [crates/aura/README.md](crates/aura/README.md).
Compiler library testing notes live in [crates/aura-compiler/README.md](crates/aura-compiler/README.md).
The categorized example library is documented in [examples/README.md](examples/README.md).
The tutorial track lives in [tutorials/README.md](tutorials/README.md).
The VitePress book lives in [docs/index.md](docs/index.md) and includes the guided Learn track plus the normative language and API reference.
The repo testing strategy is documented in [docs/testing_strategy.md](docs/testing_strategy.md).
The forward-looking ML systems roadmap lives in [docs/ml_systems_support_plan.md](docs/ml_systems_support_plan.md).
The implementation architecture guide lives in [architecture_docs/README.md](architecture_docs/README.md).

Current editor tooling:

- `tools/vscode-aura`
  - VS Code extension for Aura syntax highlighting and LSP client integration
- `tools/aura-language-server`
  - Aura Language Server Protocol implementation

Current compiler workflow:

- `cargo run -p aura -- check examples/classes/point_distance.au`
  - parse and type check a program
- `cargo run -p aura -- run examples/control_flow/while_break_continue.au`
  - execute the MIR runtime
- `cargo run -p aura -- run examples/classes/methods.au`
  - execute user-defined instance and associated methods
- `cargo run -p aura -- run examples/control_flow/match_literals.au`
  - execute statement-form `match` over literal `bool`, integer, and `str` cases
- `cargo run -p aura -- run examples/control_flow/conditional_expressions.au`
  - execute lazy Python-style conditional expressions with one unified result type
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
- `cargo run -p aura -- run examples/basics/closures.au`
  - execute contextually typed expression closures with by-value captures
- `cargo run -p aura -- run examples/basics/len_and_str.au`
  - execute `int64` member lengths, `len(value) == value.len()`, Unicode-scalar
    str length versus UTF-8 byte length, and `str(value)`
- `cargo run -p aura -- run examples/collections/vec_basics.au`
  - execute list literals, `list[T]` methods, and indexed element access
- `cargo run -p aura -- run examples/collections/vec_polish.au`
  - execute negative list indexing, cast-free length-driven indexing,
    non-copy cloned reads, mutable list iteration, `insert(...)`,
    `swap(...)`, `reverse()`, `clear()`, richer list methods, and list equality
- `cargo run -p aura -- run examples/collections/vec_algorithms.au`
  - execute stable natural/key sorting plus eager, source-retaining
    `list.map(...)` and `list.filter(...)`
- `cargo run -p aura -- run examples/collections/comprehensions.au`
  - execute eager owned list, set, and dictionary comprehensions with filters and
    nested outer-major clauses
- `cargo run -p aura -- run examples/collections/slices.au`
  - execute owned list and Unicode-scalar str slices, omitted and negative
    endpoints, and source/result independence
- `cargo run -p aura -- run examples/collections/map_basics.au`
  - execute `dict[K, V]` literals, tuple-valued `items()`, `update(...)`, and the maintained dictionary method surface
- `cargo run -p aura -- run examples/collections/set_basics.au`
  - execute `set[T]` literals, shared set iteration, membership, and the maintained set method surface
- `cargo run -p aura -- run examples/basics/pass_keyword.au`
  - execute the `pass` no-op statement in intentionally empty blocks
- `cargo run -p aura -- run examples/basics/assertions.au`
  - execute introspectable comparisons and membership with lazy messages and source-located failures
- `cargo run -p aura -- run examples/basics/multiline_expressions.au`
  - continue calls, signatures, grouping, indexes, and collection literals
    across physical lines while a source delimiter remains open
- `cargo run -p aura -- run examples/basics/tuples.au`
  - execute fixed-arity tuple values, recursive unpacking, tuple-pattern
    matching, copy-only constant indexing, and same-type recursive `==` and
    `!=` that retain both operands; tuple ordering remains rejected
- `cargo run -p aura -- run examples/modules/simple_import.au`
  - execute local file modules with `import`, `from ... import ...`, and `public` module boundaries
- `cargo run -p aura -- run examples/packages/local_path_dependencies/app/src/main.au`
  - execute a manifest-rooted package with `src/`, a sibling path dependency, and package-local helpers
- `cargo run -p aura -- run examples/packages/workspace/app/src/main.au`
  - execute a workspace member package with a workspace-root `Aura.toml`
- `cargo run -p aura -- run --backend mir examples/packages/ffi_getpid/src/main.au`
  - execute an explicitly authorized FFI v0 package that calls the
    process-global C `getpid` symbol on a Unix-family host; use
    `--backend direct` for the maintained backend-parity path
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
- `cargo run -p aura -- run examples/bytes/codecs_and_hashing.au`
  - convert strict UTF-8 text, encode canonical hex/base64, and compute raw
    SHA-256 bytes without consuming the inputs
- `cargo run -p aura -- run examples/numbers/numeric_casts.au`
  - execute explicit numeric casts with `expr as Type`
- `cargo run -p aura -- run examples/numbers/numeric_builtins.au`
  - execute the maintained builtin numeric helper surface `abs(...)`, `min(...)`, `max(...)`, `sqrt(...)`, and `float64.sqrt()`
- `cargo run -p aura -- run examples/numbers/numeric_arrays.au`
  - execute contiguous row-major `Array[T]` construction, multidimensional
    indexing, mutation, first-axis owned slicing, mapping, reductions,
    exact-shape/scalar kernels, and explicit wrapping/saturating integer modes
- `cargo run -p aura -- run examples/strings/string_methods.au`
  - execute single-quoted strings, `int64` Unicode-scalar `len()`, `int64`
    UTF-8 `byte_len()`, and the maintained `str` method surface including
    `split`, `replace`, case conversion, and prefix/suffix stripping
- `cargo run -p aura -- run examples/strings/string_parsing_and_formatting.au`
  - execute parsing builtins, scalar/boolean `.to_string()`, and `str.join(...)`
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
- `cargo run -p aura -- run examples/agents/retry_with_backoff.au`
  - execute `control.retry(...)` through eventual success and exact
    last-error exhaustion with a zero-delay backoff
- `cargo run -p aura -- run examples/agents/retrying_network_worker.au`
  - execute an application-level HTTP retry worker that retries only `503`,
    uses deterministic seed-42 jitter with exponential `Duration` backoff,
    applies explicit deadlines, and closes its task/listener/response resources
    through structured scopes; the maintained product regression pins the same
    seven-request trace on the MIR and forced-direct backends
- `cargo run -p aura -- run app.au -- --model small`
  - pass program arguments exposed through `sys.args()`
- `cargo run -p aura -- new agent-app`
  - create a manifest-rooted project without overwriting existing files
- `cargo run -p aura -- fmt --check agent-app`
  - verify Aura source normalization
- `cargo run -p aura -- test agent-app/tests`
  - run package-aware Aura test programs
- `cargo run -p aura -- run examples/resources/with_resource.au`
  - execute deterministic scoped cleanup with `with`
- `cargo run -p aura -- run examples/concurrency/task_group_start.au`
  - execute the maintained queue/task concurrency surface
- `cargo run -p aura -- run examples/concurrency/bounded_queue.au`
  - execute bounded queues with `Queue[T](capacity=...)` across the
    pinned-worker scheduler; task captures, task results, and Queue payloads
    use compiler-derived structural `Transfer`, while non-repeatable task
    results have one consuming observation right
- `cargo run -p aura -- run examples/concurrency/sleep_builtin.au`
  - execute `sleep(duration)` delays in the MIR-backed runtime path
- `cargo run -p aura -- run examples/concurrency/yield_now.au`
  - execute bounded CPU-work chunks with explicit cooperative scheduling
    points; ordinary loop backedges also receive automatic checks
- `cargo run -p aura -- run examples/concurrency/typed_select.au`
  - execute typed heterogeneous Queue, Task, and relative-deadline selection
    with deterministic source indexes on both maintained backends
- `cargo run -p aura -- build -o ./target/aura-point examples/point.au`
  - compile a standalone native binary through the default auto backend
- `cargo run -p aura -- build --backend direct -o ./target/aura-direct ./examples/basic_addition.au`
  - force the direct native backend for the full currently implemented Aura language surface
- `cargo run -p aura -- ast examples/classes/point_distance.au`
  - print the parsed syntax tree
- `cargo run -p aura -- ast-json examples/classes/point_distance.au`
  - print the parsed syntax tree as machine-readable JSON
- `cargo run -p aura -- mir examples/control_flow/while_break_continue.au`
  - print the lowered MIR for the checked program
- `cargo run -p aura -- analyze examples/classes/point_distance.au`
  - print machine-readable compiler analysis for diagnostics, symbols, hover, and definition
- `cargo run -p aura -- check --format json examples/classes/point_distance.au`
  - emit the stable, schema-versioned compiler diagnostic document used by CLI
    tooling; `run` and `build` accept the same format, including typed
    `call_frames` and `task_ancestry` arrays for runtime failures
- `cargo run -p aura -- help`
  - print CLI usage and exit successfully
- `cargo run -p aura -- --version`
  - print the preview channel and the 12-hex-digit source commit, so preview
    builds cannot be confused with a future final release
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
  - type-check a package entrypoint using `Aura.toml`, `src/`, local path dependencies, and git dependencies
- `cargo run -p aura -- deps update`
  - refresh all branch/tag/default-main git dependencies for the current package or workspace and rewrite `Aura.lock`
- `cargo run -p aura -- deps update util`
  - refresh only the `util` git dependency for the current package or workspace
- `cat examples/modules/simple_import.au | cargo run -p aura -- check --stdin "$(pwd)/examples/modules/simple_import.au"`
  - type-check an editor-style buffer while still resolving local imports relative to the supplied path
- `npm run coverage:compiler`
  - measure current Rust compiler coverage with `cargo-llvm-cov`, using the full Rust workspace test surface while reporting compiler production files
- `npm run coverage:compiler:check`
  - enforce the current compiler coverage floor
- `npm run test:rust`
  - run the Rust test suite at Cargo/libtest's default parallelism with a larger test stack so deep parser-limit regressions do not overflow the host test harness
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
  - start the VitePress Aura book locally
- `npm run docs:build`
  - build the VitePress Aura book
- `npm run check:reference`
  - verify that the normative language-reference pages, navigation, and core conformance statements stay present
- `npm run ci`
  - run the current repo-quality gate locally, including formatting, the default-parallel main Rust suite, the separately serialized backend-parity gate, Node tests, coverage floors, docs build, audit, Clippy warnings-as-errors, and diff hygiene; the instrumented compiler-coverage wrapper also retains its own narrow single-threaded test setting for stable coverage collection

GitHub Actions:

- `.github/workflows/ci.yml`
  - runs the repo gate on Linux and macOS
- `.github/workflows/docs.yml`
  - builds the VitePress book and deploys it to GitHub Pages from `main`
- `.github/workflows/release.yml`
  - builds Linux and macOS CLI archives, packages the VS Code extension and
    docs, and publishes them for pushed `v*` tags; manual runs are build-only
    by default and require an explicit publish opt-in

Current `build` status:

- `aura build` now accepts `--backend auto|direct`
- `aura build` defaults to `auto`
- `auto` first tries the direct native backend and may fall back to a standalone embedded-MIR launcher when direct emission is unavailable
- `direct` now performs true low-level native code generation for the full currently implemented Aura language surface
- the built binary no longer reparses source or compiles a generated Rust runner at build time
- the built binary no longer depends on the original `.au` source files at runtime
- built binaries now render runtime failures with file, line, caret, typed
  Aura call-chain, and child-task ancestry context from embedded source
- release archives include the Aura native runtime and do not require Cargo or a source checkout; `aura build` still requires a host C compiler
- manifest-aware commands now resolve local path dependencies, git dependencies, and workspace members when the entry file lives under a package with `Aura.toml`
- git dependencies support `git = "..."` with `rev`, `tag`, or `branch`, and default to `branch = "main"` when no selector is provided
- the current package-system milestone writes a local `Aura.lock` at the package root or workspace root, pinning resolved git revisions and recording relative paths for local path dependencies
- both maintained execution paths now cover the builtin `io`, `fs`, `net`, and `process` module surface for scheduler-aware text/binary file I/O, reactor-driven TCP/UDP/WebSocket/Unix/TLS socket I/O, higher-level HTTP helpers, shell-free subprocess execution with captured pipes, and supervised child processes with restart policy support

Current `run` status:

- `aura run` defaults to the MIR runtime for the current implemented Aura surface; `--backend direct` requires native execution and `--backend auto` prefers it with visible fallback
- queues, task groups, wait helpers, `try`, `with`, scheduler-aware file I/O, the maintained reactor-driven socket networking surface, and the shell-free `process` module now run through the same MIR-backed public execution path
- task bodies use pinned scheduler workers: the default worker count is the
  available parallelism reported by the host, and the provisional
  `AURA_WORKERS=<positive integer>` override selects an explicit count; each
  child keeps its
  spawn-time worker for its lifetime, with no stack migration or work stealing
- scheduler waits use persistent descriptor registrations, a timer heap, and
  direct Queue, task-completion, and blocking-pool notifications, including
  cross-worker wakes; an idle worker blocks until local work, a notification,
  an event, or a deadline without a periodic tick
- blocking host operations use a separate lazy process-wide pool:
  `AURA_BLOCKING_WORKERS=<positive integer>` selects an exact worker count
  without clamping, while the absent default derives `2..=8` workers from host
  parallelism with fallback `4`;
  `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` optionally bounds
  accepted pending jobs only, with FIFO scheduler-aware admission and an
  unbounded compatibility default
- invalid blocking-pool settings fail with `AU4006` before user code under
  MIR, direct, and standalone execution; the first runtime preflight records
  one immutable process-lifetime configuration, but starts no blocking-pool
  worker threads
- first blocking submission creates the complete worker set, which production
  reuses until process exit without an Aura shutdown/join surface;
  pre-acceptance timeout/cancellation prevents submission, and accepted host
  work remains non-retractable, so a queue bound cannot guarantee progress
  for unrelated blocking I/O while every worker is stuck
- `select(source, ...)` provides typed heterogeneous Queue, Task, and
  relative-deadline waiting with cancellation-first/lowest-index arbitration,
  one winner, and loser cleanup; it is an ordinary builtin, not statement
  syntax
- `yield_now()` yields only to runnable work on the current task's worker;
  task scheduling, cross-worker completion, and program-output order are
  deliberately unspecified
- Queue and Task handles are the maintained cross-worker communication
  surface; compiler-derived `Transfer` keeps all other task captures and
  results share-nothing, and cancellation and diagnostic context remain
  isolated per task
- MIR execution and direct native execution use the same pinned-worker
  contract and execute Aura tasks across multiple cores; preemption, work
  stealing, detached tasks, and worker introspection are unavailable, while
  parallel speedup depends on the workload
- every loop backedge includes a compiler-inserted cooperative scheduling
  check; native concurrent code amortizes it with function-local fuel, while
  sequential native code elides checks when no sibling task can exist
- the maintained execution architecture is now the MIR runtime for `run` plus native direct codegen for `build`

## VS Code install

The extension has two server pieces:

- the JavaScript LSP transport bundled inside the VSIX
- the compiler-owned semantic service started as `aura lsp`

Build both pieces before installing from this checkout. In particular, do not
reuse an existing `tools/vscode-aura/aura-language.vsix` after the language
server changes; that ignored local artifact may contain an older server bundle.

Install the current server and extension:

1. Run `npm ci` from the repo root.
2. Build the repo-local compiler service with `cargo build -p aura`. To install
   the actual `aura lsp` server binary on your `PATH` for every Aura
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
   code --install-extension tools/vscode-aura/aura-language.vsix --force
   ```

   If the `code` shell command is unavailable, use **Extensions → … → Install
   from VSIX…** and select the same file.
5. Run **Developer: Reload Window** in VS Code, then reopen an `.au` file.

The language server keeps one persistent `aura lsp` compiler service for
diagnostics, document symbols, hover, go-to-definition, and completions. In
this repository it discovers `target/debug/aura` or `target/release/aura`.
For an Aura workspace elsewhere, put `aura` on `PATH` or launch VS Code with
`AURA_LSP_AURA_PATH` set to the absolute compiler path:

```bash
AURA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aura-project
```

The LSP bridge preserves the compiler's stable `AU####` codes, related spans,
notes, help, edits, typed call frames, and task ancestry. If the compiler
process is unavailable, a small lexical recovery layer provides basic
declarations and top-level completions.

Full extension install and packaging steps are documented in [tools/vscode-aura/INSTALL.md](tools/vscode-aura/INSTALL.md).
