# 2026-03-15 Print Rename Session

## Goal

Rename the builtin line-printing function from `println` to `print` across the language surface and tooling.

## Changes

- semantic checker now recognizes `print(...)` instead of `println(...)`
- interpreter now executes `print(...)` and preserves the current newline-terminated output behavior
- examples and proposal snippets now use `print`
- language-server top-level completions now include `print`
- regenerated `docs/aurora_language_proposal.html`

## Verification

- `cargo test` passed
- `npm run test:lsp` passed
- `npm run test:extension` passed
- `examples/point.au`, `examples/basic_addition.au`, and `examples/top_level_addition.au` all run successfully with `print`
