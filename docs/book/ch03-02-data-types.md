# Data types

Every value in Aura has a type, and the compiler knows it before the program
runs. That makes Aura a *statically typed* language. Most of the time you don't
write the types yourself, because the compiler works them out from the values,
but they are always there, and they decide what you can do with each value.

A handful of types turn up in almost every program: integers, floating-point
numbers, booleans, strings, `None`, and tuples. We'll go through each of them,
and then look at how to convert between them, which in Aura you always do
yourself. Lists,
dictionaries, and sets get a chapter of their own,
[Chapter 8](/book/ch08-00-collections).

## Integers

An integer is a whole number. When you write `count = 3`, `count` is an
`int64`: a signed integer stored in 64 bits, which can hold any value from
about minus nine quintillion to plus nine quintillion. That is Aura's default
integer type, and it is the right choice almost all the time. You can also
spell it `int`, which is another name for `int64`.

When you need a specific size, Aura has a full set of fixed-width integer
types:

| Signed | Unsigned |
| --- | --- |
| `int8` | `uint8` |
| `int16` | `uint16` |
| `int32` | `uint32` |
| `int64` | `uint64` |
| `int128` | `uint128` |
| `intsize` | `uintsize` |

A signed type can hold negative numbers; an unsigned one holds only zero and
up, but goes twice as high. The number is how many bits it uses, and
`intsize` and `uintsize` are the size of a memory address on the machine you
are running on. You pick one of these when a value has to fit a particular
format, such as a byte in a file (`uint8`) or a field in a network protocol,
or when you are calling an API that asks for one. The exit code from `main` is
an `int32` for this reason. Otherwise, stick to `int64`.

Integer literals can use underscores to group digits, and a prefix to choose
another base:

```aura
def main():
    population = 8_100_000_000
    mask: uint8 = 0xFF
    permissions: uint16 = 0o755
    flags: uint8 = 0b1010_0110
    print(population)
    print(mask)
```

The underscores are only for the reader; `8_100_000_000` and `8100000000` are
the same number. `0x` is hexadecimal, `0o` is octal, and `0b` is binary.

### Overflow is an error

A fixed-size integer has a largest value. What should happen when a
calculation goes past it? Many languages wrap around: in C, Java, or Go, adding
one to the largest 32-bit integer gives you the most negative one. That has
caused a long list of real bugs, because the program carries on with a number
that is wildly wrong and nothing tells you. Python avoids the problem by
making its integers grow as large as needed, which costs speed and memory on
every integer operation.

Aura checks instead. Here two values that each fit comfortably in an `int32`
add up to one that doesn't:

```aura
def main():
    a: int32 = 2_000_000_000
    b: int32 = 2_000_000_000
    print(a + b)
```

```console
$ aura run src/main.au
error[AU4002]: integer value `4000000000` does not fit in `int32`
 --> src/main.au:4:11
  |
4 |     print(a + b)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

This program passes `aura check`, so the failure happens while it runs. The
program stops at the addition, and the message says what the true result
would have been and that it doesn't fit. Aura calls a failure like this a
*trap*: it stops the task straight away with a diagnostic, instead of letting
the wrong value travel on. When the compiler can see the problem in advance,
for example with a literal that's too big for its type, it reports it before
the program runs:

```aura check-fail:AU2999
def main():
    small: int8 = 300
```

```console
$ aura run src/main.au
error[AU2999]: integer literal `300` does not fit in `int8`
 --> src/main.au:2:19
  |
2 |     small: int8 = 300
  |                   ^
```

Sometimes wrapping is exactly what you want, as in a hash function or a
checksum. For those cases every integer type has methods that say so in the
code: `wrapping_add`, `wrapping_sub`, and `wrapping_mul` wrap around, and
`saturating_add`, `saturating_sub`, and `saturating_mul` stop at the largest
or smallest value instead.

```aura
def main():
    top: int32 = 2147483647
    print(top.wrapping_add(1))
    print(top.saturating_add(1))
