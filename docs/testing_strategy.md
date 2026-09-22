# Aura Testing Strategy

This page defines how the Aura repository is tested. The goal is not a high
test count. The goal is to fail early when language behavior, diagnostics, or
editor tooling regress.

## Principles

### 1. Test First

For language features and bug fixes:

1. add a failing test or fixture first
2. implement the feature or fix
3. verify the full relevant surface

### 2. Test The Product Surface

For a language project, the product includes:

- parse results
- type-checking behavior
- runtime behavior
- MIR lowering behavior
- diagnostics
- CLI output
- LSP and editor behavior
- runnable examples

MIR is Aura's mid-level intermediate representation. LSP is the Language
Server Protocol that the editor uses to talk to the compiler.

### 3. Regression Tests Are Mandatory

Every bug fix adds a regression test that reproduces the real failing case as
directly as possible. If a bug appeared in a real example file, test that
file.

## Test Layers

### Compiler Library: `crates/aura-compiler`

Use these layers together:

- unit tests for small helper logic
- fixture tests for parser, checker, runtime, and diagnostic behavior
- example smoke tests for maintained `.au` programs
- MIR tests for structural lowering invariants

Recommended fixture categories:

- `parse-pass`
- `check-pass`
- `check-fail`
- `run-pass`
- `mir-pass`

### CLI: `crates/aura`

CLI tests verify:

- command success
- command failure
- annotated diagnostics
- stable command semantics for `check`, `run`, `build`, `new`, `fmt`, `test`, `ast`, `ast-json`, `analyze`, `complete`, `lsp`, and `mir`

### Language Server: `tools/aura-language-server`

The LSP test suite covers:

- compiler-bridge behavior
- completions
- diagnostics
- hover
- go-to-definition
- scope tracking
- parenthesized receiver and member resolution
- real example files

Measure coverage continuously. The package moves toward enforced 100%
coverage before its semantic surface expands further.

### VS Code Extension: `tools/vscode-aura`

Extension tests stay focused on:

- build integrity
- bundle integrity
- packaged install integrity

Semantic logic belongs in the language server, not in the extension package.

### Examples And Tutorials

Examples are part of the supported implementation surface. Exercise them in
tests where practical.

Tutorials do not run themselves. Each tutorial chapter points to maintained
examples that run in CI or local verification.

## Coverage Policy

Coverage is useful when it is tied to stable modules and real behavior tests:

- Mature packages move toward 100% enforced coverage.
- Fast-changing packages may run below that threshold for a while. The gap
  must be visible and intentional.
- Every regression adds direct behavior coverage, not only line coverage.

### Commands

| Purpose | Command |
| --- | --- |
| Compiler and examples | `npm run test:rust`, equivalent to `RUST_MIN_STACK=33554432 cargo test` |
| Compiler coverage | `npm run coverage:compiler`, `npm run coverage:compiler:check` |
| Language server coverage | `npm run coverage:lsp`, `npm run coverage:lsp:check` |
| Full repository gate | `npm run ci` |
| Native and MIR behavior parity | `npm run test:backend-parity` |
| Scheduler verification | `npm run test:scheduler-model`, `npm run test:scheduler-stress` |
| Hygiene and release readiness | `npm run check:format`, `npm run check:clippy`, `npm run check:audit`, `npm run check:hygiene`, `npm run docs:build` |

`npm run check:audit` runs the npm advisory audit and the RustSec
`cargo audit`.

A weekly safety and performance workflow runs:

- parser, checker, and MIR fuzz targets under `fuzz/`
- AddressSanitizer tests for the runtime FFI and generated direct binaries
- a compiler pipeline benchmark artifact

FFI is the foreign function interface.

### Enforced Floor

| Package | Metric | Floor |
| --- | --- | --- |
| Compiler | lines | `96.46%` |
| Compiler | functions | `97.33%` |
| Compiler | regions | `95.23%` |
| Language server | statements | `100%` |
| Language server | branches | `100%` |
| Language server | functions | `100%` |
| Language server | lines | `100%` |

These floors are non-regression gates. They are not a roadmap to 100%
compiler coverage. A floor may rise but never fall. New behavior still needs
focused tests.

Compiler coverage runs the full Rust workspace test suite but reports only
compiler production code:

- It ignores sibling `crates/aura-compiler/src/*_tests.rs` files, which only
  hold extracted unit-test scaffolding.
- It excludes `crates/aura/**` from the reported files. CLI product tests can
  exercise compiler behavior without counting CLI source toward the compiler
  floor.

GitHub Actions mirrors the local gate:

- CI runs on Linux and macOS.
- The docs workflow builds the VitePress book and deploys it to GitHub Pages.
- The release workflow builds platform CLI archives, the VS Code extension,
  and the static docs archive for GitHub Releases.

### Hosted Runner Reliability

Local and hosted gates complement each other. A local green gate cannot prove
environment-conditional Linux or macOS behavior. Work is not complete until
you have inspected the latest GitHub Actions runs with `gh run list`.

**Timing tests.** Some tests pass only if an operation finishes under a
measured wall-clock limit. One CI-aware margin policy covers that whole
family: scheduler cancellation wakeups, safepoint latency, blocking-service
timeout budgets, socket timing, and related reactor fairness probes.

- Local limits keep their calibrated real-hardware values.
- Under `GITHUB_ACTIONS`, the limit and any deliberately slow comparison
  operation are both scaled by four. The test can still tell prompt progress
  from blocked behavior, and shared runners get scheduling headroom.
- Ordering tests, such as bounded queues, use explicit host and program
  handshakes instead of time estimates.
- Ordinary timeouts that only guard against deadlock stay unchanged.
- The Rust suite stays parallel on both hosted systems.

**Job budget.** The complete cold-run repository gate has a 180-minute job
budget on the standard runners. The budget stops the workflow from being
cancelled during compilation, parity, and coverage. It does not relax any
individual test contract.

**Runner setup.**

- CI installs every non-system command the gate needs, including `ripgrep`.
  The gate does not depend on what a particular hosted runner image happens
  to contain.
- CI fetches `HEAD` and its parent. The commit-level whitespace gate then
  compares the new commit instead of treating a shallow `HEAD` as the
  repository root.
- Linux test executables that exercise the name-based FFI adapter export
  their test-owned C symbols dynamically. This matches the lookup contract
  that generated Aura programs use.

## Workflow For A New Feature

When adding a new Aura feature:

1. add a failing compiler fixture
2. add a failing runtime or diagnostic fixture if needed
3. add or update an example
4. add or update an LSP test if editor behavior should change
5. implement the compiler, runtime, or tooling change
6. update tutorial chapters that teach the feature
7. record the work in `work/`

## Workflow For A Bug Fix

When fixing a bug:

1. reproduce it with a failing test first
2. prefer testing the real file or exact expression that failed
3. fix the behavior
4. run the relevant package test suites
5. record the regression in `work/` if it was non-trivial

### Native foundations and Rust references

`npm run ci` also runs the unit tests for the integer-loop and binary-size
tooling. It builds the pinned standalone Rust references for exact protocol
and checksum smoke checks. It publishes no benchmark timing. The advisory
review of the Rust references is recorded in the foundations work note. It is
not part of the repository advisory gate.
