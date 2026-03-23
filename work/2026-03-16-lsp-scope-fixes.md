# 2026-03-16 LSP Scope Fixes

## Problem

The Aurora language server was producing false diagnostics in top-level script files and had a gap in member resolution for parenthesized receiver expressions.

Examples:

- `examples/control_flow.au` showed `unknown name total`
- completions and navigation around `(dx * dx + dy * dy).sqrt()` were incomplete

## Cause

The current LSP analysis layer is still a lightweight JavaScript semantic pass. It had two limitations:

- it only tracked bindings declared inside functions, not top-level script bindings
- receiver extraction for `.` only handled simple identifier chains, not parenthesized expressions

## Fix

- added top-level binding collection to the analysis model
- resolved top-level bindings for diagnostics, hover, and go-to-definition
- generalized `.` receiver extraction so parenthesized expressions can participate in member lookup and completion
- added regression tests covering both cases

## Verification

- `npm run test:lsp`
  - passed
- `npm run check:extension`
  - passed
- `npm run test:extension`
  - passed

## Follow-up

- added direct regression tests for the exact `examples/point.au` `.sqrt()` path
- added `npm run coverage:lsp` so the package now has a repeatable coverage report instead of relying on spot checks