```

```console
$ aura run src/main.au
-2147483648
2147483647
```

The point is that you choose. The ordinary `+` never surprises you.

### Integer arithmetic

The arithmetic operators are the ones you know from Python: `+`, `-`, `*`,
`//` for floor division, `%` for the remainder, and `**` for powers.

```aura
def main():
    print(7 // 2)
    print(7 % 2)
    print(-7 // 2)
    print(-7 % 2)
    print(2 ** 10)
```

```console
$ aura run src/main.au
3
1
-4
1
1024
```

Floor division rounds toward negative infinity, as Python's does, so
`-7 // 2` is `-4` rather than `-3`, and the remainder takes the sign of the
divisor. One operator is missing:

```aura check-fail:AU2003
def main():
    total = 7
    people = 2
    print(total / people)
```

```console
$ aura run src/main.au
error[AU2003]: integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
 --> src/main.au:4:11
  |
4 |     print(total / people)
  |           ^
```

Python 2 and Python 3 disagree about what `7 / 2` means on integers: one
gives `3`, the other `3.5`. So do C and JavaScript. Rather than pick one and
surprise half its readers, Aura doesn't allow `/` on integers at all. The
message lists the two things you might have meant. If you want a whole-number
answer, write `//`. If you want `3.5`, convert both numbers to floating point
with `.to_float()`, which we'll come back to at the end of this section.

Dividing by zero, with `//` or `%`, is a trap (`AU4004`, "division by zero").

## Floating-point numbers

A floating-point number has a fractional part, like `2.5` or `0.1`. Aura has
two floating-point types, `float64` and `float32`, which use the standard
64-bit and 32-bit IEEE 754 formats. A literal with a decimal point, like
`2.5`, is a `float64` unless something asks for a `float32`. As with
integers, use the 64-bit type unless you have a reason not to.

```aura
def main():
    print(10.0 / 4.0)
    print(1.0 / 3.0)
    print(0.1 + 0.2)
    print(3.0)
```

```console
$ aura run src/main.au
2.5
0.3333333333333333
0.30000000000000004
3.0
```

`/` on floating-point numbers is ordinary division. `0.1 + 0.2` printing
`0.30000000000000004` is not an Aura quirk: neither `0.1` nor `0.2` can be
represented exactly in binary floating point, and every language that uses
IEEE 754 doubles gives the same answer. Aura prints the shortest decimal that
reads back as exactly the same number, which is why you see all of the digits
here, and why a whole number such as `3.0` keeps its `.0` so you can tell it
apart from an integer.

Floating-point division by zero is a trap in Aura too, with the same
`AU4004` error, rather than producing an infinity or a "not a number" value
that spreads quietly through later calculations.

An integer literal can stand in for a float when the type is already known.
`whole: float64 = 2` is fine, and so is `price * 4` when `price` is a
`float64`, because the `4` takes on the type its context expects. That only
works for literals that the float type can represent exactly, and it never
applies to a binding that already holds an integer, as we'll see next.

## Nothing mixes implicitly

Here is a calculation that works in Python and almost every other language:

```aura check-fail:AU2999
def main():
    price = 2.5
    quantity = 4
    print(price * quantity)
```

```console
$ aura run src/main.au
error[AU2999]: binary operator operands must match, found `float64` and `int64`
 --> src/main.au:4:11
  |
4 |     print(price * quantity)
  |           ^
```

`price` is a `float64` and `quantity` is an `int64`, and Aura won't multiply
them. Arithmetic in Aura always works on two values of the same type. The
same applies to two integer types:

```aura check-fail:AU2999
def main():
    steps: int32 = 12
    total: int64 = 1000
    print(total + steps)
```

```console
$ aura run src/main.au
error[AU2999]: binary operator operands must match, found `int64` and `int32`
 --> src/main.au:4:11
  |
4 |     print(total + steps)
  |           ^
```

