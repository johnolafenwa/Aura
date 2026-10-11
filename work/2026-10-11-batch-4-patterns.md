# Batch 4 Section B: Rest, Sequence, And Class Patterns

## Goal

Implement section B of the Batch 4 design checkpoint
(`architecture_docs/18-batch-4-design-checkpoint.md`) on both backends:

- B1: one `*name` or `*_` rest element in tuple unpacking targets and tuple
  patterns
- B2: list sequence patterns such as `[]`, `[x]`, and `[first, *rest]` in a
  bare `match`
- B3: keyword class patterns such as `Point(x=0, y=y)`, over a class
  scrutinee or a union member class

Language-server analysis must give hover and definition for the new pattern
bindings.

## Work Completed

- AST: `BindingTarget::Rest`, `Pattern::Rest`, `Pattern::Sequence`,
  `Pattern::Class`, and the shared `rest_element_ranges` helper.
- Parser: rest targets and rest elements, sequence patterns, keyword class
  patterns, and placement checks. New codes `AU2040` (invalid class pattern)
  and `AU2041` (invalid rest pattern or target) are registered.
- Checker: rest ranges and tuple-slice types, sequence and class binding
  rules per capability, class resolution through imports and module aliases,
  field visibility, union member selection, and coverage for lists and
  classes. Enum variant patterns over a class scrutinee report `AU2040`.
- MIR lowering: tuple rest values, one length read per sequence arm, element
  reads, owned rest slices, class field reads, union member views, and
  `match own` field moves. The direct backend consumes the same MIR.
- Analysis: hover and definition for rest, sequence, and class bindings, plus
  class-name and field-name occurrences in class patterns.
- Fixtures: five run-pass fixtures, one check-path-fail fixture, five
  parse-fail fixtures, and nineteen check-fail fixtures. Retired the
  class-pattern-deferred fixtures and renamed the class variant-shape fixture.
- Docs: `enums-and-match`, `tuples`, `grammar`, `current-limits`,
  `conformance`, and `diagnostics` manual pages; the collections and data
  modeling learn pages; tutorials 09, 14, and 25; the new
  `examples/enums/structural_patterns.au`; the ADR-0063 implementation line;
  and section B4 of the design checkpoint, which records the refinements.

## Verification

- `cargo fmt --check`
- `npm run check:clippy`
- `cargo test -p aura-compiler --test fixtures`
- `cargo test -p aura-compiler --lib`
- targeted `batch1_coverage_sema` and `cli` tests
- `npm run check:doc-examples`
- `npm run check:hygiene`
- `npm run test:lsp`
- MIR and direct backend outputs compared for every new run-pass fixture and
  the new example.

Local runs used `CARGO_TARGET_DIR` inside the worktree. The coverage gate and
the full backend-parity matrix run in hosted CI.

## Follow-Up

- `match mut` for class and sequence patterns, and `match own` for sequence
  patterns, need element and field writeback rules.
- Properties, when they land, must stay rejected inside class patterns.
- Regenerate `docs/public/llms-full.txt` with `npm run docs:llms` after the
  Batch 4 branches merge.
