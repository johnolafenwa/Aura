# 2026-03-30 String Utilities

## Goal

Finish the next practical string-utility pass by adding the maintained `String` methods `split`, `replace`, `to_lower`, `to_upper`, `strip_prefix`, and `strip_suffix` across the compiler, runtimes, tooling, examples, and docs.

## Work Completed

- Added builtin member declarations, arity binding, docs, and completion metadata for the new `String` methods.
- Implemented checker typing for the new methods, including `Vec[String]` returns from `split(...)` and `Option[String]` returns from `strip_prefix(...)` / `strip_suffix(...)`.
- Implemented interpreter, MIR runtime, and direct native backend support for the full expanded `String` utility surface.
- Added compiler fixture coverage for the new runtime behavior and updated compiler-backed plus fallback LSP completion tests.
- Expanded the maintained `examples/strings/string_methods.au` example and aligned compiler/CLI example smoke expectations with the new output.
- Updated the string tutorial, current-language-surface reference, root README, CLI README, examples index, and task board.

## Verification

- `cargo test -p aurora-compiler --test fixtures run_pass_fixtures_match_expected_stdout -- --nocapture`
- `npm --prefix tools/aurora-language-server test -- --runInBand analysis.test.js compiler_bridge.test.js`
- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- The next high-value pass is still the broader practical-programming milestone: more string utilities if needed, then another real collection surface such as `Map[K, V]` ergonomics or a new lookup-oriented type.
