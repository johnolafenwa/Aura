# Batch 4: Trailing Commas And Parenthesized Imports

## Goal

Implement section A of the
[Batch 4 design checkpoint](../architecture_docs/18-batch-4-design-checkpoint.md):
one optional trailing comma in every comma-separated list closed by `)`, `]`,
or `}`, and parenthesized from-import lists.

## Work Completed

- Parser: one `eat_list_comma` helper ends a list at a trailing comma before
  its closing delimiter. It covers arguments, parameters and receivers,
  function-type parameters, type arguments and type parameters, enum
  payloads, implementation trait arguments, collection literals, tuples,
  tuple targets, tuple and variant patterns, multi-index subscripts, and
  lambda capture lists. The specialization and function-type lookahead
  scanners accept the same trailing comma.
- `from module import (a, b as c,)` may span lines. An unparenthesized import
  list or unpack target that ends with a comma is rejected with a message
  that points at parentheses.
- `(x)` stays grouping, `(x,)` stays a singleton tuple, and `grid[0,]` stays
  rejected.
- The design checkpoint, ADR-0063 status, grammar, manual, learn pages,
  tutorials, and the multiline and module examples describe the new rule.

## Verification

- `cargo test -p aura-compiler --lib parser`: 94 passed, including the new
  span-free AST equality test for every list position and the rejection
  table.
- `cargo test -p aura-compiler --test fixtures --test modules`: passed,
  including the new `trailing_commas` run-pass fixture, two parse-fail
  fixtures, and the parenthesized-import module test.
- The `trailing_commas` fixture prints identical output on MIR and the
  direct backend.
- `cargo fmt --all --check`, `npm run check:clippy`,
  `npm run check:doc-examples`, and `npm run check:hygiene` pass.

## Follow-Up

- The formatter uses trailing commas for exploded lists once this lands.
- Two older defects found while writing the fixture, a tuple-literal move
  inside `print` and a direct-backend double release for a shared-loan
  lambda capture, are fixed separately.
