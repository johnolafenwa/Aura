# 2026-03-22 Pass And Sleep

## Goal

Close the remaining completeness gap around intentionally empty blocks and simple time-based delays by adding the `pass` statement and a maintained `sleep(duration)` builtin.

## Work Completed

- Added `pass` to the lexer, parser, AST, semantic checker, interpreter, MIR lowering, MIR runtime, and compiler analysis.
- Added `sleep(duration)` to the shared builtin call model, semantic checking, interpreter runtime, MIR runtime, compiler analysis, and editor fallback analysis.
- Added parse/check/run fixtures for both features.
- Added maintained runnable examples:
  - `examples/basics/pass_keyword.au`
  - `examples/concurrency/sleep_builtin.au`
- Updated tutorials and example documentation so the user-facing surface matches the implementation.
- Updated repo-level and CLI README files so the new maintained surface is discoverable.

## Verification

- `cargo test`
- `npm run test:lsp`
- `npm run test:extension`

## Follow-up

- Continue auditing the remaining proposal-v1 gaps now that the language surface includes `pass`, `sleep(duration)`, default parameters, traits, generics, and numeric casts.
- Move from measured coverage toward enforced compiler/LSP/package gates before expanding the semantic surface much further.
