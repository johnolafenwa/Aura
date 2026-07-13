# ADR-0002: Integer division and modulo

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D2

## Decision

`int / int` is permanently rejected. Its diagnostic offers `//` for floor
division and `.to_float()` for true division. `//` and `%` use Python floor
semantics, including a remainder with the divisor's sign. Float division is
unchanged.

## Completion tests

- Lexer/parser/operator unit tests in `crates/aurora-compiler/src/`.
- Check-fail fixtures for integer `/` and exact fix text.
- Run-pass fixtures covering signed floor division and modulo on both forced backends.
- LSP diagnostic fixtures under `tools/aurora-language-server/test/`.
