# ADR-0002: Integer division and modulo

- Status: Accepted
- Date: 2026-07-13
- Amended: 2026-07-22 (Phase 3 `FloorDiv` trigger)
- Roadmap decision: D2

## Decision

Builtin `/` and `/=` are permanently rejected when both operands are the same
integer type. Both forms use the same teaching diagnostic:

```text
integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
```

Float `/` and `/=` remain true division. User-defined non-numeric `/` remains
the `Div.div` operator-trait spelling.

`//` is builtin floor division for equal integer operands and equal floating
operands. `//=` is its compound-assignment form. Together with the existing
operators, the complete arithmetic compound-assignment family is `+=`, `-=`,
`*=`, `/=`, `%=`, and `//=`.

### Phase 3 amendment: `FloorDiv`

The Phase 3 `Duration // int64` operation activates the extension point that
this decision reserved. Aurora now has a `FloorDiv[Rhs, Out]` operator trait
whose required method is `floor_div(borrow self, rhs: Rhs) -> Out`. Builtin
numeric floor division and the builtin Duration rule take precedence; when no
builtin rule applies, `//` and `//=` may dispatch through one applicable
`FloorDiv.floor_div` implementation. This is an additive change to the
original D2 surface and does not alter any numeric result.

For integer operands and nonzero divisor `b`, floor division and remainder
satisfy `a == (a // b) * b + (a % b)`, `a // b` is the mathematical quotient
rounded toward negative infinity, and a nonzero `a % b` has the divisor's
sign. An unrepresentable quotient is checked integer overflow. Integer `//`
and `%` by zero trap.

For floating operands, `//` and `%` use the CPython-compatible floating
divmod algorithm, including its rounding correction and divisor-sign
remainder rule. Floating `//`, `/`, and `%` by either signed zero trap rather
than produce IEEE infinity or NaN.

Every integer type has `.to_float() -> float64`. It performs IEEE-754 binary64
conversion in round-to-nearest, ties-to-even mode and may round; for example,
`9007199254740993.to_float()` is `9007199254740992.0`. The explicit
`as float64` cast remains an exactness-checked conversion and rejects that same
loss of precision.

## Rationale

Rejecting integer `/` prevents a familiar spelling from silently selecting
either truncating division or floating true division. `//` makes the requested
integer quotient explicit, while `.to_float()` makes a possibly rounding
numeric-domain change explicit. Matching Python floor/remainder behavior keeps
the quotient/remainder identity useful for negative values and avoids backend
drift in difficult floating-point cases. Adding `FloorDiv` only when a
heterogeneous language operation needs it keeps the original builtin numeric
semantics stable while giving `Duration // int64` and user-defined nonnumeric
types one ordinary operator-dispatch contract.

## Completion tests

- Lexer/parser/operator unit tests in `crates/aurora-compiler/src/`.
- Check-fail fixtures for integer `/` and `/=` with the exact fix text.
- Run-pass fixtures covering signed integer and floating floor division and
  modulo, compound assignments, zero failures, and `.to_float()` rounding.
- Backend-parity coverage for all observable arithmetic results and traps.
- `FloorDiv` declaration, dispatch, ambiguity, compound-assignment, and
  builtin-precedence tests, including `Duration // int64`.
- LSP diagnostic fixtures under `tools/aurora-language-server/test/`.
- Manual, tutorial, maintained example, and editor-grammar updates.
