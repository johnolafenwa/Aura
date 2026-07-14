# Lexical Structure

This chapter defines how Aurora source text becomes tokens and indentation markers. It is normative for source spelling. The complete token-level productions are collected in [Grammar](/manual/grammar); name binding and reserved builtin names are defined by [Names And Scopes](/manual/names-and-scopes) and [Static Semantics](/manual/static-semantics).

## Source Files And Text

Aurora source files conventionally use the `.au` extension and contain UTF-8 text. One UTF-8 byte-order mark is ignored only when it occurs at the beginning of the file.

Source is processed as physical lines. Except for the narrow multiline-match accommodation described below, a physical line is also the boundary of an Aurora logical line.

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
return if elif else and or not match case for in while break
continue pass try with as true false
```

`true` and `false` produce boolean-literal tokens. The other words introduce declarations, control flow, ownership forms, imports, or operators and cannot normally be used as ordinary identifiers. `own` is reserved everywhere; in the current grammar its accepted use is the consuming receiver spelling `own self`.

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
2. A count greater than the current count emits one `INDENT` and records the new count.
3. A smaller count emits one or more `DEDENT` tokens. The new count must equal a previously recorded indentation level.
4. The line contents are tokenized and followed by `NEWLINE`.
5. End of file emits all outstanding `DEDENT` tokens and then `EOF`.

Aurora does not require an indentation width of four spaces, but sibling lines must return to exactly the same recorded count. The maintained examples use four spaces.

A suite must contain at least one nonblank, non-comment line. Use `pass` for an intentionally empty suite.

## Physical-Line Boundaries

Parentheses, brackets, and braces do not generally suppress `NEWLINE`, `INDENT`, or `DEDENT`. Function signatures, calls, type-argument lists, and collection literals therefore stay on one physical line in Aurora 0.1:

```python
# Valid.
result = call(first, second)

# Not valid Aurora 0.1 continuation.
# result = call(
#     first,
#     second,
# )
```

Backslash continuation is not implemented. A complete multiline match expression may appear inside a delimited expression through a narrow parser rule; that exception is defined in [Expressions](/manual/expressions) and [Grammar](/manual/grammar#match-expressions). It does not enable general multiline calls or literals.

## Punctuation And Operators

Aurora 0.1 recognizes:

```text
( ) [ ] { } : , . ?
= == != < <= > >=
+ += - -= * *= / /= // //= % %=
->
```

There is no semicolon. Multiple statements cannot share one physical line. Aurora 0.1 also has no exponentiation, unary `+`, bitwise operators, assignment expressions, lambda arrow, tuple punctuation, or conditional-expression operator. The lexer chooses the longest operator spelling, so `//=` is one token rather than `//` followed by `=`.

Comma-separated lists do not accept a trailing comma. This applies to arguments, parameters, imports, type arguments, generic parameters, enum payloads, patterns, collection elements, and trait lists.

## Integer Literals

An integer literal is one or more decimal digits:

```ebnf
INTEGER = digit, { digit } ;
```

Examples are `0`, `42`, and `170000`. The lexical value must fit an unsigned 128-bit integer. Static checking then selects an expected integer type when available and verifies that the value fits; otherwise the literal defaults to `int64`. The source spelling `int` is an alias for `int64`.

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

`10ms`, `2s`, and `1m` represent 10, 2,000, and 60,000 milliseconds respectively and have type `Duration`. Scaling must fit signed 128-bit milliseconds. Fractional durations and duration arithmetic syntax are not supported.

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
