# Lexical Structure

This chapter defines how Aurora source text becomes tokens and indentation markers. It is normative for source spelling. The complete token-level productions are collected in [Grammar](/manual/grammar); name binding and reserved builtin names are defined by [Names And Scopes](/manual/names-and-scopes) and [Static Semantics](/manual/static-semantics).

## Source Files And Text

Aurora source files conventionally use the `.au` extension and contain UTF-8 text. One UTF-8 byte-order mark is ignored only when it occurs at the beginning of the file.

Source is processed as physical lines and logical lines. Outside an open source
delimiter, a nonblank physical line normally ends one logical line. While a
`(`, `[`, or `{` remains open, ordinary physical line boundaries are lexical
whitespace and the logical line continues. The expression-form `match` layout
island described below is the one exception that preserves block tokens inside
an enclosing delimiter.

## Identifiers

Identifiers are ASCII and case-sensitive. Their exact spelling is:

```ebnf
ascii-letter = "A" … "Z" | "a" … "z" ;
digit        = "0" … "9" ;
IDENT        = (ascii-letter | "_"),
               { ascii-letter | digit | "_" } ;
```

Examples of identifiers are `count`, `_message`, `Result`, and `worker2`. `résultat` is not an identifier because non-ASCII letters are not accepted in names. Unicode remains valid inside strings.

An identifier spelling can still be rejected by static checking. Builtin types and functions reserve maintained names, declarations cannot collide in the same namespace, and some positions impose additional rules. See [Names And Scopes](/manual/names-and-scopes).

## Token Words And Contextual Words

The lexer recognizes these words specially:

```text
class enum def trait impl import from mut borrow own indirect public
return assert if elif else and or not match case for in while break
continue pass try with as true false
```

`true` and `false` produce boolean-literal tokens. The other words introduce declarations, control flow, ownership forms, imports, or operators and cannot normally be used as ordinary identifiers. `own` is reserved everywhere; it marks consuming ordinary parameters and collection loops as well as the consuming receiver spelling `own self`.

`from` is contextual. At module level, a complete prefix of the form `from module.path import ...` begins an import. In other identifier positions, `from` can name a parameter, local binding, expression, member, type-path component, or named argument:

```python
def replace(from: String, to: String) -> String:
    return from + to

mut from = "left"
from = replace(from=from, to="right")
```

Several other spellings are lexed as ordinary identifiers and become special only in a defined context:

| Spelling | Contextual meaning |
| --- | --- |
| `copy` | Modifies `class` when immediately before it. |
| `self` | Declares or refers to a method receiver. |
| `Self` | Refers to the current type in supported trait and implementation type positions. |
| `None` | The unit value, or `Option.None` when an expected option type makes that interpretation unambiguous. |
| `Set` | Begins the explicit set literal `Set{...}` and names the builtin set type. |
| `_` | The wildcard in a match pattern; elsewhere it is an identifier spelling subject to static rules. |

## Comments

`#` begins a comment outside a string and consumes the remainder of the physical line:

```python
# A comment-only line.
print("ready") # A trailing comment.
```

Aurora 0.1 has no block comments.

## Spaces, Tabs, And Indentation

Blocks are indentation-based:

```python
if ready:
    print("yes")
else:
    print("no")
```

Indentation uses ASCII spaces. A physical tab character anywhere in a source line is a lexical error, including inside indentation, a comment, or a quoted string. The two-character escape `\t` is valid inside a string because it contains a backslash and `t` in source and creates a tab only in the decoded value.

Blank and comment-only lines do not produce tokens and do not change indentation. Every other line is handled as follows:

1. The lexer counts its leading spaces.
2. When no ordinary delimiter continuation is active, a count greater than the
   current block count emits one `INDENT` and records the new count.
3. When no ordinary delimiter continuation is active, a smaller count emits
   one or more `DEDENT` tokens. The new count must equal a previously recorded
   indentation level.
4. The line contents are tokenized. The lexer emits `NEWLINE` only when the
   physical boundary is a logical boundary or belongs to a delimited
   expression-form `match` layout island.
5. End of file emits all outstanding `DEDENT` tokens and then `EOF`.

Aurora does not require an indentation width of four spaces, but sibling lines must return to exactly the same recorded count. The maintained examples use four spaces.

A suite must contain at least one nonblank, non-comment line. Use `pass` for an intentionally empty suite.

## Physical And Logical Line Boundaries

Inside an unmatched `(`, `[`, or `{`, an ordinary physical newline does not
emit `NEWLINE`, `INDENT`, or `DEDENT`. The next nonblank physical line
continues the same logical token sequence. Delimiters may be nested and mixed,
but they must close in last-opened, first-closed order with the matching kind.