This rule surprises people more than any other in this chapter, so let's be
clear about why it exists. Converting a number from one type to another can
lose information. A large `int64` might not fit in an `int32`, and a large
integer might not have an exact `float64` equivalent. A language that converts
for you has to decide, at every mixed operation, which way to go and what to
do if information is lost, and it makes that decision silently, in code where
nobody can see it. Most of the overflow and precision bugs in C programs live
in exactly those silent conversions. Aura makes every conversion something
you write, so every place a number changes type is visible in the source and
has an explicit rule about what happens if it doesn't fit.

In practice this matters less than it sounds. Because the default types are
`int64` and `float64`, most code never has two integer types to mix. When it
does, the fix is one conversion, which we'll see shortly. Function calls
follow the same rule: passing an `int32` to a parameter declared as `int64` is
an error (`AU2002`), not a widening.

## Booleans

The `bool` type has two values, `true` and `false`. They are lowercase, and
the compiler notices if you bring Python's spelling with you:

```aura check-fail:AU2005
def main():
    ready = True
```

```console
$ aura run src/main.au
error[AU2005]: unknown name `True`; did you mean `true`?
 --> src/main.au:2:13
  |
2 |     ready = True
  |             ^
```

Comparisons (`==`, `!=`, `<`, `<=`, `>`, `>=`) produce a `bool`, and `and`,
`or`, and `not` combine them, all as in Python.

The bigger difference from Python is that only a `bool` can be a condition.
Python lets you write `if pending:` for a number, a string, or a list, and
treats zero and empty values as false. Aura does not:

```aura check-fail:AU2002
def main():
    pending = 3
    if pending:
        print("still working")
```

```console
$ aura run src/main.au
error[AU2002]: `if` condition must have type `bool`, found `int64`
 --> src/main.au:3:5
  |
3 |     if pending:
  |     ^
```

Truthiness is convenient, but it hides what the condition is testing. Does
`if user:` mean "is there a user", or "does this user's name have any
characters in it"? With a `bool` condition, the code has to say. Write the
comparison you mean:

```aura
def main():
    pending = 3
    if pending > 0:
        print("still working")
    name = "Ada"
    if name != "":
        print(f"hello, {name}")
    ready = pending == 0
    print(ready)
    print(not ready and pending < 10)
```

```console
$ aura run src/main.au
still working
hello, Ada
false
true
```

## Strings

A `str` is a piece of text. Aura strings are always valid UTF-8, so they can
hold any language and any emoji. String literals use double or single quotes,
which mean the same thing, and support the usual escapes, such as `\n` for a
newline, `\t` for a tab, `\\` for a backslash, and `\"` or `\'` for a quote.
Two other forms are useful. A string in triple quotes can span several lines,
and keeps them exactly as written, including indentation and the newline
before the closing quotes. A string with an `r` prefix is *raw*: backslashes
in it are ordinary characters, which is handy for Windows paths and regular
expressions.

```aura
def main():
    quote = 'She said "hi"'
    path = r"C:\Users\ada"
    lines = "one\ntwo"
    print(quote)
    print(path)
    print(lines)
    poem = """Roses are red,
  violets are blue.
"""
    print(poem)
```

```console
$ aura run src/main.au
She said "hi"
C:\Users\ada
one
two
Roses are red,
  violets are blue.

```

The blank line at the end of the output is the newline inside the
triple-quoted string, followed by the one `print` adds.

`+` joins two strings, and there is a set of methods you'll recognize from
Python, such as `to_upper`, `to_lower`, `trim`, `split`, `replace`,
`starts_with`, and `contains`. A string's length needs a little care, because
characters and bytes are not the same thing in UTF-8:

```aura
def main():
    word = "naïve"
    print(word.len())
    print(word.byte_len())
    print(word[0:2])
    print(word[-3:])
    print(word.to_upper())
```

```console
$ aura run src/main.au
5
6
na
ïve
NAÏVE
```

`len()` counts characters (strictly, Unicode scalar values), and
`byte_len()` counts the bytes of the UTF-8 encoding, where `ï` takes two. A
slice like `word[0:2]` counts in characters too, and gives you a new string.
What you can't do is index a string with a single number:

