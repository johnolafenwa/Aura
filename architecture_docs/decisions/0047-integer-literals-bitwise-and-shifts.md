# ADR-0047: Integer literal bases, bitwise operators, and shifts

- Status: Accepted
- Date: 2026-08-02
- Roadmap decision: Batch S1, S4.2
- Builds on: ADR-0002, ADR-0016, and ADR-0041

## Context

Checksums, binary protocols, masks, flags, and packed identifiers need literal
bases and fixed-width bit operations. Aura's integers have declared widths and
checked arithmetic, so shift behavior must be explicit at invalid counts and
overflow boundaries. Host-language masking and backend instruction quirks are
not part of the language contract.

## Decision

### Integer literal spelling

Integer literals accept decimal, hexadecimal, binary, and octal forms:

```aura
1_000_000
0xFF
0b1010_0110
0o755
```

The prefixes `0x`/`0X`, `0b`/`0B`, and `0o`/`0O` select bases 16, 2, and 8.
Hexadecimal digits are case-insensitive. An underscore may occur only between
two digits valid for the selected base. It may not immediately follow a base
prefix, begin or end the digit sequence, repeat without an intervening digit,
or separate a unary sign from the literal. Underscores have no value effect.

The sign is a unary operator and is not part of the literal token. Literal
typing remains contextual: a literal adopts an expected integer type when it
fits and otherwise defaults to `int64`. Every spelling denotes one exact
mathematical integer before target-width checking. A value outside its target
type is rejected statically. Integer-base prefixes and separators do not apply
to floating or duration literals.

### Bitwise operators

Every signed and unsigned integer type supports:

```text
left & right
left | right
left ^ right
~value
left << count
left >> count
```

Binary operands, including a shift's count, must have the same exact concrete
integer type. `int` and `int64` satisfy this rule because they have one type
identity. Aura inserts no promotion or width conversion. `&`, `|`, and `^`
operate on the fixed-width two's-complement bit representation and return that
same type. Unary `~` flips every bit at the operand width and returns the
operand type.

Bitwise precedence from tightest to loosest is: unary `~`; shifts; `&`; `^`;
`|`. Shifts bind below additive arithmetic and above comparisons. Each binary
level associates left. Parentheses remain the explicit way to communicate a
mixed bitwise expression.

### Shift counts and right shift

For a value whose declared width is `W`, every shift form requires
`0 <= count < W`. This includes ordinary, wrapping, and saturating left and
right shifts. Signed negative counts and all counts at or above `W` trap with
`AU4002`. A count of zero returns the unchanged value.

`>>` is a logical right shift for unsigned integers and an arithmetic,
sign-extending right shift for signed integers. It therefore equals floor
division by `2 ** count` for signed values. No right-shift form traps because
of discarded low bits after the count has passed validation.

### Checked, wrapping, and saturating left shift

Ordinary `left << count` is checked arithmetic. It computes the mathematical
product `left * 2 ** count` and traps with `AU4002` if that result is outside
the left operand's declared type. This rule applies equally to positive and
negative signed values.

Every integer type also provides:

```aura
value.wrapping_shl(count)
value.wrapping_shr(count)
value.saturating_shl(count)
value.saturating_shr(count)
```

The count has the receiver's exact type and always receives the same range
check as an operator count. `wrapping_shl` shifts at the fixed width and drops
high bits, equivalent to the mathematical product reduced modulo `2 ** W` and
reinterpreted in the receiver type. `saturating_shl` computes the mathematical
product and clamps it to the receiver type's minimum or maximum.

Right shift has neither magnitude overflow nor a wrapping alternative in its
bit result. After count validation, `wrapping_shr` and `saturating_shr` retain
exactly the signed arithmetic or unsigned logical semantics of ordinary `>>`.
Their names make arithmetic-mode-generic code possible without changing the
meaning of right shift.

### Evaluation and compound assignment

The left operand evaluates once before the right operand. Place-retention and
conflict rules follow ADR-0016. The compound assignments `&=`, `|=`, `^=`,
`<<=`, and `>>=` select their target place once, capture its current Copy
integer value, evaluate the right operand once, apply the corresponding
operator, and store only after success. A trap leaves the target unchanged.

Constant folding is permitted only when it preserves the same accepted result
or diagnostic, including exact width, count validation, overflow, and source
span.

## Diagnostics

- `AU1001` reports an invalid base digit, missing digits after a prefix, or an
  invalid underscore position.
- `AU2002` reports unequal operand types.
- `AU2003` reports a bitwise operator or shift applied to a non-integer type.
- `AU4002` reports a negative or excessive shift count and checked-left-shift
  overflow. The diagnostic includes the count, operand type, and required
  range `0..W`.
- Existing assignment and place diagnostics apply to compound assignment.

The compiler rejects provably invalid typed literals during checking. Shift
count validity remains an evaluation rule even when optimization can prove the
value; optimized and unoptimized execution report the same diagnostic.

## Backend requirements

MIR and direct execution use the same width table and checked helpers. Native
code generation must guard a count before emitting or invoking a host shift;
it may not rely on a target instruction's masked-count behavior. Signed right
shift, checked overflow, wrapping bit loss, saturation, and compound-store
ordering are byte-parity requirements.

`intsize` and `uintsize` use the selected compilation target's pointer width.
Artifacts for different target widths may consequently accept different
literal bounds, but MIR and direct execution for one target agree exactly.

## Limits

There are no arbitrary-precision integers, implicit bit-width widening,
unsigned right-shift operator distinct from `>>`, rotate operators, literal
suffixes, hexadecimal floating literals, or alternate-prefix output implied by
these operators. Formatting bases is governed by ADR-0046.

## Consequences

Binary-data code becomes direct and readable while retaining fixed-width,
loud arithmetic. Every shift has one count rule, and the explicit arithmetic
families let callers choose checked, wrapping, or saturating left-shift
behavior without backend dependence.

## Completion test matrix

- lexer/parser tests for all base-prefix cases, upper/lower hexadecimal,
  separator placement, missing digits, invalid digits, unary signs,
  precedence, associativity, compound forms, and malformed operators
- static tests for contextual literal types and every signed/unsigned width,
  exact minimum/maximum boundaries, out-of-range values, exact-type operands,
  `int` identity, and all non-integer rejections
- runtime tests for `&`, `|`, `^`, `~`, zero counts, every `W - 1` count,
  negative and `W` counts, signed arithmetic right shift, unsigned logical
  right shift, checked left boundaries, wrapping bit loss, saturation at both
  signed bounds, and right-method equivalence
- compound-assignment tests for single target/operand evaluation, projected
  places, unchanged targets on traps, and ownership-conflict diagnostics
- constant-folding parity tests for successful and failing expressions
- byte-identical MIR/direct tests across maintained target widths, plus the
  checksum or bit-packing maintained example, executable Manual block,
  compiler analysis, completion, hover, formatter, language-server, and
  bundled-editor coverage

## Ratification

Batch S1 accepts this as Aura 0.3's fixed-width integer literal and bitwise
contract. The parser, checker, runtime helpers, both backends, diagnostics,
reference, example, and tooling land together.