```python
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

The verified program prints `42`. It deliberately has no trailing comma after
`right`, `22`, or `values[1]`: newline continuation does not change the
comma-separated-list grammar.

Leading spaces on an ordinary continuation line are formatting rather than
block indentation. They do not consult or modify the surrounding indentation
stack. The maintained style uses one additional four-space level. Physical
tabs remain invalid even when they appear only in continuation indentation.
Blank and comment-only lines remain ignored, and a trailing comment may end a
continued physical line.

The newline after the outermost closing delimiter ends the logical line
normally. A newline does not continue merely because the preceding token is an
operator or comma: some `(`, `[`, or `{` must still be open at that physical
boundary.

An expression-form `match` inside a delimiter retains the layout tokens needed
by its `case` arms. That arm block is a layout island inside the continued
outer expression. It accepts both the existing closer after a final inline arm
and a closer placed on its own line. See
[Expressions](/manual/expressions#match-expressions) and
[Grammar](/manual/grammar#match-expressions).

Backslash continuation is not implemented. Ordinary strings and f-strings remain single-line;
delimiters inside them do not continue source, and an f-string interpolation
cannot cross a physical newline.

## Punctuation And Operators

Aurora 0.1 recognizes:

```text
( ) [ ] { } : , . ?
= == != < <= > >=
+ += - -= * *= / /= // //= % %=
->
```

There is no semicolon. Multiple statements cannot share one physical line.
Aurora 0.1 also has no exponentiation, unary `+`, bitwise operators, assignment
expressions, lambda arrow, or conditional-expression operator. The lexer
chooses the longest operator spelling, so `//=` is one token rather than `//`
followed by `=`.

Comma-separated lists do not accept a trailing comma. This applies to
arguments, parameters, imports, type arguments, generic parameters, enum
payloads, collection elements, and trait lists. The tuple grammar is the one
exception: its singleton value, type, target, and pattern forms require one
comma, while multi-element tuples reject a trailing comma.

## Integer Literals

An integer literal is one or more decimal digits:

```ebnf
INTEGER = digit, { digit } ;
```

Examples are `0`, `42`, and `170000`. The lexical value must fit an unsigned
128-bit integer. Static checking selects an expected integer type when
available and verifies that the value fits. It may instead select an expected
`float32` or `float64` when the integer's value is exactly representable in
that type; otherwise the literal defaults to `int64`. The source spelling
`int` is an alias for `int64`.

`-7` is not one signed token. It is unary `-` applied to the positive integer literal `7`. Aurora has no hexadecimal, octal, binary, or underscore-separated integer syntax.

## Floating-Point Literals

Floating literals use a required fractional digit or an exponent:

```ebnf
EXPONENT = ("e" | "E"), [ "+" | "-" ], digit, { digit } ;
FLOAT    = INTEGER, ".", digit, { digit }, [ EXPONENT ]
         | INTEGER, EXPONENT ;
```

Valid examples include `1.0`, `0.25`, `1e3`, `2.5e-1`, and `3E+4`. `.5` and `3.` are not floating literals. The lexical value must be finite as an `f64`. Static checking defaults it to `float64` or adopts an expected `float32`/`float64` type.

## Duration Literals

A duration literal is a non-negative integral count followed immediately by `ms`, `s`, or `m`:

```ebnf
DURATION = INTEGER, ("ms" | "s" | "m") ;
```

`10ms`, `2s`, and `1m` represent 10, 2,000, and 60,000 milliseconds
respectively and have type `Duration`. The lexer stores the exact value as
signed 128-bit nanoseconds, so suffix scaling must fit that range. A duration
literal itself is always non-negative and integral in its written unit. There
is no `ns` suffix, fractional literal such as `1.5ms`, or unary
`-Duration`; use the signed constructors and checked binary Duration operators
described in [Expressions](/manual/expressions#arithmetic-and-comparison) for
computed or negative values.

## Boolean And `None`

`true` and `false` are the two `bool` literals. They are lowercase.

`None` is lexically an identifier but statically denotes the unit value of type `None`, or the payload-free `Option.None` variant when an expected `Option[T]` type resolves the meaning. There is no null value distinct from these typed forms.

## String Literals

Ordinary string literals use matching single or double quote delimiters and are
single-line:

```python
double = "Aurora"
single = 'Aurora'
apostrophe = 'Aurora\'s strings'
quotation = 'the compiler said "ready"'
```

Both delimiters produce a `String` and support the same escapes:

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

Unknown escapes, invalid Unicode scalars, missing hexadecimal digits, and
missing or mismatched closing quotes are lexical errors. Triple-quoted, raw,
and byte-string literals are not part of Aurora 0.1. A one-character literal
such as `'x'` is a `String`, not a distinct character type.

A string literal has type `String`. See [Types](/manual/types) for ownership and [Execution Model](/manual/execution-model#evaluation-order) for expression evaluation order.

## F-Strings

An f-string begins with `f"` and is double-quoted and single-line:

```python
name = "aurora"
print(f"hello {name}")
```

Text inside `{` and `}` is parsed as an ordinary Aurora expression.
Interpolations may contain indexing, calls, nested braces used by expressions,
and either form of ordinary string literal, including braces inside those
strings. Empty or syntactically invalid interpolations are rejected.

Use two consecutive opening braces for a literal opening brace. Two consecutive closing braces decode to one literal closing brace; Aurora 0.1 also treats a lone closing brace outside an interpolation as literal text:

```python
print(f"{{name}} = {name}")
```

F-strings support the same escapes as ordinary strings. F-strings themselves
remain double-quoted: `f'...'` is not Aurora 0.1 syntax. They do not support
conversion flags such as `!r` or a format-specifier mini-language.
Interpolations are evaluated from left to right and the result is an owned
`String`.

## Complexity Limits

The maintained parser rejects excessive nesting and expression chains instead of risking host stack exhaustion. The current 128-level limits for expressions, types, patterns, statements, f-string braces, and chained operators are defined in [Grammar](/manual/grammar#syntactic-complexity-limits) and summarized in [Current Limits](/manual/current-limits).

## Grammar

The token productions, reserved words, indentation protocol, delimiters,
operators, and literal forms in this chapter are normative. Their composition
into declarations, statements, patterns, types, and expressions is defined by
the complete [Grammar](/manual/grammar). A source spelling not accepted by
those productions is not an extension point.

## Typing Rules

Lexing does not assign expression types, but it preserves the literal kind and
mathematical or decoded value used by static checking. Integer literals may
later adopt an exact expected integer or floating type; floating literals may
adopt `float32` or `float64`; duration, Boolean, ordinary-string, and f-string
tokens enter checking as `Duration`, `bool`, `String`, and an interpolated
`String` expression respectively. No lexical spelling performs a runtime
coercion.

## Runtime Semantics

Tokenization has no runtime side effects. Decoded string scalars, literal
numbers, duration nanoseconds, and f-string text segments become constants or
MIR inputs only after the complete module has parsed and checked. A lexical
failure prevents execution.

Suppressing a physical line boundary has no runtime action. The resulting
token sequence evaluates exactly as the same tokens written on one physical
line.

## Ownership And Evaluation Order

Tokens do not own or borrow runtime values. Ordinary and f-string literals
produce owned values when evaluated; f-string interpolation expressions run
left to right as specified by [Expressions](/manual/expressions). Indentation,
comments, and physical-line markers have no runtime evaluation.

Physical-line placement and continuation indentation do not create, extend, or
end a borrow and do not change move/copy decisions. Source-order evaluation
follows the joined logical token sequence.

## Diagnostics

`AU1001` reports invalid lexical input, including physical tabs, invalid
escapes, malformed or unterminated literals, invalid characters, invalid block
indentation, and delimiter pairing failures. An unexpected closer is primary
at that closer. A mismatched closer names the expected delimiter and carries a
labeled secondary span for its opener. An unclosed delimiter reports at EOF
and likewise labels its opener. `AU1002` reports the focused single-quoted
f-string spelling and directs the author to `f"..."`. Once tokenization
succeeds, syntax failures belong to parser code `AU1101` rather than this page.

## Backend Support

The compiler tokenizes source once before MIR lowering or native code
generation. The MIR runtime and direct native backend therefore accept exactly
the same lexical language; there is no backend-specific lexer.

## Limits And Implementation-Defined Behavior

Identifiers are ASCII, source is UTF-8, physical tabs are rejected,
continuation requires an unmatched source delimiter, ordinary lists reject
trailing commas, backslash continuation and multiline ordinary/f-strings are
unavailable, and literal magnitude and parser-complexity caps are fixed by
this chapter and [Current Limits](/manual/current-limits). Continuation
indentation is not semantically significant, but delimiter matching, token
spans, and the expression-match layout island are defined behavior rather than
implementation choices.

## Status

The forms described as accepted above are implemented. Delimiter continuation
and its layout/diagnostic policy are Provisional under ADR-0025 pending the
Batch 2 checkpoint review. Raw, byte, triple-quoted, and single-quoted
f-strings; alternate integer bases; digit separators; block comments;
semicolons; ordinary trailing commas other than the required singleton-tuple
comma; backslash continuation; and multiline string or f-string literals are
unavailable, not partially implemented.
