# Aurora

Aurora is a systems programming language designed to have Python-like simplicity, the memory safety of Rust, and the concurrency model of Go.

The goal is to build a systems programming language that is easy to learn and very effective for building agents and ML infrastructure.

## Monorepo layout

This repository is intended to evolve as a monorepo for the Aurora language and its associated tools.

- `crates/`
  - Rust compiler/runtime/bootstrap tooling
- `tools/`
  - editor integrations and other developer tools
- `package.json`
  - npm workspace manifest for repo-managed tools
- `examples/`
  - categorized sample Aurora programs
- `tutorials/`
  - Markdown tutorials covering the implemented language subset
- `docs/`
  - language proposal and supporting documentation
- `work/`
  - persistent task board and implementation notes

Compiler build and direct binary usage are documented in [crates/aura/README.md](crates/aura/README.md).
Compiler library testing notes live in [crates/aurora-compiler/README.md](crates/aurora-compiler/README.md).
The categorized example library is documented in [examples/README.md](examples/README.md).
The tutorial track lives in [tutorials/README.md](tutorials/README.md).
The repo testing strategy is documented in [docs/testing_strategy.md](docs/testing_strategy.md).

Current editor tooling:

- `tools/vscode-aurora`
  - VS Code extension for Aurora syntax highlighting and LSP client integration
- `tools/aurora-language-server`
  - Aurora Language Server Protocol implementation

Current bootstrap compiler workflow:

- `cargo run -p aura -- check examples/classes/point_distance.au`
  - parse and type check a program
- `cargo run -p aura -- run examples/control_flow/while_break_continue.au`
  - execute the interpreter-backed bootstrap runtime
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
  - execute non-copy Vec indexing, mutable Vec iteration, `insert(...)`, `reverse()`, `clear()`, richer Vec methods, and Vec equality
- `cargo run -p aura -- run examples/collections/map_basics.au`
  - execute `Map[K, V]` literals, `items()` / `entries()`, `extend(...)`, and the maintained map method surface
- `cargo run -p aura -- run examples/collections/set_basics.au`
  - execute `Set[T]` literals, shared-borrow set iteration, and the maintained set method surface
- `cargo run -p aura -- run examples/basics/pass_keyword.au`
  - execute the `pass` no-op statement in intentionally empty blocks
- `cargo run -p aura -- run examples/modules/simple_import.au`
  - execute local file modules with `import`, `from ... import ...`, and `public` module boundaries
- `cargo run -p aura -- run examples/traits/greeter.au`
  - execute trait declarations, `impl Trait for Type`, and bounded generic calls
- `cargo run -p aura -- run examples/traits/generic_trait_impl.au`
  - execute generic trait declarations and generic impl headers
- `cargo run -p aura -- run examples/traits/specialized_trait_dispatch.au`
  - execute bounded dispatch across specialized generic trait impls
- `cargo run -p aura -- run examples/numbers/numeric_casts.au`
  - execute explicit numeric casts with `expr as Type`
- `cargo run -p aura -- run examples/numbers/numeric_builtins.au`
  - execute the maintained builtin numeric helper surface `abs(...)`, `min(...)`, `max(...)`, `sqrt(...)`, and `float64.sqrt()`
- `cargo run -p aura -- run examples/strings/string_methods.au`
  - execute the maintained `String` method surface including `split`, `replace`, case conversion, and prefix/suffix stripping
- `cargo run -p aura -- run examples/strings/string_parsing_and_formatting.au`
  - execute parsing builtins, scalar/boolean `.to_string()`, and `String.join(...)`
- `cargo run -p aura -- run examples/resources/with_resource.au`
  - execute deterministic scoped cleanup with `with`
- `cargo run -p aura -- run examples/concurrency/channels_spawn.au`
  - execute bootstrap channels and spawned tasks
- `cargo run -p aura -- run examples/concurrency/sleep_builtin.au`
  - execute `sleep(duration)` delays in the bootstrap runtime and MIR path
- `cargo run -p aura -- run-mir examples/classes/methods.au`
  - execute the current MIR runtime path for the current implemented Aurora surface
- `cargo run -p aura -- run-mir examples/collections/vec_polish.au`
  - execute the finished maintained `Vec[T]` surface through the MIR path
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
- `cat examples/modules/simple_import.au | cargo run -p aura -- run-mir --stdin "$(pwd)/examples/modules/simple_import.au"`
  - run the MIR path against stdin-backed source while still resolving local imports relative to the supplied path
- `cat examples/modules/simple_import.au | cargo run -p aura -- check --stdin "$(pwd)/examples/modules/simple_import.au"`
  - type-check an editor-style buffer while still resolving local imports relative to the supplied path
- `npm run coverage:compiler`
  - measure current Rust compiler-library coverage with `cargo-llvm-cov`
- `npm run coverage:compiler:check`
  - enforce the current compiler coverage floor
- `npm run coverage:lsp:check`
  - enforce the current LSP coverage floor
- `npm run ci`
  - run the current repo-quality gate locally

Current `build` status:

- `aura build` now accepts `--backend auto|direct`
- `aura build` defaults to `auto`
- `auto` uses the direct native backend for the maintained Aurora surface
- `direct` now performs true low-level native code generation for the full currently implemented Aurora language surface
- the built binary no longer reparses source or compiles a generated Rust runner at build time
- the built binary no longer depends on the original `.au` source files at runtime
- built binaries now render arithmetic runtime failures with file, line, and caret context from embedded source
- the current build path still requires Cargo/Rust plus a host C compiler when producing artifacts

Current `run-mir` status:

- `aura run-mir` executes programs natively through the MIR runtime for the current implemented Aurora surface
- `spawn`, `select`, channels, task groups, `try`, and `with` now run through MIR
- the direct backend now covers the maintained Aurora surface, so `run-mir` remains primarily useful as an alternate execution path and backend-debugging tool

## VS Code install

Development install:

1. Run `npm install` from the repo root.
2. Run `npm run build:extension`.
3. Run `npm run check:lsp`, `npm run test:lsp`, `npm run check:extension`, and `npm run test:extension`.
4. Open the repo in VS Code.
5. Open `tools/vscode-aurora`.
6. Press `F5` to launch an Extension Development Host.
7. Open an `.au` file such as `examples/classes/point_distance.au` in the Extension Development Host.

The language server now prefers compiler-owned analysis from `aura analyze` and `aura complete` for diagnostics, document symbols, hover, go-to-definition, and completions. That compiler path now understands local module imports for file-backed and stdin-backed buffers. It falls back to the in-repo JS analysis layer when the compiler cannot analyze the current buffer.

Packaged install:

1. Run `npm install`.
2. Run `npm run package:extension`.
3. In VS Code, use `Install from VSIX...` and select `tools/vscode-aurora/aurora-language.vsix`.

Full extension install and packaging steps are documented in [tools/vscode-aurora/INSTALL.md](tools/vscode-aurora/INSTALL.md).
