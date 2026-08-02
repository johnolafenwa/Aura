# ADR-0046: String literal forms and f-string format specifications

- Status: Accepted
- Date: 2026-08-02
- Roadmap decision: Batch S1, S4.1 and S4.6
- Builds on: ADR-0004, ADR-0016, ADR-0019, and ADR-0025

## Context

Aura needs multiline text, literal backslash-heavy text, and controlled value
formatting for configuration, protocols, reports, and agent prompts. These
forms must remain ordinary owned UTF-8 `str` values, preserve Aura's
left-to-right expression sequencing, and produce byte-identical output in MIR
and direct execution.

The formatting surface intentionally implements a practical, closed subset.
An unsupported specification is a compile-time error, not a request for a
backend or host formatter to improvise.

## Decision

### Ordinary, triple-quoted, and raw strings

Ordinary strings use either quote delimiter and retain the existing escape
contract:

```aura
"one line"
'one line'
```

Triple-quoted strings use three matching single or double quotes:

```aura
message = """first line
second line
"""

query = '''select *
from jobs'''
```

The value is the exact logical source text between the delimiters after the
ordinary escape rules are applied. Aura performs no automatic indentation
removal, common-margin calculation, leading-newline removal, trailing-newline
removal, or whitespace normalization. A newline immediately after the opening
delimiter is part of the value. A newline and indentation immediately before
the closing delimiter are also part of the value. Triple-quoted strings may
contain unescaped single instances and pairs of their delimiter quote; three
unescaped matching quotes terminate the literal.

Raw strings are single-line and use a lowercase `r` immediately followed by a
single- or double-quote delimiter:

```aura
windows_path = r"C:\agents\run"
pattern = r'\d+\.\d+'
```

Backslashes in a raw string are content and do not introduce character
escapes. A backslash may precede the active quote delimiter to keep that quote
inside the literal; both the backslash and quote remain in the resulting
value. Consequently, a raw string cannot end in an odd run of backslashes.
Raw strings cannot contain a physical newline. Raw triple-quoted strings and
raw f-strings are outside this decision.

All string forms produce fresh owned `str` values containing valid UTF-8.
Quote choice and literal form do not create distinct static types. Literal
contents receive no Unicode normalization.

### F-string interpolation and delimiter selection

An f-string remains a single-line `f"..."` literal. Each interpolation has
one of these forms:

```aura
f"{expression}"
f"{expression:format_spec}"
```

The parser recognizes nested string, call, index, collection, and other
delimiter pairs before considering a colon. It parses the interpolation as a
whole Aura expression first. An eligible colon at interpolation top level
then separates that complete expression from a format specification. Colons
inside nested delimiters belong to the expression. A colon cannot be used to
truncate an invalid expression into a valid prefix.

The format specification is static source text. It contains no nested
interpolations, expressions, or dynamically computed width or precision. An
empty specification is equivalent to omitting it.

### Supported format grammar

The accepted grammar is:

```text
format_spec := [[fill]align] [sign] [width] [","] ["." precision] [type]
align       := "<" | "^" | ">"
sign        := "+" | "-" | " "
width       := decimal_digits
precision   := decimal_digits
type        := "d" | "f" | "e" | "x" | "X" | "b" | "o" | "%" | "s"
```

The optional comma follows the optional width and is also valid when width is
omitted. It may appear at most once. `fill` is exactly one Unicode scalar value
immediately followed by an alignment character. Without explicit alignment,
strings align left and numbers align right. The default fill is ASCII space.
Width is a minimum Unicode-scalar count, never a truncation request.

Both width and precision are decimal integers in `0..=1_000_000`. A larger
value is rejected at compile time. Formatting also remains subject to the
maintained maximum `str` allocation limit; exceeding it at runtime reports
`AU4005` before an oversized allocation is committed.

The type-specific rules are:

| Type code | Accepted value | Meaning |
| --- | --- | --- |
| omitted | any renderable value | Existing `str`/print rendering; floating values use shortest round-trip spelling. |
| `s` | `str` | Text; precision is a maximum Unicode-scalar count. |
| `d` | any integer type | Signed decimal integer. |
| `x`, `X` | any integer type | Lowercase or uppercase hexadecimal magnitude with a leading minus sign for a negative value. |
| `b` | any integer type | Binary magnitude with a leading minus sign for a negative value. |
| `o` | any integer type | Octal magnitude with a leading minus sign for a negative value. |
| `f` | an integer or floating type | Fixed-point decimal; default precision is six. |
| `e` | an integer or floating type | Lowercase scientific notation; default precision is six and the exponent always has a sign and at least two digits. |
| `%` | an integer or floating type | Multiply by 100, format as `f`, then append `%`; default precision is six. |