```aura check-fail:AU2999
def main():
    word = "naïve"
    print(word[0])
```

```console
$ aura run src/main.au
error[AU2999]: cannot index non-Array, list, or dict value `str`
 --> src/main.au:3:11
  |
3 |     print(word[0])
  |           ^
```

Aura has no separate character type. A one-character piece of text is still
a `str`, and you get one with a slice, `word[0:1]`, as the word counter did.
Leaving out single-number indexing keeps you from writing code that looks
like it takes constant time but has to scan the string from the start, which
any UTF-8 string has to do to find the nth character. When you really want
every character, `word.chars()` gives them to you as a list in one pass;
Chapter 8 shows it along with `ord` and `chr`.

### f-strings

The easiest way to build a string from other values is an f-string, which
works as it does in Python. Put `f` before a double-quoted string, and any
expression in braces is evaluated and inserted. After a colon, a format
specification controls how the value looks:

```aura
def main():
    name = "Ada"
    score = 0.8765
    count = 1234567
    print(f"{name} scored {score:.1%}")
    print(f"[{name:<6}] [{name:>6}] [{name:^7}]")
    print(f"{count:,d} rows, {score:.2f} ratio")
    print(f"{{literal braces}}")
```

```console
$ aura run src/main.au
Ada scored 87.6%
[Ada   ] [   Ada] [  Ada  ]
1,234,567 rows, 0.88 ratio
{literal braces}
```

`:.1%` shows a number as a percentage with one decimal place, `<`, `>`, and
`^` align text in a field of the given width, `,d` groups digits with commas,
and `.2f` rounds to two decimal places. Doubled braces produce literal ones.
The compiler checks each format specification against the type of the value
before the program runs, so a typo in one is a compile error rather than a
surprise in your output. Aura f-strings always use double quotes; the
`f'...'` form isn't supported.

## `None`

`None` is a type with exactly one value, which is also written `None`. A
function that doesn't return anything useful returns `None`, which is why the
`main` functions in Chapter 1 didn't need a return type. You will see it far
more often as part of a larger type. In the word counter, `io.read_line`
returned `str | None`: a string, or `None` at the end of input. Aura has no
`null` that can turn up anywhere. A value can only be `None` if its type says
so, and then the compiler makes you deal with that case before using it.
[Chapter 6](/book/ch06-00-enums-and-match) covers these union types.

## Tuples

A tuple groups a fixed number of values, which can have different types.
You write one with parentheses and commas:

```aura
def main():
    point = (3, 4)
    x, y = point
    print(f"x = {x}, y = {y}")
    print(point[0])
    print(point)
    reading = ("Lagos", 31.5, true)
    city, temperature, sunny = reading
    print(f"{city}: {temperature} degrees, sunny: {sunny}")
    print((3, 4) == point)
    single = (42,)
    print(single)
```

```console
$ aura run src/main.au
x = 3, y = 4
3
(3, 4)
Lagos: 31.5 degrees, sunny: true
true
(42,)
```

The type of `reading` is `(str, float64, bool)`, and the order is part of the
type. You take a tuple apart by *unpacking* it into names, as in
`city, temperature, sunny = reading`, the same way the word counter unpacked
`(word, count)` pairs. You can also read one element with a number in square
brackets, starting from `0`. A one-element tuple needs a trailing comma,
`(42,)`, because `(42)` is `42` in parentheses. Two tuples of the same
type can be compared with `==` and `!=`.

Tuples are meant for small, fixed groups, like a pair of coordinates or the
two results of a function. They are not lists: they have no methods, you can't
loop over them, and the index has to be a literal number, not a binding:

```aura check-fail:AU2003
def main():
    point = (3, 4)
    i = 1
    print(point[i])
```

```console
$ aura run src/main.au
error[AU2003]: tuple indices must be non-negative integer literals
 --> src/main.au:4:17
  |
4 |     print(point[i])
  |                 ^
```

