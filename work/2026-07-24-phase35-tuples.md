# Phase 3.5 tuples

## Goal

Implement the second Phase 3.5 language change as one minimal, provisional
tuple kernel: parenthesized fixed-arity values and types, function returns,
recursive assignment/loop unpacking and patterns, whole-source ownership, and
copy-only constant indexing.

## Work Completed

- Froze the intended semantics in Provisional ADR-0026, including recursive
  Copy classification, left-to-right capture, whole-source non-Copy moves,
  shared unpack provenance, owned `own`/Queue items, canonical rendering, and
  the absence of equality/order and mutable tuple writeback.
- Defined the exact grammar distinction between parenthesized tuple value
  expressions and top-level comma-separated assignment/`for` targets.
- Added the normative Tuples Manual page and aligned Grammar, Lexical
  Structure, Types, Functions, Expressions, Statements, pattern matching,
  ownership, status, Current Limits, conformance, Manual navigation, and the
  executable-reference metadata.
- Added the maintained `examples/basics/tuples.au` oracle and
  `tutorials/25-tuples.md`, then updated the example/tutorial indexes and
  current-surface tutorial.
- Added reference guards for the tuple grammar, ADR, Manual, example, tutorial,
  conformance evidence, and stale pre-tuple wording.
- Implemented tuple syntax and recursive binding targets across the AST,
  parser, checker, analysis API, MIR, direct code generator, and both runtime
  value layers while preserving the legacy JSON shape for named types and
  single-name `for` targets.
- Implemented recursive Copy classification, left-to-right one-time tuple
  evaluation, whole-source moves for non-Copy unpacking, shared-borrow
  provenance, owned Queue-receive unpacking, and copy-only non-negative
  constant indexing.
- Added product-aware exhaustiveness and unreachable-arm analysis for boolean,
  enum, nested tuple, and correlated tuple patterns while retaining wildcard
  requirements for open domains.
- Added teaching diagnostics for arity/type mismatches, invalid mutable-borrow
  unpacking, use after move, non-Copy/dynamic/negative/out-of-bounds indexing,
  tuple comparison, and recursive tuple fields. Recursive links must live in a
  separately named indirect field because tuple types themselves cannot be
  indirect.
- Added compiler, CLI, runtime ABI, native-codegen, analysis/LSP, fixture, and
  backend-parity regressions, including generic tuple returns and exact
  MIR/direct output parity.

## Verification

- The focused tuple compiler suite passes 39 tests within the 847-test library
  suite, including parser/checker/MIR/runtime/native/analysis public-surface
  behavior.
- All fixture categories pass, including exact diagnostics and exact
  MIR/direct output for exhaustive tuple-pattern unions.
- The language-server bridge passes all 66 tests, including tuple annotation,
  nested binding scope, index hover, and definition regressions.
- The executable-reference integrity gate passes all nine tests, the docs
  build passes, and `git diff --check` is clean.
- The exact compiler coverage gate passes all 259 instrumented CLI tests, 847
  compiler library tests, and supporting suites at 62,917/65,489 lines
  (96.072622883232299%), 4,097/4,225 functions (96.970414201183431%), and
  92,077/97,666 regions (94.277435340855575%), above the frozen
  96.06/96.79/94.15 floors.
- Coverage closure used only observable behavior: exact diagnostics, recursive
  product-pattern exhaustiveness, ownership-sensitive MIR/runtime behavior,
  imported/generic tuple identity, and native specialized trait dispatch.
  No synthetic-coverage test or coverage exclusion was added.
- The coverage pass also removed or collapsed defensive branches whose
  preconditions are already guaranteed by parsing, checking, or MIR
  validation: a second tuple-type fallback in `lower_type_with_self`,
  wrong-shape/arity tuple-pattern branches after successful checking,
  consuming variant registration at a caller that accepts only irrefutable
  patterns, and a duplicate native tuple-literal arity rejection after
  `validate_rvalue`. These are invariant-preserving restructures, not hidden
  exclusions; retained `debug_assert`/`unreachable!` checks document the
  internal contracts.

## Follow-Up

1. Run the complete `npm run ci` decision gate.
2. Commit this ticket as one logical Phase 3.5 decision before starting
   conditional expressions.