Integer precision is accepted only through `f`, `e`, and `%`. A sign flag is
valid only for numeric formatting. `-` means the ordinary minus sign for
negative values without forcing a plus sign. `+` emits a sign for both
polarities, and space emits one leading space for non-negative values.
Thousands separation is valid for `d`, `f`, and `%`; it groups the decimal
integer portion in threes. It is rejected with `e`, `x`, `X`, `b`, `o`, and
`s`. Non-finite floating values render as `nan`, `inf`, or `-inf`; sign and
alignment still apply, while precision and the thousands separator do not
alter those words.

Floating rounding required by precision is round-to-nearest, ties-to-even.
Formatting a `float32` uses its exact binary32 value and formatting a
`float64` uses its exact binary64 value. Numeric conversion performed solely
for `f`, `e`, or `%` does not change the static type or stored value.

### Evaluation and ownership

Literal text and each reached interpolation are appended in source order.
Every interpolation expression evaluates exactly once, and its value is
rendered immediately before the next interpolation begins. Existing retained
place and move rules apply during expression evaluation. A formatting failure
does not evaluate any later interpolation and cleans up the partial result.

The specification is validated statically against the interpolation's known
type. Formatting does not invoke user code and does not consume a non-Copy
interpolation value merely to display it.

## Diagnostics

- `AU1001` reports an unterminated or malformed ordinary, triple-quoted, or raw
  literal, including an odd terminal backslash run in a raw string.
- `AU1002` reports malformed f-string delimiters, braces, and interpolation
  boundaries.
- `AU1101` reports malformed format-spec grammar, nested replacement fields,
  duplicate flags, or a width/precision above `1_000_000`.
- `AU2002` reports a format type, sign, precision, or separator that is
  incompatible with the interpolation type. The diagnostic lists the
  supported codes and the subset valid for that value.
- `AU4005` reports a maintained output-allocation limit or allocation failure.

Diagnostics point to the smallest literal or specification span that proves
the error. MIR and direct execution use the same formatter and diagnostic
payloads.

## Limits

F-strings are single-line and double-quoted. Dynamic specifications, nested
replacement fields, conversion flags, locale-aware formatting, `g`, `G`,
`n`, `c`, alternate form `#`, zero-padding as a distinct flag, and `=`
alignment are not part of this contract. Triple strings are ordinary strings,
not syntax-level documentation objects by themselves.

## Consequences

Aura gains exact multiline and raw text plus deterministic protocol- and
report-friendly formatting. Preserving triple-string content prevents source
indentation from silently changing data. The closed format grammar keeps
output stable across hosts and backends.

## Completion test matrix

- lexer/parser tests for both ordinary quote forms, both triple delimiters,
  embedded quote runs, all newline boundary positions, escapes, raw
  backslashes and quotes, terminal-backslash rejection, UTF-8, malformed
  delimiters, and the absence of implicit dedent or trimming
- parser tests proving whole-expression-first interpolation parsing, nested
  slice/map/call colons, an eligible top-level format colon, empty and
  malformed specifications, and no nested replacement fields
- static tests for every format code/value family, sign/separator/precision
  restrictions, Unicode fill, width/precision boundaries at `1_000_000` and
  `1_000_001`, and focused source spans
- runtime golden tests for alignment, fill, signs, grouping, every type code,
  signed zero, infinities, NaN, Unicode-scalar width/truncation, ties-to-even,
  shortest-roundtrip omission, and allocation-limit failure
- sequencing tests proving one evaluation per interpolation, immediate
  rendering, left-to-right effects, no hidden move, early failure, and partial
  result cleanup
- byte-identical MIR/direct fixtures plus compiler analysis, completion,
  hover, formatter idempotence, language-server, bundled-editor, maintained
  example, and executable Manual coverage

## Ratification

Batch S1 accepts this as Aura 0.3's complete string-literal and f-string
format-specification contract. Syntax, diagnostics, reference text, tooling,
tests, and both backend implementations land together.
