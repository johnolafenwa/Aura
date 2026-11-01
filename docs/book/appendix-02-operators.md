# Appendix B: Operators and symbols

This appendix collects Aura's operators and the punctuation that carries
meaning in its syntax. If you know Python, almost all of it will look
familiar, and the differences are deliberate: Aura has no implicit numeric
conversion, no truthiness, and no integer `/`, because each of those is a
place where Python quietly picks an answer for you. The
[Expressions](/manual/expressions) page of the Manual has the full typing and
evaluation rules.

## Arithmetic

| Operator | Meaning | Example |
| --- | --- | --- |
| `+` | addition, string concatenation, `Duration` sum | `total + 1`, `"ab" + "cd"`, `2s + 500ms` |
| `-` | subtraction, and unary negation | `end - start`, `-offset` |
| `*` | multiplication, `Duration` times `int64` | `width * height`, `100ms * 3` |
| `/` | true division, floating-point types only | `7.0 / 2.0` |
| `//` | floor division | `7 // 2` |
| `%` | remainder paired with `//` | `-7 % 2` |
| `**` | power | `2 ** 10` |

Both operands of a binary arithmetic operator must have the same type. An
`int32` plus an `int64` is an error, not a widening:

```aura check-fail:AU2999
def main():
    a: int32 = 1
    b: int64 = 2
    print(a + b)
```

```console
$ aura check mix.au
error[AU2999]: binary operator operands must match, found `int32` and `int64`
 --> mix.au:4:11
  |
4 |     print(a + b)
  |           ^
```

Convert one side explicitly with `as`, for example `a as int64 + b`. Unsuffixed
literals adapt to the type they are combined with, so `a + 1` is fine.

Integer `/` is rejected with `AU2003`, and the message tells you which of the
two operations you probably meant: `//` for floor division, or `.to_float()`
on both operands for true division. Floor division rounds toward negative
infinity, as in Python, so `-7 // 2` is `-4` and `-7 % 2` is `1`. Dividing by
zero traps with `AU4004`.

Integer arithmetic is checked. A result that does not fit its type traps
with `AU4002` instead of wrapping around:

```console
$ aura run ov.au
error[AU4002]: integer value `2147483648` does not fit in `int32`
 --> ov.au:3:11
  |
3 |     print(big + 1)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

When wrapping is what you want, every integer type has explicit methods such
as `wrapping_add` and `saturating_mul`.

Aura has no unary `+`. Writing `+3` is a syntax error.

## Comparison and membership

| Operator | Meaning |
| --- | --- |
| `==`, `!=` | equality and inequality |
| `<`, `<=`, `>`, `>=` | ordering, for numbers, `Duration`, and `str` |
| `in`, `not in` | membership in a list, dict, set, or substring in a `str` |
| `is None`, `is not None` | test a `T \| None` value, and narrow it |

Comparisons chain the way they do in Python: `1 < x < 10` means
`1 < x and x < 10`, with `x` evaluated once. String ordering compares the
underlying bytes, so `"Zebra" < "apple"` is true; there is no locale-aware
ordering built in.

`is` is not a general identity test. It only appears in `is None` and
`is not None`, and after the test the compiler treats the value as narrowed to
the non-`None` type in the branch where that is known.

## Boolean operators

`and`, `or`, and `not` work on `bool` and nothing else. `and` and `or`
short-circuit: the right side is only evaluated when it can change the
answer. Because there is no truthiness, `if values:` is an error; write
`if len(values) > 0:` instead.

The conditional expression `value if condition else other` picks one of two
values and has the lowest precedence of all.

## Bitwise and shift operators

| Operator | Meaning |
| --- | --- |
| `&` | bitwise and |
| `\|` | bitwise or; also the union type operator, as in `str \| None` |
| `^` | bitwise exclusive or |
| `~` | bitwise not (unary) |
| `<<`, `>>` | shift left, shift right |

These work on integers of one exact type. A shift count must be at least zero
and less than the width of the type, and `<<` traps with `AU4002` if bits
would be lost off the top.

## Assignment

`=` binds or assigns. Every arithmetic and bitwise operator has a compound
form that updates a mutable binding in place: `+=`, `-=`, `*=`, `/=`, `//=`,
`%=`, `**=`, `&=`, `|=`, `^=`, `<<=`, and `>>=`. Assignment is a statement,
never an expression, so there is no walrus operator and no `a = b = 0`.

## Precedence

From loosest to tightest binding:

1. `value if condition else other`
2. `or`
3. `and`
4. `not`
5. comparisons, `in`, `not in` (all one level, chained)
6. `|`
7. `^`
8. `&`
9. `<<`, `>>`
10. `+`, `-`
11. `*`, `/`, `//`, `%`
12. unary `-`, unary `~`, and the prefix forms `try` and `match`
13. `**`
14. calls, indexing, member access, specialization, and `as` casts

Two results surprise people. `**` binds tighter than unary minus, so `-2 ** 2`
is `-4`. And `**` groups to the right, so `2 ** 3 ** 2` is `2 ** 9`, which is
`512`. Parentheses are always allowed and often kinder to readers.

This program exercises most of the table:

```aura
def main():
    print(-7 // 2)
    print(2 ** 3 ** 2)
    print(-2 ** 2)
    x = 5
    print(1 < x < 10)
    print(6 & 3)
    print(1 << 4)
    print(2s + 500ms)
    print(3 in [1, 2, 3])
    print("big" if x > 3 else "small")
```

```console
$ aura run ops.au
-4
512
-4
true
2
16
2500ms
true
big
```

## Symbols

| Symbol | Where it appears |
| --- | --- |
| `:` | ends a block header (`def`, `if`, `for`, `class`, `case`, `with`); separates a name from its type (`count: int64`); separates keys from values in dict literals; separates slice bounds (`text[1:4]`) |
| `->` | introduces a function's return type |
| `.` | member access and method calls, and module paths (`process.run`) |
| `,` | separates arguments, elements, and tuple parts; a trailing comma before a closing bracket is allowed |
| `( )` | calls, grouping, tuples; `(x,)` is a one-element tuple |
| `[ ]` | list literals, indexing, slicing, and type arguments (`list[str]`, `Queue[Job]`) |
| `{ }` | dict and set literals, and interpolation inside f-strings |
| `=` | binding and assignment; also named arguments and defaults (`timeout=1s`) |
| `@` | a decorator line above a function, or `@property` in a class |
| `#` | starts a comment that runs to the end of the line |
| `f"..."` | an f-string with `{expression}` interpolation |
| `"""..."""` | a triple-quoted string, also used for docstrings |
| `ms`, `s`, `m` | suffixes on integer literals that make a `Duration`: `10ms`, `2s`, `1m` |
| `_` | the wildcard pattern, and digit separators in numbers (`1_000_000`) |
| `*name` | a rest pattern in tuple or sequence matching |

Aura has no semicolons: each statement gets its own line, and `x = 1; y = 2`
is rejected with `AU1001` at the `;`. There are no block comments either; use
a `#` on each line.
