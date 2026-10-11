# Batch 4: Syntax-Reflowing Formatter

Date: 2026-10-11. Branch: `claude/batch-4-kcxtn7-fmt`.

## Goal

Implement section F of the
[Batch 4 design checkpoint](../architecture_docs/18-batch-4-design-checkpoint.md#f-syntax-reflowing-formatter).
Replace the whitespace-only `aura fmt` with a token-based formatter. Add
`aura fmt --stdin` and language-server whole-document formatting.

## Work Completed

- `crates/aura-compiler/src/format.rs` holds the formatter engine, so the
  compiler coverage gate measures it. It scans tokens with exact source text,
  groups them into statement lines, and lays out bracketed groups.
- Rules: four-space indentation, operator, comma, and colon spacing, blank-line
  caps with two blank lines around top-level declarations, 88-column reflow
  that breaks the outermost bracket first, magic trailing commas, comment
  preservation, and byte-exact literals.
- Safety: the result must lex and parse, and its token stream must match the
  input, including `Newline`, `Indent`, and `Dedent`. Otherwise `aura fmt`
  reports an internal formatter error and writes nothing.
- A block-form `match` expression inside brackets keeps its original lines,
  shifted to the new indentation. So does a statement whose comment cannot
  stay at the end of its line in the new layout.
- `aura fmt --stdin [--check]` formats standard input to standard output.
- The `aura lsp` compiler service has a `format` method. The language server
  advertises `documentFormattingProvider` and returns one whole-document edit.
  The VS Code extension gets formatting through its language client.
- `EMIT_TRAILING_COMMAS` in `format.rs` controls the trailing comma on
  exploded lists. It is off because the parser on `main` rejects most trailing
  commas. Turn it on once section A merges, then update the golden tests and
  the "no trailing comma" sentences in `docs/manual/cli-and-tooling.md` and
  `docs/manual/current-limits.md`.
- Updated the CLI manual, current limits, crate and tool READMEs, the tooling
  tutorial, ADR-0063, and the implementation notes in checkpoint section F.

## Repository Formatting

CI does not run `aura fmt --check` on the repository. So this change does not
reformat the maintained `.au` files. Running the new formatter over
`examples/` would rewrite most files. The main reason is the rule of two blank
lines between top-level declarations, because the examples use one. Golden tests
check that every file under `examples/` and every `aura` block in `tutorials/`
and `docs/manual/` formats idempotently with an unchanged token stream. They
check every compiler fixture that parses too.

## Verification

See the final report for the exact commands and results.

## Follow-Up

- Turn on `EMIT_TRAILING_COMMAS` after section A lands.
- Decide whether to reformat `examples/` and the documentation code blocks in
  one dedicated change, and then add `aura fmt --check examples` to CI.
- Breaking long expressions at binary operators inside brackets is not
  implemented. Such a line stays long once its brackets cannot break further.
