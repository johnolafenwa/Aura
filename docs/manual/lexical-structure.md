# Lexical Structure

This chapter defines how Aura source text becomes tokens and indentation
markers. It is normative for source spelling.

Related pages:

- [Grammar](/manual/grammar) collects the complete token-level productions.
- [Names And Scopes](/manual/names-and-scopes) and
  [Static Semantics](/manual/static-semantics) define name binding and
  reserved builtin names.

## Source Files And Text

Aura source files use the `.au` extension by convention and contain UTF-8
text. The lexer ignores one UTF-8 byte-order mark, and only at the beginning
of the file.

Source is processed as physical lines and logical lines:

- Outside an open delimiter, a nonblank physical line normally ends one
  logical line.
- While a `(`, `[`, or `{` is open, physical line boundaries are lexical
  whitespace and the logical line continues.
- The one exception is an expression-form `match` inside a delimiter. This
  layout island, described below, keeps its block tokens.

## Identifiers

Identifiers are ASCII and case-sensitive. Their exact spelling is:

```ebnf
ascii-letter = "A" … "Z" | "a" … "z" ;
digit        = "0" … "9" ;
IDENT        = (ascii-letter | "_"),
               { ascii-letter | digit | "_" } ;
```

`count`, `_message`, `buffer`, `Result`, and `worker2` are identifiers.
`résultat` is not, because names do not accept non-ASCII letters. Unicode is
still valid inside strings.

Static checking can reject a valid identifier spelling:

- Builtin types and functions reserve maintained names.
- Declarations cannot collide in the same namespace.
- Some positions impose additional rules.

See [Names And Scopes](/manual/names-and-scopes).

## Token Words And Contextual Words

The lexer recognizes these words specially:

```text
class enum def trait impl import from mut own indirect public extern opaque
return assert if elif else and or not match case for in while break
continue pass try with as true false
```

`true` and `false` produce boolean-literal tokens. The other words introduce
declarations, control flow, ownership forms, imports, or operators. They
cannot normally be used as ordinary identifiers.

Some of these words have further rules:

- `extern` and `opaque` introduce the bodyless declarations described by
  [FFI v0](/manual/ffi).
- `own` is reserved everywhere. It marks consuming ordinary parameters,
  collection loops, and matches, and the consuming receiver spelling
  `own self`.
- `mut` marks mutable parameters, loops, matches, and the receiver spelling
  `mut self`. It also introduces a mutable local binding.
- `from` is contextual. At module level, a complete prefix of the form
  `from module.path import ...` begins an import. In other identifier
  positions, `from` can name a parameter, local binding, expression, member,
  type-path component, or named argument.

```aura
def replace(from: str, to: str) -> str:
    return from + to

def main():
    mut from = "left"
    from = replace(from=from, to="right")
```

Several other spellings are lexed as ordinary identifiers. They become special
only in a defined context:

| Spelling | Contextual meaning |
| --- | --- |
| `copy` | Modifies `class` when immediately before it. |
| `self` | Declares or refers to a method receiver. |
| `Self` | Refers to the current type in supported trait and implementation type positions. |
| `None` | The unit value, or the absence member of an expected `T \| None` union. |
| `set` | Names the builtin set type and its constructor. |
| `lambda` | Introduces a lambda at the start of an expression. It stays an identifier token in member and named-argument positions. |
| `_` | The wildcard in a match pattern. Elsewhere it is an identifier spelling subject to static rules. |

## Comments

`#` outside a string begins a comment. The comment runs to the end of the
physical line.

```aura
# A comment-only line.
print("ready") # A trailing comment.
```

Aura 0.3 has no block comments.

## Spaces, Tabs, And Indentation

Blocks are indentation-based:

```aura
if ready:
    print("yes")
else:
    print("no")
```

Indentation uses ASCII spaces. A physical tab character is a lexical error
anywhere in a source line, including indentation, comments, and ordinary
quoted strings. The one exception is the content of a triple-quoted string.
The two-character escape `\t` is valid inside a string. In source it is a
backslash and a `t`, and it becomes a tab only in the decoded value.

Blank and comment-only lines produce no tokens and do not change indentation.
The lexer handles every other line as follows:

1. It counts the line's leading spaces.
2. When no ordinary delimiter continuation is active and the count is greater
   than the current block count, it emits one `INDENT` and records the new
   count.
3. When no ordinary delimiter continuation is active and the count is smaller,
   it emits one or more `DEDENT` tokens. The new count must equal a previously
   recorded indentation level.
4. It tokenizes the line contents. It emits `NEWLINE` only when the physical
   boundary is a logical boundary, or when the line belongs to a delimited
   expression-form `match` layout island.