Because the index is always a literal, the compiler always knows which
element, and therefore which type, you are reading. When you find yourself
wanting to name the elements of a tuple, that's a sign it should be a class,
which [Chapter 5](/book/ch05-00-classes) introduces. The
[Tuples](/manual/tuples) page of the Manual has the full rules.

## Converting between types

Since nothing converts implicitly, you need ways to convert on purpose. There
are three, and each is for a different job.

### `as` for numbers

`value as Type` converts a number to another numeric type:

```aura
def main():
    small: int32 = 7
    wide = small as int64
    print(wide + 100)
    print(3.99 as int64)
    print(-3.99 as int64)
    print(7 as float64)
    big: int64 = 5_000_000_000
    print(big as int32)
```

```console
$ aura run src/main.au
107
3
-3
7.0
error[AU4002]: integer value `5000000000` does not fit in `int32`
 --> src/main.au:9:11
  |
9 |     print(big as int32)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

`as` is exact or it traps. Widening an `int32` to an `int64` always works.
Converting a float to an integer drops the fractional part, rounding toward
zero, so `3.99` becomes `3` and `-3.99` becomes `-3`. But a value that doesn't
fit its new type stops the program, as `5_000_000_000 as int32` did on the
last line. Aura never wraps the value around or quietly keeps the low bits.
Read `x as int32` as a promise that `x` fits; the program checks the promise
when it runs.

The same goes for integers that are too precise for a `float64`, which has
only 53 bits for the digits of a number:

```aura
def main():
    n: int64 = 9_007_199_254_740_993
    print(n.to_float())
    print(n as float64)
```

```console
$ aura run src/main.au
9007199254740992.0
error[AU4002]: integer value `9007199254740993` cannot be represented exactly as `float64`
 --> src/main.au:4:11
  |
4 |     print(n as float64)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

The first line shows the other way to turn an integer into a float.
`.to_float()` rounds to the nearest `float64`, and here the nearest one is
`9007199254740992.0`, one less than the original. Use `.to_float()` when
rounding is fine, which it usually is when you are calculating an average or a
ratio, and `as float64` when you need the value to be exact. That is also how
you get true division of two integers, as the error message for `/` suggested:
`total.to_float() / people.to_float()`.

### Text to numbers and back

`as` only works between numbers. Converting text to a number is a different
kind of operation, because it can fail: not every string is a number.

```aura check-fail:AU2002
def main():
    text = "12"
    n = text as int64
```

```console
$ aura run src/main.au
error[AU2002]: casts are only supported between numeric types, found `str` and `int64`
 --> src/main.au:3:9
  |
3 |     n = text as int64
  |         ^
```

To parse text, use `parse_int64`, `parse_int32`, or `parse_float64`. Each
returns a `Result`, the same type `io.read_line` returned in the word counter,
so you have to say what happens when the text isn't a number. Going the other
way can't fail, so it doesn't need a `Result`: `str(value)`, an f-string, or
the `.to_string()` method all turn a number into text.

```aura
def main():
    count = 42
    print("count: " + str(count))
    print(f"count: {count}")
    print(count.to_string())
    match parse_int64("123"):
        case Result.Ok(value):
            print(value + 1)
        case Result.Err(message):
            print(message)
    match parse_int64("12x"):
        case Result.Ok(value):
            print(value + 1)
        case Result.Err(message):
            print(message)
```

```console
$ aura run src/main.au
count: 42
count: 42
42
124
invalid digit found in string
```

Notice that `"count: " + count` without the `str(...)` would be an error, for
the same reason as `price * quantity`: `+` needs two values of the same type.

That's the full set: `as` for numbers when the value must fit,
`.to_float()` when rounding is acceptable, a `parse_` function when the input
is text, and `str` or an f-string to produce text. The
[Types](/manual/types) page of the Manual has the complete conversion table,
and [Chapter 10](/book/ch10-00-generics-traits-conversions) shows how to make
your own types convertible. With values and their types in hand, we can start
writing functions that take and return them.
