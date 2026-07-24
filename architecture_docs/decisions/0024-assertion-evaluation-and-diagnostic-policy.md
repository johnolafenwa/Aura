# ADR-0024: Assertion evaluation and diagnostic policy

- Status: Provisional
- Date: 2026-07-24
- Roadmap decision: Phase 3 assertion gap-fill policy

## Context

ADR-0011 accepts assertions as the unrecoverable invariant-checking
counterpart to typed `Result` errors. The Phase 3 roadmap fixes the two source
forms and requires backend parity, but it does not by itself settle the exact
default message, eager versus lazy message evaluation, source span, cleanup
precedence, top-level legality, or whether a release mode may remove
assertions. Those choices are observable and must not vary by backend or build
configuration.

## Provisional decision

- An assertion is a statement with exactly one of these forms:
  `assert condition` or `assert condition, message`.
- `condition` must have exactly type `bool`; Aurora applies no truthiness
  conversion. `message`, when present, must have exactly type `String`.
- The condition evaluates exactly once. If it is `true`, execution continues
  and the optional message is not evaluated. If it is `false`, the optional
  message evaluates exactly once before the assertion traps.
- A failure without a message uses the exact text `assertion failed`. A custom
  message is preserved byte-for-byte, including an empty or whitespace-only
  String.
- Assertion failure is `AU4001`, and its primary source location points at the
  `assert` keyword rather than the condition or message.
- A trap while evaluating the condition or message occurs before the
  assertion and remains primary.
- Assertion failure exits active `with` scopes normally. Registered cleanups
  run in reverse order, but a cleanup failure cannot replace the already
  established assertion diagnostic.
- An assertion has normal fallthrough for static control-flow analysis. It
  does not establish a lasting type refinement or make a following path
  statically unreachable.
- Assertions are executable top-level statements in script-style entry
  modules. The existing entrypoint rule still rejects a file that mixes any
  executable top-level statement with a local `main`.
- No optimization, build backend, or CLI mode strips assertions. Aurora does
  not provide an assertion-disable flag.
- The MIR runtime and direct native backend implement the same sequencing,
  exact messages, diagnostic code/span, cleanup behavior, and process failure.

These choices remain Provisional pending the Phase 3 checkpoint review.

## Completion tests

- Lexer/parser/checker tests and parse/check fixtures pin the keyword, both
  forms, exact operand types, span, top-level legality, and fallthrough.
- MIR and direct tests pin once-only condition evaluation, lazy once-only
  message evaluation, exact default/custom/empty/whitespace messages,
  operand-trap precedence, and assertion-over-cleanup precedence.
- Runtime failure fixtures and CLI tests pin `AU4001`, source rendering,
  buffered standard-output ordering, backend parity, and file-level
  `aura test`.
- Compiler-analysis/LSP tests pin identifier occurrences in both operands and
  compiler/recovery keyword completion. Extension packaging tests pin syntax
  highlighting in the shipped grammar.
- The maintained example, tutorial, Manual, API index, conformance map, and
  executable reference fence document the same contract.