5. At end of file, it emits all outstanding `DEDENT` tokens and then `EOF`.

Aura does not require a four-space indentation width. Sibling lines must
return to exactly the same recorded count. The maintained examples use four
spaces.

A suite must contain at least one line that is neither blank nor a comment.
Use `pass` for an intentionally empty suite.

## Physical And Logical Line Boundaries

Inside an unmatched `(`, `[`, or `{`, a physical newline does not emit
`NEWLINE`, `INDENT`, or `DEDENT`. The next nonblank physical line continues
the same logical token sequence. Delimiters may be nested and mixed. They must
close in last-opened, first-closed order, each with the matching kind.

```aura
def combine(
    left: int64,
    right: int64
) -> int64:
    return left + right

def main():
    values = [
        20,
        22
    ]
    result = combine(
        values[0],
        values[1]
    )
    print(result)
```

This program prints `42`. It has no trailing comma after `right`, `22`, or
`values[1]`, because newline continuation does not change the grammar of
comma-separated lists.

Leading spaces on a continuation line are formatting, not block indentation:

- They neither consult nor modify the surrounding indentation stack.
- The maintained style uses one additional four-space level.
- Physical tabs are still invalid, even in continuation indentation.
- Blank and comment-only lines are still ignored.
- A trailing comment may end a continued physical line.

The newline after the outermost closing delimiter ends the logical line
normally. A preceding operator or comma does not continue a line. Some `(`,
`[`, or `{` must still be open at that physical boundary.

