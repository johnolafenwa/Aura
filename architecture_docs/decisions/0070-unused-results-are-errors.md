# ADR-0070: An unused `Result` is an error

- Status: Accepted
- Date: 2026-11-02
- Builds on: ADR-0057

## Context

An expression statement drops its value. When that value is a `Result`, its
error goes with it: `save("settings.conf")` on a full disk printed nothing,
and the program carried on as if the save had worked. The book had to warn
readers about it. Silently dropped errors are the bug class `Result` exists
to prevent. John approved making this an error on 2026-11-02 (row 5 of the
language-design questions), knowing it breaks existing code.

## Decision

- An expression statement whose type is `Result[T, E]` is rejected with
  `AU2018` ("unused result"), with a help line pointing at `try`, `match`, and
  `_ = ...`.
- `_ = expression` is an expression statement that discards its value on
  purpose. It binds no name, so it can repeat in one scope, and it is the only
  way to drop a `Result`.
- Only `Result` is covered. Other values, including `T | None`, can still be
  dropped by an expression statement.
- `Queue.put`, `Queue.try_put`, `process.run`, and the network calls return a
  `Result`, so standalone calls to them now need `_ =` or handling. Every
  fixture, example, benchmark, and documentation page in the repository was
  updated.

## Implementation

- The parser turns `_ = expr` into an `ExprStmt` with `discarded: true`
  rather than an assignment, so nothing downstream sees a binding named `_`.
- The checker's expression-statement arm rejects a `Result` type unless the
  statement is `discarded`. MIR and both backends are unchanged: they already
  drop an expression statement's value.
