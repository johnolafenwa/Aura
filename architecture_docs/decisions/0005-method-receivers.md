# ADR-0005: Method receivers

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D5

## Decision

Bare `self` is a shared borrow. `own self` is the consuming receiver and
`borrow mut self` remains the mutable receiver. A first parameter spelled
`self: SomeType` is rejected with a diagnostic naming the receiver forms.

## Completion tests

- Parser and semantic unit tests for all receiver forms.
- Check-pass/check-fail fixtures pinning move behavior and diagnostic text.
- MIR/direct method-call parity fixtures.
- LSP hover/completion/diagnostic tests plus class examples and tutorials.