An expression-form `match` inside a delimiter keeps the layout tokens its
`case` arms need. That arm block is a layout island inside the continued outer
expression. The outer closer may follow the final inline arm, or it may sit on
its own line. See [Expressions](/manual/expressions#match-expressions) and
[Grammar](/manual/grammar#match-expressions).

Backslash continuation is not implemented. Ordinary, raw, and f-strings are
single-line. Delimiters inside them do not continue source, and an f-string
interpolation cannot cross a physical newline.

## Punctuation And Operators

Aura 0.3 recognizes these punctuation and operator tokens:

```text
( ) [ ] { } : , . ?
= == != < <= > >=
+ += - -= * *= ** **= / /= // //= % %=
& &= | |= ^ ^= ~ << <<= >> >>=
->
```

The lexer chooses the longest operator spelling, so `**=`, `<<=`, `>>=`, and
`//=` are each one token.

Aura has none of these:

- a semicolon, so multiple statements cannot share one physical line
- a unary `+`
- assignment expressions
- a lambda arrow, because lambdas use `lambda parameters: expression`

Comma-separated lists do not accept a trailing comma. This applies to
arguments, parameters, imports, type arguments, generic parameters, enum
payloads, collection elements, and trait lists. Tuples are the one exception.
The singleton tuple value, type, target, and pattern forms require one comma.
Multi-element tuples reject a trailing comma.

## Integer Literals

Integer literals may use decimal, hexadecimal, binary, or octal digits:

```ebnf
decimal-digit   = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
binary-digit    = "0" | "1" ;
octal-digit     = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" ;
hex-digit       = decimal-digit | "a" | "b" | "c" | "d" | "e" | "f"
                  | "A" | "B" | "C" | "D" | "E" | "F" ;
decimal-digits  = decimal-digit, { decimal-digit } ;
decimal-integer = decimal-digit, { decimal-digit | ("_", decimal-digit) } ;
hex-integer     = ("0x" | "0X"), hex-digit,
                  { hex-digit | ("_", hex-digit) } ;
binary-integer  = ("0b" | "0B"), binary-digit,
                  { binary-digit | ("_", binary-digit) } ;
octal-integer   = ("0o" | "0O"), octal-digit,
                  { octal-digit | ("_", octal-digit) } ;
INTEGER         = decimal-integer | hex-integer | binary-integer | octal-integer ;
```

Examples are `0`, `42`, `1_000_000`, `0xFF`, `0b1010_0110`, and `0o755`.
Hexadecimal digits are case-insensitive.

An underscore is valid only between two digits of the literal's base. It
cannot follow the prefix, begin or end the digit sequence, or repeat without a
digit in between. Separators and base prefixes do not apply to floating-point
or duration literals.

The lexical value must fit an unsigned 128-bit integer. Static checking then
picks the literal's type:

- When an integer type is expected, the checker selects it and verifies that
  the value fits.
- When `float32` or `float64` is expected, the checker may select it if the
  value is exactly representable in that type.
- Otherwise the literal defaults to `int64`.

The source spelling `int` is an alias for `int64`.

`-0x7F` is not one signed token. It is unary `-` applied to the positive
integer literal `0x7F`.

## Floating-Point Literals

A floating literal needs a fractional part or an exponent:

```ebnf
EXPONENT = ("e" | "E"), [ "+" | "-" ], decimal-digits ;
FLOAT    = decimal-digits, ".", decimal-digits, [ EXPONENT ]
         | decimal-digits, EXPONENT ;
```

`1.0`, `0.25`, `1e3`, `2.5e-1`, and `3E+4` are valid. `.5` and `3.` are not
floating literals.

The lexical value must be finite as an `f64`. Static checking defaults the
literal to `float64`, or adopts an expected `float32` or `float64` type.

## Duration Literals

A duration literal is a non-negative integral count followed immediately by
`ms`, `s`, or `m`:

```ebnf
DURATION = decimal-digits, ("ms" | "s" | "m") ;
```

`10ms`, `2s`, and `1m` represent 10, 2,000, and 60,000 milliseconds. Their
type is `Duration`.

The lexer stores the exact value as signed 128-bit nanoseconds, so the scaled
value must fit that range. A duration literal is always non-negative and
integral in its written unit. Aura has no `ns` suffix, no fractional literal
such as `1.5ms`, and no unary `-Duration`. For computed or negative values,
use the signed constructors and checked binary Duration operators in
[Expressions](/manual/expressions#arithmetic-and-comparison).

## Boolean And `None`

`true` and `false` are the two `bool` literals. They are lowercase.

`None` is lexically an identifier. Statically it denotes the unit value of
type `None`. When an expected `T | None` union type resolves the meaning, it
denotes the absence member instead. Aura has no null value apart from these
typed forms, and no `?` type suffix.

## str Literals

Ordinary string literals are single-line and use matching single or double
quotes:

```aura
double = "Aura"
single = 'Aura'
apostrophe = 'Aura\'s strings'
quotation = 'the compiler said "ready"'
```

Both quote styles produce a `str` and support the same escapes:

| Escape | Decoded value |
| --- | --- |
| `\n` | Line feed |
| `\t` | Tab |
| `\"` | Double quote |
| `\'` | Single quote |
| `\\` | Backslash |
| `\0` | NUL |
| `\xHH` | Scalar from exactly two hexadecimal digits |
| `\u{H...}` | Unicode scalar from one or more hexadecimal digits |

These are lexical errors: unknown escapes, invalid Unicode scalars, missing
hexadecimal digits, and missing or mismatched closing quotes. A
one-character literal such as `'x'` is a `str`. Aura has no separate
character type.

Three matching quotes create an exact multiline string:

```aura
prompt = """Classify this request.
Return one label and one reason.
"""
```

The value contains every scalar between the delimiters. Aura performs no
dedent, margin calculation, trimming, leading-newline or trailing-newline
removal, or Unicode normalization. Escapes keep their ordinary meaning.
Physical tabs inside the delimiters are content.

A lowercase `r` creates a single-line raw string:

```aura
path = r"C:\agents\run"
pattern = r'\d+\.\d+'
```

In a raw string, backslashes are content. A backslash may keep the active
quote inside the value, and both characters remain. A raw string cannot end in
an odd run of backslashes or contain a physical newline. Raw triple strings
and byte strings are unavailable.

Every string literal has type `str`. See [Types](/manual/types) for ownership
and [Execution Model](/manual/execution-model#evaluation-order) for expression
evaluation order.

## F-Strings

An f-string begins with `f"`. It is double-quoted and single-line:

```aura
name = "aura"
print(f"hello {name}")
```

Text inside `{` and `}` is parsed as an ordinary Aura expression. An
interpolation may contain indexing, calls, nested braces used by expressions,
and either form of ordinary string literal, including braces inside those
strings. The parser rejects empty or syntactically invalid interpolations.

To write a literal brace:

- Two consecutive opening braces produce one literal opening brace.
- Two consecutive closing braces produce one literal closing brace.
- Aura 0.3 also treats a lone closing brace outside an interpolation as
  literal text.

```aura
print(f"{{name}} = {name}")
```

F-strings support the same escapes as ordinary strings.

### Format Specifications

An interpolation accepts a static format specification after a top-level
colon:

```aura
def main():
    count: int64 = 1234567
    ratio: float32 = 0.875
    label = "Aura"
    print(f"{count:>12,d}")
    print(f"{ratio:+.2%}")
    print(f"{label:·^16.8s}")
```

The specification grammar is `[[fill]align][sign][width][,][.precision][type]`.

| Part | Rule |
| --- | --- |
| `align` | `<`, `^`, or `>`. Strings default to left alignment and numbers to right alignment. |
| `type` | `d`, `f`, `e`, `x`, `X`, `b`, `o`, `%`, or `s`. |
| `width` | Counts Unicode scalars and never truncates. A numeric width beginning with `0` selects zero padding. Limited to `1_000_000`. |
| `,` | Decimal grouping. Available with `d`, `f`, and `%`. |
| `precision` | For `s`, the maximum scalar count. For numbers, rounds ties to even. Limited to `1_000_000`. |

With zero padding and no explicit alignment, the zeros follow the sign. For
example, `f"{-1.25:09.3f}"` produces `-0001.250`.

Aura parses the complete interpolation expression before it looks for the
top-level colon. Colons inside nested slices, dictionaries, calls, and
collection literals stay part of the expression.

These forms are unavailable:

- dynamic specifications
- nested fields
- conversion flags
- single-quoted f-strings

`rf"..."`, `fr"..."`, and `f"""..."""` receive `AU1002` guidance pointing to
the supported single-line `f"..."` spelling.

Interpolations evaluate once, from left to right. The result is an owned
`str`.

## Complexity Limits

The parser rejects excessive nesting and expression chains instead of risking
host stack exhaustion. The limits are 128 levels for expressions, types,
patterns, statements, f-string braces, and chained operators.
[Grammar](/manual/grammar#syntactic-complexity-limits) defines them exactly,
and [Current Limits](/manual/current-limits) summarizes them.

## Grammar

This chapter's token productions, reserved words, indentation protocol,
delimiters, operators, and literal forms are normative. The complete
[Grammar](/manual/grammar) defines how they compose into declarations,
statements, patterns, types, and expressions. A source spelling those
productions do not accept is not an extension point.

## Typing Rules

Lexing does not assign expression types. It keeps each literal's kind and its
mathematical or decoded value for static checking:

| Token | Type during checking |
| --- | --- |
| Integer literal | May adopt an exact expected integer or floating type. |
| Floating literal | May adopt `float32` or `float64`. |
| Duration literal | `Duration` |
| Boolean literal | `bool` |
| Ordinary string | `str` |
| F-string | An interpolated `str` expression |

No lexical spelling performs a runtime coercion.

## Runtime Semantics

Tokenization has no runtime side effects. Decoded string scalars, literal
numbers, duration nanoseconds, and f-string text segments become constants or
MIR inputs only after the complete module has parsed and checked. A lexical
failure prevents execution.

Suppressing a physical line boundary has no runtime action. The resulting
token sequence evaluates exactly as if the same tokens were written on one
physical line.

## Ownership And Evaluation Order

Tokens do not own or borrow runtime values. Ordinary and f-string literals
produce owned values when evaluated. F-string interpolation expressions run
left to right, as [Expressions](/manual/expressions) specifies. Indentation,
comments, and physical-line markers have no runtime evaluation.

Physical-line placement and continuation indentation do not create, extend, or
end a borrow. They do not change move or copy decisions. Evaluation follows
the source order of the joined logical token sequence.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1001` | Invalid lexical input: physical tabs, invalid escapes, malformed or unterminated literals, invalid characters, invalid block indentation, and delimiter pairing failures. |
| `AU1002` | The single-quoted f-string spelling. The message directs the author to `f"..."`. |

Delimiter pairing failures report as follows:

- An unexpected closer reports at that closer, as the primary span.
- A mismatched closer names the expected delimiter and labels its opener with
  a secondary span.
- An unclosed delimiter reports at EOF and labels its opener.

Once tokenization succeeds, syntax failures use parser code `AU1101`, which
this page does not cover.

## Backend Support

The compiler tokenizes source once, before MIR lowering or native code
generation. The MIR runtime and the direct native backend therefore accept
exactly the same lexical language. There is no backend-specific lexer.

## Limits And Implementation-Defined Behavior

These lexical limits apply:

- Identifiers are ASCII.
- Source is UTF-8.
- Physical tabs are rejected outside triple-quoted string content.
- Continuation requires an unmatched source delimiter.
- Ordinary lists reject trailing commas.
- Backslash continuation and multiline f-strings are unavailable.
- This chapter and [Current Limits](/manual/current-limits) fix the literal
  magnitude and parser-complexity caps.

Continuation indentation is not semantically significant. Delimiter matching,
token spans, and the expression-match layout island are defined behavior, not
implementation choices.

## Status

The forms this page describes as accepted are implemented, including
delimiter continuation and its layout and diagnostic policy.

These forms are unavailable:

- raw triple strings
- raw f-strings
- byte strings
- single-quoted f-strings
- block comments
- semicolons
- ordinary trailing commas, other than the required singleton-tuple comma
- backslash continuation
- multiline f-string literals

Design record: [ADR-0025, newline continuation and delimited layout](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0025-newline-continuation-and-delimited-layout.md).
