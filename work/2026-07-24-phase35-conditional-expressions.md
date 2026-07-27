# Phase 3.5 Conditional Expressions

## Goal

Implement the Batch 2 expression-kernel form
`value if condition else alternative` as one frozen language/tooling packet,
without beginning a later Phase 3.5 ticket.

## Test-First Evidence

- Parser tests and parse/check/run fixtures were added before the AST and
  parser accepted the form. The first focused compiler run failed because
  `ExprKind::Conditional` did not yet exist.
- The fixtures pin missing `else`, exact-bool conditions, contextual arm
  typing, incompatible arms, conditional moves, condition-first evaluation,
  one-arm laziness, right-associated nesting, grouping, and nested contextual
  collection literals inside tuple arms.
- A MIR regression first exposed a reachable nested-context defect:
  `([], 1) if ready else (values, 2)` retained `Vec[Unknown]` instead of the
  concrete peer arm's `Vec[int32]`. The recursive type-context fix made that
  test pass and the maintained run fixture now pins both selected paths.
- Ownership regressions then exposed that separately type-checking an
  expression and consuming its result could either reject an exactly-once
  branch transfer (`take(text) if flag else text`) or miss a source-ordered
  double move (`(text if flag else "fallback", take(text))`). Focused tests
  were red for both defects before the result-use replay repair.
- A full-suite audit of the replay repair itself then found three reachable
  regressions in previously green behavior, each reproduced before it was
  fixed:
  - Every member expression was routed through branch-aware result
    consumption, so enum-variant and module-qualified paths such as
    `io.Error.NotFound`, `json.Value.Null`, `process.RestartPolicy.OnFailure`,
    and `Outer.Empty` were type-checked as field reads of a value object and
    rejected with `AU2002`.
  - Call-argument place collection resolved module-rooted paths as places, so
    `json.dumps(json.Value.Null)` failed while `json.dumps(value=...)` passed.
  - Call-argument place collection dropped every copy-typed access, losing the
    retained `mut int32` access that
    `call_borrow_mut_then_copy_read_rejected` pins, and the new source-ordered
    rejection pre-empted the parameter-aware same-level overlap diagnostic for
    plain place arguments.

## Work Completed

- Added the conditional AST form and Python-compatible lowest-precedence,
  right-associative parser production.
- Added exact-bool checking, one result type with expected-context propagation,
  and conservative branch ownership-state merging.
- Made contextual inference structural across named generic types and tuples,
  and made condition and both arm type failures explicitly use `AU2002`.
- Added pre-expression, source-ordered result-use replay for owned composite,
  conditional, and match results. Replay entries are isolated during both
  speculative typing and nested replay, so probes cannot overwrite live
  ownership state.
- Extended peer-arm context exchange through list, set, and map literal
  shapes, propagated owned transfer through `try`, and covered projected
  fields of conditional and match results without moving unrelated fields.
- Included direct conditional result places in call access-overlap checking,
  so a possible owned arm conflicts with a retained shared argument in either
  source order.
- Scoped that repair so it adds coverage without displacing existing behavior:
  branch-aware member consumption is used only when the member object is a
  conditional or match result; module-rooted paths are not places; copy-typed
  arguments retain their access unless they are passed by value; and the
  source-ordered rejection defers a plain place argument to the pairwise
  same-level check that produces the parameter-aware diagnostic.
- Lowered the expression to an explicit condition branch and typed join value,
  shared by MIR execution and direct native code generation.
- Extended compiler analysis and the language-server bridge so every operand
  retains hover/definition coverage and invalid conditions expose `AU2002`.
- Added the maintained run fixture and
  `examples/control_flow/conditional_expressions.au`, updated the example
  indexes and control-flow tutorial, and froze the normative grammar, typing,
  execution, ownership, and conformance rules.
- Added Provisional ADR-0027 for checkpoint review. No Python-hint fixture was
  retired because the existing hint family had no conditional-expression
  rejection.

## Verification

- Focused parser, semantic, MIR, and analysis tests pass.
- The focused conditional compiler suite passes all 32 tests, including
  symmetric contextual inference, nested tuple context, default-parameter
  references, owned-arm moves, compatible borrowed returns, retained-access
  conflicts, editor hover/completion, and exact diagnostic codes.
- Additional ownership-focused checks pass for both direct/call arm orders,
  same-arm double moves, nested composite ordering, speculative nested calls,
  condition-side moves, prior partial-field moves, conditional and match
  field projection, `try` transfer, and shared/owned call overlap.
- A new focused regression pins that module-qualified builtin enum variants and
  user enum payload-free variants stay consumable in annotated bindings, host
  builtin arguments, collection literals, and owned user-function arguments.
- The complete compiler library, fixture, integration, and 259-test `aura` CLI
  product suites pass on the repaired tree, including the four B2.0 retained
  non-copy regressions and the parameter-aware same-level overlap oracles.
- The check-pass fixture family accepts contextual integer, floating, and
  `Option` arms.
- The conditional run fixture produces its exact stdout through MIR.
- A forced-direct binary for the same fixture produces the identical exact
  stdout, including condition-before-arm ordering, unselected-arm laziness,
  and nested empty-collection selection (`0` then `2`).
- The executable reference inventory passes after the grammar contract hash
  update.
- The exact compiler coverage gate passes at 63,752/66,360 lines
  (96.06992163954189%), 4,137/4,268 functions (96.9306466729147%), and
  93,478/99,158 regions (94.27176828899331%), above the frozen
  96.06/96.79/94.15 floors.
- The coverage closure consists entirely of observable typing, diagnostics,
  ownership, analysis/completion, runtime, and backend-parity behavior. No
  synthetic test or coverage exclusion was added. Two duplicated walks in the
  replay repair were collapsed, each with its invariant retained and stated in
  the source:
  - The replay walk no longer restates the type rules its own precondition
    already proved; it reproduces the accepted result type and reports
    ownership diagnostics only.
  - The direct consumption walk no longer carries a second, non-branch-aware
    copy of composite, conditional, match, cast, and `try` handling; those
    shapes delegate to the single branch-aware walk from the current state.
- The complete `npm run ci` decision gate exits zero on the integrated tree,
  including `cargo fmt --all --check`, the 888-test compiler library suite, the
  259-test CLI product suite, every fixture and integration suite, the full
  forced MIR/direct parity matrix, the 67-test language-server suite and its
  enforced 100% coverage, the extension checks and 13-test extension suite, the
  compiler coverage floors, executable reference integrity, the documentation
  build, `npm audit` and `cargo audit`, strict Clippy, and hygiene.

## Follow-Up

- Present Provisional ADR-0027 at the Batch 2 checkpoint.
- Continue with the next authorized Phase 3.5 expression-kernel ticket after
  this packet is integrated and full-gated.
