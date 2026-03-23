# Task Board

Last updated: 2026-03-22

## In Progress

- Expand the frontend from the current bootstrap toward the broader frozen v1 surface without breaking the working path.
- Keep the language server and VS Code tooling aligned with compiler and proposal changes while the language surface is still moving quickly.
- Keep the categorized example library and `tutorials/` track synchronized with the implemented language subset as the compiler evolves.
- Move the repo onto a stricter test-first workflow with fixture-driven compiler cases and explicit coverage policy.

## Todo

- Raise the Aurora compiler library toward enforced 100% coverage now that Rust coverage is measurable.
- Raise the Aurora language-server analysis package to enforced 100% coverage before expanding its semantic surface further.
- Extend the trait system toward the remaining proposal surface, including generic traits, generic impl headers, and operator traits.
- Extend compiler-backed `analyze` / `complete` and the LSP from local-module diagnostics/hover/completions to fully correct cross-file definitions for imported items.
- Replace the new bootstrap MIR-artifact launcher path with true native MIR codegen for macOS and Linux.
- Add broader product-level backend coverage around native codegen outputs and `run-mir`.
- Narrow the JS fallback further now that completions, diagnostics, symbols, hover, and definition all have compiler-backed paths.

## Done

- Added a Rust workspace root with `aurora-compiler` and `aura`.
- Added the first compiler modules: diagnostics, AST, lexer, parser, semantic checker, and evaluator.
- Added the first milestone sample program at `examples/point.au`.
- Added `examples/README.md` with instructions for running, checking, and inspecting example programs.
- Added `crates/aura/README.md` with release-build and direct binary usage instructions.
- Added in-repo work tracking under `work/`.
- Verified `cargo test` passes.
- Verified `cargo run -p aura -- run examples/point.au` prints `5`.
- Added support for `def name(...):` as shorthand for `-> None`.
- Added support for running top-level script statements without an explicit `main`.
- Renamed primitive language types to explicit spellings like `int32`, `uint64`, and `float64`.
- Renamed the line-printing builtin from `println` to `print`.
- Verified `examples/basic_addition.au` and `examples/top_level_addition.au` both run and print `16`.
- Added `tools/vscode-aurora` as an in-repo VS Code extension package.
- Added `tools/aurora-language-server` as an in-repo LSP package.
- Added a root npm workspace manifest for repo-managed tools.
- Verified the VS Code extension analysis/tests with `npm run check:extension` and `npm run test:extension`.
- Switched the VS Code package from local editor analysis to an LSP client.
- Added a bundled `dist/` build for the VS Code extension so VSIX packaging stays self-contained inside the monorepo.
- Verified `npm run package:extension` produces `tools/vscode-aurora/aurora-language.vsix`.
- Regenerated `docs/aurora_language_proposal.html` from the updated proposal Markdown.
- Added parser, semantic checker, and interpreter support for `if`, `elif`, `else`, `while`, `break`, `continue`, strings, booleans, comparison operators, and compound assignment.
- Added `examples/control_flow.au` and verified the control-flow bootstrap path.
- Improved CLI diagnostics so parser/type/runtime errors render with source context and a caret.
- Staged compiler MIR lowering with explicit basic blocks and a new `aura mir <file.au>` command.
- Added LSP hover, go-to-definition, and document diagnostics on top of the current Aurora-aware analysis layer.
- Added categorized examples covering most of the currently implemented language surface.
- Added a `tutorials/` directory with Markdown chapters for the implemented subset and documented the maintenance rule that examples and tutorials must evolve with the language.
- Fixed LSP false positives for top-level script bindings and added member resolution for parenthesized receiver expressions such as `(dx * dx + dy * dy).sqrt()`.
- Added a repo-level `AGENTS.md` and `docs/testing_strategy.md` to define the test-first workflow.
- Added fixture-based compiler tests for parse/check/run/diagnostic behavior under `crates/aurora-compiler/tests/fixtures/`.
- Added `crates/aurora-compiler/README.md` documenting compiler test layers and fixture categories.
- Added `npm run coverage:lsp` as the repeatable language-server coverage command and documented it in the repo.
- Added `npm run coverage:compiler` and measured the first Rust compiler-library coverage baseline with `cargo-llvm-cov`.
- Added parser, checker, interpreter, MIR, examples, and LSP support for non-generic enums with unit and single-payload variants plus exhaustive statement-form `match`.
- Added parser, checker, interpreter, MIR, examples, tutorials, and LSP support for `for` loops over `range(...)`.
- Added parser, checker, interpreter, examples, tutorials, and LSP support for user-defined instance methods with `borrow self` plus associated methods.
- Added built-in generic `Result[T, E]` and `Option[T]` support across the checker, interpreter, examples, tutorials, and LSP analysis.
- Added fuller mutating receiver semantics with member-target assignment, `borrow mut self`, mutating methods, and regression fixtures.
- Added `try expr` over built-in `Result[T, E]` with checker/runtime support, examples, tutorials, and diagnostics.
- Added `with` scoped cleanup using `close(borrow mut self)` resources, plus examples, tutorials, and runtime cleanup on early return.
- Added bootstrap concurrency with `Channel[T]`, `channel()`, `spawn`, `Task[T]`, `send`, `recv`, `close`, and `join()`, plus examples, fixtures, and LSP support.
- Added bootstrap structured concurrency with `task_group()`, `with task_group() as group:`, `group.spawn(...)`, `group.cancel()`, cooperative `cancelled()`, `select`, and duration literals for `after(...)`, plus examples, fixtures, tutorials, MIR support, and LSP coverage.
- Added explicit detached tasks with `spawn detached`, proposal-level `Channel.send() -> Result[None, SendError[T]]`, and broader `select` send/recv/timer arm support across the compiler, runtime, examples, fixtures, tutorials, syntax highlighting, and LSP.
- Fixed LSP false diagnostics for `after(...)` select timers and duration literals like `5ms` in concurrency examples.
- Added machine-readable compiler analysis output plus `aura analyze` and `aura ast-json`.
- Switched the language server to prefer compiler-owned diagnostics, symbols, hover, and go-to-definition via `aura analyze`, with local JS analysis kept as fallback and for completions.
- Added machine-readable compiler completions via `aura complete`.
- Switched the language server to prefer compiler-owned completions, leaving the JS analysis layer as fallback for incomplete or currently-invalid buffers.
- Expanded the tutorial track so it covers the full currently implemented bootstrap language surface, not just the features already represented by the example walkthroughs.
- Fixed VS Code indentation so pressing Enter after Aurora block headers keeps the expected block indent instead of jumping back to column 0.
- Added an Aurora-specific VS Code Enter handler so indentation now deterministically follows Aurora block structure instead of relying only on editor heuristics.
- Added named arguments for ordinary functions, instance methods, associated methods, and spawned function targets, aligning callable syntax more closely with class construction.
- Added a shared compiler-side call binding layer for user-defined callables and builtins.
- Added named arguments for supported builtins, including `print(value=...)`, `range(stop=...)`, `range(start=..., stop=...)`, `after(duration=...)`, and `Channel.send(value=...)`.
- Added compiler and LSP regression coverage plus categorized examples and tutorial updates for builtin named arguments.
- Added integer-literal range enforcement for fixed-width integer annotations and default `int32` literals.
- Added support for `String.clone()` in the checker/runtime and removed unsupported `String.as_str()` from the documented current surface and completions.
- Improved the diagnostic for builtin method references like `ch.send` so they report a missing call instead of a misleading generic-type error.
- Clarified current limitations and `aura complete` semantics in the README and tutorial track so the documented bootstrap surface matches the implementation more closely.
- Made `aura complete --trigger .` tolerate the common incomplete-editor state where the current buffer contains a dangling member access like `counter.`.
- Made `aura analyze` recover symbols and occurrences for the common dangling-dot editor state while still surfacing the parse diagnostic.
- Added CLI product tests for broken-pipe stdout handling in `ast` and `mir`, and fixed those commands to exit cleanly when piped into consumers like `head`.
- Added `aura build -o <output>` as a bootstrap standalone-binary path by generating and compiling a Rust launcher linked against `aurora-compiler`.
- Added a MIR runtime for the current simpler subset plus `aura run-mir` for exercising that execution path directly.
- Expanded `aura run-mir` so it now covers the current implemented Aurora surface natively through MIR, including concurrency, `try`, and `with`.
- Switched `aura build` from embedding source execution to embedding checked MIR and running it directly through `run_mir(...)`.
- Added backend regression coverage for native MIR execution through both `run-mir` and built binaries.
- Added native MIR support for `try expr`, removing `try` from the backend fallback surface.
- Added native MIR support for `with` cleanup, removing `with` from the backend fallback surface.
- Added boolean operators `and`, `or`, and `not` across the parser, checker, interpreter, MIR lowering, and MIR runtime.
- Added unary minus support across the parser, checker, interpreter, MIR lowering, and MIR runtime.
- Added checker-level use-after-move diagnostics for straight-line moves through function arguments, value receivers, constructors, enum payloads, and channel sends.
- Added clean Aurora diagnostics for division by zero and integer overflow in both the interpreter and MIR runtime.
- Added runtime enforcement for annotated fixed-width integer bindings and assignments instead of silently widening values.
- Unified `main` parameter validation so both execution paths reject parameterized `main` functions during checking.
- Added contextual `float32` literal support so floating-point literals can be used in typed `float32` bindings, parameters, returns, and class fields.
- Added explicit numeric casts with `expr as Type` across the parser, checker, interpreter, MIR runtime, compiler analysis, fixtures, and maintained examples.
- Added user-defined generic `class`, `enum`, and `def` declarations with generic inference across the checker, runtimes, fixtures, examples, tutorials, and LSP fallback analysis.
- Added first-pass traits with `trait`, `impl Trait for Type`, bounded generic functions, trait method checking, interpreter/MIR trait dispatch, compiler-backed trait symbols/completions, and maintained examples/tutorial coverage.
- Added default parameter values on ordinary functions and class methods, including checker/runtime/MIR support, call-site omission handling, and proposal-aligned restrictions on ordering and parameter references.
- Promoted multiple trait bounds with `T: A + B` from an untracked capability to a maintained surface with fixtures, examples, and tutorial coverage.
- Fixed the compiler-backed LSP bridge to prefer the current source-tree compiler via `cargo run` inside the Aurora repo, avoiding stale `target/debug/aura` behavior during local development and tests.
- Added `pass` as a maintained no-op statement for intentionally empty blocks.
- Added the `sleep(duration)` builtin across checking, runtime, MIR, examples, tutorials, and editor tooling.
- Added local file module support with `import`, `from ... import ...`, and `public` module boundaries across checking, interpreter execution, MIR execution, CLI run/build, examples, tutorials, and compiler tests.
- Extended compiler-backed `aura analyze` / `aura complete` and the LSP bridge so stdin/file analysis now resolves local module imports for diagnostics, hover, and completions.
- Added CI-style repo gates plus enforced baseline coverage thresholds for the compiler and language server.

## Blocked

- None currently.
