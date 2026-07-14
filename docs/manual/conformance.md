# Conformance

Aurora keeps the language reference and implementation aligned through executable conformance layers. This page identifies which tests substantiate each part of the specification and what a conforming implementation is expected to do.

## Conforming Programs And Implementations

A **conforming Aurora program** uses only syntax and APIs defined by this Manual and satisfies all static rules.

A **conforming Aurora implementation**:

- accepts every conforming program within documented implementation limits
- rejects programs that violate a MUST-level lexical, grammatical, name, type, ownership, or entrypoint rule
- preserves the observable evaluation and cleanup behavior defined by the Manual
- produces the specified typed outcomes or runtime failures
- provides the maintained public API surface
- does not expose proposal-only constructs as accepted 0.1 language features

Exact diagnostic prose is normative only where a fixture or this Manual explicitly requires it. A conforming implementation otherwise needs a clear diagnostic with an accurate source location and the same semantic category.

## Executable Reference Map

| Reference area | Primary executable evidence |
| --- | --- |
| UTF-8, indentation, tokens, literals, escapes | `crates/aurora-compiler/src/lexer_tests.rs` |
| grammar and parser limits | `crates/aurora-compiler/src/parser_tests.rs`, `tests/fixtures/parse-pass`, `tests/fixtures/parse-fail` |
| names, types, calls, traits, patterns, moves, and borrows | `crates/aurora-compiler/src/sema_tests.rs`, `tests/fixtures/check-pass`, `tests/fixtures/check-fail` |
| integer `/` rejection, floor division/remainder, and `.to_float()` | lexer/parser/integer/runtime-value unit tests plus `integer_true_division_*`, `floor_division_and_modulo`, and `integer_to_float_rounding` fixtures |
| module and package resolution | `crates/aurora-compiler/tests/modules.rs`, `tests/packages.rs`, `src/package_tests.rs` |
| MIR semantics and runtime behavior | `src/mir_tests.rs`, `src/mir_runtime_tests.rs`, `tests/fixtures/run-pass`, `tests/fixtures/run-fail` |
| native semantics and resource ABI | `src/native_codegen_tests.rs`, `src/native_runtime_tests.rs`, `tests/native_runtime_ffi.rs` |
| MIR/native observable equivalence | `crates/aura/tests/backend_parity.rs` |
| CLI, entrypoints, diagnostics, and installed builds | `crates/aura/tests/cli.rs`, `crates/aura/tests/packages.rs` |
| analysis, completion, hover, definitions, invalidation | `tools/aurora-language-server/test` |
| maintained examples | compiler example smoke tests and CLI product tests |

The exact repository gate is `npm run ci`. It runs formatting, Rust tests, backend parity, language-server and extension tests, compiler and LSP coverage gates, this reference check, the documentation build, dependency audits, Clippy with warnings denied, and repository hygiene.

## Fixture Classes

The compiler fixture directories have distinct contracts:

- `parse-pass`: source MUST form a valid AST; later static checking is not implied.
- `parse-fail`: source MUST be rejected during lexing or parsing with the stored diagnostic.
- `check-pass`: source MUST parse and satisfy the static semantics.
- `check-fail`: source MUST parse and then be rejected by static checking with the stored diagnostic.
- `run-pass`: source MUST check and produce the stored standard output through the maintained execution path.
- `run-fail`: source MUST check far enough to reach the intended runtime failure and produce the stored diagnostic behavior.

Regression tests supplement fixtures when a case needs multiple files, temporary packages, local sockets, processes, timing, cancellation, or comparison of execution backends.

## Backend Equivalence

Aurora 0.1 has two maintained semantic runtime representations:

- `aura run` lowers checked source to MIR and executes it in the MIR runtime.
- `aura build --backend direct` lowers checked source to native code through the direct backend and links the native runtime.
- the default `aura build --backend auto` first attempts direct emission and may instead build a native launcher containing serialized MIR plus the MIR runtime.

For the maintained source subset, the paths MUST agree on:

- standard output and integer exit status
- return values and pattern results
- checked arithmetic and collection failures
- move/borrow-sensitive mutation and writeback
- `with` cleanup order and primary runtime diagnostics
- task, queue, cancellation, process, filesystem, and network outcomes within platform constraints

The parity matrix executes every eligible runtime fixture through both paths. A fixture may be excluded only through the explicit exclusion list, with a reason that corresponds to an intentional harness or platform boundary rather than an unexplained semantic divergence.

## Documentation Conformance

Reference changes are checked by `npm run check:reference`. The gate requires the normative specification pages, navigation entries, complete grammar anchors, execution-order statement, conformance mapping, and removal of claims that the deleted legacy evaluator remains supported.

The documentation build checks links and rendering. It does not by itself prove that every code block compiles. Language-facing changes therefore also require compiler fixtures or executable examples as directed by `AGENTS.md`.

## Adding Or Changing A Rule

A language or tooling behavior change is complete only when the same pass updates, where relevant:

1. a failing compiler, runtime, CLI, or LSP test
2. the implementation
3. the normative Manual page and grammar when syntax changes
4. the API Index when public APIs change
5. Current Limits when a boundary is added or removed
6. categorized examples and Learn/tutorial material
7. the task board and dated work note

Syntax expansion is frozen for the 0.1 hardening cycle. A new construct therefore needs an explicit compatibility decision rather than being accepted solely because it is easy to parse.

## Deriving A Book

A book may treat this reference as its factual source. It may introduce concepts in a different order, add motivation, diagrams, exercises, and larger examples, or omit advanced details from early chapters. It must preserve these constraints:

- every taught syntax form appears in the complete grammar
- every claimed type or ownership behavior agrees with the static semantics
- every runtime/API claim links back to a maintained contract
- proposal-only features are labeled as future design, not current Aurora
- examples are compiled or run as part of the maintained repository surface

This division lets the reference remain precise while the book remains readable.
