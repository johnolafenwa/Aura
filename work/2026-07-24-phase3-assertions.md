# Phase 3 assertions

## Goal

Complete the Batch 2 Phase 3 assertion statement as a language and product
surface: exact syntax and types, deterministic evaluation and failure
semantics, MIR/direct parity, compiler-backed editor behavior, file-level test
behavior, maintained examples and tutorials, and a reference-quality normative
specification.

## Test-first evidence

- Added parser, checker, run-pass, and run-fail fixtures before the complete
  lowering/runtime implementation. The first fixture run stopped at the
  missing MIR `assert` lowering arm, establishing the intended implementation
  gap.
- Added recovery and extension-package regressions before updating their
  keyword lists. They initially failed because `assert` was absent from the
  recovery keyword set and TextMate grammar.
- Added CLI product tests that execute both maintained backends and pin the
  diagnostic, observable side effects, trap precedence, cleanup precedence,
  top-level behavior, and file-level `aura test` result.

## Work completed

- Added `assert condition` and `assert condition, message` statements. The
  condition must be exactly `bool`; an explicit message must be exactly
  `String`.
- Defined the exact `AU4001` failure text at the `assert` keyword for the
  default message, custom message, empty message, and whitespace-only message.
- Made evaluation deterministic: the condition is evaluated once, the message
  is evaluated lazily and at most once, prior stdout remains ordered, operand
  traps precede assertion failure, and assertion failure remains primary when
  scope cleanup also traps.
- Preserved assertion statements through lowering and both execution
  backends. Top-level script assertions are valid only for scripts without a
  declared `main`; a file that combines top-level executable assertions with
  `main` is rejected consistently with the existing script-entrypoint carve-out.
- Added source occurrence analysis for assertion operands and compiler-backed
  LSP coverage for exact operand-use ranges, hover, definition, and keyword
  completion. Invalid assertion analysis now pins the `AU2002` diagnostic and
  keyword range exposed to the editor. Added recovery support and VS Code
  syntax/package coverage.
- Kept the accepted D12 boolean-only policy instructional: a non-boolean
  assertion condition includes explicit-comparison help rather than reporting
  only a type mismatch.
- Added a source-starting ownership regression showing that a bare `String`
  message lowers as a borrowed place, stays lazy on the true path, is
  snapshotted exactly once for the selected diagnostic, and leaves its source
  allocation intact.
- Added `examples/basics/assertions.au`, registered its exact stdout in the
  maintained example smoke matrix, and added the book-style
  `tutorials/23-assertions.md` chapter.
- Added the normative Assertions Manual chapter and updated grammar, lexical
  structure, statements, static semantics, execution, diagnostics,
  conformance, API/navigation, CLI/tooling, and reference-integrity metadata.
  The grammar explicitly keeps the assertion separator comma outside each
  non-tuple operand.
- Added Provisional ADR-0024 to record evaluation, diagnostic, entrypoint, trap,
  cleanup, backend, and tooling policy.

## Verification

- `cargo test -p aurora-compiler --test fixtures -- --test-threads=1` passes
  all nine fixture categories.
- The focused `aura` CLI assertion matrix passes on both MIR and direct
  execution, including exact default/custom/empty/whitespace diagnostics,
  condition/message evaluation counts and ordering, operand-trap precedence,
  cleanup precedence, and file-level test behavior.
- The focused compiler assertion suite passes all 12 tests, including the
  source-starting lazy-message ownership regression.
- The direct-runtime assertion suite also pins the exported ABI boundary:
  non-String opaque messages fail with the exact defensive `AU4001` without
  consuming the caller-owned value, and invalid source coordinates produce
  unspanned default/custom diagnostics without changing ownership.
- `npm run test:lsp` passes all 60 tests, including exact assertion operand
  ranges, invalid diagnostic placement, hover results, definitions,
  completion, and recovery.
- `npm run test:extension` passes all 10 tests, including source grammar and
  packaged-language-server checks.
- `npm run check:reference` passes across 33 pages: 23 feature pages, 10
  structural pages, 237 total fences, 187 Aurora blocks, and 108 verified
  Aurora blocks. The Assertions page contains one verified example and one
  illustrative output fence.
- `npm run docs:build` passes. Its existing unsupported-language highlighting
  fallbacks and chunk-size advisory remain warnings only.
- The maintained assertion example type-checks and its focused execution emits
  exactly `checking` followed by `all assertions passed`. A broader combined
  example run encountered an unrelated empty-stdout result from the existing
  TCP echo example while concurrent workspace work was active; the focused
  assertion smoke itself passed.
- `npm run coverage:compiler:check` passes all 256 instrumented CLI tests, 795
  compiler library tests, and supporting suites at 60,904/63,399 lines
  (96.06460669726651%), 3,976/4,099 functions (96.99926811417419%), and
  88,875/94,275 regions (94.27207637231504%), above the frozen
  96.06/96.79/94.15 floors. The first otherwise-green coverage pass was five
  covered lines short of the line floor; the gap was closed only with the
  observable direct-runtime diagnostic and ownership tests described above.
  No synthetic-coverage test, exclusion, or production-only coverage
  restructuring was added.
- The complete exact-tree `npm run ci` decision gate passes.

## Follow-up

- Present Provisional ADR-0024 with the other Phase 3 gap-fill decisions at the
  checkpoint.
- Continue Phase 3 with the retry-worker gate after the assertion decision
  commit.
