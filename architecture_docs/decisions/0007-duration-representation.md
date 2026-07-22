# ADR-0007: Duration representation

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D7

## Decision

`Duration` is a copy value represented as a signed 128-bit count of
nanoseconds. The `ms`, `s`, and `m` literal suffixes scale their non-negative
integer payloads to nanoseconds and reject values outside that signed range.
There is no `ns` literal suffix and no unary `-Duration`; negative values are
created only by constructors or checked arithmetic.

Direct code passes every Duration literal exactly as the low and high 64-bit
limbs of its signed two's-complement nanosecond count. The native runtime
reconstructs the same signed 128-bit value. No direct path may narrow a
Duration through `int64`, `uint64`, milliseconds, or a host timer type.

The maintained associated constructors are:

- `Duration.ms(value: int64) -> Duration`
- `Duration.seconds(value: int64) -> Duration`
- `Duration.minutes(value: int64) -> Duration`

Duration arithmetic consists of checked `Duration + Duration`,
`Duration - Duration`, `Duration * int64`, `int64 * Duration`, and
`Duration // int64`. Floor division applies to the signed nanosecond count,
rounds toward negative infinity, and returns `Duration`. Equality and all four
ordering operators compare the represented nanosecond count. Overflow and a
zero divisor use the ordinary arithmetic runtime diagnostics.

This representation makes computed backoff such as `attempt * 1ms`
expressible without rounding. A representable language value remains distinct
from whether a particular host timer can accept it. Conversion methods,
rendering, invalid host-timer inputs, and omitted-timeout encoding are the
Provisional policy recorded by ADR-0019.

## Completion tests

- Literal and constructor boundary tests in lexer/checker, MIR, native
  codegen, and native runtime units.
- Forced-backend run fixtures at and beyond `i64::MAX` nanoseconds and near
  both signed-128-bit limits.
- Exact two-limb direct-ABI tests for positive and negative values.
- Arithmetic, ordering, runtime-factor backoff, zero-divisor, and overflow
  fixtures through both maintained backends.
- Separate host-timer conversion failures and complete Duration API
  documentation.
