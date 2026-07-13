# ADR-0012: Boolean-only conditions

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D12

## Decision

Conditions require `bool`; Aurora does not use implicit truthiness. A
diagnostic suggests the relevant explicit form, such as `len() > 0`,
`== None`, or `match`.

## Completion tests

- Check-fail fixtures for numbers, strings, collections, Option, and user types.
- Diagnostic fixtures pinning context-specific suggestions and move-site spans.
- LSP diagnostic and code-action coverage.
